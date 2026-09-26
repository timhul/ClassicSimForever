//! Attack tables. Port of `CombatRoll/AttackTables/*`.
//!
//! All chances are stored as ranges out of 10 000 (`100` = 1%). Rolls are integers in `0..10000`.
//! Tables that need a second roll (the two-roll crit check of special attacks, block crits) take the
//! generator as a parameter instead of holding a pointer to it like the C++ classes did.

use crate::mechanics::Mechanics;
use crate::rng::Random;

use super::{MagicAttackResult, MagicResistResult, PhysicalAttackResult};

/// The size of the roll space; rolls must be below this value.
pub const ROLL_RANGE: u32 = 10_000;

/// Converts a fractional chance to a range out of [`ROLL_RANGE`], rounding like the C++ code.
pub fn chance_to_range(chance: f64) -> u32 {
    (chance * f64::from(ROLL_RANGE)).round() as u32
}

/// Which avoidance outcomes an attack can have. Spells with `CANNOT_BE_DODGED` etc. exclude entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncludedOutcomes {
    pub dodge: bool,
    pub parry: bool,
    pub block: bool,
    pub miss: bool,
}

impl IncludedOutcomes {
    /// Every avoidance outcome is possible.
    pub const ALL: IncludedOutcomes = IncludedOutcomes {
        dodge: true,
        parry: true,
        block: true,
        miss: true,
    };

    /// Nothing is avoided: the attack hits or crits (the strikes of a landed attack).
    pub const NONE: IncludedOutcomes = IncludedOutcomes {
        dodge: false,
        parry: false,
        block: false,
        miss: false,
    };
}

impl Default for IncludedOutcomes {
    fn default() -> Self {
        Self::ALL
    }
}

fn check_roll(roll: u32) {
    assert!(roll < ROLL_RANGE, "Roll outside range");
}

/// Single-roll table for white (auto attack) melee hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeleeWhiteHitTable {
    pub wpn_skill: u32,
    miss_range: u32,
    dodge_range: u32,
    parry_range: u32,
    glancing_range: u32,
    block_range: u32,
}

impl MeleeWhiteHitTable {
    /// `miss` is a range out of 10 000; the other chances are fractions.
    pub fn new(
        wpn_skill: u32,
        miss: u32,
        dodge: f64,
        parry: f64,
        glancing: f64,
        block: f64,
    ) -> Self {
        Self {
            wpn_skill,
            miss_range: miss,
            dodge_range: chance_to_range(dodge),
            parry_range: chance_to_range(parry),
            glancing_range: chance_to_range(glancing),
            block_range: chance_to_range(block),
        }
    }

    /// Resolves `roll` against the table. `crit_chance` is a range out of 10 000.
    pub fn get_outcome(
        &self,
        random: &mut Random,
        roll: u32,
        crit_chance: u32,
        included: IncludedOutcomes,
    ) -> PhysicalAttackResult {
        check_roll(roll);

        let mut range = 0;

        if included.miss && roll < self.miss_range {
            return PhysicalAttackResult::Miss;
        }
        range += if included.miss { self.miss_range } else { 0 };

        if included.dodge && roll < range + self.dodge_range {
            return PhysicalAttackResult::Dodge;
        }
        range += if included.dodge { self.dodge_range } else { 0 };

        if included.parry && roll < range + self.parry_range {
            return PhysicalAttackResult::Parry;
        }
        range += if included.parry { self.parry_range } else { 0 };

        if roll < range + self.glancing_range {
            return PhysicalAttackResult::Glancing;
        }
        range += self.glancing_range;

        if included.block && roll < range + self.block_range {
            if random.get_roll() < range + crit_chance {
                return PhysicalAttackResult::BlockCritical;
            }
            return PhysicalAttackResult::Block;
        }
        range += if included.block { self.block_range } else { 0 };

        if roll < range + crit_chance {
            return PhysicalAttackResult::Critical;
        }

        PhysicalAttackResult::Hit
    }

    pub fn update_miss_chance(&mut self, miss: u32) {
        self.miss_range = miss;
    }

    pub fn update_dodge_chance(&mut self, dodge: f64) {
        self.dodge_range = chance_to_range(dodge);
    }

    pub fn update_parry_chance(&mut self, parry: f64) {
        self.parry_range = chance_to_range(parry);
    }

    pub fn update_glancing_chance(&mut self, glancing: f64) {
        self.glancing_range = chance_to_range(glancing);
    }

    pub fn update_block_chance(&mut self, block: f64) {
        self.block_range = chance_to_range(block);
    }
}

/// Two-roll table for yellow (special ability) melee hits: avoidance on the first roll, crit on a
/// second independent roll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeleeSpecialTable {
    pub wpn_skill: u32,
    miss_range: u32,
    dodge_range: u32,
    parry_range: u32,
    block_range: u32,
}

impl MeleeSpecialTable {
    /// `miss` is a range out of 10 000; the other chances are fractions.
    pub fn new(wpn_skill: u32, miss: u32, dodge: f64, parry: f64, block: f64) -> Self {
        Self {
            wpn_skill,
            miss_range: miss,
            dodge_range: chance_to_range(dodge),
            parry_range: chance_to_range(parry),
            block_range: chance_to_range(block),
        }
    }

    /// Resolves `roll` against the table. `crit_chance` is a range out of 10 000.
    pub fn get_outcome(
        &self,
        random: &mut Random,
        roll: u32,
        crit_chance: u32,
        included: IncludedOutcomes,
    ) -> PhysicalAttackResult {
        check_roll(roll);

        let mut range = 0;

        if included.miss && roll < self.miss_range {
            return PhysicalAttackResult::Miss;
        }
        range += if included.miss { self.miss_range } else { 0 };

        if included.dodge && roll < range + self.dodge_range {
            return PhysicalAttackResult::Dodge;
        }
        range += if included.dodge { self.dodge_range } else { 0 };

        if included.parry && roll < range + self.parry_range {
            return PhysicalAttackResult::Parry;
        }
        range += if included.parry { self.parry_range } else { 0 };

        if included.block && roll < range + self.block_range {
            // Ported as-is: the C++ code scales the (already scaled) crit chance by 10 000 here.
            let block_crit_range = (f64::from(crit_chance) * f64::from(ROLL_RANGE)).round();
            if f64::from(random.get_roll()) < block_crit_range {
                return PhysicalAttackResult::BlockCritical;
            }
            return PhysicalAttackResult::Block;
        }

        if random.get_roll() < crit_chance {
            return PhysicalAttackResult::Critical;
        }

        PhysicalAttackResult::Hit
    }

    pub fn update_miss_chance(&mut self, miss: u32) {
        self.miss_range = miss;
    }

    pub fn update_dodge_chance(&mut self, dodge: f64) {
        self.dodge_range = chance_to_range(dodge);
    }

    pub fn update_parry_chance(&mut self, parry: f64) {
        self.parry_range = chance_to_range(parry);
    }

    pub fn update_block_chance(&mut self, block: f64) {
        self.block_range = chance_to_range(block);
    }
}

/// Hit and resist table for a magic school.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MagicAttackTable {
    miss_range: u32,
    full_resist: u32,
    partial_75: u32,
    partial_50: u32,
    partial_25: u32,
}

impl MagicAttackTable {
    /// `spell_hit` is a range out of 10 000; `target_resistance` is the effective resistance value.
    pub fn new(mechanics: &Mechanics, clvl: u32, spell_hit: u32, target_resistance: u32) -> Self {
        let mut table = Self {
            miss_range: 0,
            full_resist: 0,
            partial_75: 0,
            partial_50: 0,
            partial_25: 0,
        };
        table.update_miss_chance(mechanics, clvl, spell_hit);
        table.update_target_resistance(target_resistance);
        table
    }

    /// Resolves the hit roll; crits use a second independent roll.
    pub fn get_hit_outcome(
        &self,
        random: &mut Random,
        roll: u32,
        crit_chance: u32,
    ) -> MagicAttackResult {
        if roll < self.miss_range {
            return MagicAttackResult::Miss;
        }

        if random.get_roll() < crit_chance {
            return MagicAttackResult::Critical;
        }

        MagicAttackResult::Hit
    }

    pub fn get_resist_outcome(&self, roll: u32) -> MagicResistResult {
        if roll < self.full_resist {
            MagicResistResult::FullResist
        } else if roll < self.partial_75 {
            MagicResistResult::Partial75
        } else if roll < self.partial_50 {
            MagicResistResult::Partial50
        } else if roll < self.partial_25 {
            MagicResistResult::Partial25
        } else {
            MagicResistResult::NoResist
        }
    }

    pub fn update_miss_chance(&mut self, mechanics: &Mechanics, clvl: u32, spell_hit: u32) {
        let spell_hit = f64::from(spell_hit) / f64::from(ROLL_RANGE);
        let miss_chance = mechanics.spell_miss_chance_from_lvl_diff(clvl, spell_hit);
        self.miss_range = chance_to_range(miss_chance);
    }

    pub fn update_target_resistance(&mut self, target_resistance: u32) {
        self.full_resist = chance_to_range(Mechanics::full_resist_chance(target_resistance));
        self.partial_75 =
            self.full_resist + chance_to_range(Mechanics::partial_75_chance(target_resistance));
        self.partial_50 =
            self.partial_75 + chance_to_range(Mechanics::partial_50_chance(target_resistance));
        self.partial_25 =
            self.partial_50 + chance_to_range(Mechanics::partial_25_chance(target_resistance));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random() -> Random {
        Random::from_seed(0, ROLL_RANGE, 1)
    }

    #[test]
    fn white_hit_table() {
        let mut random = random();
        let all = IncludedOutcomes::ALL;

        let table = MeleeWhiteHitTable::new(300, 0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(
            table.get_outcome(&mut random, 0, 0, all),
            PhysicalAttackResult::Hit
        );
        assert_eq!(
            table.get_outcome(&mut random, 9999, 0, all),
            PhysicalAttackResult::Hit
        );

        let table = MeleeWhiteHitTable::new(300, 1, 0.0001, 0.0001, 0.0001, 0.0001);
        assert_eq!(
            table.get_outcome(&mut random, 0, 0, all),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            table.get_outcome(&mut random, 1, 0, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 2, 0, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 3, 0, all),
            PhysicalAttackResult::Glancing
        );
        assert_eq!(
            table.get_outcome(&mut random, 4, 0, all),
            PhysicalAttackResult::Block
        );
        assert_eq!(
            table.get_outcome(&mut random, 5, 1, all),
            PhysicalAttackResult::Critical
        );
        assert_eq!(
            table.get_outcome(&mut random, 6, 1, all),
            PhysicalAttackResult::Hit
        );
        assert_eq!(
            table.get_outcome(&mut random, 9999, 0, all),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    fn white_hit_table_excluded_outcomes_shift_ranges() {
        let mut random = random();
        let table = MeleeWhiteHitTable::new(300, 100, 0.01, 0.01, 0.0, 0.0);

        let no_avoidance = IncludedOutcomes {
            dodge: false,
            parry: false,
            block: false,
            miss: false,
        };
        assert_eq!(
            table.get_outcome(&mut random, 0, 0, no_avoidance),
            PhysicalAttackResult::Hit
        );
        assert_eq!(
            table.get_outcome(&mut random, 299, 300, no_avoidance),
            PhysicalAttackResult::Critical
        );

        let no_dodge = IncludedOutcomes {
            dodge: false,
            ..IncludedOutcomes::ALL
        };
        assert_eq!(
            table.get_outcome(&mut random, 99, 0, no_dodge),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            table.get_outcome(&mut random, 100, 0, no_dodge),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 200, 0, no_dodge),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    fn white_hit_table_updates() {
        let mut random = random();
        let all = IncludedOutcomes::ALL;
        let mut table = MeleeWhiteHitTable::new(300, 0, 0.0, 0.0, 0.0, 0.0);

        table.update_miss_chance(500);
        assert_eq!(
            table.get_outcome(&mut random, 499, 0, all),
            PhysicalAttackResult::Miss
        );
        table.update_dodge_chance(0.05);
        assert_eq!(
            table.get_outcome(&mut random, 999, 0, all),
            PhysicalAttackResult::Dodge
        );
        table.update_parry_chance(0.05);
        assert_eq!(
            table.get_outcome(&mut random, 1499, 0, all),
            PhysicalAttackResult::Parry
        );
        table.update_glancing_chance(0.05);
        assert_eq!(
            table.get_outcome(&mut random, 1999, 0, all),
            PhysicalAttackResult::Glancing
        );
        table.update_block_chance(0.05);
        assert_eq!(
            table.get_outcome(&mut random, 2499, 0, all),
            PhysicalAttackResult::Block
        );
        assert_eq!(
            table.get_outcome(&mut random, 2500, 0, all),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    #[should_panic(expected = "Roll outside range")]
    fn white_hit_table_rejects_roll_out_of_range() {
        let mut random = random();
        let table = MeleeWhiteHitTable::new(300, 0, 0.0, 0.0, 0.0, 0.0);
        table.get_outcome(&mut random, 10_000, 0, IncludedOutcomes::ALL);
    }

    #[test]
    fn special_hit_table() {
        let mut random = random();
        let all = IncludedOutcomes::ALL;

        let table = MeleeSpecialTable::new(300, 0, 0.0, 0.0, 0.0);
        assert_eq!(
            table.get_outcome(&mut random, 0, 0, all),
            PhysicalAttackResult::Hit
        );

        let table = MeleeSpecialTable::new(300, 1, 0.0001, 0.0001, 0.0001);
        assert_eq!(
            table.get_outcome(&mut random, 0, 10000, all),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            table.get_outcome(&mut random, 1, 10000, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 2, 10000, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 3, 10000, all),
            PhysicalAttackResult::BlockCritical
        );
        assert_eq!(
            table.get_outcome(&mut random, 4, 10000, all),
            PhysicalAttackResult::Critical
        );
        assert_eq!(
            table.get_outcome(&mut random, 9999, 10000, all),
            PhysicalAttackResult::Critical
        );
        assert_eq!(
            table.get_outcome(&mut random, 3, 0, all),
            PhysicalAttackResult::Block
        );
        assert_eq!(
            table.get_outcome(&mut random, 4, 0, all),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    fn special_hit_table_updates() {
        let mut random = random();
        let all = IncludedOutcomes::ALL;
        let mut table = MeleeSpecialTable::new(300, 0, 0.0, 0.0, 0.0);

        table.update_miss_chance(500);
        table.update_dodge_chance(0.05);
        table.update_parry_chance(0.05);
        table.update_block_chance(0.05);
        assert_eq!(
            table.get_outcome(&mut random, 499, 0, all),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            table.get_outcome(&mut random, 999, 0, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 1499, 0, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 1999, 0, all),
            PhysicalAttackResult::Block
        );
        assert_eq!(
            table.get_outcome(&mut random, 2000, 0, all),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    fn magic_attack_table() {
        let mut random = random();
        let mut mechanics = Mechanics::new(63);
        let mut table = MagicAttackTable::new(&mechanics, 60, 0, 0);

        let expect_miss_below = |table: &MagicAttackTable, random: &mut Random, miss: u32| {
            assert_eq!(table.get_hit_outcome(random, 0, 0), MagicAttackResult::Miss);
            assert_eq!(
                table.get_hit_outcome(random, miss - 1, 0),
                MagicAttackResult::Miss
            );
            assert_eq!(
                table.get_hit_outcome(random, miss, 0),
                MagicAttackResult::Hit
            );
            assert_eq!(
                table.get_hit_outcome(random, 9999, 0),
                MagicAttackResult::Hit
            );
        };

        expect_miss_below(&table, &mut random, 1700);

        for (target_level, miss) in [
            (62, 600),
            (61, 500),
            (60, 400),
            (59, 300),
            (58, 200),
            (57, 100),
            (56, 100),
        ] {
            mechanics.set_target_level(target_level);
            table.update_miss_chance(&mechanics, 60, 0);
            expect_miss_below(&table, &mut random, miss);
        }
    }

    #[test]
    fn magic_attack_table_crit_uses_second_roll() {
        let mut random = random();
        let table = MagicAttackTable::new(&Mechanics::new(63), 60, 0, 0);
        assert_eq!(
            table.get_hit_outcome(&mut random, 9999, 10_000),
            MagicAttackResult::Critical
        );
        assert_eq!(
            table.get_hit_outcome(&mut random, 9999, 0),
            MagicAttackResult::Hit
        );
    }

    #[test]
    fn magic_resist_outcomes() {
        let mut table = MagicAttackTable::new(&Mechanics::new(63), 60, 0, 0);
        assert_eq!(table.get_resist_outcome(0), MagicResistResult::NoResist);
        assert_eq!(table.get_resist_outcome(9999), MagicResistResult::NoResist);

        // Resistance 150: full 1%, partial75 11%, partial50 37%, partial25 39%.
        table.update_target_resistance(150);
        assert_eq!(table.get_resist_outcome(0), MagicResistResult::FullResist);
        assert_eq!(table.get_resist_outcome(99), MagicResistResult::FullResist);
        assert_eq!(table.get_resist_outcome(100), MagicResistResult::Partial75);
        assert_eq!(table.get_resist_outcome(1199), MagicResistResult::Partial75);
        assert_eq!(table.get_resist_outcome(1200), MagicResistResult::Partial50);
        assert_eq!(table.get_resist_outcome(4899), MagicResistResult::Partial50);
        assert_eq!(table.get_resist_outcome(4900), MagicResistResult::Partial25);
        assert_eq!(table.get_resist_outcome(8799), MagicResistResult::Partial25);
        assert_eq!(table.get_resist_outcome(8800), MagicResistResult::NoResist);
    }
}
