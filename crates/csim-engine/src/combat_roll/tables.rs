//! Attack tables. Port of `CombatRoll/AttackTables/*`.
//!
//! All chances are stored as ranges out of 10 000 (`100` = 1%). Rolls are integers in `0..10000`.
//! Tables that need a second roll (the two-roll crit check of special attacks, block crits) take the
//! generator as a parameter instead of holding a pointer to it like the C++ classes did.

use crate::mechanics::Mechanics;
use crate::rng::Random;

use super::{MagicAttackResult, MagicResistResult, PhysicalAttackResult, SpellRoll};

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

    /// The miss range, out of 10 000.
    pub fn miss_range(&self) -> u32 {
        self.miss_range
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

    /// The miss range, out of 10 000.
    pub fn miss_range(&self) -> u32 {
        self.miss_range
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

/// Cumulative ranges of the 75, 50 and 25 % partial resists of a non-binary spell; the rest of
/// the roll space is no resist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PartialResistRanges {
    partial_75: u32,
    partial_50: u32,
    partial_25: u32,
}

impl PartialResistRanges {
    /// The ranges for a resistance of `ratio` of the cap.
    fn new(ratio: f64) -> Self {
        let [_, p25, p50, p75] = Mechanics::partial_resist_chances(ratio);
        Self {
            partial_75: chance_to_range(p75),
            partial_50: chance_to_range(p75 + p50),
            partial_25: chance_to_range(p75 + p50 + p25),
        }
    }

    fn outcome(&self, roll: u32) -> MagicResistResult {
        check_roll(roll);
        if roll < self.partial_75 {
            MagicResistResult::Partial75
        } else if roll < self.partial_50 {
            MagicResistResult::Partial50
        } else if roll < self.partial_25 {
            MagicResistResult::Partial25
        } else {
            MagicResistResult::NoResist
        }
    }
}

/// Hit and resist table of one magic school, after royalgiraffe's resist guide
/// (<https://royalgiraffe.github.io/resist-guide>).
///
/// A **non-binary** spell (it only deals damage) rolls its hit against the level-based miss
/// chance less the spell hit (at least 1 %), then, if it landed, a partial resist of 0, 25, 50
/// or 75 % from the target's resistance plus its level-based resistance (8 per level above
/// the caster), as a share of the resistance cap. It is never fully resisted.
///
/// A **binary** spell (any other effect) either lands or not in one roll that combines the
/// level-based hit chance, the resistance (without the level-based part) and the spell hit.
///
/// The ticks of a damage-over-time without direct damage roll their partial resist against a
/// tenth of the resistance (Vanilla's special DoT rule); other ticks roll like direct damage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MagicAttackTable {
    miss_range: u32,
    binary_miss_range: u32,
    /// The binary miss range plus its full resist range.
    binary_fail_range: u32,
    partial: PartialResistRanges,
    periodic_partial: PartialResistRanges,
}

impl MagicAttackTable {
    /// `spell_hit` is a range out of 10 000; `target_resistance` the target's resistance after
    /// spell penetration, without the level-based part. A table of an unresistable school
    /// (physical) only misses.
    pub fn new(
        mechanics: &Mechanics,
        clvl: u32,
        spell_hit: u32,
        target_resistance: u32,
        resistible: bool,
    ) -> Self {
        let spell_hit = f64::from(spell_hit) / f64::from(ROLL_RANGE);
        let miss_chance = mechanics.spell_miss_chance_from_lvl_diff(clvl, spell_hit);

        let (resistance, level_based) = if resistible {
            (
                f64::from(target_resistance),
                f64::from(mechanics.level_based_resistance(clvl)),
            )
        } else {
            (0.0, 0.0)
        };
        let direct_ratio = Mechanics::resistance_ratio(resistance + level_based, clvl);
        let periodic_ratio = Mechanics::resistance_ratio((resistance + level_based) / 10.0, clvl);

        let binary_ratio = Mechanics::resistance_ratio(resistance, clvl);
        let binary_fail = 1.0 - mechanics.binary_spell_land_chance(clvl, spell_hit, binary_ratio);
        let binary_miss = miss_chance.min(binary_fail);

        Self {
            miss_range: chance_to_range(miss_chance),
            binary_miss_range: chance_to_range(binary_miss),
            binary_fail_range: chance_to_range(binary_fail),
            partial: PartialResistRanges::new(direct_ratio),
            periodic_partial: PartialResistRanges::new(periodic_ratio),
        }
    }

    /// Resolves the hit roll of a non-binary spell; crits use a second independent roll.
    pub fn get_hit_outcome(
        &self,
        random: &mut Random,
        roll: u32,
        crit_chance: u32,
    ) -> MagicAttackResult {
        check_roll(roll);
        if roll < self.miss_range {
            return MagicAttackResult::Miss;
        }

        if random.get_roll() < crit_chance {
            return MagicAttackResult::Critical;
        }

        MagicAttackResult::Hit
    }

    /// Resolves the single hit-and-resist roll of a binary spell; crits use a second
    /// independent roll.
    pub fn get_binary_outcome(
        &self,
        random: &mut Random,
        roll: u32,
        crit_chance: u32,
    ) -> SpellRoll {
        check_roll(roll);
        if roll < self.binary_miss_range {
            return SpellRoll::MISS;
        }
        if roll < self.binary_fail_range {
            return SpellRoll::FULL_RESIST;
        }
        let result = if random.get_roll() < crit_chance {
            MagicAttackResult::Critical
        } else {
            MagicAttackResult::Hit
        };
        SpellRoll {
            result,
            resist: MagicResistResult::NoResist,
        }
    }

    /// The partial resist of a landed non-binary spell's direct damage (or of a tick of a DoT
    /// with direct damage).
    pub fn get_resist_outcome(&self, roll: u32) -> MagicResistResult {
        self.partial.outcome(roll)
    }

    /// The partial resist of a tick of a damage-over-time without direct damage.
    pub fn get_periodic_resist_outcome(&self, roll: u32) -> MagicResistResult {
        self.periodic_partial.outcome(roll)
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

    /// Port of `TestAttackTables::test_magic_attack_table`: the miss range from the level
    /// difference.
    #[test]
    fn magic_attack_table() {
        let mut random = random();

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

        for (target_level, miss) in [
            (63, 1700),
            (62, 600),
            (61, 500),
            (60, 400),
            (59, 300),
            (58, 200),
            (57, 100),
            (56, 100),
        ] {
            let table = MagicAttackTable::new(&Mechanics::new(target_level), 60, 0, 0, true);
            expect_miss_below(&table, &mut random, miss);
        }

        // Spell hit lowers it to the 1 % floor.
        let table = MagicAttackTable::new(&Mechanics::new(63), 60, 1000, 0, true);
        expect_miss_below(&table, &mut random, 700);
        let table = MagicAttackTable::new(&Mechanics::new(63), 60, 2000, 0, true);
        expect_miss_below(&table, &mut random, 100);
    }

    #[test]
    fn magic_attack_table_crit_uses_second_roll() {
        let mut random = random();
        let table = MagicAttackTable::new(&Mechanics::new(63), 60, 0, 0, true);
        assert_eq!(
            table.get_hit_outcome(&mut random, 9999, 10_000),
            MagicAttackResult::Critical
        );
        assert_eq!(
            table.get_hit_outcome(&mut random, 9999, 0),
            MagicAttackResult::Hit
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 9999, 10_000).result,
            MagicAttackResult::Critical
        );
    }

    #[test]
    fn magic_resist_outcomes() {
        // Against a level 60 target nothing is resisted without resistance.
        let table = MagicAttackTable::new(&Mechanics::new(60), 60, 0, 0, true);
        for roll in [0, 5000, 9999] {
            assert_eq!(table.get_resist_outcome(roll), MagicResistResult::NoResist);
            assert_eq!(
                table.get_periodic_resist_outcome(roll),
                MagicResistResult::NoResist
            );
        }

        // 100 resistance out of the 300 cap: 24 / 55 / 18 / 3 %, never a full resist.
        let table = MagicAttackTable::new(&Mechanics::new(60), 60, 0, 100, true);
        assert_eq!(table.get_resist_outcome(0), MagicResistResult::Partial75);
        assert_eq!(table.get_resist_outcome(299), MagicResistResult::Partial75);
        assert_eq!(table.get_resist_outcome(300), MagicResistResult::Partial50);
        assert_eq!(table.get_resist_outcome(2099), MagicResistResult::Partial50);
        assert_eq!(table.get_resist_outcome(2100), MagicResistResult::Partial25);
        assert_eq!(table.get_resist_outcome(7599), MagicResistResult::Partial25);
        assert_eq!(table.get_resist_outcome(7600), MagicResistResult::NoResist);

        // A pure DoT's ticks see a tenth of it: 10 of 300.
        let [_, p25, p50, p75] = Mechanics::partial_resist_chances(10.0 / 300.0);
        let first_no_resist = chance_to_range(p75 + p50 + p25);
        assert_eq!(
            table.get_periodic_resist_outcome(first_no_resist - 1),
            MagicResistResult::Partial25
        );
        assert_eq!(
            table.get_periodic_resist_outcome(first_no_resist),
            MagicResistResult::NoResist
        );
    }

    /// A boss adds 24 level-based resistance against a level 60 caster: 8 % of the cap.
    #[test]
    fn level_based_resistance_of_a_boss() {
        let table = MagicAttackTable::new(&Mechanics::new(63), 60, 0, 0, true);
        let [_, p25, p50, p75] = Mechanics::partial_resist_chances(0.08);
        assert_eq!(
            table.get_resist_outcome(chance_to_range(p75) - 1),
            MagicResistResult::Partial75
        );
        assert_eq!(
            table.get_resist_outcome(chance_to_range(p75 + p50 + p25)),
            MagicResistResult::NoResist
        );
        assert_eq!(
            table.get_resist_outcome(chance_to_range(p75 + p50 + p25) - 1),
            MagicResistResult::Partial25
        );

        // Not for an unresistable school.
        let physical = MagicAttackTable::new(&Mechanics::new(63), 60, 0, 100, false);
        assert_eq!(physical.get_resist_outcome(0), MagicResistResult::NoResist);
    }

    /// Binary spells: one roll of miss, then full resist, then land; the level-based resistance
    /// does not apply.
    #[test]
    fn binary_outcomes() {
        let mut random = random();
        let mechanics = Mechanics::new(60);
        // Same level, 100 resistance of 300: lands 72 %, misses 4 %, resisted 24 %.
        let table = MagicAttackTable::new(&mechanics, 60, 0, 100, true);
        assert_eq!(
            table.get_binary_outcome(&mut random, 399, 0),
            SpellRoll::MISS
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 400, 0),
            SpellRoll::FULL_RESIST
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 2799, 0),
            SpellRoll::FULL_RESIST
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 2800, 0),
            SpellRoll::HIT
        );

        // Against a boss without resistance: only the 17 % miss.
        let table = MagicAttackTable::new(&Mechanics::new(63), 60, 0, 0, true);
        assert_eq!(
            table.get_binary_outcome(&mut random, 1699, 0),
            SpellRoll::MISS
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 1700, 0),
            SpellRoll::HIT
        );

        // Spell hit above the cap offsets resistance: 96 % × 75 % + 20 % = 92 %.
        let table = MagicAttackTable::new(&mechanics, 60, 2000, 100, true);
        assert_eq!(
            table.get_binary_outcome(&mut random, 99, 0),
            SpellRoll::MISS
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 799, 0),
            SpellRoll::FULL_RESIST
        );
        assert_eq!(
            table.get_binary_outcome(&mut random, 800, 0),
            SpellRoll::HIT
        );
    }
}
