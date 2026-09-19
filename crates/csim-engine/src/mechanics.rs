//! Combat mechanics formulas.
//!
//! Port of `Mechanics/Mechanics.*`. Every formula here is a pure function of the player's level or
//! weapon skill and the target's level. The C++ class read the level from a `Target*`; the Rust
//! version stores the target level and is updated by its owner when the target changes.
//!
//! Chances are expressed as fractions (`0.05` = 5%).

/// Combat mechanics against a target of a given level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mechanics {
    target_level: u32,
}

impl Mechanics {
    /// Base armor of a raid boss.
    pub const BOSS_BASE_ARMOR: i32 = 3750;

    pub fn new(target_level: u32) -> Self {
        Self { target_level }
    }

    pub fn target_level(&self) -> u32 {
        self.target_level
    }

    pub fn set_target_level(&mut self, target_level: u32) {
        self.target_level = target_level;
    }

    /// Defense skill of the target (level * 5 for creatures).
    pub fn target_defense(&self) -> u32 {
        self.target_level * 5
    }

    fn defense_minus_wpn_skill(&self, wpn_skill: u32) -> i32 {
        self.target_defense() as i32 - wpn_skill as i32
    }

    fn level_diff(&self, clvl: u32) -> i32 {
        self.target_level as i32 - clvl as i32
    }

    /// Miss chance for yellow (special) melee attacks. Same as the two-hand white miss chance.
    pub fn yellow_miss_chance(&self, wpn_skill: u32) -> f64 {
        self.two_hand_white_miss_chance(wpn_skill)
    }

    /// Miss chance for white attacks while dual wielding.
    pub fn dual_wield_white_miss_chance(&self, wpn_skill: u32) -> f64 {
        self.two_hand_white_miss_chance(wpn_skill) * 0.8 + 0.2
    }

    /// Miss chance for white attacks with a single weapon.
    pub fn two_hand_white_miss_chance(&self, wpn_skill: u32) -> f64 {
        let diff = self.defense_minus_wpn_skill(wpn_skill);

        if diff > 10 {
            0.05 + 0.01 + f64::from(diff) * 0.002
        } else {
            0.05 + f64::from(diff) * 0.001
        }
    }

    /// Chance for a white melee attack to be a glancing blow.
    ///
    /// Non-melee classes do not follow this formula.
    pub fn glancing_blow_chance(&self, clvl: u32) -> f64 {
        let level_diff = self.level_diff(clvl);
        if level_diff < 0 {
            return 0.0;
        }

        0.1 + f64::from(level_diff) * 5.0 * 0.02
    }

    /// Chance for the target to dodge.
    ///
    /// Creatures at the player's level have a 5% chance to dodge; each level above the player adds
    /// 0.5%, and weapon skill counters that difference slightly. See
    /// <https://github.com/magey/classic-warrior/wiki/Attack-table#dodge>.
    pub fn dodge_chance(&self, wpn_skill: u32) -> f64 {
        let diff = self.defense_minus_wpn_skill(wpn_skill);
        (0.05 + f64::from(diff) * 0.001).max(0.0)
    }

    /// Chance for the target to parry (only when attacking from the front).
    pub fn parry_chance(&self, wpn_skill: u32) -> f64 {
        let diff = self.defense_minus_wpn_skill(wpn_skill);
        (0.14 + f64::from(diff) * 0.001).max(0.0)
    }

    /// Chance for the target to block.
    pub fn block_chance(&self) -> f64 {
        0.0
    }

    /// Lower bound of the glancing blow damage multiplier.
    pub fn glancing_blow_dmg_penalty_min(&self, clvl: u32, wpn_skill: u32) -> f64 {
        if self.level_diff(clvl) < 1 {
            return 1.0;
        }

        let diff = self.defense_minus_wpn_skill(wpn_skill);
        (1.3 - 0.05 * f64::from(diff)).clamp(0.55, 0.91)
    }

    /// Upper bound of the glancing blow damage multiplier.
    pub fn glancing_blow_dmg_penalty_max(&self, clvl: u32, wpn_skill: u32) -> f64 {
        if self.level_diff(clvl) < 1 {
            return 1.0;
        }

        let diff = self.defense_minus_wpn_skill(wpn_skill);
        (1.2 - 0.03 * f64::from(diff)).clamp(0.75, 0.99)
    }

    /// Melee crit chance lost per level the target is above the player (1% per level).
    pub fn melee_crit_suppression(&self, clvl: u32) -> f64 {
        (0.01 * f64::from(self.level_diff(clvl))).max(0.0)
    }

    /// Crit chance from auras/equipment after the per-level suppression.
    ///
    /// `aura_crit` is expressed in hundredths of a percent (`100` = 1%); 0.6% is lost per level.
    pub fn suppressed_aura_crit_chance(&self, clvl: u32, aura_crit: u32) -> u32 {
        let level_diff = self.level_diff(clvl);
        if level_diff <= 0 {
            return aura_crit;
        }

        let penalty = 60 * level_diff as u32;
        aura_crit.saturating_sub(penalty)
    }

    /// Damage reduction from armor against an attacker of level `clvl`.
    pub fn reduction_from_armor(armor: i32, clvl: u32) -> f64 {
        let armor = f64::from(armor);
        armor / (armor + 400.0 + 85.0 * f64::from(clvl))
    }

    /// Spell miss chance from the level difference, reduced by `spell_hit` (fraction). Never below 1%.
    pub fn spell_miss_chance_from_lvl_diff(&self, clvl: u32, spell_hit: f64) -> f64 {
        let level_diff = self.level_diff(clvl);

        if level_diff < -2 {
            return 0.01;
        }

        let lvl_diff_penalty = match level_diff {
            -2 => 0.02,
            -1 => 0.03,
            0 => 0.04,
            1 => 0.05,
            2 => 0.06,
            3 => 0.17,
            4 => 0.28,
            _ => 0.39,
        };

        (lvl_diff_penalty - spell_hit).max(0.01)
    }

    /// Chance for a full resist given the target's effective resistance.
    ///
    /// Piecewise-linear approximation of:
    ///
    /// | Target resistance | 0-150 | 150-200 | 200-300 |
    /// |---|---|---|---|
    /// | % occurrence      | 0-1 % | 1-4 %   | 4-25 %  |
    ///
    /// This only serves as an initial approximation and is likely incorrect.
    pub fn full_resist_chance(t_resistance: u32) -> f64 {
        piecewise_linear(
            &[(0, 0.0), (150, 0.01), (200, 0.04), (300, 0.25)],
            t_resistance,
        )
    }

    /// Chance for a 75% partial resist. See [`Mechanics::full_resist_chance`] for the caveat.
    pub fn partial_75_chance(t_resistance: u32) -> f64 {
        piecewise_linear(
            &[
                (0, 0.0),
                (20, 0.01),
                (50, 0.02),
                (80, 0.03),
                (100, 0.04),
                (120, 0.06),
                (150, 0.11),
                (200, 0.23),
                (300, 0.55),
            ],
            t_resistance,
        )
    }

    /// Chance for a 50% partial resist. See [`Mechanics::full_resist_chance`] for the caveat.
    pub fn partial_50_chance(t_resistance: u32) -> f64 {
        piecewise_linear(
            &[
                (0, 0.0),
                (10, 0.02),
                (20, 0.04),
                (30, 0.05),
                (40, 0.07),
                (50, 0.09),
                (60, 0.11),
                (70, 0.13),
                (80, 0.15),
                (90, 0.17),
                (100, 0.19),
                (120, 0.24),
                (150, 0.37),
                (200, 0.48),
                (300, 0.16),
            ],
            t_resistance,
        )
    }

    /// Chance for a 25% partial resist. See [`Mechanics::full_resist_chance`] for the caveat.
    pub fn partial_25_chance(t_resistance: u32) -> f64 {
        if t_resistance >= 300 {
            // Ported as-is: the C++ implementation returns 0.3 here although its table ends at 3%.
            return 0.3;
        }

        piecewise_linear(
            &[
                (0, 0.0),
                (10, 0.06),
                (20, 0.12),
                (30, 0.18),
                (40, 0.23),
                (50, 0.28),
                (60, 0.33),
                (70, 0.37),
                (80, 0.41),
                (90, 0.45),
                (100, 0.47),
                (120, 0.49),
                (150, 0.39),
                (200, 0.21),
                (300, 0.03),
            ],
            t_resistance,
        )
    }
}

/// Linear interpolation over `(x, y)` breakpoints sorted by `x`; clamps to the last `y` beyond the
/// final breakpoint.
fn piecewise_linear(points: &[(u32, f64)], x: u32) -> f64 {
    debug_assert!(points.len() >= 2);

    for window in points.windows(2) {
        let (x_min, y_min) = window[0];
        let (x_max, y_max) = window[1];
        if x < x_max {
            return linear_increase_in_range(x_min, y_min, x_max, y_max, x);
        }
    }

    points[points.len() - 1].1
}

fn linear_increase_in_range(x_min: u32, y_min: f64, x_max: u32, y_max: f64, x_curr: u32) -> f64 {
    debug_assert!(x_curr >= x_min, "x_curr < x_min");
    debug_assert!(x_curr <= x_max, "x_curr > x_max");

    let x_delta = f64::from(x_max) - f64::from(x_min);
    let k = (y_max - y_min) / x_delta;

    (f64::from(x_curr) - f64::from(x_min)) * k + y_min
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Same tolerance as the C++ `almost_equal`.
    const EPS: f64 = 0.0001;

    fn assert_close(expected: f64, actual: f64) {
        assert_abs_diff_eq!(expected, actual, epsilon = EPS);
    }

    #[test]
    fn dodge_from_wpn_skill_diff() {
        let mut mechanics = Mechanics::new(63);

        for (skill, expected) in (300..=315).zip((50..=65).rev()) {
            assert_close(f64::from(expected) / 1000.0, mechanics.dodge_chance(skill));
        }
        assert_close(0.045, mechanics.dodge_chance(320));

        mechanics.set_target_level(60);
        assert_close(0.05, mechanics.dodge_chance(300));
    }

    #[test]
    fn parry_from_wpn_skill_diff() {
        let mechanics = Mechanics::new(63);
        assert_close(0.155, mechanics.parry_chance(300));
        assert_close(0.14, mechanics.parry_chance(315));
        assert_close(0.0, mechanics.block_chance());
    }

    #[test]
    fn two_hand_white_miss() {
        let mechanics = Mechanics::new(63);

        assert_close(0.090, mechanics.two_hand_white_miss_chance(300));
        assert_close(0.088, mechanics.two_hand_white_miss_chance(301));
        assert_close(0.086, mechanics.two_hand_white_miss_chance(302));
        assert_close(0.084, mechanics.two_hand_white_miss_chance(303));
        assert_close(0.082, mechanics.two_hand_white_miss_chance(304));
        assert_close(0.060, mechanics.two_hand_white_miss_chance(305));
        assert_close(0.059, mechanics.two_hand_white_miss_chance(306));
        assert_close(0.058, mechanics.two_hand_white_miss_chance(307));
        assert_close(0.057, mechanics.two_hand_white_miss_chance(308));
        assert_close(0.056, mechanics.two_hand_white_miss_chance(309));
        assert_close(0.055, mechanics.two_hand_white_miss_chance(310));
        assert_close(0.054, mechanics.two_hand_white_miss_chance(311));
        assert_close(0.053, mechanics.two_hand_white_miss_chance(312));
        assert_close(0.052, mechanics.two_hand_white_miss_chance(313));
        assert_close(0.051, mechanics.two_hand_white_miss_chance(314));
        assert_close(0.050, mechanics.two_hand_white_miss_chance(315));
        assert_close(0.045, mechanics.two_hand_white_miss_chance(320));

        assert_close(
            mechanics.two_hand_white_miss_chance(300),
            mechanics.yellow_miss_chance(300),
        );
    }

    #[test]
    fn dual_wield_white_miss() {
        let mechanics = Mechanics::new(63);

        assert_close(0.272, mechanics.dual_wield_white_miss_chance(300));
        assert_close(0.2704, mechanics.dual_wield_white_miss_chance(301));
        assert_close(0.2688, mechanics.dual_wield_white_miss_chance(302));
        assert_close(0.2672, mechanics.dual_wield_white_miss_chance(303));
        assert_close(0.2656, mechanics.dual_wield_white_miss_chance(304));
        assert_close(0.248, mechanics.dual_wield_white_miss_chance(305));
        assert_close(0.2472, mechanics.dual_wield_white_miss_chance(306));
        assert_close(0.2464, mechanics.dual_wield_white_miss_chance(307));
        assert_close(0.2456, mechanics.dual_wield_white_miss_chance(308));
        assert_close(0.2448, mechanics.dual_wield_white_miss_chance(309));
        assert_close(0.244, mechanics.dual_wield_white_miss_chance(310));
        assert_close(0.2432, mechanics.dual_wield_white_miss_chance(311));
        assert_close(0.2424, mechanics.dual_wield_white_miss_chance(312));
        assert_close(0.2416, mechanics.dual_wield_white_miss_chance(313));
        assert_close(0.2408, mechanics.dual_wield_white_miss_chance(314));
        assert_close(0.24, mechanics.dual_wield_white_miss_chance(315));
        assert_close(0.236, mechanics.dual_wield_white_miss_chance(320));
    }

    #[test]
    fn glancing_blow_rate() {
        let mut mechanics = Mechanics::new(63);

        assert_close(6.3, mechanics.glancing_blow_chance(1));
        assert_close(0.4, mechanics.glancing_blow_chance(60));

        mechanics.set_target_level(62);
        assert_close(0.3, mechanics.glancing_blow_chance(60));

        mechanics.set_target_level(61);
        assert_close(0.2, mechanics.glancing_blow_chance(60));

        mechanics.set_target_level(60);
        assert_close(0.1, mechanics.glancing_blow_chance(60));

        mechanics.set_target_level(59);
        assert_close(0.0, mechanics.glancing_blow_chance(60));
    }

    #[test]
    fn glancing_dmg_penalty() {
        let mechanics = Mechanics::new(63);

        assert_close(0.55, mechanics.glancing_blow_dmg_penalty_min(60, 5));
        assert_close(0.55, mechanics.glancing_blow_dmg_penalty_min(60, 300));
        assert_close(0.80, mechanics.glancing_blow_dmg_penalty_min(60, 305));
        assert_close(0.91, mechanics.glancing_blow_dmg_penalty_min(60, 310));
        assert_close(0.91, mechanics.glancing_blow_dmg_penalty_min(60, 315));
        assert_close(0.91, mechanics.glancing_blow_dmg_penalty_min(60, 10000));

        assert_close(0.75, mechanics.glancing_blow_dmg_penalty_max(60, 5));
        assert_close(0.75, mechanics.glancing_blow_dmg_penalty_max(60, 300));
        assert_close(0.90, mechanics.glancing_blow_dmg_penalty_max(60, 305));
        assert_close(0.99, mechanics.glancing_blow_dmg_penalty_max(60, 310));
        assert_close(0.99, mechanics.glancing_blow_dmg_penalty_max(60, 315));
        assert_close(0.99, mechanics.glancing_blow_dmg_penalty_max(60, 10000));

        // No penalty against targets of equal or lower level.
        assert_close(1.0, mechanics.glancing_blow_dmg_penalty_min(63, 300));
        assert_close(1.0, mechanics.glancing_blow_dmg_penalty_max(64, 300));
    }

    #[test]
    fn physical_crit_suppression_from_target_level() {
        let mechanics = Mechanics::new(63);

        assert_close(0.03, mechanics.melee_crit_suppression(60));
        assert_close(0.02, mechanics.melee_crit_suppression(61));
        assert_close(0.01, mechanics.melee_crit_suppression(62));
        assert_close(0.00, mechanics.melee_crit_suppression(63));
        assert_close(0.00, mechanics.melee_crit_suppression(64));
    }

    #[test]
    fn suppressed_aura_crit_chance() {
        let mechanics = Mechanics::new(63);

        assert_eq!(mechanics.suppressed_aura_crit_chance(60, 1000), 820);
        assert_eq!(mechanics.suppressed_aura_crit_chance(60, 100), 0);
        assert_eq!(mechanics.suppressed_aura_crit_chance(63, 100), 100);
        assert_eq!(mechanics.suppressed_aura_crit_chance(64, 100), 100);
    }

    #[test]
    fn reduction_from_armor() {
        assert_close(0.0, Mechanics::reduction_from_armor(0, 60));
        assert_close(
            3750.0 / (3750.0 + 400.0 + 85.0 * 60.0),
            Mechanics::reduction_from_armor(Mechanics::BOSS_BASE_ARMOR, 60),
        );
    }

    #[test]
    fn spell_miss_chance_from_lvl_diff() {
        let mechanics = Mechanics::new(63);

        assert_close(0.17, mechanics.spell_miss_chance_from_lvl_diff(60, 0.0));
        assert_close(0.07, mechanics.spell_miss_chance_from_lvl_diff(60, 0.10));
        assert_close(0.01, mechanics.spell_miss_chance_from_lvl_diff(60, 0.20));
        assert_close(0.04, mechanics.spell_miss_chance_from_lvl_diff(63, 0.0));
        assert_close(0.39, mechanics.spell_miss_chance_from_lvl_diff(50, 0.0));
        assert_close(0.01, mechanics.spell_miss_chance_from_lvl_diff(70, 0.0));
    }

    #[test]
    fn full_resistance_chance() {
        assert_close(0.0, Mechanics::full_resist_chance(0));
        assert_close(0.005, Mechanics::full_resist_chance(75));
        assert_close(0.01, Mechanics::full_resist_chance(150));
        assert_close(0.04, Mechanics::full_resist_chance(200));
        assert_close(0.25, Mechanics::full_resist_chance(300));
        assert_close(0.25, Mechanics::full_resist_chance(400));
    }

    #[test]
    fn partial_resist_chances_hit_breakpoints() {
        assert_close(0.0, Mechanics::partial_75_chance(0));
        assert_close(0.11, Mechanics::partial_75_chance(150));
        assert_close(0.39, Mechanics::partial_75_chance(250));
        assert_close(0.55, Mechanics::partial_75_chance(300));

        assert_close(0.02, Mechanics::partial_50_chance(10));
        assert_close(0.19, Mechanics::partial_50_chance(100));
        assert_close(0.32, Mechanics::partial_50_chance(250));
        assert_close(0.16, Mechanics::partial_50_chance(300));

        assert_close(0.06, Mechanics::partial_25_chance(10));
        assert_close(0.47, Mechanics::partial_25_chance(100));
        assert_close(0.12, Mechanics::partial_25_chance(250));
        assert_close(0.3, Mechanics::partial_25_chance(300));
    }
}
