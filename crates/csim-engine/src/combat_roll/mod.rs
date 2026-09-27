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
    IncludedOutcomes, MagicAttackTable, MeleeSpecialTable, MeleeWhiteHitTable, ROLL_RANGE,
    chance_to_range,
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

impl MagicAttackResult {
    /// The physical result with the same meaning (a spell's miss, hit or crit), for the code
    /// that handles the outcomes of both tables alike.
    pub fn as_physical(self) -> PhysicalAttackResult {
        match self {
            MagicAttackResult::Miss => PhysicalAttackResult::Miss,
            MagicAttackResult::Critical => PhysicalAttackResult::Critical,
            MagicAttackResult::Hit => PhysicalAttackResult::Hit,
        }
    }
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

/// The outcome of a spell on the magic table: its hit roll and, if it landed, how much of it
/// the target resisted. A binary spell resisted by the target's resistance is a
/// [`MagicResistResult::FullResist`]; its hit roll counts as a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpellRoll {
    pub result: MagicAttackResult,
    pub resist: MagicResistResult,
}

impl SpellRoll {
    pub const HIT: SpellRoll = SpellRoll {
        result: MagicAttackResult::Hit,
        resist: MagicResistResult::NoResist,
    };
    pub const MISS: SpellRoll = SpellRoll {
        result: MagicAttackResult::Miss,
        resist: MagicResistResult::NoResist,
    };
    pub const FULL_RESIST: SpellRoll = SpellRoll {
        result: MagicAttackResult::Hit,
        resist: MagicResistResult::FullResist,
    };

    /// Whether the spell landed (possibly partially resisted).
    pub fn landed(self) -> bool {
        self.result != MagicAttackResult::Miss && self.resist != MagicResistResult::FullResist
    }

    pub fn is_critical(self) -> bool {
        self.landed() && self.result == MagicAttackResult::Critical
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
    /// Dodge and parry chance the target loses (Weapon Expertise), as a range out of 10 000.
    pub expertise: u32,
}

/// Character state needed to build a magic attack table for one school.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MagicRollContext {
    pub clvl: u32,
    /// Spell hit chance for the school, as a range out of 10 000.
    pub spell_hit_chance: u32,
    /// The target's resistance to the school after spell penetration, without the level-based
    /// resistance (the table adds it).
    pub target_resistance: u32,
}

/// How a spell's resistance is rolled: royalgiraffe's binary spells (any effect besides damage)
/// in one roll with the hit, non-binary ones (only damage) as a partial resist after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpellResistKind {
    Binary,
    NonBinary,
}

/// Cache key for magic tables: the school and what the table was built from, so a change of
/// spell hit or resistance builds a new table instead of updating the old one.
type MagicTableKey = (MagicSchool, MagicRollContext);

/// Cache key for melee tables: weapon skill, whether the attack comes from behind and the
/// expertise.
type MeleeTableKey = (u32, bool, u32);

fn melee_key(ctx: &RollContext, wpn_skill: u32) -> MeleeTableKey {
    (wpn_skill, ctx.attacking_from_behind, ctx.expertise)
}

/// The expertise of `ctx` as a fraction.
fn expertise_chance(ctx: &RollContext) -> f64 {
    f64::from(ctx.expertise) / f64::from(ROLL_RANGE)
}

/// Rolls attack outcomes for one character.
#[derive(Debug)]
pub struct CombatRoll {
    mechanics: Mechanics,
    random: Random,
    glance_roll: Random,
    melee_white_tables: HashMap<MeleeTableKey, MeleeWhiteHitTable>,
    melee_special_tables: HashMap<MeleeTableKey, MeleeSpecialTable>,
    magic_attack_tables: HashMap<MagicTableKey, MagicAttackTable>,
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

        let table = &self.melee_white_tables[&melee_key(ctx, wpn_skill)];
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

        let table = &self.melee_special_tables[&melee_key(ctx, wpn_skill)];
        table.get_outcome(&mut self.random, roll, crit, included)
    }

    /// Rolls a spell of `school` on the magic table: the hit (and, with `crit_chance`, a range
    /// out of 10 000, the crit), then a non-binary spell's partial resist.
    pub fn get_spell_ability_result(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
        crit_chance: u32,
        kind: SpellResistKind,
    ) -> SpellRoll {
        let roll = self.random.get_roll();
        let mechanics = self.mechanics;
        let table = ensure_magic_table(&mut self.magic_attack_tables, &mechanics, ctx, school);
        if kind == SpellResistKind::Binary {
            return table.get_binary_outcome(&mut self.random, roll, crit_chance);
        }

        let result = table.get_hit_outcome(&mut self.random, roll, crit_chance);
        let resist = if result == MagicAttackResult::Miss {
            MagicResistResult::NoResist
        } else {
            self.get_spell_resist_result(ctx, school)
        };
        SpellRoll { result, resist }
    }

    /// Rolls the partial resist of a non-binary spell of `school`. Physical damage is never
    /// resisted.
    pub fn get_spell_resist_result(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
    ) -> MagicResistResult {
        if school == MagicSchool::Physical {
            return MagicResistResult::NoResist;
        }

        let roll = self.random.get_roll();
        self.get_magic_attack_table(ctx, school)
            .get_resist_outcome(roll)
    }

    /// Rolls the partial resist of a periodic damage tick of `school`: against a tenth of the
    /// resistance for a damage-over-time without direct damage (`pure_dot`), like direct
    /// damage otherwise. Physical ticks are never resisted.
    pub fn get_periodic_resist_result(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
        pure_dot: bool,
    ) -> MagicResistResult {
        if school == MagicSchool::Physical {
            return MagicResistResult::NoResist;
        }

        let roll = self.random.get_roll();
        let table = self.get_magic_attack_table(ctx, school);
        if pure_dot {
            table.get_periodic_resist_outcome(roll)
        } else {
            table.get_resist_outcome(roll)
        }
    }

    /// Returns (building if needed) the white hit table for `wpn_skill` and the facing in `ctx`.
    pub fn get_melee_white_table(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
    ) -> &MeleeWhiteHitTable {
        self.ensure_melee_white_table(ctx, wpn_skill);
        &self.melee_white_tables[&melee_key(ctx, wpn_skill)]
    }

    /// Returns (building if needed) the special hit table for `wpn_skill` and the facing in `ctx`.
    pub fn get_melee_special_table(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
    ) -> &MeleeSpecialTable {
        self.ensure_melee_special_table(ctx, wpn_skill);
        &self.melee_special_tables[&melee_key(ctx, wpn_skill)]
    }

    /// Returns (building if needed) the magic attack table for `school` and `ctx`.
    pub fn get_magic_attack_table(
        &mut self,
        ctx: &MagicRollContext,
        school: MagicSchool,
    ) -> &MagicAttackTable {
        let mechanics = self.mechanics;
        ensure_magic_table(&mut self.magic_attack_tables, &mechanics, ctx, school)
    }

    /// The white hit table for `wpn_skill` and the facing in `ctx`, for tests that force
    /// outcomes by reshaping it.
    #[cfg(test)]
    pub(crate) fn melee_white_table_mut(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
    ) -> &mut MeleeWhiteHitTable {
        self.ensure_melee_white_table(ctx, wpn_skill);
        self.melee_white_tables
            .get_mut(&melee_key(ctx, wpn_skill))
            .expect("ensured above")
    }

    /// The special hit table for `wpn_skill` and the facing in `ctx`, for tests that force
    /// outcomes by reshaping it.
    #[cfg(test)]
    pub(crate) fn melee_special_table_mut(
        &mut self,
        ctx: &RollContext,
        wpn_skill: u32,
    ) -> &mut MeleeSpecialTable {
        self.ensure_melee_special_table(ctx, wpn_skill);
        self.melee_special_tables
            .get_mut(&melee_key(ctx, wpn_skill))
            .expect("ensured above")
    }

    fn ensure_melee_white_table(&mut self, ctx: &RollContext, wpn_skill: u32) {
        let key = melee_key(ctx, wpn_skill);
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
            self.dodge_chance(ctx, wpn_skill),
            parry,
            glancing,
            self.block_chance(ctx, wpn_skill),
        );
        self.melee_white_tables.insert(key, table);
    }

    fn ensure_melee_special_table(&mut self, ctx: &RollContext, wpn_skill: u32) {
        let key = melee_key(ctx, wpn_skill);
        if self.melee_special_tables.contains_key(&key) {
            return;
        }

        let miss = self.yellow_miss_range(ctx, wpn_skill);
        let parry = self.parry_chance(ctx, wpn_skill);

        let table = MeleeSpecialTable::new(
            wpn_skill,
            miss,
            self.dodge_chance(ctx, wpn_skill),
            parry,
            self.block_chance(ctx, wpn_skill),
        );
        self.melee_special_tables.insert(key, table);
    }

    /// The target's dodge chance, less the character's expertise.
    fn dodge_chance(&self, ctx: &RollContext, wpn_skill: u32) -> f64 {
        (self.mechanics.dodge_chance(wpn_skill) - expertise_chance(ctx)).max(0.0)
    }

    /// The target's parry chance (none from behind), less the character's expertise.
    fn parry_chance(&self, ctx: &RollContext, wpn_skill: u32) -> f64 {
        if ctx.attacking_from_behind {
            0.0
        } else {
            (self.mechanics.parry_chance(ctx.clvl, wpn_skill) - expertise_chance(ctx)).max(0.0)
        }
    }

    fn block_chance(&self, ctx: &RollContext, wpn_skill: u32) -> f64 {
        if ctx.attacking_from_behind {
            0.0
        } else {
            self.mechanics.block_chance(wpn_skill)
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
            .saturating_sub(self.get_suppressed_hit(wpn_skill, ctx.melee_hit_chance))
    }

    fn yellow_miss_range(&self, ctx: &RollContext, wpn_skill: u32) -> u32 {
        chance_to_range(self.get_yellow_miss_chance(wpn_skill))
            .saturating_sub(self.get_suppressed_hit(wpn_skill, ctx.melee_hit_chance))
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
            let hit = suppressed_hit(&mechanics, table.wpn_skill, ctx.melee_hit_chance);
            let miss =
                chance_to_range(mechanics.yellow_miss_chance(table.wpn_skill)).saturating_sub(hit);
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
            let hit = suppressed_hit(&mechanics, table.wpn_skill, ctx.melee_hit_chance);
            table.update_miss_chance(chance_to_range(chance).saturating_sub(hit));
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

    /// The part of `hit_chance` (hundredths of a percent) that counts against a mob whose
    /// defense exceeds `wpn_skill` by more than 10: the first `(difference − 10) × 0.2 %` is
    /// ignored. The counterpart of [`Self::get_suppressed_crit`] for hit.
    pub fn get_suppressed_hit(&self, wpn_skill: u32, hit_chance: u32) -> u32 {
        suppressed_hit(&self.mechanics, wpn_skill, hit_chance)
    }
}

/// The cached magic table of `school` for `ctx`, built on first use.
fn ensure_magic_table<'a>(
    tables: &'a mut HashMap<MagicTableKey, MagicAttackTable>,
    mechanics: &Mechanics,
    ctx: &MagicRollContext,
    school: MagicSchool,
) -> &'a MagicAttackTable {
    tables.entry((school, *ctx)).or_insert_with(|| {
        MagicAttackTable::new(
            mechanics,
            ctx.clvl,
            ctx.spell_hit_chance,
            ctx.target_resistance,
            school != MagicSchool::Physical,
        )
    })
}

fn suppressed_hit(mechanics: &Mechanics, wpn_skill: u32, hit_chance: u32) -> u32 {
    hit_chance.saturating_sub(chance_to_range(mechanics.hit_suppression(wpn_skill)))
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
            expertise: 0,
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

        // 8 % single-weapon miss plus the flat 19 % dual-wield penalty = 27 %.
        let base_miss_dw = 2699;
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

        // With 300 skill vs 315 defense the first 1% of hit is ignored: 27% - (3% - 1%).
        assert_eq!(roll.get_suppressed_hit(300, 300), 200);
        assert_eq!(roll.get_suppressed_hit(305, 300), 300);
        assert_eq!(roll.get_suppressed_hit(300, 50), 0);
        let white = roll.get_melee_white_table(&ctx, 300).clone();
        assert_eq!(
            white.get_outcome(&mut random, 2499, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            white.get_outcome(&mut random, 2500, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Dodge
        );

        // Yellow: 8% - (3% - 1% suppressed) = 6%.
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
        // 8% miss, 6.5% dodge, 14% parry, 40% glancing, 5% block.
        assert_eq!(
            table.get_outcome(&mut random, 799, 0, all),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            table.get_outcome(&mut random, 800, 0, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 1449, 0, all),
            PhysicalAttackResult::Dodge
        );
        assert_eq!(
            table.get_outcome(&mut random, 1450, 0, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 2849, 0, all),
            PhysicalAttackResult::Parry
        );
        assert_eq!(
            table.get_outcome(&mut random, 2850, 0, all),
            PhysicalAttackResult::Glancing
        );
        assert_eq!(
            table.get_outcome(&mut random, 6849, 0, all),
            PhysicalAttackResult::Glancing
        );
        assert_eq!(
            table.get_outcome(&mut random, 6850, 0, all),
            PhysicalAttackResult::Block
        );
        assert_eq!(
            table.get_outcome(&mut random, 7349, 0, all),
            PhysicalAttackResult::Block
        );
        assert_eq!(
            table.get_outcome(&mut random, 7350, 0, all),
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
            table.get_outcome(&mut random, 2700 + 650, 0, IncludedOutcomes::ALL),
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
            updated.get_outcome(&mut random, 799, 0, IncludedOutcomes::ALL),
            PhysicalAttackResult::Miss
        );
        assert_eq!(
            updated.get_outcome(&mut random, 800, 0, IncludedOutcomes::ALL),
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

    fn spell_ctx(spell_hit_chance: u32, target_resistance: u32) -> MagicRollContext {
        MagicRollContext {
            clvl: 60,
            spell_hit_chance,
            target_resistance,
        }
    }

    fn rate(n: u32, mut roll: impl FnMut() -> bool) -> f64 {
        (0..n).filter(|_| roll()).count() as f64 / f64::from(n)
    }

    /// Miss rates against a boss, with and without spell hit.
    #[test]
    fn spell_miss_rates() {
        let mut roll = CombatRoll::from_seed(63, 11);
        let n = 100_000;
        let non_binary = SpellResistKind::NonBinary;

        let ctx = spell_ctx(0, 0);
        let miss = rate(n, || {
            roll.get_spell_ability_result(&ctx, MagicSchool::Nature, 0, non_binary)
                .result
                == MagicAttackResult::Miss
        });
        assert!((miss - 0.17).abs() < 0.005, "miss rate {miss}");

        // Precision's 5 % spell hit.
        let ctx = spell_ctx(500, 0);
        let miss = rate(n, || {
            !roll
                .get_spell_ability_result(&ctx, MagicSchool::Nature, 0, non_binary)
                .landed()
        });
        assert!((miss - 0.12).abs() < 0.005, "miss rate {miss}");

        // At and above the 16 % cap.
        let ctx = spell_ctx(2000, 0);
        let miss = rate(n, || {
            !roll
                .get_spell_ability_result(&ctx, MagicSchool::Nature, 0, non_binary)
                .landed()
        });
        assert!((miss - 0.01).abs() < 0.002, "miss rate {miss}");
    }

    /// Crits come from the crit chance among the spells that land.
    #[test]
    fn spell_crit_rate() {
        let mut roll = CombatRoll::from_seed(63, 13);
        let ctx = spell_ctx(0, 0);
        let n = 100_000;
        let mut landed = 0;
        let mut crits = 0;
        for _ in 0..n {
            let spell = roll.get_spell_ability_result(
                &ctx,
                MagicSchool::Nature,
                1000,
                SpellResistKind::NonBinary,
            );
            if spell.landed() {
                landed += 1;
            }
            if spell.is_critical() {
                crits += 1;
            }
        }
        let crit_rate = f64::from(crits) / f64::from(landed);
        assert!((crit_rate - 0.10).abs() < 0.005, "crit rate {crit_rate}");
    }

    /// The partial resist distribution of a landed non-binary spell matches royalgiraffe's
    /// table for the resistance plus the boss's 24 level-based resistance, and is never full.
    #[test]
    fn partial_resist_distribution() {
        let mut roll = CombatRoll::from_seed(63, 17);
        // 76 + 24 = 100 of the 300 cap: 24 / 55 / 18 / 3 %.
        let ctx = spell_ctx(2000, 76);
        let n = 200_000;
        let mut counts: HashMap<MagicResistResult, u32> = HashMap::new();
        let mut landed = 0;
        for _ in 0..n {
            let spell = roll.get_spell_ability_result(
                &ctx,
                MagicSchool::Nature,
                0,
                SpellResistKind::NonBinary,
            );
            if spell.result != MagicAttackResult::Miss {
                landed += 1;
                *counts.entry(spell.resist).or_default() += 1;
            }
        }
        let fraction =
            |resist| f64::from(counts.get(&resist).copied().unwrap_or(0)) / f64::from(landed);
        for (resist, expected) in [
            (MagicResistResult::NoResist, 0.24),
            (MagicResistResult::Partial25, 0.55),
            (MagicResistResult::Partial50, 0.18),
            (MagicResistResult::Partial75, 0.03),
        ] {
            let actual = fraction(resist);
            assert!((actual - expected).abs() < 0.005, "{resist:?}: {actual}");
        }
        assert_eq!(counts.get(&MagicResistResult::FullResist), None);

        // Physical damage is never resisted.
        for _ in 0..1000 {
            assert_eq!(
                roll.get_spell_resist_result(&ctx, MagicSchool::Physical),
                MagicResistResult::NoResist
            );
            assert_eq!(
                roll.get_periodic_resist_result(&ctx, MagicSchool::Physical, false),
                MagicResistResult::NoResist
            );
        }
    }

    /// Binary spells are fully resisted instead, without the level-based resistance.
    #[test]
    fn binary_spells_are_fully_resisted() {
        let mut roll = CombatRoll::from_seed(63, 19);
        let n = 100_000;
        let binary = SpellResistKind::Binary;

        // No resistance: only the 17 % miss.
        let ctx = spell_ctx(0, 0);
        let mut outcomes: HashMap<SpellRoll, u32> = HashMap::new();
        for _ in 0..n {
            *outcomes
                .entry(roll.get_spell_ability_result(&ctx, MagicSchool::Shadow, 0, binary))
                .or_default() += 1;
        }
        let fraction = |outcomes: &HashMap<SpellRoll, u32>, spell| {
            f64::from(outcomes.get(&spell).copied().unwrap_or(0)) / f64::from(n)
        };
        assert!((fraction(&outcomes, SpellRoll::MISS) - 0.17).abs() < 0.005);
        assert_eq!(outcomes.get(&SpellRoll::FULL_RESIST), None);

        // 150 resistance of 300: 83 % × 62.5 % lands; 17 % miss, the rest resisted.
        let ctx = spell_ctx(0, 150);
        let mut outcomes: HashMap<SpellRoll, u32> = HashMap::new();
        for _ in 0..n {
            *outcomes
                .entry(roll.get_spell_ability_result(&ctx, MagicSchool::Shadow, 0, binary))
                .or_default() += 1;
        }
        let landed = 0.83 * 0.625;
        assert!((fraction(&outcomes, SpellRoll::HIT) - landed).abs() < 0.005);
        assert!((fraction(&outcomes, SpellRoll::MISS) - 0.17).abs() < 0.005);
        assert!((fraction(&outcomes, SpellRoll::FULL_RESIST) - (0.83 - landed)).abs() < 0.005);
    }

    /// A pure DoT's ticks see a tenth of the resistance; a boss's level-based 24 then costs a
    /// tick next to nothing.
    #[test]
    fn periodic_resists() {
        let mut roll = CombatRoll::from_seed(63, 23);
        let n = 100_000;
        let average = |roll: &mut CombatRoll, ctx: &MagicRollContext, pure_dot: bool| {
            (0..n)
                .map(|_| {
                    1.0 - roll
                        .get_periodic_resist_result(ctx, MagicSchool::Nature, pure_dot)
                        .damage_modifier()
                })
                .sum::<f64>()
                / f64::from(n)
        };
        let ctx = spell_ctx(0, 176);
        // (176 + 24) / 300 = ⅔: 50 % on average for a tick of a DoT with direct damage.
        let direct = average(&mut roll, &ctx, false);
        assert!((direct - 0.5).abs() < 0.005, "average resist {direct}");
        // 20 / 300 for a pure DoT's tick: 5 %.
        let pure = average(&mut roll, &ctx, true);
        assert!((pure - 0.05).abs() < 0.005, "average resist {pure}");
    }

    /// A change of spell hit or resistance takes effect: tables are keyed by what they were
    /// built from.
    #[test]
    fn magic_tables_follow_the_context() {
        let mut roll = CombatRoll::from_seed(63, 29);
        let miss_range = |roll: &mut CombatRoll, ctx: &MagicRollContext| {
            let mut random = Random::from_seed(0, ROLL_RANGE, 1);
            let table = roll
                .get_magic_attack_table(ctx, MagicSchool::Nature)
                .clone();
            (0..ROLL_RANGE)
                .find(|&r| table.get_hit_outcome(&mut random, r, 0) != MagicAttackResult::Miss)
                .unwrap()
        };
        assert_eq!(miss_range(&mut roll, &spell_ctx(0, 0)), 1700);
        assert_eq!(miss_range(&mut roll, &spell_ctx(300, 0)), 1400);
        assert_eq!(roll.magic_attack_tables.len(), 2);
        roll.drop_tables();
        assert!(roll.magic_attack_tables.is_empty());
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
