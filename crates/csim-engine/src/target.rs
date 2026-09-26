//! The simulated target. Port of `Target/Target.*`.
//!
//! The target holds its level, armor, resistances, creature type, the stat bag that target debuffs
//! write into, and the 16-slot debuff limit with priorities. Buffs are referred to by [`InstanceId`];
//! whenever the C++ target called back into a buff (`cancel_buff`, `use_charge`), the Rust method
//! instead returns the ids for the caller to act on.

use serde::{Deserialize, Serialize};

use crate::ids::InstanceId;
use crate::magic_school::MagicSchool;
use crate::mechanics::Mechanics;
use crate::stats::{MultiplicativeStack, Stats, TargetStatView};

/// Creature type of the target; several stats depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CreatureType {
    Beast,
    Demon,
    Dragonkin,
    Elemental,
    Giant,
    Humanoid,
    Mechanical,
    Undead,
}

impl CreatureType {
    /// Every creature type, in declaration order.
    pub const ALL: [CreatureType; 8] = [
        CreatureType::Beast,
        CreatureType::Demon,
        CreatureType::Dragonkin,
        CreatureType::Elemental,
        CreatureType::Giant,
        CreatureType::Humanoid,
        CreatureType::Mechanical,
        CreatureType::Undead,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            CreatureType::Beast => "Beast",
            CreatureType::Demon => "Demon",
            CreatureType::Dragonkin => "Dragonkin",
            CreatureType::Elemental => "Elemental",
            CreatureType::Giant => "Giant",
            CreatureType::Humanoid => "Humanoid",
            CreatureType::Mechanical => "Mechanical",
            CreatureType::Undead => "Undead",
        }
    }

    /// The `CreatureType.ID` of the client tables.
    pub fn game_id(self) -> u32 {
        match self {
            CreatureType::Beast => 1,
            CreatureType::Dragonkin => 2,
            CreatureType::Demon => 3,
            CreatureType::Elemental => 4,
            CreatureType::Giant => 5,
            CreatureType::Undead => 6,
            CreatureType::Humanoid => 7,
            CreatureType::Mechanical => 9,
        }
    }
}

/// A set of creature types, written as a list (`[Giant, Dragonkin]`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "Vec<CreatureType>", into = "Vec<CreatureType>")]
pub struct CreatureTypes(u8);

impl CreatureTypes {
    pub fn contains(self, creature_type: CreatureType) -> bool {
        self.0 & (1 << creature_type.index()) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The types in a creature type mask of the client tables, bit `ID − 1` per type
    /// (`MOD_DAMAGE_DONE_VERSUS` misc value 80: Giant and Humanoid).
    pub fn from_game_mask(mask: u32) -> Self {
        CreatureType::ALL
            .into_iter()
            .filter(|t| mask & (1 << (t.game_id() - 1)) != 0)
            .collect()
    }
}

impl FromIterator<CreatureType> for CreatureTypes {
    fn from_iter<I: IntoIterator<Item = CreatureType>>(iter: I) -> Self {
        CreatureTypes(iter.into_iter().fold(0, |bits, t| bits | 1 << t.index()))
    }
}

impl From<Vec<CreatureType>> for CreatureTypes {
    fn from(types: Vec<CreatureType>) -> Self {
        types.into_iter().collect()
    }
}

impl From<CreatureTypes> for Vec<CreatureType> {
    fn from(types: CreatureTypes) -> Self {
        CreatureType::ALL
            .into_iter()
            .filter(|&t| types.contains(t))
            .collect()
    }
}

/// Priority of a debuff when competing for the target's debuff slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Invalid,
    Trash,
    Low,
    Mid,
    High,
}

impl Priority {
    const COUNT: usize = 5;

    fn index(self) -> usize {
        self as usize
    }
}

/// When a charge debuff loses a charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsumedWhen {
    /// Consumed whenever the flat spell damage bonus is read.
    OnSpellDamageFlat,
    /// Consumed whenever the school damage modifier is read.
    OnSpellDamageMod,
}

/// The target's debuff slots are full and nothing of lower priority could be evicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("target debuff limit reached")]
pub struct DebuffLimitReached;

/// The simulated enemy.
#[derive(Debug, Clone)]
pub struct Target {
    level: u32,
    base_armor: i32,
    /// Damage a blocked attack loses. Mobs have no known block value in the data, so it
    /// defaults to 0 and a blocked hit lands for its full damage.
    block_value: u32,
    creature_type: CreatureType,
    stats: Stats,
    resistances: [i32; MagicSchool::ALL.len()],
    magic_school_damage: [MultiplicativeStack; MagicSchool::ALL.len()],
    magic_school_modifier_charge_debuffs: [Vec<InstanceId>; MagicSchool::ALL.len()],
    spell_damage_charge_debuffs: Vec<InstanceId>,
    debuffs: [Vec<InstanceId>; Priority::COUNT],
    size_debuffs: usize,
    /// The armor reductions that share one slot (`EXCLUSIVE_ARMOR_REDUCTION`: Sunder Armor,
    /// Expose Armor), summed per spell; only the strongest is applied to the armor.
    exclusive_armor: Vec<(u32, i32)>,
}

impl Target {
    /// Maximum number of debuffs on a target.
    pub const DEBUFF_LIMIT: usize = 16;

    /// Default resistance of a raid boss to the resistable schools.
    pub const DEFAULT_RESISTANCE: i32 = 70;

    /// A raid boss of `level`: base armor 3750, 70 resistance to all but holy, Dragonkin.
    pub fn new(level: u32) -> Self {
        let mut stats = Stats::new();
        stats.increase_armor(Mechanics::BOSS_BASE_ARMOR);

        let mut resistances = [0; MagicSchool::ALL.len()];
        for school in MagicSchool::MAGIC {
            if school != MagicSchool::Holy {
                resistances[school as usize] = Self::DEFAULT_RESISTANCE;
            }
        }

        Self {
            level,
            base_armor: Mechanics::BOSS_BASE_ARMOR,
            block_value: 0,
            creature_type: CreatureType::Dragonkin,
            stats,
            resistances,
            magic_school_damage: Default::default(),
            magic_school_modifier_charge_debuffs: Default::default(),
            spell_damage_charge_debuffs: Vec::new(),
            debuffs: Default::default(),
            size_debuffs: 0,
            exclusive_armor: Vec::new(),
        }
    }

    /// Stats granted to attackers by debuffs on the target (e.g. Hunter's Mark, Curse of Shadow).
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn stats_mut(&mut self) -> &mut Stats {
        &mut self.stats
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn set_level(&mut self, level: u32) {
        self.level = level;
    }

    /// Defense skill (level * 5 for creatures).
    pub fn defense(&self) -> u32 {
        self.level * 5
    }

    /// Current armor, never negative.
    pub fn armor(&self) -> i32 {
        self.stats.get_armor().max(0)
    }

    pub fn base_armor(&self) -> i32 {
        self.base_armor
    }

    /// Damage a blocked attack loses.
    pub fn block_value(&self) -> u32 {
        self.block_value
    }

    pub fn set_block_value(&mut self, block_value: u32) {
        self.block_value = block_value;
    }

    /// Replaces the base armor while keeping every debuff delta applied on top of it.
    pub fn set_base_armor(&mut self, armor: i32) {
        let delta = self.base_armor - armor;
        self.base_armor = armor;
        self.stats.decrease_armor(delta);
    }

    pub fn increase_armor(&mut self, armor: i32) {
        self.stats.increase_armor(armor);
    }

    pub fn decrease_armor(&mut self, armor: i32) {
        self.stats.decrease_armor(armor);
    }

    /// Changes the exclusive armor reduction of `spell` by `delta` (negative: a stronger
    /// reduction; one call per stack). Of all spells' reductions only the strongest lowers the
    /// armor, so Sunder Armor and Expose Armor do not stack (forever-bugs #112). Keyed by spell:
    /// the same spell from two casters adds up, as other target debuffs do.
    pub fn change_exclusive_armor_reduction(&mut self, spell: u32, delta: i32) {
        let before = self.exclusive_armor_reduction();
        match self.exclusive_armor.iter().position(|(id, _)| *id == spell) {
            Some(index) => {
                self.exclusive_armor[index].1 += delta;
                if self.exclusive_armor[index].1 == 0 {
                    self.exclusive_armor.swap_remove(index);
                }
            }
            None if delta != 0 => self.exclusive_armor.push((spell, delta)),
            None => {}
        }
        let after = self.exclusive_armor_reduction();
        self.stats.decrease_armor(after - before);
    }

    /// The armor the exclusive armor reductions take away: the strongest one.
    pub fn exclusive_armor_reduction(&self) -> i32 {
        self.exclusive_armor
            .iter()
            .map(|(_, amount)| -amount)
            .max()
            .unwrap_or(0)
            .max(0)
    }

    /// Resistance to `school`, never negative.
    pub fn resistance(&self, school: MagicSchool) -> i32 {
        self.resistances[school as usize].max(0)
    }

    pub fn increase_resistance(&mut self, school: MagicSchool, value: i32) {
        self.resistances[school as usize] += value;
    }

    pub fn decrease_resistance(&mut self, school: MagicSchool, value: i32) {
        self.resistances[school as usize] -= value;
    }

    pub fn creature_type(&self) -> CreatureType {
        self.creature_type
    }

    pub fn set_creature_type(&mut self, creature_type: CreatureType) {
        self.creature_type = creature_type;
    }

    /// Damage multiplier for `school` from debuffs on the target.
    ///
    /// Charge debuffs registered with [`ConsumedWhen::OnSpellDamageMod`] lose a charge when this
    /// value is used for damage; see [`Target::charge_debuffs_for_school_mod`].
    pub fn magic_school_damage_mod(&self, school: MagicSchool) -> f64 {
        self.magic_school_damage[school as usize].modifier()
    }

    pub fn increase_magic_school_damage_mod(&mut self, percent: i32, school: MagicSchool) {
        self.magic_school_damage[school as usize].add(percent);
    }

    pub fn decrease_magic_school_damage_mod(&mut self, percent: i32, school: MagicSchool) {
        self.magic_school_damage[school as usize].remove(percent);
    }

    /// Flat spell damage bonus for `school` from debuffs on the target.
    ///
    /// Charge debuffs registered with [`ConsumedWhen::OnSpellDamageFlat`] lose a charge when this
    /// value is used for damage; see [`Target::charge_debuffs_for_spell_damage`].
    pub fn spell_damage(&self, school: MagicSchool) -> u32 {
        self.stats.get_spell_damage(school)
    }

    /// Buffs that lose a charge when the flat spell damage bonus is consumed.
    pub fn charge_debuffs_for_spell_damage(&self) -> &[InstanceId] {
        &self.spell_damage_charge_debuffs
    }

    /// Buffs that lose a charge when the damage modifier for `school` is consumed.
    pub fn charge_debuffs_for_school_mod(&self, school: MagicSchool) -> &[InstanceId] {
        &self.magic_school_modifier_charge_debuffs[school as usize]
    }

    /// Registers a charge debuff that is not school specific.
    ///
    /// # Panics
    /// Panics for [`ConsumedWhen::OnSpellDamageMod`], which needs a school.
    pub fn add_charge_debuff(&mut self, buff: InstanceId, consumed_when: ConsumedWhen) {
        match consumed_when {
            ConsumedWhen::OnSpellDamageFlat => self.spell_damage_charge_debuffs.push(buff),
            ConsumedWhen::OnSpellDamageMod => {
                panic!("Target::add_charge_debuff generic failed for {buff:?}")
            }
        }
    }

    /// Registers a school specific charge debuff.
    ///
    /// # Panics
    /// Panics for [`ConsumedWhen::OnSpellDamageFlat`], which is not school specific.
    pub fn add_charge_debuff_for_school(
        &mut self,
        buff: InstanceId,
        consumed_when: ConsumedWhen,
        school: MagicSchool,
    ) {
        match consumed_when {
            ConsumedWhen::OnSpellDamageMod => {
                self.magic_school_modifier_charge_debuffs[school as usize].push(buff)
            }
            ConsumedWhen::OnSpellDamageFlat => {
                panic!("Target::add_charge_debuff school-specific failed for {buff:?}")
            }
        }
    }

    pub fn remove_charge_debuff(&mut self, buff: InstanceId, consumed_when: ConsumedWhen) {
        match consumed_when {
            ConsumedWhen::OnSpellDamageFlat => {
                remove_buff_if_exists(&mut self.spell_damage_charge_debuffs, buff);
            }
            ConsumedWhen::OnSpellDamageMod => {
                panic!("Target::remove_charge_debuff generic failed for {buff:?}")
            }
        }
    }

    pub fn remove_charge_debuff_for_school(
        &mut self,
        buff: InstanceId,
        consumed_when: ConsumedWhen,
        school: MagicSchool,
    ) {
        match consumed_when {
            ConsumedWhen::OnSpellDamageMod => {
                remove_buff_if_exists(
                    &mut self.magic_school_modifier_charge_debuffs[school as usize],
                    buff,
                );
            }
            ConsumedWhen::OnSpellDamageFlat => {
                panic!("Target::remove_charge_debuff school-specific failed for {buff:?}")
            }
        }
    }

    /// Claims a debuff slot for `buff`.
    ///
    /// When all slots are taken, the oldest debuff of a lower priority is evicted and returned so
    /// the caller can cancel it. Fails when nothing of lower priority is present.
    ///
    /// # Panics
    /// Panics for [`Priority::Invalid`].
    pub fn add_debuff(
        &mut self,
        buff: InstanceId,
        priority: Priority,
    ) -> Result<Option<InstanceId>, DebuffLimitReached> {
        assert!(
            priority != Priority::Invalid,
            "Debuff {buff:?} has invalid priority"
        );

        let evicted = if self.size_debuffs == Self::DEBUFF_LIMIT {
            Some(
                self.remove_oldest_lowest_priority_debuff(priority)
                    .ok_or(DebuffLimitReached)?,
            )
        } else {
            None
        };

        self.debuffs[priority.index()].push(buff);
        self.size_debuffs += 1;
        Ok(evicted)
    }

    fn remove_oldest_lowest_priority_debuff(&mut self, up_to: Priority) -> Option<InstanceId> {
        for slot in &mut self.debuffs[..up_to.index()] {
            if slot.is_empty() {
                continue;
            }

            let evicted = slot.remove(0);
            self.size_debuffs -= 1;
            return Some(evicted);
        }

        None
    }

    /// Releases the debuff slot held by `buff`, if any.
    pub fn remove_debuff(&mut self, buff: InstanceId) {
        for slot in &mut self.debuffs {
            if remove_buff_if_exists(slot, buff) {
                self.size_debuffs -= 1;
                return;
            }
        }
    }

    /// Number of debuff slots in use.
    pub fn debuff_count(&self) -> usize {
        self.size_debuffs
    }

    pub fn has_debuff(&self, buff: InstanceId) -> bool {
        self.debuffs.iter().any(|slot| slot.contains(&buff))
    }

    /// Asserts that every debuff has been removed at the end of an iteration.
    ///
    /// # Panics
    /// Panics if a debuff or charge debuff is still registered.
    pub fn check_clean(&self) {
        assert!(
            self.debuffs.iter().all(Vec::is_empty),
            "Target debuffs not properly cleared"
        );
        assert!(
            self.magic_school_modifier_charge_debuffs
                .iter()
                .all(Vec::is_empty),
            "Magic school modifier buffs not properly cleared"
        );
        assert!(
            self.spell_damage_charge_debuffs.is_empty(),
            "Damage bonus buffs with charges not properly cleared"
        );
        assert_eq!(
            self.size_debuffs, 0,
            "Target debuff size unexpectedly non-zero"
        );
    }

    /// Snapshot of the target values that feed into character stats.
    pub fn stat_view(&self) -> TargetStatView {
        let mut view = TargetStatView {
            creature_type: self.creature_type,
            ranged_ap_debuff: self.stats.get_base_ranged_ap(),
            ..TargetStatView::default()
        };
        for school in MagicSchool::ALL {
            let index = school as usize;
            view.resistances[index] = self.resistance(school);
            view.spell_crit[index] = self.stats.get_spell_crit_chance(school);
            view.spell_damage[index] = self.stats.get_spell_damage(school);
            view.magic_school_damage_mod[index] = self.magic_school_damage_mod(school);
        }
        view
    }
}

fn remove_buff_if_exists(buffs: &mut Vec<InstanceId>, buff: InstanceId) -> bool {
    match buffs.iter().position(|&stored| stored == buff) {
        Some(index) => {
            buffs.remove(index);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creature_types_are_a_set_written_as_a_list() {
        let types: CreatureTypes = serde_yaml::from_str("[Giant, Dragonkin, Giant]").unwrap();
        assert!(types.contains(CreatureType::Giant));
        assert!(types.contains(CreatureType::Dragonkin));
        assert!(!types.contains(CreatureType::Humanoid));
        assert!(!types.is_empty());
        assert!(CreatureTypes::default().is_empty());
        let murder = CreatureTypes::from_game_mask(80);
        assert_eq!(
            Vec::from(murder),
            [CreatureType::Giant, CreatureType::Humanoid]
        );
        assert_eq!(
            Vec::from(CreatureTypes::from_game_mask(32)),
            [CreatureType::Undead]
        );
        assert_eq!(
            Vec::from(CreatureTypes::from_game_mask(1)),
            [CreatureType::Beast]
        );
        assert_eq!(
            serde_yaml::to_string(&types).unwrap(),
            "- Dragonkin
- Giant
"
        );
    }

    #[test]
    fn creature_type_serde_uses_display_names() {
        let parsed: CreatureType = serde_yaml::from_str("Dragonkin").unwrap();
        assert_eq!(parsed, CreatureType::Dragonkin);
        assert_eq!(
            serde_yaml::to_string(&CreatureType::Undead).unwrap().trim(),
            "Undead"
        );
        for creature_type in CreatureType::ALL {
            assert_eq!(creature_type.name(), format!("{creature_type:?}"));
        }
    }

    #[test]
    fn priority_serde_uses_lowercase() {
        assert_eq!(
            serde_yaml::from_str::<Priority>("trash").unwrap(),
            Priority::Trash
        );
        assert_eq!(
            serde_yaml::from_str::<Priority>("high").unwrap(),
            Priority::High
        );
        assert!(Priority::Trash < Priority::High);
    }

    #[test]
    fn values_after_initialization() {
        let target = Target::new(60);
        assert_eq!(target.level(), 60);
        assert_eq!(target.armor(), Mechanics::BOSS_BASE_ARMOR);
        assert_eq!(target.defense(), 300);
        assert_eq!(target.creature_type(), CreatureType::Dragonkin);
        for school in [
            MagicSchool::Arcane,
            MagicSchool::Fire,
            MagicSchool::Frost,
            MagicSchool::Nature,
            MagicSchool::Shadow,
        ] {
            assert_eq!(target.resistance(school), 70);
        }
        assert_eq!(target.resistance(MagicSchool::Holy), 0);
        assert_eq!(target.resistance(MagicSchool::Physical), 0);
        assert_eq!(target.debuff_count(), 0);
    }

    #[test]
    fn armor_increase_does_increase_armor() {
        let mut target = Target::new(60);
        target.increase_armor(500);
        assert_eq!(target.armor(), Mechanics::BOSS_BASE_ARMOR + 500);
    }

    #[test]
    fn armor_decrease_does_decrease_armor() {
        let mut target = Target::new(60);
        target.decrease_armor(500);
        assert_eq!(target.armor(), Mechanics::BOSS_BASE_ARMOR - 500);
    }

    #[test]
    fn armor_never_reduced_below_zero() {
        let mut target = Target::new(60);
        target.decrease_armor(Mechanics::BOSS_BASE_ARMOR);
        assert_eq!(target.armor(), 0);
        target.decrease_armor(500);
        assert_eq!(target.armor(), 0);
    }

    #[test]
    fn armor_increase_does_not_increase_armor_if_other_effects_outweigh() {
        let mut target = Target::new(60);
        target.decrease_armor(Mechanics::BOSS_BASE_ARMOR - 1);
        assert_eq!(target.armor(), 1);
        target.decrease_armor(500);
        target.decrease_armor(1);
        target.increase_armor(500);
        assert_eq!(target.armor(), 0);
    }

    #[test]
    fn only_the_strongest_exclusive_armor_reduction_applies() {
        const SUNDER: u32 = 11597;
        const EXPOSE: u32 = 11198;
        let mut target = Target::new(63);
        let base = Mechanics::BOSS_BASE_ARMOR;
        for stacks in 1..=5 {
            target.change_exclusive_armor_reduction(SUNDER, -450);
            assert_eq!(target.armor(), base - 450 * stacks);
        }
        // A 3 point Expose Armor is weaker than 5 Sunders: nothing changes.
        target.change_exclusive_armor_reduction(EXPOSE, -1350);
        assert_eq!(target.armor(), base - 2250);
        target.change_exclusive_armor_reduction(EXPOSE, 1350);
        // A stronger one replaces Sunder's share, and Sunder's returns when it ends.
        target.change_exclusive_armor_reduction(EXPOSE, -2700);
        assert_eq!(target.armor(), base - 2700);
        target.decrease_armor(100);
        assert_eq!(target.armor(), base - 2800, "other reductions still add up");
        target.change_exclusive_armor_reduction(EXPOSE, 2700);
        assert_eq!(target.armor(), base - 2350);
        for _ in 0..5 {
            target.change_exclusive_armor_reduction(SUNDER, 450);
        }
        assert_eq!(target.armor(), base - 100);
        assert_eq!(target.exclusive_armor_reduction(), 0);
    }

    #[test]
    fn set_base_armor_keeps_debuff_deltas() {
        let mut target = Target::new(60);
        target.decrease_armor(450);
        target.set_base_armor(4000);
        assert_eq!(target.base_armor(), 4000);
        assert_eq!(target.armor(), 3550);
        target.set_base_armor(1000);
        assert_eq!(target.armor(), 550);
    }

    #[test]
    fn resistances_never_negative() {
        let mut target = Target::new(60);
        target.decrease_resistance(MagicSchool::Fire, 100);
        assert_eq!(target.resistance(MagicSchool::Fire), 0);
        target.increase_resistance(MagicSchool::Fire, 50);
        assert_eq!(target.resistance(MagicSchool::Fire), 20);
    }

    #[test]
    fn magic_school_damage_mods_stack_multiplicatively() {
        let mut target = Target::new(60);
        target.increase_magic_school_damage_mod(10, MagicSchool::Shadow);
        target.increase_magic_school_damage_mod(20, MagicSchool::Shadow);
        assert!((target.magic_school_damage_mod(MagicSchool::Shadow) - 1.32).abs() < 1e-9);
        assert_eq!(target.magic_school_damage_mod(MagicSchool::Fire), 1.0);
        target.decrease_magic_school_damage_mod(10, MagicSchool::Shadow);
        target.decrease_magic_school_damage_mod(20, MagicSchool::Shadow);
        assert_eq!(target.magic_school_damage_mod(MagicSchool::Shadow), 1.0);
    }

    #[test]
    fn debuff_slots_evict_lower_priority() {
        let mut target = Target::new(60);
        for i in 0..8 {
            assert_eq!(target.add_debuff(InstanceId(i), Priority::Trash), Ok(None));
        }
        for i in 8..16 {
            assert_eq!(target.add_debuff(InstanceId(i), Priority::Mid), Ok(None));
        }
        assert_eq!(target.debuff_count(), 16);

        // No room for another trash debuff.
        assert_eq!(
            target.add_debuff(InstanceId(100), Priority::Trash),
            Err(DebuffLimitReached)
        );
        assert_eq!(target.debuff_count(), 16);

        // A high priority debuff evicts the oldest trash debuff.
        assert_eq!(
            target.add_debuff(InstanceId(101), Priority::High),
            Ok(Some(InstanceId(0)))
        );
        assert!(!target.has_debuff(InstanceId(0)));
        assert!(target.has_debuff(InstanceId(101)));
        assert_eq!(target.debuff_count(), 16);

        // Mid evicts trash before other mids.
        assert_eq!(
            target.add_debuff(InstanceId(102), Priority::Mid),
            Ok(Some(InstanceId(1)))
        );

        // Once trash is gone, high evicts the oldest mid.
        for i in 2..8 {
            assert_eq!(
                target.add_debuff(InstanceId(200 + i), Priority::High),
                Ok(Some(InstanceId(i)))
            );
        }
        assert_eq!(
            target.add_debuff(InstanceId(300), Priority::High),
            Ok(Some(InstanceId(8)))
        );

        target.remove_debuff(InstanceId(300));
        assert_eq!(target.debuff_count(), 15);
        target.remove_debuff(InstanceId(300));
        assert_eq!(target.debuff_count(), 15);
    }

    #[test]
    #[should_panic(expected = "has invalid priority")]
    fn invalid_priority_panics() {
        let mut target = Target::new(60);
        let _ = target.add_debuff(InstanceId(1), Priority::Invalid);
    }

    #[test]
    fn check_clean_passes_when_empty_and_panics_otherwise() {
        let mut target = Target::new(60);
        target.check_clean();
        target.add_debuff(InstanceId(1), Priority::Low).unwrap();
        let result = std::panic::catch_unwind(|| target.check_clean());
        assert!(result.is_err());
        target.remove_debuff(InstanceId(1));
        target.check_clean();
    }

    #[test]
    fn charge_debuffs_are_tracked_per_kind() {
        let mut target = Target::new(60);
        target.add_charge_debuff(InstanceId(1), ConsumedWhen::OnSpellDamageFlat);
        target.add_charge_debuff_for_school(
            InstanceId(2),
            ConsumedWhen::OnSpellDamageMod,
            MagicSchool::Fire,
        );
        assert_eq!(target.charge_debuffs_for_spell_damage(), &[InstanceId(1)]);
        assert_eq!(
            target.charge_debuffs_for_school_mod(MagicSchool::Fire),
            &[InstanceId(2)]
        );
        assert!(
            target
                .charge_debuffs_for_school_mod(MagicSchool::Frost)
                .is_empty()
        );

        target.remove_charge_debuff(InstanceId(1), ConsumedWhen::OnSpellDamageFlat);
        target.remove_charge_debuff_for_school(
            InstanceId(2),
            ConsumedWhen::OnSpellDamageMod,
            MagicSchool::Fire,
        );
        target.check_clean();
    }

    #[test]
    fn stat_view_reflects_target_state() {
        let mut target = Target::new(63);
        target.set_creature_type(CreatureType::Undead);
        target.stats_mut().increase_base_ranged_ap(110);
        target
            .stats_mut()
            .increase_spell_damage_vs_school(30, MagicSchool::Shadow);
        target
            .stats_mut()
            .increase_spell_crit_for_school(MagicSchool::Fire, 200);
        target.increase_magic_school_damage_mod(15, MagicSchool::Frost);
        target.decrease_resistance(MagicSchool::Arcane, 100);

        let view = target.stat_view();
        assert_eq!(view.creature_type, CreatureType::Undead);
        assert_eq!(view.ranged_ap_debuff, 110);
        assert_eq!(view.spell_damage[MagicSchool::Shadow as usize], 30);
        assert_eq!(view.spell_damage[MagicSchool::Fire as usize], 0);
        assert_eq!(view.spell_crit[MagicSchool::Fire as usize], 200);
        assert!((view.magic_school_damage_mod[MagicSchool::Frost as usize] - 1.15).abs() < 1e-9);
        assert_eq!(view.resistances[MagicSchool::Arcane as usize], 0);
        assert_eq!(view.resistances[MagicSchool::Shadow as usize], 70);
    }
}
