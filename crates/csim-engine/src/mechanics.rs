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

    /// Miss chance for white attacks while dual wielding: a flat 19 % on top of the single
    /// weapon miss chance (27 % with 300 skill against a level 63 mob). The pre-TBC
    /// `80 % x miss + 20 %` the C++ used was shown to be wrong on the Classic PTR; see
    /// <https://github.com/magey/classic-warrior/wiki/Attack-table#miss>.
    pub fn dual_wield_white_miss_chance(&self, wpn_skill: u32) -> f64 {
        self.two_hand_white_miss_chance(wpn_skill) + 0.19
    }

    /// Miss chance for white attacks with a single weapon: 5 % plus 0.1 % per point of defense
    /// over the weapon skill, or 0.2 % per point once the difference exceeds 10 (8 % with 300
    /// skill against a level 63 mob). See
    /// <https://github.com/magey/classic-warrior/wiki/Attack-table#miss>.
    pub fn two_hand_white_miss_chance(&self, wpn_skill: u32) -> f64 {
        let diff = self.defense_minus_wpn_skill(wpn_skill);

        if diff > 10 {
            0.05 + f64::from(diff) * 0.002
        } else {
            0.05 + f64::from(diff) * 0.001
        }
    }

    /// Hit chance from talents and gear that is ignored against a mob whose defense exceeds
    /// the weapon skill by more than 10: 0.2 % per point beyond that (the first 1 % with 300
    /// skill against a level 63 mob, which puts the hit cap at 9 % rather than 8 %). The
    /// counterpart of [`Self::melee_crit_suppression`] for hit.
    pub fn hit_suppression(&self, wpn_skill: u32) -> f64 {
        let diff = self.defense_minus_wpn_skill(wpn_skill);

        if diff > 10 {
            f64::from(diff - 10) * 0.002
        } else {
            0.0
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
    ///
    /// Blizzard confirmed 14 % for creatures 3 levels above the player; the classic-warrior
    /// logs measured ~5 %, 5.75 % and 6.55 % at +0, +1 and +2 and 13.49 % (±0.40) at +3 with
    /// +5 weapon skill. Below +3 the chance follows the dodge formula (5 % plus 0.1 % per point
    /// of defense over the weapon skill); from +3 on it is 14 % adjusted by 0.1 % per point of
    /// weapon skill above or below the character's own level cap. See
    /// <https://github.com/magey/classic-warrior/wiki/Attack-table>.
    pub fn parry_chance(&self, clvl: u32, wpn_skill: u32) -> f64 {
        if self.level_diff(clvl) >= 3 {
            let skill_over_cap = wpn_skill as i32 - clvl as i32 * 5;
            (0.14 - f64::from(skill_over_cap) * 0.001).max(0.0)
        } else {
            let diff = self.defense_minus_wpn_skill(wpn_skill);
            (0.05 + f64::from(diff) * 0.001).max(0.0)
        }
    }

    /// Chance for the target to block (only when attacking from the front): 5 % adjusted by
    /// 0.1 % per point of defense over the weapon skill, but never more than 5 % for a mob
    /// ("mobs cannot block more than 5% of attacks regardless of rating difference"). See
    /// <https://github.com/magey/classic-warrior/wiki/Attack-table#block>.
    pub fn block_chance(&self, wpn_skill: u32) -> f64 {
        let diff = self.defense_minus_wpn_skill(wpn_skill);
        (0.05 + f64::from(diff) * 0.001).clamp(0.0, 0.05)
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

    /// Spell miss chance from the level difference, reduced by `spell_hit` (fraction). Never below
    /// 1 %. From 3 levels above the caster on, each level adds 11 % (royalgiraffe's resist guide,
    /// "Spell hit/miss based on level", PvE column).
    pub fn spell_miss_chance_from_lvl_diff(&self, clvl: u32, spell_hit: f64) -> f64 {
        (self.base_spell_miss_chance(clvl) - spell_hit).max(Self::MIN_SPELL_MISS_CHANCE)
    }

    /// The smallest chance a spell misses whatever the spell hit (the "hit cap").
    pub const MIN_SPELL_MISS_CHANCE: f64 = 0.01;

    /// Spell miss chance from the level difference alone, before spell hit.
    fn base_spell_miss_chance(&self, clvl: u32) -> f64 {
        match self.level_diff(clvl) {
            ..=-3 => 0.01,
            -2 => 0.02,
            -1 => 0.03,
            0 => 0.04,
            1 => 0.05,
            2 => 0.06,
            diff => (0.17 + 0.11 * f64::from(diff - 3)).min(1.0),
        }
    }

    /// Resistance at or above which a spell of a caster of `clvl` is resisted the most:
    /// `5 × level`, as if level 20 below it.
    pub fn resistance_cap(clvl: u32) -> u32 {
        (5 * clvl).max(100)
    }

    /// The resistance a higher-level target has against the non-binary spells of a caster of
    /// `clvl` on top of its own: 8 per level above the caster (24 for a boss), which spell
    /// penetration and curses do not reduce.
    pub fn level_based_resistance(&self, clvl: u32) -> u32 {
        8 * self.level_diff(clvl).max(0) as u32
    }

    /// The share of the resistance cap that `resistance` is, at most 1: what every resist
    /// chance is a function of.
    pub fn resistance_ratio(resistance: f64, clvl: u32) -> f64 {
        (resistance / f64::from(Self::resistance_cap(clvl))).clamp(0.0, 1.0)
    }

    /// Chance that a binary spell (one with any effect besides damage) is fully resisted by
    /// the resistance roll alone: linear from 0 to 75 % at the resistance cap.
    pub fn binary_resist_chance(ratio: f64) -> f64 {
        0.75 * ratio
    }

    /// Chance that a binary spell lands: the level-based hit chance reduced by the resistance
    /// roll, plus the spell hit, at most 99 %. The spell hit is added after the resistance, so
    /// hit above the usual cap still offsets the target's resistance.
    pub fn binary_spell_land_chance(&self, clvl: u32, spell_hit: f64, ratio: f64) -> f64 {
        let hit = 1.0 - self.base_spell_miss_chance(clvl);
        (hit * (1.0 - Self::binary_resist_chance(ratio)) + spell_hit)
            .clamp(0.0, 1.0 - Self::MIN_SPELL_MISS_CHANCE)
    }

    /// Chances of a 0, 25, 50 and 75 % partial resist of a non-binary spell (one that only
    /// deals damage), which is never fully resisted.
    ///
    /// Royalgiraffe's estimates from logs (resist guide, "Partial resist tables", and the
    /// `computeResistOutcomes` of the resistance calculator): piecewise linear in `ratio`
    /// between the breakpoints 0, ⅓, ⅔ and 1, where the average resist is 0, 25, 50 and about
    /// 69 %. Below ⅔ of the cap a full-damage hit keeps at least a 1 % chance; the chances are
    /// then scaled back to sum to 1.
    pub fn partial_resist_chances(ratio: f64) -> [f64; 4] {
        const BREAKPOINTS: [[f64; 4]; 4] = [
            [1.00, 0.00, 0.00, 0.00],
            [0.24, 0.55, 0.18, 0.03],
            [0.00, 0.22, 0.56, 0.22],
            [0.00, 0.04, 0.16, 0.80],
        ];
        let ratio = ratio.clamp(0.0, 1.0);
        let scaled = 3.0 * ratio;
        let segment = (scaled.floor() as usize).min(2);
        let t = scaled - segment as f64;
        let mut chances = [0.0; 4];
        for (i, chance) in chances.iter_mut().enumerate() {
            *chance = BREAKPOINTS[segment][i] * (1.0 - t) + BREAKPOINTS[segment + 1][i] * t;
        }
        if ratio < 2.0 / 3.0 - 1e-6 {
            chances[0] = chances[0].max(0.01);
        }
        let total: f64 = chances.iter().sum();
        chances.map(|chance| chance / total)
    }

    /// Average share of a non-binary spell's damage that `ratio` resists: 75 % of the ratio up
    /// to ⅔ of the cap, 25 % less per point above it (about 69 % at the cap).
    pub fn average_partial_resist(ratio: f64) -> f64 {
        let ratio = ratio.clamp(0.0, 1.0);
        0.75 * ratio - 3.0 / 16.0 * (ratio - 2.0 / 3.0).max(0.0)
    }
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
    fn parry_from_level_and_wpn_skill_diff() {
        let mechanics = Mechanics::new(63);
        assert_close(0.14, mechanics.parry_chance(60, 300));
        assert_close(0.135, mechanics.parry_chance(60, 305));
        assert_close(0.125, mechanics.parry_chance(60, 315));
        assert_close(0.145, mechanics.parry_chance(60, 295));

        assert_close(0.06, Mechanics::new(62).parry_chance(60, 300));
        assert_close(0.055, Mechanics::new(61).parry_chance(60, 300));
        assert_close(0.05, Mechanics::new(60).parry_chance(60, 300));
        assert_close(0.045, Mechanics::new(60).parry_chance(60, 305));
        assert_close(0.045, Mechanics::new(59).parry_chance(60, 300));
    }

    #[test]
    fn two_hand_white_miss() {
        let mechanics = Mechanics::new(63);

        assert_close(0.080, mechanics.two_hand_white_miss_chance(300));
        assert_close(0.078, mechanics.two_hand_white_miss_chance(301));
        assert_close(0.076, mechanics.two_hand_white_miss_chance(302));
        assert_close(0.074, mechanics.two_hand_white_miss_chance(303));
        assert_close(0.072, mechanics.two_hand_white_miss_chance(304));
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
    fn block_is_capped_at_5_percent_against_mobs() {
        let mechanics = Mechanics::new(63);
        assert_close(0.05, mechanics.block_chance(300));
        assert_close(0.05, mechanics.block_chance(315));
        assert_close(0.049, mechanics.block_chance(316));
        assert_close(0.045, mechanics.block_chance(320));

        let mechanics = Mechanics::new(60);
        assert_close(0.05, mechanics.block_chance(300));
        assert_close(0.04, mechanics.block_chance(310));
        assert_close(0.0, mechanics.block_chance(400));
    }

    #[test]
    fn hit_suppression_above_a_defense_difference_of_10() {
        let mechanics = Mechanics::new(63);

        assert_close(0.010, mechanics.hit_suppression(300));
        assert_close(0.008, mechanics.hit_suppression(301));
        assert_close(0.006, mechanics.hit_suppression(302));
        assert_close(0.004, mechanics.hit_suppression(303));
        assert_close(0.002, mechanics.hit_suppression(304));
        assert_close(0.0, mechanics.hit_suppression(305));
        assert_close(0.0, mechanics.hit_suppression(315));
        assert_close(0.0, mechanics.hit_suppression(320));

        assert_close(0.0, Mechanics::new(60).hit_suppression(300));
        assert_close(0.002, Mechanics::new(62).hit_suppression(299));
    }

    #[test]
    fn dual_wield_white_miss() {
        let mechanics = Mechanics::new(63);

        assert_close(0.270, mechanics.dual_wield_white_miss_chance(300));
        assert_close(0.268, mechanics.dual_wield_white_miss_chance(301));
        assert_close(0.266, mechanics.dual_wield_white_miss_chance(302));
        assert_close(0.264, mechanics.dual_wield_white_miss_chance(303));
        assert_close(0.262, mechanics.dual_wield_white_miss_chance(304));
        assert_close(0.250, mechanics.dual_wield_white_miss_chance(305));
        assert_close(0.249, mechanics.dual_wield_white_miss_chance(306));
        assert_close(0.248, mechanics.dual_wield_white_miss_chance(307));
        assert_close(0.247, mechanics.dual_wield_white_miss_chance(308));
        assert_close(0.246, mechanics.dual_wield_white_miss_chance(309));
        assert_close(0.245, mechanics.dual_wield_white_miss_chance(310));
        assert_close(0.244, mechanics.dual_wield_white_miss_chance(311));
        assert_close(0.243, mechanics.dual_wield_white_miss_chance(312));
        assert_close(0.242, mechanics.dual_wield_white_miss_chance(313));
        assert_close(0.241, mechanics.dual_wield_white_miss_chance(314));
        assert_close(0.240, mechanics.dual_wield_white_miss_chance(315));
        assert_close(0.235, mechanics.dual_wield_white_miss_chance(320));
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

    /// Royalgiraffe's resist guide, "Spell hit/miss based on level" (PvE), which agrees with
    /// ClassicSim's `TestAttackTables::test_magic_attack_table` up to 5 levels above.
    #[test]
    fn spell_miss_chance_from_lvl_diff() {
        let mechanics = Mechanics::new(63);

        assert_close(0.17, mechanics.spell_miss_chance_from_lvl_diff(60, 0.0));
        assert_close(0.07, mechanics.spell_miss_chance_from_lvl_diff(60, 0.10));
        // The hit cap against a boss is 16 %: 1 % of the spells always miss.
        assert_close(0.01, mechanics.spell_miss_chance_from_lvl_diff(60, 0.16));
        assert_close(0.01, mechanics.spell_miss_chance_from_lvl_diff(60, 0.20));
        assert_close(0.04, mechanics.spell_miss_chance_from_lvl_diff(63, 0.0));
        assert_close(0.06, mechanics.spell_miss_chance_from_lvl_diff(61, 0.0));
        assert_close(0.28, mechanics.spell_miss_chance_from_lvl_diff(59, 0.0));
        assert_close(0.39, mechanics.spell_miss_chance_from_lvl_diff(58, 0.0));
        assert_close(0.50, mechanics.spell_miss_chance_from_lvl_diff(57, 0.0));
        assert_close(0.01, mechanics.spell_miss_chance_from_lvl_diff(70, 0.0));

        for (target_level, miss) in [(62, 0.06), (61, 0.05), (60, 0.04), (59, 0.03), (58, 0.02)] {
            let mechanics = Mechanics::new(target_level);
            assert_close(miss, mechanics.spell_miss_chance_from_lvl_diff(60, 0.0));
        }
        assert_close(
            0.01,
            Mechanics::new(57).spell_miss_chance_from_lvl_diff(60, 0.0),
        );
        assert_close(
            0.01,
            Mechanics::new(56).spell_miss_chance_from_lvl_diff(60, 0.0),
        );
    }

    #[test]
    fn resistance_cap_and_level_based_resistance() {
        assert_eq!(Mechanics::resistance_cap(60), 300);
        assert_eq!(Mechanics::resistance_cap(63), 315);
        assert_eq!(Mechanics::resistance_cap(10), 100);

        let boss = Mechanics::new(63);
        assert_eq!(boss.level_based_resistance(60), 24);
        assert_eq!(boss.level_based_resistance(63), 0);
        assert_eq!(boss.level_based_resistance(70), 0);

        assert_close(0.5, Mechanics::resistance_ratio(150.0, 60));
        assert_close(1.0, Mechanics::resistance_ratio(400.0, 60));
        assert_close(0.0, Mechanics::resistance_ratio(0.0, 60));
    }

    /// The worked examples of royalgiraffe's resist guide, "Binary spells".
    #[test]
    fn binary_spell_land_chance() {
        assert_close(0.0, Mechanics::binary_resist_chance(0.0));
        assert_close(0.75, Mechanics::binary_resist_chance(1.0));

        // Same level, 100 resistance out of the 300 cap: 96 % × 75 % = 72 %.
        let same_level = Mechanics::new(60);
        let ratio = Mechanics::resistance_ratio(100.0, 60);
        assert_close(0.72, same_level.binary_spell_land_chance(60, 0.0, ratio));
        // Level 70, 15 % spell hit, 70 resistance out of 350: 96 % × 85 % + 15 % = 96.6 %.
        let level_70 = Mechanics::new(70);
        let ratio = Mechanics::resistance_ratio(70.0, 70);
        assert_close(0.966, level_70.binary_spell_land_chance(70, 0.15, ratio));
        // Hit above the cap offsets the resistance, up to 99 %.
        assert_close(0.99, level_70.binary_spell_land_chance(70, 0.30, ratio));
        // Without resistance it is the ordinary hit chance.
        let boss = Mechanics::new(63);
        assert_close(0.83, boss.binary_spell_land_chance(60, 0.0, 0.0));
        assert_close(0.99, boss.binary_spell_land_chance(60, 0.20, 0.0));
    }

    fn assert_chances(expected: [f64; 4], ratio: f64) {
        let chances = Mechanics::partial_resist_chances(ratio);
        for (expected, actual) in expected.iter().zip(chances) {
            assert_abs_diff_eq!(*expected, actual, epsilon = 1e-9);
        }
    }

    /// Royalgiraffe's partial resist table (resist calculator, `computeResistOutcomes`) at its
    /// breakpoints, and the guide's example at 20 % of the cap: 54 / 33 / 11 / 2 %.
    #[test]
    fn partial_resist_chances() {
        assert_chances([1.0, 0.0, 0.0, 0.0], 0.0);
        assert_chances([0.24, 0.55, 0.18, 0.03], 1.0 / 3.0);
        assert_chances([0.0, 0.22, 0.56, 0.22], 2.0 / 3.0);
        assert_chances([0.0, 0.04, 0.16, 0.80], 1.0);
        assert_chances([0.0, 0.04, 0.16, 0.80], 1.5);
        assert_chances([0.544, 0.33, 0.108, 0.018], 0.2);

        // A full-damage hit is possible up to just below ⅔ of the cap: 1 % at 209 resistance
        // against a level 63 caster, none at 210.
        let at_209 = Mechanics::partial_resist_chances(Mechanics::resistance_ratio(209.0, 63));
        assert_abs_diff_eq!(0.01, at_209[0], epsilon = 0.001);
        let at_210 = Mechanics::partial_resist_chances(Mechanics::resistance_ratio(210.0, 63));
        assert_abs_diff_eq!(0.0, at_210[0], epsilon = 1e-9);

        for step in 0..=100 {
            let chances = Mechanics::partial_resist_chances(f64::from(step) / 100.0);
            assert_abs_diff_eq!(1.0, chances.iter().sum::<f64>(), epsilon = 1e-9);
        }
    }

    /// The table's average resist follows the guide's two-piece formula: 75 % of the ratio up to
    /// ⅔ of the cap, 69 % at the cap; a boss's 24 level-based resistance costs 6 %.
    #[test]
    fn average_partial_resist_matches_the_table() {
        let table_average = |ratio: f64| {
            let chances = Mechanics::partial_resist_chances(ratio);
            chances
                .iter()
                .enumerate()
                .map(|(i, chance)| chance * 0.25 * i as f64)
                .sum::<f64>()
        };
        for step in 0..=60 {
            let ratio = f64::from(step) / 60.0;
            assert_abs_diff_eq!(
                Mechanics::average_partial_resist(ratio),
                table_average(ratio),
                epsilon = 0.01
            );
        }
        assert_close(0.25, Mechanics::average_partial_resist(1.0 / 3.0));
        assert_close(0.5, Mechanics::average_partial_resist(2.0 / 3.0));
        assert_close(0.6875, Mechanics::average_partial_resist(1.0));

        let boss = Mechanics::new(63);
        let level_based = f64::from(boss.level_based_resistance(60));
        let ratio = Mechanics::resistance_ratio(level_based, 60);
        assert_abs_diff_eq!(0.06, table_average(ratio), epsilon = 1e-9);
    }
}
