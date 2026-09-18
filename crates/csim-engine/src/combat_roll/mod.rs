//! Combat rolls: turning character/target state into hit outcomes.
//!
//! Port of `CombatRoll/CombatRoll.*` and the result enums in `PhysicalAttackResult.h` /
//! `MagicAttackResult.h`.
//!
//! The C++ `CombatRoll` reached back into the character for hit chance, level, dual-wield state and
//! the ruleset. Here the caller passes that information in a [`RollContext`] (or
//! [`MagicRollContext`]) so the roll module does not depend on the character.
//!
//! Attack tables are cached per weapon skill (and facing) because building one is comparatively
//! expensive; they are dropped between sets of iterations, or when the stats they were built from
//! change (`update_*` methods).

pub mod tables;

use std::collections::HashMap;

use crate::magic_school::MagicSchool;
use crate::mechanics::Mechanics;
use crate::rng::Random;

pub use tables::{
    chance_to_range, IncludedOutcomes, MagicAttackTable, MeleeSpecialTable, MeleeWhiteHitTable,
    ROLL_RANGE,
};

/// Outcome of a physical attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicalAttackResult {
    Miss,
    Dodge,
    Parry,
    Glancing,
    Block,
    BlockCritical,
    Critical,
    Hit,
}

impl PhysicalAttackResult {
    /// Whether the attack connected (possibly blocked).
    pub fn is_success(self) -> bool {
        !matches!(self, Self::Miss | Self::Dodge | Self::Parry)
    }
}

/// Outcome of a magic hit roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagicAttackResult {
    Miss,
    Critical,
    Hit,
}

/// Outcome of a magic resist roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagicResistResult {
    NoResist,
    Partial25,
    Partial50,
    Partial75,
    FullResist,
}

impl MagicResistResult {
    /// Fraction of the damage that gets through.
    pub fn damage_modifier(self) -> f64 {
        match self {
            MagicResistResult::FullResist => 0.0,
            MagicResistResult::Partial75 => 0.25,
            MagicResistResult::Partial50 => 0.5,
            MagicResistResult::Partial25 => 0.75,
            MagicResistResult::NoResist => 1.0,
        }
    }
}

/// Character state needed to build and use melee attack tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollContext {
    /// Character level.
    pub clvl: u32,
    /// Melee hit chance from gear/talents/buffs, as a range out of 10 000.
    pub melee_hit_chance: u32,
    /// Whether the character is dual wielding (affects white miss chance).
    pub dual_wielding: bool,
    /// Whether the character attacks from behind (no parries).
    pub attacking_from_behind: bool,
    /// Whether glancing blows can occur (disabled by the Loatheb ruleset).
    pub glancing_blows: bool,
}

/// Character state needed to build a magic attack table for one school.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagicRollContext {
    pub clvl: u32,
    /// Spell hit chance for the school, as a range out of 10 000.
    pub spell_hit_chance: u32,
    /// Effective target resistance for the school.
    pub target_resistance: u32,
}

/// Cache key for melee tables: weapon skill and whether the attack comes from behind.
type MeleeTableKey = (u32, bool);

/// Rolls attack outcomes for one character.
#[derive(Debug)]
pub struct CombatRoll {
    mechanics: Mechanics,
    random: Random,
    glance_roll: Random,
    melee_white_tables: HashMap<MeleeTableKey, MeleeWhiteHitTable>,
    melee_special_tables: HashMap<MeleeTableKey, MeleeSpecialTable>,
    magic_attack_tables: HashMap<MagicSchool, MagicAttackTable>,
}

impl CombatRoll {
    /// Creates a roller against a target of `target_level`, seeded from the system clock.
    pub fn new(target_level: u32) -> Self {
        Self {
            mechanics: Mechanics::new(target_level),
            random: Random::new(0, ROLL_RANGE),
            glance_roll: Random::new(0, ROLL_RANGE),
            melee_white_tables: HashMap::new(),
            melee_special_tables: HashMap::new(),
            magic_attack_tables: HashMap::new(),
        }
    }

    /// Creates a roller with a fixed seed.
    pub fn from_seed(target_level: u32, seed: u64) -> Self {
        let mut roll = Self::new(target_level);
        roll.set_new_seed(seed);
        roll
    }

    pub fn mechanics(&self) -> &Mechanics {
        &self.mechanics
    }

    /// Changes the target level. Cached tables are dropped since they depend on it.
    pub fn set_target_level(&mut self, target_level: u32) {
        self.mechanics.set_target_level(target_level);
        self.drop_tables();
    }

    /// Re-seeds both generators deterministically from `seed`.
    pub fn set_new_seed(&mut self, seed: u64) {
        self.random.set_gen_from_seed(seed);
        self.glance_roll.set_gen_from_seed(seed.wrapping_add(1));
    }

    /// The main roll generator (exposed for tests that need to force outcomes).
    pub fn random_mut(&mut self) -> &mut Random {
        &mut self.random
    }

    /// Rolls a white (auto attack) melee hit. `crit_chance` is a range out of 10 000.
    pub fn get_melee_hit_result(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
        crit_chance: u32,
    ) -> PhysicalAttackResult {
        let roll = self.random.get_roll();
        let crit = self.get_suppressed_crit(ctx.clvl, crit_chance);
        self.ensure_melee_white_table(ctx, wpn_skill);

        let table = &self.melee_white_tables[&(wpn_skill, ctx.attacking_from_behind)];
        table.get_outcome(&mut self.random, roll, crit, IncludedOutcomes::ALL)
    }

    /// Rolls a yellow (special ability) melee hit. `crit_chance` is a range out of 10 000.
    pub fn get_melee_ability_result(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
        crit_chance: u32,
        included: IncludedOutcomes,
    ) -> PhysicalAttackResult {
        let roll = self.random.get_roll();
        let crit = self.get_suppressed_crit(ctx.clvl, crit_chance);
        self.ensure_melee_special_table(ctx, wpn_skill);

        let table = &self.melee_special_tables[&(wpn_skill, ctx.attacking_from_behind)];
        table.get_outcome(&mut self.random, roll, crit, included)
    }

    /// Rolls a spell hit for `school`. `crit_chance` is a range out of 10 000.
    pub fn get_spell_ability_result(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
        crit_chance: u32,
    ) -> MagicAttackResult {
        let roll = self.random.get_roll();
        self.ensure_magic_attack_table(ctx, school);

        let table = &self.magic_attack_tables[&school];
        table.get_hit_outcome(&mut self.random, roll, crit_chance)
    }

    /// Rolls the resist outcome for `school`. Physical damage is never resisted.
    pub fn get_spell_resist_result(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
    ) -> MagicResistResult {
        if school == MagicSchool::Physical {
            return MagicResistResult::NoResist;
        }

        let roll = self.random.get_roll();
        self.ensure_magic_attack_table(ctx, school);

        self.magic_attack_tables[&school].get_resist_outcome(roll)
    }

    /// Returns (building if needed) the white hit table for `wpn_skill` and the facing in `ctx`.
    pub fn get_melee_white_table(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
    ) -> &MeleeWhiteHitTable {
        self.ensure_melee_white_table(ctx, wpn_skill);
        &self.melee_white_tables[&(wpn_skill, ctx.attacking_from_behind)]
    }

    /// Returns (building if needed) the special hit table for `wpn_skill` and the facing in `ctx`.
    pub fn get_melee_special_table(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
    ) -> &MeleeSpecialTable {
        self.ensure_melee_special_table(ctx, wpn_skill);
        &self.melee_special_tables[&(wpn_skill, ctx.attacking_from_behind)]
    }

    /// Returns (building if needed) the magic attack table for `school`.
    pub fn get_magic_attack_table(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
    ) -> &MagicAttackTable {
        self.ensure_magic_attack_table(ctx, school);
        &self.magic_attack_tables[&school]
    }

    fn ensure_melee_white_table(&mut self, ctx: &RollContext, wpn_skill: u32) {
        let key = (wpn_skill, ctx.attacking_from_behind);
        if self.melee_white_tables.contains_key(&key) {
            return;
        }

        let miss = self.white_miss_range(ctx, wpn_skill);
        let glancing = if ctx.glancing_blows {
            self.mechanics.glancing_blow_chance(ctx.clvl)
        } else {
            0.0
        };
        let parry = self.parry_chance(ctx, wpn_skill);

        let table = MeleeWhiteHitTable::new(
            wpn_skill,
            miss,
            self.mechanics.dodge_chance(wpn_skill),
            parry,
            glancing,
            self.mechanics.block_chance(),
        );
        self.melee_white_tables.insert(key, table);
    }

    fn ensure_melee_special_table(&mut self, ctx: &RollContext, wpn_skill: u32) {
        let key = (wpn_skill, ctx.attacking_from_behind);
        if self.melee_special_tables.contains_key(&key) {
            return;
        }

        let miss = self.yellow_miss_range(ctx, wpn_skill);
        let parry = self.parry_chance(ctx, wpn_skill);

        let table = MeleeSpecialTable::new(
            wpn_skill,
            miss,
            self.mechanics.dodge_chance(wpn_skill),
            parry,
            self.mechanics.block_chance(),
        );
        self.melee_special_tables.insert(key, table);
    }

    fn ensure_magic_attack_table(&mut self, ctx: &MagicRollContext, school: MagicSchool) {
        if self.magic_attack_tables.contains_key(&school) {
            return;
        }

        let table = MagicAttackTable::new(
            &self.mechanics,
            ctx.clvl,
            ctx.spell_hit_chance,
            ctx.target_resistance,
        );
        self.magic_attack_tables.insert(school, table);
    }

    fn parry_chance(&self, ctx: &RollContext, wpn_skill: u32) -> f64 {
        if ctx.attacking_from_behind {
            0.0
        } else {
            self.mechanics.parry_chance(wpn_skill)
        }
    }

    /// White miss chance (fraction) before hit chance is applied.
    pub fn get_white_miss_chance(&self, ctx: &RollContext, wpn_skill: u32) -> f64 {
        if ctx.dual_wielding {
            self.mechanics.dual_wield_white_miss_chance(wpn_skill)
        } else {
            self.mechanics.two_hand_white_miss_chance(wpn_skill)
        }
    }

    /// Yellow miss chance (fraction) before hit chance is applied.
    pub fn get_yellow_miss_chance(&self, wpn_skill: u32) -> f64 {
        self.mechanics.yellow_miss_chance(wpn_skill)
    }

    fn white_miss_range(&self, ctx: &RollContext, wpn_skill: u32) -> u32 {
        chance_to_range(self.get_white_miss_chance(ctx, wpn_skill))
            .saturating_sub(ctx.melee_hit_chance)
    }

    fn yellow_miss_range(&self, ctx: &RollContext, wpn_skill: u32) -> u32 {
        chance_to_range(self.get_yellow_miss_chance(wpn_skill)).saturating_sub(ctx.melee_hit_chance)
    }

    /// Rolls the glancing blow damage multiplier for `wpn_skill`.
    pub fn get_glancing_blow_dmg_penalty(&mut self, clvl: u32, wpn_skill: u32) -> f64 {
        let min = self
            .mechanics
            .glancing_blow_dmg_penalty_min(clvl, wpn_skill);
        let max = self
            .mechanics
            .glancing_blow_dmg_penalty_max(clvl, wpn_skill);

        self.glance_roll
            .set_new_range(chance_to_range(min), chance_to_range(max));

        f64::from(self.glance_roll.get_roll()) / f64::from(ROLL_RANGE)
    }

    /// Recomputes the miss range of every cached special table (after hit chance changed).
    pub fn update_melee_yellow_miss_chance(&mut self, ctx: &RollContext) {
        let mechanics = self.mechanics;
        for table in self.melee_special_tables.values_mut() {
            let miss = chance_to_range(mechanics.yellow_miss_chance(table.wpn_skill))
                .saturating_sub(ctx.melee_hit_chance);
            table.update_miss_chance(miss);
        }
    }

    /// Recomputes the miss range of every cached white table (after hit chance or dual-wield
    /// state changed).
    pub fn update_melee_white_miss_chance(&mut self, ctx: &RollContext) {
        let mechanics = self.mechanics;
        for table in self.melee_white_tables.values_mut() {
            let chance = if ctx.dual_wielding {
                mechanics.dual_wield_white_miss_chance(table.wpn_skill)
            } else {
                mechanics.two_hand_white_miss_chance(table.wpn_skill)
            };
            table.update_miss_chance(chance_to_range(chance).saturating_sub(ctx.melee_hit_chance));
        }
    }

    /// Recomputes the miss range of the cached table for `school`, if any.
    pub fn update_spell_miss_chance(&mut self, clvl: u32, school: MagicSchool, spell_hit: u32) {
        if let Some(table) = self.magic_attack_tables.get_mut(&school) {
            table.update_miss_chance(&self.mechanics, clvl, spell_hit);
        }
    }

    /// Recomputes the resist ranges of the cached table for `school`, if any.
    pub fn update_target_resistance(&mut self, school: MagicSchool, target_resistance: u32) {
        if let Some(table) = self.magic_attack_tables.get_mut(&school) {
            table.update_target_resistance(target_resistance);
        }
    }

    /// Drops every cached table. Called between sets of iterations.
    pub fn drop_tables(&mut self) {
        self.melee_white_tables.clear();
        self.melee_special_tables.clear();
        self.magic_attack_tables.clear();
    }

    /// Applies the per-level crit suppression to a crit chance (range out of 10 000).
    pub fn get_suppressed_crit(&self, clvl: u32, crit_chance: u32) -> u32 {
        let suppression = chance_to_range(self.mechanics.melee_crit_suppression(clvl));
        crit_chance.saturating_sub(suppression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dual_wield_ctx() -> RollContext {
        RollContext {
            clvl: 60,
            melee_hit_chance: 0,
            dual_wielding: true,
            attacking_from_behind: true,
            glancing_blows: true,
        }
    }

    fn roll_with(random: &mut Random, seed: u64) {
        random.set_gen_from_seed(seed);
    }

    /// Port of `TestAttackTables::test_white_hit_table_update` with the Warrior replaced by a
    /// dual-wielding level 60 context against a level 63 target.
    #[test]
    fn white_hit_table_from_context() {
        let ctx = dual_wield_ctx();
        let mut roll = CombatRoll::from_seed(63, 1);
        let mut random = Random::from_seed(0, ROLL_RANGE, 1);
        let all = IncludedOutcomes::ALL;

        let table = roll.get_melee_white_table(&ctx, 300).clone();

        let base_miss_dw = 2719;
        let base_dodge = 650;
        let glancing_rate = 4000;

        let mut outcome = |roll: u32, crit: u32| table.get_outcome(&mut random, roll, crit, all);
        assert_eq!(outcome(0, 1), PhysicalAttackResult::Miss);
        assert_eq!(outcome(base_miss_dw, 1), PhysicalAttackResult::Miss);
        assert_eq!(outcome(base_miss_dw + 1, 1), PhysicalAttackResult::Dodge);
        assert_eq!(
            outcome(base_miss_dw + base_dodge, 1),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            outcome(base_miss_dw + base_dodge + 1, 1),
            PhysicalAttackResult::Glancing
        );
        assert_eq!(
            outcome(base_miss_dw + base_dodge + glancing_rate, 1),
            PhysicalAttackResult::Glancing
        );
        assert_eq!(
            outcome(base_miss_dw + base_dodge + glancing_rate + 1, 1),
            PhysicalAttackResult::Critical
        );

        let glancing_range = base_miss_dw + base_dodge + glancing_rate;
        let crit_chance = 500;
        assert_eq!(
            outcome(glancing_range + crit_chance, crit_chance),
            PhysicalAttackResult::Critical
        );
        assert_eq!(
            outcome(glancing_range + crit_chance + 1, crit_chance),
            PhysicalAttackResult::Hit
        );
        assert_eq!(
            outcome(glancing_range + crit_chance, crit_chance - 1),
            PhysicalAttackResult::Hit
        );
        assert_eq!(outcome(9999, 999_999), PhysicalAttackResult::Critical);
    }

    #[test]
    fn hit_chance_reduces_miss_range() {
        let ctx = RollContext {
            melee_hit_chance: 300,
            ..dual_wield_ctx()
        };
        let mut roll = CombatRoll::from_seed(63, 1);
        let mut random = Random::from_seed(0, ROLL_RANGE, 1);

        let white = roll.get_melee_white_table(&ctx, 300).clone();
        assert_eq!(
            white.get_outcome(&mut random, 2419, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            white.get_outcome(&mut random, 2420, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Dodge
        );

        // Yellow: 9% - 3% = 6%.
        let special = roll.get_melee_special_table(&ctx, 300).clone();
        assert_eq!(
            special.get_outcome(&mut random, 599, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            special.get_outcome(&mut random, 600, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Dodge
        );

        // Hit chance above the miss chance clamps at zero.
        let capped = RollContext {
            melee_hit_chance: 5000,
            ..ctx
        };
        roll.drop_tables();
        let white = roll.get_melee_white_table(&capped, 300).clone();
        assert_eq!(
            white.get_outcome(&mut random, 0, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Dodge
        );
    }

    #[test]
    fn single_weapon_and_front_facing_change_table() {
        let ctx = RollContext {
            dual_wielding: false,
            attacking_from_behind: false,
            ..dual_wield_ctx()
        };
        let mut roll = CombatRoll::from_seed(63, 1);
        let mut random = Random::from_seed(0, ROLL_RANGE, 1);
        let all = IncludedOutcomes::ALL;

        let table = roll.get_melee_white_table(&ctx, 300).clone();
        // 9% miss, 6.5% dodge, 15.5% parry, 40% glancing.
        assert_eq!(
            table.get_outcome(&mut random, 899, 0, all),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            table.get_outcome(&mut random, 900, 0, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 1549, 0, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 1550, 0, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 3099, 0, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 3100, 0, all),
            PhysicalAttackResult::Glancing
        );
        assert_eq!(
            table.get_outcome(&mut random, 7100, 0, all),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    fn loatheb_ruleset_disables_glancing_blows() {
        let ctx = RollContext {
            glancing_blows: false,
            ..dual_wield_ctx()
        };
        let mut roll = CombatRoll::from_seed(63, 1);
        let mut random = Random::from_seed(0, ROLL_RANGE, 1);

        let table = roll.get_melee_white_table(&ctx, 300).clone();
        assert_eq!(
            table.get_outcome(&mut random, 2720 + 650, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Hit
        );
    }

    #[test]
    fn tables_are_cached_and_updated_in_place() {
        let mut ctx = dual_wield_ctx();
        let mut roll = CombatRoll::from_seed(63, 1);

        let before = roll.get_melee_white_table(&ctx, 300).clone();
        assert_eq!(roll.melee_white_tables.len(), 1);
        assert_eq!(roll.get_melee_white_table(&ctx, 300), &before);

        ctx.dual_wielding = false;
        roll.update_melee_white_miss_chance(&ctx);
        let mut random = Random::from_seed(0, ROLL_RANGE, 1);
        let updated = roll.get_melee_white_table(&ctx, 300);
        assert_eq!(
            updated.get_outcome(&mut random, 899, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            updated.get_outcome(&mut random, 900, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Dodge
        );

        ctx.melee_hit_chance = 900;
        roll.get_melee_special_table(&ctx, 300);
        roll.update_melee_yellow_miss_chance(&ctx);
        let special = roll.get_melee_special_table(&ctx, 300);
        assert_eq!(
            special.get_outcome(&mut random, 0, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Dodge
        );

        roll.drop_tables();
        assert!(roll.melee_white_tables.is_empty());
        assert!(roll.melee_special_tables.is_empty());
    }

    #[test]
    fn set_target_level_drops_tables_and_updates_mechanics() {
        let ctx = dual_wield_ctx();
        let mut roll = CombatRoll::from_seed(63, 1);
        roll.get_melee_white_table(&ctx, 300);

        roll.set_target_level(60);
        assert!(roll.melee_white_tables.is_empty());
        assert_eq!(roll.mechanics().target_level(), 60);
        assert_eq!(roll.get_suppressed_crit(60, 100), 100);
    }

    #[test]
    fn crit_suppression_from_target_level() {
        let roll = CombatRoll::from_seed(63, 1);
        assert_eq!(roll.get_suppressed_crit(60, 1000), 700);
        assert_eq!(roll.get_suppressed_crit(60, 200), 0);
        assert_eq!(roll.get_suppressed_crit(63, 200), 200);
    }

    /// Port of `TestCombatRoll::test_glancing_penalties`.
    #[test]
    fn glancing_penalties() {
        let mut roll = CombatRoll::from_seed(63, 7);

        for (skill, min, max) in [
            (300, 0.54999, 0.75001),
            (305, 0.7999, 0.9001),
            (310, 0.90999, 0.99001),
            (315, 0.90999, 0.99001),
        ] {
            for _ in 0..1000 {
                let penalty = roll.get_glancing_blow_dmg_penalty(60, skill);
                assert!(penalty > min && penalty < max, "skill {skill}: {penalty}");
            }
        }
    }

    #[test]
    fn melee_rolls_produce_expected_distribution() {
        let ctx = dual_wield_ctx();
        let mut roll = CombatRoll::from_seed(63, 3);
        let mut counts: HashMap<PhysicalAttackResult, u32> = HashMap::new();
        let n = 100_000;
        for _ in 0..n {
            *counts
                .entry(roll.get_melee_hit_result(&ctx, 300, 800))
                .or_default() += 1;
        }

        let fraction = |result| f64::from(counts.get(&result).copied().unwrap_or(0)) / f64::from(n);
        let within = |result, expected: f64| (fraction(result) - expected).abs() < 0.01;
        assert!(within(PhysicalAttackResult::Miss, 0.272));
        assert!(within(PhysicalAttackResult::Dodge, 0.065));
        assert!(within(PhysicalAttackResult::Glancing, 0.40));
        // 8% crit minus 3% suppression.
        assert!(within(PhysicalAttackResult::Critical, 0.05));
        assert_eq!(counts.get(&PhysicalAttackResult::Parry), None);
    }

    #[test]
    fn ability_rolls_respect_included_outcomes() {
        let ctx = dual_wield_ctx();
        let mut roll = CombatRoll::from_seed(63, 5);
        let no_avoidance = IncludedOutcomes {
            dodge: false,
            parry: false,
            block: false,
            miss: false,
        };
        for _ in 0..10_000 {
            let result = roll.get_melee_ability_result(&ctx, 300, 0, no_avoidance);
            assert_eq!(result, PhysicalAttackResult::Hit);
        }

        roll_with(roll.random_mut(), 5);
        let saw_miss = (0..10_000).any(|_| {
            roll.get_melee_ability_result(&ctx, 300, 0, IncludedOutcomes::ALL)
                == PhysicalAttackResult::Miss
        });
        assert!(saw_miss);
    }

    #[test]
    fn spell_rolls() {
        let ctx = MagicRollContext {
            clvl: 60,
            spell_hit_chance: 0,
            target_resistance: 0,
        };
        let mut roll = CombatRoll::from_seed(63, 11);

        for _ in 0..1000 {
            assert_eq!(
                roll.get_spell_resist_result(&ctx, MagicSchool::Physical),
                MagicResistResult::NoResist
            );
            assert_eq!(
                roll.get_spell_resist_result(&ctx, MagicSchool::Fire),
                MagicResistResult::NoResist
            );
        }

        let n = 100_000;
        let misses = (0..n)
            .filter(|_| {
                roll.get_spell_ability_result(&ctx, MagicSchool::Fire, 0) == MagicAttackResult::Miss
            })
            .count();
        let miss_rate = misses as f64 / f64::from(n);
        assert!((miss_rate - 0.17).abs() < 0.01, "miss rate {miss_rate}");

        roll.update_spell_miss_chance(60, MagicSchool::Fire, 1600);
        let misses = (0..n)
            .filter(|_| {
                roll.get_spell_ability_result(&ctx, MagicSchool::Fire, 0) == MagicAttackResult::Miss
            })
            .count();
        let miss_rate = misses as f64 / f64::from(n);
        assert!((miss_rate - 0.01).abs() < 0.005, "miss rate {miss_rate}");

        roll.update_target_resistance(MagicSchool::Fire, 300);
        let full_resists = (0..n)
            .filter(|_| {
                roll.get_spell_resist_result(&ctx, MagicSchool::Fire)
                    == MagicResistResult::FullResist
            })
            .count();
        let rate = full_resists as f64 / f64::from(n);
        assert!((rate - 0.25).abs() < 0.01, "full resist rate {rate}");
    }

    #[test]
    fn seeded_rolls_are_reproducible() {
        let ctx = dual_wield_ctx();
        let mut a = CombatRoll::from_seed(63, 99);
        let mut b = CombatRoll::from_seed(63, 99);
        for _ in 0..1000 {
            assert_eq!(
                a.get_melee_hit_result(&ctx, 300, 500),
                b.get_melee_hit_result(&ctx, 300, 500)
            );
            assert_eq!(
                a.get_glancing_blow_dmg_penalty(60, 300),
                b.get_glancing_blow_dmg_penalty(60, 300)
            );
        }
    }

    #[test]
    fn result_helpers() {
        assert!(PhysicalAttackResult::Hit.is_success());
        assert!(PhysicalAttackResult::Block.is_success());
        assert!(!PhysicalAttackResult::Dodge.is_success());
        assert_eq!(MagicResistResult::Partial75.damage_modifier(), 0.25);
        assert_eq!(MagicResistResult::NoResist.damage_modifier(), 1.0);
    }
}
