//! Character statistics.
//!
//! [`Stats`] is a plain bag of stat values (port of `Character/Stats.*`), used for base stats,
//! aura effects, the aggregated stats of equipped items, and target stats.
//! [`CharacterStats`] combines several such bags with multiplicative modifiers into the values the
//! combat code uses (port of `Character/CharacterStats.*`).
//!
//! Hit and crit chances are stored as ranges out of 10 000 (`100` = 1%), matching the attack
//! tables.

pub mod character_stats;

pub use character_stats::{
    CharacterStats, ClassStatRules, RaceStats, StatContext, TargetStatView, WeaponProfile,
};

use crate::item::{ItemStat, WeaponType};
use crate::magic_school::MagicSchool;
use crate::target::CreatureType;

/// Error returned when an [`ItemStat`] cannot be stored in a [`Stats`] bag because it is a dynamic
/// character-level modifier (attack/casting speed, mana skill reduction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("item stat {0:?} is not a static stat; apply it through CharacterStats instead")]
pub struct UnsupportedItemStat(pub ItemStat);

macro_rules! stat_accessors {
    ($(($field:ident, $get:ident, $inc:ident, $dec:ident)),* $(,)?) => {
        $(
            pub fn $get(&self) -> u32 {
                self.$field
            }

            pub fn $inc(&mut self, value: u32) {
                self.$field += value;
            }

            pub fn $dec(&mut self, value: u32) {
                self.$field = sub_checked(self.$field, value, stringify!($field));
            }
        )*
    };
}

fn sub_checked(current: u32, value: u32, what: &str) -> u32 {
    current
        .checked_sub(value)
        .unwrap_or_else(|| panic!("Underflow decrease {what}: {current} - {value}"))
}

/// A bag of additive stat values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    // Base attributes
    strength: u32,
    agility: u32,
    stamina: u32,
    intellect: u32,
    spirit: u32,

    // Defensive stats
    armor: i32,
    defense: i32,
    block_value: i32,
    block_chance: f64,
    dodge_chance: f64,
    parry_chance: f64,
    resistances: [i32; MagicSchool::ALL.len()],

    // Weapon skills
    weapon_skills: [u32; WeaponType::COUNT],

    melee_ap: u32,
    feral_ap: u32,
    ranged_ap: u32,
    melee_hit: u32,
    melee_crit: u32,
    ranged_hit: u32,
    ranged_crit: u32,
    ranged_attack_speed: u32,
    spell_hit: u32,
    spell_crit: u32,
    flat_weapon_damage: u32,

    mp5: u32,
    hp5: u32,
    spell_damage: u32,

    melee_ap_against_creature: [u32; CreatureType::COUNT],
    ranged_ap_against_creature: [u32; CreatureType::COUNT],
    spell_damage_against_creature: [u32; CreatureType::COUNT],
    magic_school_damage_bonus: [u32; MagicSchool::ALL.len()],
    magic_school_hit_bonus: [u32; MagicSchool::ALL.len()],
    magic_school_crit_bonus: [u32; MagicSchool::ALL.len()],
    magic_school_spell_penetration_bonus: [u32; MagicSchool::ALL.len()],
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a bag from item stat key/value pairs as found in data files.
    pub fn from_item_stats(
        stats: impl IntoIterator<Item = (ItemStat, f64)>,
    ) -> Result<Self, UnsupportedItemStat> {
        let mut result = Self::new();
        for (stat, value) in stats {
            result.apply_item_stat(stat, value)?;
        }
        Ok(result)
    }

    /// Adds one item stat with data-file value semantics (chances as fractions, `0.01` = 1%).
    pub fn apply_item_stat(
        &mut self,
        stat: ItemStat,
        value: f64,
    ) -> Result<(), UnsupportedItemStat> {
        let flat = || value.round() as u32;
        let signed = || value.round() as i32;
        // Chances are given as fractions; the tables want hundredths of a percent.
        let chance = || ((value * 100.0).round() as u32) * 100;

        match stat {
            ItemStat::Strength => self.increase_strength(flat()),
            ItemStat::Agility => self.increase_agility(flat()),
            ItemStat::Stamina => self.increase_stamina(flat()),
            ItemStat::Intellect => self.increase_intellect(flat()),
            ItemStat::Spirit => self.increase_spirit(flat()),
            ItemStat::CritChance => {
                self.increase_melee_aura_crit(chance());
                self.increase_ranged_crit(chance());
            }
            ItemStat::HitChance => {
                self.increase_melee_hit(chance());
                self.increase_ranged_hit(chance());
            }
            ItemStat::AttackPower => {
                self.increase_base_melee_ap(flat());
                self.increase_base_ranged_ap(flat());
            }
            ItemStat::MeleeAttackPower => self.increase_base_melee_ap(flat()),
            ItemStat::FeralAttackPower => self.increase_base_feral_ap(flat()),
            ItemStat::RangedAttackPower => self.increase_base_ranged_ap(flat()),
            ItemStat::AttackPowerBeast
            | ItemStat::AttackPowerDemon
            | ItemStat::AttackPowerDragonkin
            | ItemStat::AttackPowerElemental
            | ItemStat::AttackPowerGiant
            | ItemStat::AttackPowerHumanoid
            | ItemStat::AttackPowerMechanical
            | ItemStat::AttackPowerUndead => {
                let creature = stat.creature_type().expect("creature stat");
                self.increase_melee_ap_against_type(creature, flat());
            }
            ItemStat::WeaponDamage => self.increase_flat_weapon_damage(flat()),
            ItemStat::AxeSkill
            | ItemStat::DaggerSkill
            | ItemStat::FistSkill
            | ItemStat::MaceSkill
            | ItemStat::SwordSkill
            | ItemStat::TwohandAxeSkill
            | ItemStat::TwohandMaceSkill
            | ItemStat::TwohandSwordSkill
            | ItemStat::BowSkill
            | ItemStat::CrossbowSkill
            | ItemStat::GunSkill => {
                let weapon_type = stat.weapon_type().expect("skill stat");
                self.increase_weapon_skill(weapon_type, flat());
            }
            ItemStat::ManaPer5 => self.increase_mp5(flat()),
            ItemStat::HealthPer5 => self.increase_hp5(flat()),
            ItemStat::SpellDamage => self.increase_base_spell_damage(flat()),
            ItemStat::SpellDamageArcane
            | ItemStat::SpellDamageFire
            | ItemStat::SpellDamageFrost
            | ItemStat::SpellDamageHoly
            | ItemStat::SpellDamageNature
            | ItemStat::SpellDamageShadow => {
                let school = stat.magic_school().expect("school stat");
                self.increase_spell_damage_vs_school(flat(), school);
            }
            ItemStat::SpellDamageBeast
            | ItemStat::SpellDamageDemon
            | ItemStat::SpellDamageDragonkin
            | ItemStat::SpellDamageElemental
            | ItemStat::SpellDamageGiant
            | ItemStat::SpellDamageHumanoid
            | ItemStat::SpellDamageMechanical
            | ItemStat::SpellDamageUndead => {
                let creature = stat.creature_type().expect("creature stat");
                self.increase_spell_damage_against_type(creature, flat());
            }
            ItemStat::SpellCritChance => self.increase_spell_crit(chance()),
            ItemStat::SpellHitChance => self.increase_spell_hit(chance()),
            ItemStat::SpellPenetration => {
                for school in MagicSchool::MAGIC {
                    self.increase_spell_penetration(school, flat());
                }
            }
            ItemStat::Armor => self.increase_armor(signed()),
            ItemStat::Defense => self.increase_defense(signed()),
            ItemStat::BlockValue => self.increase_block_value(signed()),
            ItemStat::BlockChance => self.increase_block_chance(value),
            ItemStat::DodgeChance => self.increase_dodge(value),
            ItemStat::ParryChance => self.increase_parry(value),
            ItemStat::AllResistance => {
                for school in MagicSchool::MAGIC {
                    self.increase_resistance(school, signed());
                }
            }
            ItemStat::ArcaneResistance
            | ItemStat::FireResistance
            | ItemStat::FrostResistance
            | ItemStat::HolyResistance
            | ItemStat::NatureResistance
            | ItemStat::ShadowResistance => {
                let school = stat.magic_school().expect("resistance stat");
                self.increase_resistance(school, signed());
            }
            ItemStat::RangedAttackSpeed => self.increase_ranged_attack_speed(flat()),
            ItemStat::AttackSpeed
            | ItemStat::MeleeAttackSpeed
            | ItemStat::CastingSpeed
            | ItemStat::ManaSkillReduction => return Err(UnsupportedItemStat(stat)),
        }

        Ok(())
    }

    /// Adds every value of `rhs` to this bag.
    pub fn add(&mut self, rhs: &Stats) {
        self.combine(rhs, Combine::Add);
    }

    /// Subtracts every value of `rhs` from this bag.
    pub fn remove(&mut self, rhs: &Stats) {
        self.combine(rhs, Combine::Remove);
    }

    fn combine(&mut self, rhs: &Stats, op: Combine) {
        macro_rules! u32_fields {
            ($($field:ident),* $(,)?) => {
                $( self.$field = op.apply_u32(self.$field, rhs.$field, stringify!($field)); )*
            };
        }
        macro_rules! i32_fields {
            ($($field:ident),* $(,)?) => {
                $( self.$field = op.apply_i32(self.$field, rhs.$field); )*
            };
        }
        macro_rules! f64_fields {
            ($($field:ident),* $(,)?) => {
                $( self.$field = op.apply_f64(self.$field, rhs.$field); )*
            };
        }
        macro_rules! u32_arrays {
            ($($field:ident),* $(,)?) => {
                $(
                    for (mine, theirs) in self.$field.iter_mut().zip(rhs.$field.iter()) {
                        *mine = op.apply_u32(*mine, *theirs, stringify!($field));
                    }
                )*
            };
        }

        u32_fields!(
            strength,
            agility,
            stamina,
            intellect,
            spirit,
            melee_ap,
            feral_ap,
            ranged_ap,
            melee_hit,
            melee_crit,
            ranged_hit,
            ranged_crit,
            spell_hit,
            spell_crit,
            flat_weapon_damage,
            mp5,
            hp5,
            spell_damage,
        );
        i32_fields!(armor, defense, block_value);
        f64_fields!(block_chance, dodge_chance, parry_chance);
        u32_arrays!(
            weapon_skills,
            melee_ap_against_creature,
            ranged_ap_against_creature,
            spell_damage_against_creature,
            magic_school_damage_bonus,
            magic_school_hit_bonus,
            magic_school_crit_bonus,
            magic_school_spell_penetration_bonus,
        );
        for (mine, theirs) in self.resistances.iter_mut().zip(rhs.resistances.iter()) {
            *mine = op.apply_i32(*mine, *theirs);
        }

        // Ranged attack speed keeps the "only one source at a time" rule of the C++ code.
        match op {
            Combine::Add => self.increase_ranged_attack_speed(rhs.ranged_attack_speed),
            Combine::Remove => self.decrease_ranged_attack_speed(rhs.ranged_attack_speed),
        }
    }

    stat_accessors!(
        (strength, get_strength, increase_strength, decrease_strength),
        (agility, get_agility, increase_agility, decrease_agility),
        (stamina, get_stamina, increase_stamina, decrease_stamina),
        (
            intellect,
            get_intellect,
            increase_intellect,
            decrease_intellect
        ),
        (spirit, get_spirit, increase_spirit, decrease_spirit),
        (
            melee_ap,
            get_base_melee_ap,
            increase_base_melee_ap,
            decrease_base_melee_ap
        ),
        (
            feral_ap,
            get_base_feral_ap,
            increase_base_feral_ap,
            decrease_base_feral_ap
        ),
        (
            ranged_ap,
            get_base_ranged_ap,
            increase_base_ranged_ap,
            decrease_base_ranged_ap
        ),
        (
            melee_hit,
            get_melee_hit_chance,
            increase_melee_hit,
            decrease_melee_hit
        ),
        (
            melee_crit,
            get_melee_crit_chance,
            increase_melee_aura_crit,
            decrease_melee_aura_crit
        ),
        (
            ranged_hit,
            get_ranged_hit_chance,
            increase_ranged_hit,
            decrease_ranged_hit
        ),
        (
            ranged_crit,
            get_ranged_crit_chance,
            increase_ranged_crit,
            decrease_ranged_crit
        ),
        (
            flat_weapon_damage,
            get_flat_weapon_damage,
            increase_flat_weapon_damage,
            decrease_flat_weapon_damage
        ),
        (mp5, get_mp5, increase_mp5, decrease_mp5),
        (hp5, get_hp5, increase_hp5, decrease_hp5),
        (
            spell_damage,
            get_base_spell_damage,
            increase_base_spell_damage,
            decrease_base_spell_damage
        ),
    );

    pub fn get_armor(&self) -> i32 {
        self.armor
    }

    pub fn increase_armor(&mut self, value: i32) {
        self.armor += value;
    }

    pub fn decrease_armor(&mut self, value: i32) {
        self.armor -= value;
    }

    pub fn get_defense(&self) -> i32 {
        self.defense
    }

    pub fn increase_defense(&mut self, value: i32) {
        self.defense += value;
    }

    pub fn decrease_defense(&mut self, value: i32) {
        self.defense -= value;
    }

    pub fn get_block_value(&self) -> u32 {
        self.block_value.max(0) as u32
    }

    pub fn increase_block_value(&mut self, value: i32) {
        self.block_value += value;
    }

    pub fn decrease_block_value(&mut self, value: i32) {
        self.block_value -= value;
    }

    pub fn get_block_chance(&self) -> f64 {
        self.block_chance
    }

    pub fn increase_block_chance(&mut self, value: f64) {
        self.block_chance += value;
    }

    pub fn decrease_block_chance(&mut self, value: f64) {
        self.block_chance -= value;
    }

    pub fn get_dodge_chance(&self) -> f64 {
        self.dodge_chance
    }

    pub fn increase_dodge(&mut self, value: f64) {
        self.dodge_chance += value;
    }

    pub fn decrease_dodge(&mut self, value: f64) {
        self.dodge_chance -= value;
    }

    pub fn get_parry_chance(&self) -> f64 {
        self.parry_chance
    }

    pub fn increase_parry(&mut self, value: f64) {
        self.parry_chance += value;
    }

    pub fn decrease_parry(&mut self, value: f64) {
        self.parry_chance -= value;
    }

    pub fn get_resistance(&self, school: MagicSchool) -> i32 {
        self.resistances[school as usize]
    }

    pub fn increase_resistance(&mut self, school: MagicSchool, value: i32) {
        self.resistances[school as usize] += value;
    }

    pub fn decrease_resistance(&mut self, school: MagicSchool, value: i32) {
        self.resistances[school as usize] -= value;
    }

    pub fn get_weapon_skill(&self, weapon_type: WeaponType) -> u32 {
        self.weapon_skills[weapon_type.index()]
    }

    pub fn increase_weapon_skill(&mut self, weapon_type: WeaponType, value: u32) {
        self.weapon_skills[weapon_type.index()] += value;
    }

    pub fn decrease_weapon_skill(&mut self, weapon_type: WeaponType, value: u32) {
        let skill = &mut self.weapon_skills[weapon_type.index()];
        *skill = sub_checked(*skill, value, "weapon skill");
    }

    pub fn get_ranged_attack_speed_percent(&self) -> u32 {
        self.ranged_attack_speed
    }

    /// Ranged attack speed from items does not stack: only one non-zero source may be active.
    pub fn increase_ranged_attack_speed(&mut self, value: u32) {
        assert!(
            self.ranged_attack_speed == 0 || value == 0,
            "Cannot increase non-zero ranged attack speed"
        );
        self.ranged_attack_speed += value;
    }

    pub fn decrease_ranged_attack_speed(&mut self, value: u32) {
        self.ranged_attack_speed =
            sub_checked(self.ranged_attack_speed, value, "ranged attack speed");
    }

    pub fn get_spell_hit_chance(&self, school: MagicSchool) -> u32 {
        self.spell_hit + self.magic_school_hit_bonus[school as usize]
    }

    pub fn increase_spell_hit(&mut self, value: u32) {
        self.spell_hit += value;
    }

    pub fn decrease_spell_hit(&mut self, value: u32) {
        self.spell_hit = sub_checked(self.spell_hit, value, "spell hit");
    }

    pub fn increase_spell_hit_for_school(&mut self, school: MagicSchool, value: u32) {
        self.magic_school_hit_bonus[school as usize] += value;
    }

    pub fn decrease_spell_hit_for_school(&mut self, school: MagicSchool, value: u32) {
        let bonus = &mut self.magic_school_hit_bonus[school as usize];
        *bonus = sub_checked(*bonus, value, "spell school hit");
    }

    pub fn get_spell_crit_chance(&self, school: MagicSchool) -> u32 {
        self.spell_crit + self.magic_school_crit_bonus[school as usize]
    }

    pub fn increase_spell_crit(&mut self, value: u32) {
        self.spell_crit += value;
    }

    pub fn decrease_spell_crit(&mut self, value: u32) {
        self.spell_crit = sub_checked(self.spell_crit, value, "spell crit");
    }

    pub fn increase_spell_crit_for_school(&mut self, school: MagicSchool, value: u32) {
        self.magic_school_crit_bonus[school as usize] += value;
    }

    pub fn decrease_spell_crit_for_school(&mut self, school: MagicSchool, value: u32) {
        let bonus = &mut self.magic_school_crit_bonus[school as usize];
        *bonus = sub_checked(*bonus, value, "spell school crit");
    }

    pub fn get_melee_ap_against_type(&self, creature: CreatureType) -> u32 {
        self.melee_ap_against_creature[creature.index()]
    }

    pub fn increase_melee_ap_against_type(&mut self, creature: CreatureType, value: u32) {
        self.melee_ap_against_creature[creature.index()] += value;
    }

    pub fn decrease_melee_ap_against_type(&mut self, creature: CreatureType, value: u32) {
        let ap = &mut self.melee_ap_against_creature[creature.index()];
        *ap = sub_checked(*ap, value, "melee ap against type");
    }

    pub fn get_ranged_ap_against_type(&self, creature: CreatureType) -> u32 {
        self.ranged_ap_against_creature[creature.index()]
    }

    pub fn increase_ranged_ap_against_type(&mut self, creature: CreatureType, value: u32) {
        self.ranged_ap_against_creature[creature.index()] += value;
    }

    pub fn decrease_ranged_ap_against_type(&mut self, creature: CreatureType, value: u32) {
        let ap = &mut self.ranged_ap_against_creature[creature.index()];
        *ap = sub_checked(*ap, value, "ranged ap against type");
    }

    pub fn get_spell_damage_against_type(&self, creature: CreatureType) -> u32 {
        self.spell_damage_against_creature[creature.index()]
    }

    pub fn increase_spell_damage_against_type(&mut self, creature: CreatureType, value: u32) {
        self.spell_damage_against_creature[creature.index()] += value;
    }

    pub fn decrease_spell_damage_against_type(&mut self, creature: CreatureType, value: u32) {
        let damage = &mut self.spell_damage_against_creature[creature.index()];
        *damage = sub_checked(*damage, value, "spell damage against type");
    }

    /// Base spell damage plus the school-specific bonus.
    pub fn get_spell_damage(&self, school: MagicSchool) -> u32 {
        self.spell_damage + self.magic_school_damage_bonus[school as usize]
    }

    pub fn increase_spell_damage_vs_school(&mut self, value: u32, school: MagicSchool) {
        self.magic_school_damage_bonus[school as usize] += value;
    }

    pub fn decrease_spell_damage_vs_school(&mut self, value: u32, school: MagicSchool) {
        let bonus = &mut self.magic_school_damage_bonus[school as usize];
        *bonus = sub_checked(*bonus, value, "spell damage vs school");
    }

    pub fn get_spell_penetration(&self, school: MagicSchool) -> u32 {
        self.magic_school_spell_penetration_bonus[school as usize]
    }

    pub fn increase_spell_penetration(&mut self, school: MagicSchool, value: u32) {
        self.magic_school_spell_penetration_bonus[school as usize] += value;
    }

    pub fn decrease_spell_penetration(&mut self, school: MagicSchool, value: u32) {
        let bonus = &mut self.magic_school_spell_penetration_bonus[school as usize];
        *bonus = sub_checked(*bonus, value, "spell penetration");
    }
}

#[derive(Clone, Copy)]
enum Combine {
    Add,
    Remove,
}

impl Combine {
    fn apply_u32(self, lhs: u32, rhs: u32, what: &str) -> u32 {
        match self {
            Combine::Add => lhs + rhs,
            Combine::Remove => sub_checked(lhs, rhs, what),
        }
    }

    fn apply_i32(self, lhs: i32, rhs: i32) -> i32 {
        match self {
            Combine::Add => lhs + rhs,
            Combine::Remove => lhs - rhs,
        }
    }

    fn apply_f64(self, lhs: f64, rhs: f64) -> f64 {
        match self {
            Combine::Add => lhs + rhs,
            Combine::Remove => lhs - rhs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_after_initialization() {
        let stats = Stats::new();
        for school in MagicSchool::ALL {
            assert_eq!(stats.get_spell_damage(school), 0);
            assert_eq!(stats.get_spell_hit_chance(school), 0);
            assert_eq!(stats.get_spell_crit_chance(school), 0);
        }
        assert_eq!(stats.get_strength(), 0);
        assert_eq!(stats.get_armor(), 0);
    }

    #[test]
    fn values_after_add_and_remove_from_another_stats_element() {
        let mut stats = Stats::new();
        let mut other = Stats::new();
        other.increase_base_spell_damage(1);
        for school in MagicSchool::ALL {
            other.increase_spell_damage_vs_school(1, school);
        }
        other.increase_strength(10);
        other.increase_armor(50);
        other.increase_dodge(0.01);
        other.increase_weapon_skill(WeaponType::Sword, 5);
        other.increase_melee_ap_against_type(CreatureType::Undead, 100);
        other.increase_ranged_attack_speed(15);

        stats.add(&other);
        for school in MagicSchool::ALL {
            assert_eq!(stats.get_spell_damage(school), 2);
        }
        assert_eq!(stats.get_strength(), 10);
        assert_eq!(stats.get_armor(), 50);
        assert_eq!(stats.get_dodge_chance(), 0.01);
        assert_eq!(stats.get_weapon_skill(WeaponType::Sword), 5);
        assert_eq!(stats.get_melee_ap_against_type(CreatureType::Undead), 100);
        assert_eq!(stats.get_ranged_attack_speed_percent(), 15);

        stats.remove(&other);
        assert_eq!(stats, Stats::new());
    }

    #[test]
    fn weapon_skill_gains_are_independent() {
        let mut stats = Stats::new();
        for weapon_type in [
            WeaponType::TwohandAxe,
            WeaponType::TwohandMace,
            WeaponType::TwohandSword,
        ] {
            stats.increase_weapon_skill(weapon_type, 5);
            for other in WeaponType::ALL {
                let expected = if other == weapon_type { 5 } else { 0 };
                assert_eq!(stats.get_weapon_skill(other), expected);
            }
            stats.decrease_weapon_skill(weapon_type, 5);
            assert_eq!(stats.get_weapon_skill(weapon_type), 0);
        }
    }

    #[test]
    #[should_panic(expected = "Underflow decrease strength")]
    fn decreasing_below_zero_panics() {
        let mut stats = Stats::new();
        stats.decrease_strength(1);
    }

    #[test]
    #[should_panic(expected = "Cannot increase non-zero ranged attack speed")]
    fn ranged_attack_speed_does_not_stack() {
        let mut stats = Stats::new();
        stats.increase_ranged_attack_speed(10);
        stats.increase_ranged_attack_speed(10);
    }

    #[test]
    fn item_stats_use_data_file_semantics() {
        let stats = Stats::from_item_stats([
            (ItemStat::Strength, 20.0),
            (ItemStat::CritChance, 0.01),
            (ItemStat::HitChance, 0.02),
            (ItemStat::AttackPower, 40.0),
            (ItemStat::Armor, 300.0),
            (ItemStat::AllResistance, 10.0),
            (ItemStat::FireResistance, 5.0),
            (ItemStat::SpellPenetration, 20.0),
            (ItemStat::SwordSkill, 5.0),
            (ItemStat::AttackPowerUndead, 60.0),
            (ItemStat::SpellDamageFire, 30.0),
            (ItemStat::SpellCritChance, 0.02),
            (ItemStat::ManaPer5, 6.0),
        ])
        .unwrap();

        assert_eq!(stats.get_strength(), 20);
        assert_eq!(stats.get_melee_crit_chance(), 100);
        assert_eq!(stats.get_ranged_crit_chance(), 100);
        assert_eq!(stats.get_melee_hit_chance(), 200);
        assert_eq!(stats.get_ranged_hit_chance(), 200);
        assert_eq!(stats.get_base_melee_ap(), 40);
        assert_eq!(stats.get_base_ranged_ap(), 40);
        assert_eq!(stats.get_armor(), 300);
        assert_eq!(stats.get_resistance(MagicSchool::Fire), 15);
        assert_eq!(stats.get_resistance(MagicSchool::Shadow), 10);
        assert_eq!(stats.get_resistance(MagicSchool::Physical), 0);
        assert_eq!(stats.get_spell_penetration(MagicSchool::Frost), 20);
        assert_eq!(stats.get_spell_penetration(MagicSchool::Physical), 0);
        assert_eq!(stats.get_weapon_skill(WeaponType::Sword), 5);
        assert_eq!(stats.get_melee_ap_against_type(CreatureType::Undead), 60);
        assert_eq!(stats.get_spell_damage(MagicSchool::Fire), 30);
        assert_eq!(stats.get_spell_damage(MagicSchool::Frost), 0);
        assert_eq!(stats.get_spell_crit_chance(MagicSchool::Holy), 200);
        assert_eq!(stats.get_mp5(), 6);
    }

    #[test]
    fn dynamic_item_stats_are_rejected() {
        let mut stats = Stats::new();
        assert_eq!(
            stats.apply_item_stat(ItemStat::AttackSpeed, 10.0),
            Err(UnsupportedItemStat(ItemStat::AttackSpeed))
        );
        assert_eq!(
            stats.apply_item_stat(ItemStat::CastingSpeed, 10.0),
            Err(UnsupportedItemStat(ItemStat::CastingSpeed))
        );
        assert_eq!(stats, Stats::new());
    }
}
