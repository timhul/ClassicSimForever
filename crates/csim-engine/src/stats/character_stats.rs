//! Aggregated character statistics. Port of `Character/CharacterStats.*`.
//!
//! The C++ class reached into the character, its equipment, the target and the combat roll through
//! pointers. Here every composite getter takes a [`StatContext`] with the values it needs from those
//! objects (equipment stats, race base stats, class rules, weapons, target). The character assembles
//! the context; the modifiers themselves live in [`CharacterStats`].
//!
//! Note for callers: changing hit chance or spell penetration invalidates cached attack tables in
//! `CombatRoll` (`update_*_miss_chance`, `update_target_resistance`). `CharacterStats` cannot reach
//! the roll object, so the character wrapper is responsible for refreshing them.

use crate::attack_mode::AttackMode;
use crate::ids::BuffId;
use crate::item::{ItemStat, WeaponType};
use crate::magic_school::MagicSchool;
use crate::mechanics::Mechanics;
use crate::target::CreatureType;

use super::Stats;

/// Race contribution to stats: base attributes and weapon skill bonuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RaceStats {
    pub strength: u32,
    pub agility: u32,
    pub stamina: u32,
    pub intellect: u32,
    pub spirit: u32,
    pub axe_skill_bonus: u32,
    pub sword_skill_bonus: u32,
    pub mace_skill_bonus: u32,
    pub bow_skill_bonus: u32,
    pub gun_skill_bonus: u32,
    pub thrown_skill_bonus: u32,
}

impl RaceStats {
    /// Racial weapon skill bonus for a weapon type.
    pub fn weapon_skill_bonus(&self, weapon_type: WeaponType) -> u32 {
        match weapon_type {
            WeaponType::Axe | WeaponType::TwohandAxe => self.axe_skill_bonus,
            WeaponType::Sword | WeaponType::TwohandSword => self.sword_skill_bonus,
            WeaponType::Mace | WeaponType::TwohandMace => self.mace_skill_bonus,
            WeaponType::Bow => self.bow_skill_bonus,
            WeaponType::Gun => self.gun_skill_bonus,
            WeaponType::Thrown => self.thrown_skill_bonus,
            _ => 0,
        }
    }
}

/// Class-specific stat conversion rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassStatRules {
    /// Agility needed for 1% physical crit.
    pub agility_per_percent_crit: f64,
    /// Intellect needed for 1% spell crit.
    pub intellect_per_percent_spell_crit: f64,
    pub melee_ap_per_strength: u32,
    pub melee_ap_per_agility: u32,
    pub ranged_ap_per_agility: u32,
}

/// The parts of an equipped weapon that stats depend on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponProfile {
    pub weapon_type: WeaponType,
    /// Base weapon speed in seconds.
    pub speed: f64,
}

/// Target values that feed into character stats.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetStatView {
    pub creature_type: CreatureType,
    /// Resistance per school before spell penetration.
    pub resistances: [i32; MagicSchool::ALL.len()],
    /// Ranged attack power granted by target debuffs (e.g. Hunter's Mark).
    pub ranged_ap_debuff: u32,
    /// Spell crit per school granted by target debuffs.
    pub spell_crit: [u32; MagicSchool::ALL.len()],
    /// Spell damage per school granted by target debuffs.
    pub spell_damage: [u32; MagicSchool::ALL.len()],
    /// Damage multiplier per school from target debuffs.
    pub magic_school_damage_mod: [f64; MagicSchool::ALL.len()],
}

impl Default for TargetStatView {
    fn default() -> Self {
        Self {
            creature_type: CreatureType::Dragonkin,
            resistances: [0; MagicSchool::ALL.len()],
            ranged_ap_debuff: 0,
            spell_crit: [0; MagicSchool::ALL.len()],
            spell_damage: [0; MagicSchool::ALL.len()],
            magic_school_damage_mod: [1.0; MagicSchool::ALL.len()],
        }
    }
}

/// Everything outside `CharacterStats` that its composite getters read.
#[derive(Debug, Clone, Copy)]
pub struct StatContext<'a> {
    /// Aggregated stats of the equipped items.
    pub equipment: &'a Stats,
    pub race: &'a RaceStats,
    pub class: &'a ClassStatRules,
    pub clvl: u32,
    pub mechanics: &'a Mechanics,
    pub mainhand: Option<WeaponProfile>,
    pub offhand: Option<WeaponProfile>,
    pub ranged: Option<WeaponProfile>,
    pub attack_mode: AttackMode,
    /// Druids in Cat/Bear form gain feral attack power from items.
    pub druid_feral_form: bool,
    pub target: &'a TargetStatView,
}

/// A stack of percentage modifiers that combine multiplicatively.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiplicativeStack {
    effects: Vec<i32>,
    modifier: f64,
}

impl Default for MultiplicativeStack {
    fn default() -> Self {
        Self {
            effects: Vec::new(),
            modifier: 1.0,
        }
    }
}

impl MultiplicativeStack {
    pub fn modifier(&self) -> f64 {
        self.modifier
    }

    /// Adds a percentage effect (`10` = +10%, `-20` = -20%).
    pub fn add(&mut self, percent: i32) {
        self.effects.push(percent);
        self.recalculate();
    }

    /// Removes a previously added percentage effect.
    ///
    /// # Panics
    /// Panics if no such effect is active.
    pub fn remove(&mut self, percent: i32) {
        let index = self
            .effects
            .iter()
            .position(|&effect| effect == percent)
            .expect("Failed to remove multiplicative effect");
        self.effects.swap_remove(index);
        self.recalculate();
    }

    fn recalculate(&mut self) {
        self.modifier = self
            .effects
            .iter()
            .map(|&effect| 1.0 + f64::from(effect) / 100.0)
            .product();
        assert!(self.modifier > 0.0, "Modifier negative");
    }
}

/// Combined statistics of one character.
#[derive(Debug, Clone, Default)]
pub struct CharacterStats {
    base_stats: Stats,
    aura_effects: Stats,

    melee_attack_speed: MultiplicativeStack,
    ranged_attack_speed: MultiplicativeStack,
    casting_speed: MultiplicativeStack,
    phys_dmg: MultiplicativeStack,
    threat: MultiplicativeStack,
    phys_damage_taken: MultiplicativeStack,
    spell_damage_taken: MultiplicativeStack,
    ap_total: MultiplicativeStack,
    agility: MultiplicativeStack,
    intellect: MultiplicativeStack,
    spirit: MultiplicativeStack,
    stamina: MultiplicativeStack,
    strength: MultiplicativeStack,
    armor: MultiplicativeStack,
    magic_damage_per_creature: [MultiplicativeStack; CreatureType::COUNT],
    magic_school_damage: [MultiplicativeStack; MagicSchool::ALL.len()],

    casting_time_suppression_buffs: Vec<BuffId>,
    crit_bonuses_per_weapon_type: [u32; WeaponType::COUNT],
    damage_bonuses_per_weapon_type: [i32; WeaponType::COUNT],
    damage_bonuses_per_creature: [f64; CreatureType::COUNT],
    crit_dmg_bonuses_per_creature: [f64; CreatureType::COUNT],

    mh_weapon_dmg_bonus: u32,
    oh_weapon_dmg_bonus: u32,
    ranged_weapon_dmg_bonus: u32,
    physical_flat_dmg_bonus: u32,
    mana_skill_reduction: u32,
    casting_speed_flat_reduction: u32,
    crit_penalty: u32,

    melee_ability_crit_dmg_mod: f64,
    ranged_ability_crit_dmg_mod: f64,
    spell_crit_dmg_mod: f64,
}

fn sub_checked(current: u32, value: u32, what: &str) -> u32 {
    current
        .checked_sub(value)
        .unwrap_or_else(|| panic!("Underflow decrease {what}: {current} - {value}"))
}

impl CharacterStats {
    pub fn new() -> Self {
        Self {
            melee_ability_crit_dmg_mod: 2.0,
            ranged_ability_crit_dmg_mod: 2.0,
            spell_crit_dmg_mod: 1.5,
            ..Default::default()
        }
    }

    /// Stats from talents, buffs and racial/class base values.
    pub fn base_stats(&self) -> &Stats {
        &self.base_stats
    }

    pub fn base_stats_mut(&mut self) -> &mut Stats {
        &mut self.base_stats
    }

    /// Crit from auras, subject to per-level crit suppression.
    pub fn aura_effects(&self) -> &Stats {
        &self.aura_effects
    }

    // ---------------------------------------------------------------- attributes

    pub fn get_armor(&self, ctx: &StatContext) -> u32 {
        let armor = self.base_stats.get_armor() + ctx.equipment.get_armor();
        (self.armor.modifier() * f64::from(armor)) as u32 + self.get_agility(ctx) * 2
    }

    pub fn get_block_value(&self, ctx: &StatContext) -> u32 {
        self.base_stats.get_block_value() + ctx.equipment.get_block_value()
    }

    fn attribute(
        &self,
        modifier: &MultiplicativeStack,
        base: u32,
        equipment: u32,
        race: u32,
    ) -> u32 {
        (modifier.modifier() * f64::from(base + equipment + race)).round() as u32
    }

    pub fn get_strength(&self, ctx: &StatContext) -> u32 {
        self.attribute(
            &self.strength,
            self.base_stats.get_strength(),
            ctx.equipment.get_strength(),
            ctx.race.strength,
        )
    }

    pub fn get_agility(&self, ctx: &StatContext) -> u32 {
        self.attribute(
            &self.agility,
            self.base_stats.get_agility(),
            ctx.equipment.get_agility(),
            ctx.race.agility,
        )
    }

    pub fn get_stamina(&self, ctx: &StatContext) -> u32 {
        self.attribute(
            &self.stamina,
            self.base_stats.get_stamina(),
            ctx.equipment.get_stamina(),
            ctx.race.stamina,
        )
    }

    pub fn get_intellect(&self, ctx: &StatContext) -> u32 {
        self.attribute(
            &self.intellect,
            self.base_stats.get_intellect(),
            ctx.equipment.get_intellect(),
            ctx.race.intellect,
        )
    }

    pub fn get_spirit(&self, ctx: &StatContext) -> u32 {
        self.attribute(
            &self.spirit,
            self.base_stats.get_spirit(),
            ctx.equipment.get_spirit(),
            ctx.race.spirit,
        )
    }

    pub fn increase_strength(&mut self, value: u32) {
        self.base_stats.increase_strength(value);
    }

    pub fn decrease_strength(&mut self, value: u32) {
        self.base_stats.decrease_strength(value);
    }

    pub fn increase_agility(&mut self, value: u32) {
        self.base_stats.increase_agility(value);
    }

    pub fn decrease_agility(&mut self, value: u32) {
        self.base_stats.decrease_agility(value);
    }

    pub fn increase_stamina(&mut self, value: u32) {
        self.base_stats.increase_stamina(value);
    }

    pub fn decrease_stamina(&mut self, value: u32) {
        self.base_stats.decrease_stamina(value);
    }

    pub fn increase_intellect(&mut self, value: u32) {
        self.base_stats.increase_intellect(value);
    }

    pub fn decrease_intellect(&mut self, value: u32) {
        self.base_stats.decrease_intellect(value);
    }

    pub fn increase_spirit(&mut self, value: u32) {
        self.base_stats.increase_spirit(value);
    }

    pub fn decrease_spirit(&mut self, value: u32) {
        self.base_stats.decrease_spirit(value);
    }

    pub fn increase_armor(&mut self, value: u32) {
        self.base_stats.increase_armor(value as i32);
    }

    pub fn decrease_armor(&mut self, value: u32) {
        self.base_stats.decrease_armor(value as i32);
    }

    pub fn increase_block_value(&mut self, value: u32) {
        self.base_stats.increase_block_value(value as i32);
    }

    pub fn decrease_block_value(&mut self, value: u32) {
        self.base_stats.decrease_block_value(value as i32);
    }

    pub fn increase_dodge(&mut self, value: f64) {
        self.base_stats.increase_dodge(value);
    }

    pub fn decrease_dodge(&mut self, value: f64) {
        self.base_stats.decrease_dodge(value);
    }

    // ---------------------------------------------------------------- weapon skill

    fn weapon_skill(&self, ctx: &StatContext, weapon: Option<WeaponProfile>) -> u32 {
        let level_based_skill = ctx.clvl * 5;
        let Some(weapon) = weapon else {
            return level_based_skill;
        };

        if !weapon.weapon_type.has_weapon_skill() {
            return level_based_skill;
        }

        let bonus = ctx.race.weapon_skill_bonus(weapon.weapon_type)
            + ctx.equipment.get_weapon_skill(weapon.weapon_type)
            + self.base_stats.get_weapon_skill(weapon.weapon_type);

        level_based_skill + bonus
    }

    pub fn get_mh_wpn_skill(&self, ctx: &StatContext) -> u32 {
        self.weapon_skill(ctx, ctx.mainhand)
    }

    pub fn get_oh_wpn_skill(&self, ctx: &StatContext) -> u32 {
        self.weapon_skill(ctx, ctx.offhand)
    }

    pub fn get_ranged_wpn_skill(&self, ctx: &StatContext) -> u32 {
        self.weapon_skill(ctx, ctx.ranged)
    }

    /// # Panics
    /// Panics for weapon types without a weapon skill.
    pub fn increase_wpn_skill(&mut self, weapon_type: WeaponType, value: u32) {
        assert!(
            weapon_type.has_weapon_skill(),
            "increase_wpn_skill has no effect for weapon type {weapon_type:?}"
        );
        self.base_stats.increase_weapon_skill(weapon_type, value);
    }

    /// # Panics
    /// Panics for weapon types without a weapon skill.
    pub fn decrease_wpn_skill(&mut self, weapon_type: WeaponType, value: u32) {
        assert!(
            weapon_type.has_weapon_skill(),
            "decrease_wpn_skill has no effect for weapon type {weapon_type:?}"
        );
        self.base_stats.decrease_weapon_skill(weapon_type, value);
    }

    // ---------------------------------------------------------------- hit & crit

    pub fn get_melee_hit_chance(&self, ctx: &StatContext) -> u32 {
        self.base_stats.get_melee_hit_chance() + ctx.equipment.get_melee_hit_chance()
    }

    pub fn get_ranged_hit_chance(&self, ctx: &StatContext) -> u32 {
        self.base_stats.get_ranged_hit_chance() + ctx.equipment.get_ranged_hit_chance()
    }

    fn crit_from_agility(&self, ctx: &StatContext) -> u32 {
        (f64::from(self.get_agility(ctx)) / ctx.class.agility_per_percent_crit * 100.0).round()
            as u32
    }

    fn melee_crit_chance(&self, ctx: &StatContext, weapon: Option<WeaponProfile>) -> u32 {
        let equip_effect =
            self.aura_effects.get_melee_crit_chance() + ctx.equipment.get_melee_crit_chance();
        let crit_from_wpn_type = weapon
            .map(|weapon| self.crit_bonuses_per_weapon_type[weapon.weapon_type.index()])
            .unwrap_or(0);

        let aura_crit = ctx
            .mechanics
            .suppressed_aura_crit_chance(ctx.clvl, equip_effect + crit_from_wpn_type);
        let crit_chance =
            self.crit_from_agility(ctx) + aura_crit + self.base_stats.get_melee_crit_chance();

        crit_chance.saturating_sub(self.crit_penalty)
    }

    pub fn get_mh_crit_chance(&self, ctx: &StatContext) -> u32 {
        self.melee_crit_chance(ctx, ctx.mainhand)
    }

    pub fn get_oh_crit_chance(&self, ctx: &StatContext) -> u32 {
        if ctx.offhand.is_none() {
            return 0;
        }
        self.melee_crit_chance(ctx, ctx.offhand)
    }

    pub fn get_ranged_crit_chance(&self, ctx: &StatContext) -> u32 {
        let equip_effect =
            self.base_stats.get_ranged_crit_chance() + ctx.equipment.get_ranged_crit_chance();
        let crit_from_wpn_type = ctx
            .ranged
            .map(|weapon| self.crit_bonuses_per_weapon_type[weapon.weapon_type.index()])
            .unwrap_or(0);

        let aura_crit = ctx
            .mechanics
            .suppressed_aura_crit_chance(ctx.clvl, equip_effect + crit_from_wpn_type);
        let crit_chance = self.crit_from_agility(ctx) + aura_crit;

        crit_chance.saturating_sub(self.crit_penalty)
    }

    pub fn increase_melee_hit(&mut self, value: u32) {
        self.base_stats.increase_melee_hit(value);
    }

    pub fn decrease_melee_hit(&mut self, value: u32) {
        self.base_stats.decrease_melee_hit(value);
    }

    /// Crit that is subject to per-level suppression (gear, buffs).
    pub fn increase_melee_aura_crit(&mut self, value: u32) {
        self.aura_effects.increase_melee_aura_crit(value);
    }

    pub fn decrease_melee_aura_crit(&mut self, value: u32) {
        self.aura_effects.decrease_melee_aura_crit(value);
    }

    /// Crit that is not suppressed (class base crit, talents).
    pub fn increase_melee_base_crit(&mut self, value: u32) {
        self.base_stats.increase_melee_aura_crit(value);
    }

    pub fn decrease_melee_base_crit(&mut self, value: u32) {
        self.base_stats.decrease_melee_aura_crit(value);
    }

    pub fn increase_ranged_hit(&mut self, value: u32) {
        self.base_stats.increase_ranged_hit(value);
    }

    pub fn decrease_ranged_hit(&mut self, value: u32) {
        self.base_stats.decrease_ranged_hit(value);
    }

    pub fn increase_ranged_crit(&mut self, value: u32) {
        self.base_stats.increase_ranged_crit(value);
    }

    pub fn decrease_ranged_crit(&mut self, value: u32) {
        self.base_stats.decrease_ranged_crit(value);
    }

    pub fn increase_crit_for_weapon_type(&mut self, weapon_type: WeaponType, value: u32) {
        self.crit_bonuses_per_weapon_type[weapon_type.index()] += value;
    }

    pub fn decrease_crit_for_weapon_type(&mut self, weapon_type: WeaponType, value: u32) {
        let bonus = &mut self.crit_bonuses_per_weapon_type[weapon_type.index()];
        *bonus = sub_checked(*bonus, value, "crit for weapon type");
    }

    pub fn increase_total_phys_dmg_for_weapon_type(&mut self, weapon_type: WeaponType, value: i32) {
        self.damage_bonuses_per_weapon_type[weapon_type.index()] += value;
    }

    pub fn decrease_total_phys_dmg_for_weapon_type(&mut self, weapon_type: WeaponType, value: i32) {
        self.damage_bonuses_per_weapon_type[weapon_type.index()] -= value;
    }

    pub fn increase_crit_penalty(&mut self, value: u32) {
        self.crit_penalty += value;
    }

    // ---------------------------------------------------------------- attack power

    pub fn get_melee_ap(&self, ctx: &StatContext) -> u32 {
        let creature = ctx.target.creature_type;
        let stat_melee_ap = ctx.equipment.get_base_melee_ap() + self.base_stats.get_base_melee_ap();
        let attributes_ap = self.get_strength(ctx) * ctx.class.melee_ap_per_strength
            + self.get_agility(ctx) * ctx.class.melee_ap_per_agility;
        let target_ap = ctx.equipment.get_melee_ap_against_type(creature)
            + self.base_stats.get_melee_ap_against_type(creature);
        let feral_ap = if ctx.druid_feral_form {
            ctx.equipment.get_base_feral_ap()
        } else {
            0
        };

        (self.ap_total.modifier() * f64::from(stat_melee_ap + attributes_ap + target_ap + feral_ap))
            .round() as u32
    }

    pub fn get_ranged_ap(&self, ctx: &StatContext) -> u32 {
        let creature = ctx.target.creature_type;
        let stat_ranged_ap =
            ctx.equipment.get_base_ranged_ap() + self.base_stats.get_base_ranged_ap();
        let attributes_ap = self.get_agility(ctx) * ctx.class.ranged_ap_per_agility;
        let target_ap = ctx.equipment.get_ranged_ap_against_type(creature)
            + self.base_stats.get_ranged_ap_against_type(creature);

        (self.ap_total.modifier()
            * f64::from(stat_ranged_ap + attributes_ap + target_ap + ctx.target.ranged_ap_debuff))
        .round() as u32
    }

    pub fn increase_melee_ap(&mut self, value: u32) {
        self.base_stats.increase_base_melee_ap(value);
    }

    pub fn decrease_melee_ap(&mut self, value: u32) {
        self.base_stats.decrease_base_melee_ap(value);
    }

    pub fn increase_feral_ap(&mut self, value: u32) {
        self.base_stats.increase_base_feral_ap(value);
    }

    pub fn decrease_feral_ap(&mut self, value: u32) {
        self.base_stats.decrease_base_feral_ap(value);
    }

    pub fn increase_ranged_ap(&mut self, value: u32) {
        self.base_stats.increase_base_ranged_ap(value);
    }

    pub fn decrease_ranged_ap(&mut self, value: u32) {
        self.base_stats.decrease_base_ranged_ap(value);
    }

    pub fn increase_ap_vs_type(&mut self, creature: CreatureType, value: u32) {
        self.base_stats
            .increase_melee_ap_against_type(creature, value);
        self.base_stats
            .increase_ranged_ap_against_type(creature, value);
    }

    pub fn decrease_ap_vs_type(&mut self, creature: CreatureType, value: u32) {
        self.base_stats
            .decrease_melee_ap_against_type(creature, value);
        self.base_stats
            .decrease_ranged_ap_against_type(creature, value);
    }

    pub fn add_ap_multiplier(&mut self, percent: i32) {
        self.ap_total.add(percent);
    }

    pub fn remove_ap_multiplier(&mut self, percent: i32) {
        self.ap_total.remove(percent);
    }

    // ---------------------------------------------------------------- damage modifiers

    /// Total physical damage multiplier: buffs, weapon-type talents (for the weapon of the
    /// current attack mode) and creature-type bonuses.
    pub fn get_total_physical_damage_mod(&self, ctx: &StatContext) -> f64 {
        let weapon = match ctx.attack_mode {
            AttackMode::MeleeAttack => ctx.mainhand,
            AttackMode::RangedAttack => ctx.ranged,
            AttackMode::MagicAttack => None,
        };

        let dmg_bonus_from_wpn_type = 1.0
            + weapon
                .map(|weapon| {
                    f64::from(self.damage_bonuses_per_weapon_type[weapon.weapon_type.index()])
                        / 100.0
                })
                .unwrap_or(0.0);
        let dmg_bonus_from_creature =
            1.0 + self.damage_bonuses_per_creature[ctx.target.creature_type.index()];

        self.phys_dmg.modifier() * dmg_bonus_from_wpn_type * dmg_bonus_from_creature
    }

    pub fn increase_total_phys_dmg_mod(&mut self, percent: i32) {
        self.phys_dmg.add(percent);
    }

    pub fn decrease_total_phys_dmg_mod(&mut self, percent: i32) {
        self.phys_dmg.remove(percent);
    }

    /// `value` is a fraction (`0.05` = +5%).
    pub fn increase_dmg_vs_type(&mut self, creature: CreatureType, value: f64) {
        self.damage_bonuses_per_creature[creature.index()] += value;
    }

    pub fn decrease_dmg_vs_type(&mut self, creature: CreatureType, value: f64) {
        self.damage_bonuses_per_creature[creature.index()] -= value;
    }

    /// `value` is in percent (`1` = +1% crit damage against the creature type).
    pub fn increase_crit_dmg_vs_type(&mut self, creature: CreatureType, value: u32) {
        self.crit_dmg_bonuses_per_creature[creature.index()] += f64::from(value) / 100.0;
    }

    pub fn decrease_crit_dmg_vs_type(&mut self, creature: CreatureType, value: u32) {
        self.crit_dmg_bonuses_per_creature[creature.index()] -= f64::from(value) / 100.0;
    }

    pub fn get_total_threat_mod(&self) -> f64 {
        self.threat.modifier()
    }

    pub fn increase_total_threat_mod(&mut self, percent: i32) {
        self.threat.add(percent);
    }

    pub fn decrease_total_threat_mod(&mut self, percent: i32) {
        self.threat.remove(percent);
    }

    pub fn get_physical_damage_taken_mod(&self) -> f64 {
        self.phys_damage_taken.modifier()
    }

    pub fn add_phys_damage_taken_mod(&mut self, percent: i32) {
        self.phys_damage_taken.add(percent);
    }

    pub fn remove_phys_damage_taken_mod(&mut self, percent: i32) {
        self.phys_damage_taken.remove(percent);
    }

    pub fn get_spell_damage_taken_mod(&self) -> f64 {
        self.spell_damage_taken.modifier()
    }

    pub fn add_spell_damage_taken_mod(&mut self, percent: i32) {
        self.spell_damage_taken.add(percent);
    }

    pub fn remove_spell_damage_taken_mod(&mut self, percent: i32) {
        self.spell_damage_taken.remove(percent);
    }

    pub fn get_melee_ability_crit_dmg_mod(&self, ctx: &StatContext) -> f64 {
        self.melee_ability_crit_dmg_mod
            + self.crit_dmg_bonuses_per_creature[ctx.target.creature_type.index()]
    }

    pub fn increase_melee_ability_crit_dmg_mod(&mut self, value: f64) {
        self.melee_ability_crit_dmg_mod += value;
    }

    pub fn decrease_melee_ability_crit_dmg_mod(&mut self, value: f64) {
        self.melee_ability_crit_dmg_mod -= value;
    }

    pub fn get_ranged_ability_crit_dmg_mod(&self, ctx: &StatContext) -> f64 {
        self.ranged_ability_crit_dmg_mod
            + self.crit_dmg_bonuses_per_creature[ctx.target.creature_type.index()]
    }

    pub fn get_spell_crit_dmg_mod(&self, ctx: &StatContext) -> f64 {
        self.spell_crit_dmg_mod
            + self.crit_dmg_bonuses_per_creature[ctx.target.creature_type.index()]
    }

    pub fn increase_spell_crit_dmg_mod(&mut self, value: f64) {
        self.spell_crit_dmg_mod += value;
    }

    pub fn decrease_spell_crit_dmg_mod(&mut self, value: f64) {
        self.spell_crit_dmg_mod -= value;
    }

    // ---------------------------------------------------------------- speed

    pub fn get_melee_attack_speed_mod(&self) -> f64 {
        self.melee_attack_speed.modifier()
    }

    /// Only updates the modifier; the character must re-time pending swings.
    pub fn increase_melee_attack_speed(&mut self, percent: u32) {
        self.melee_attack_speed.add(percent as i32);
    }

    pub fn decrease_melee_attack_speed(&mut self, percent: u32) {
        self.melee_attack_speed.remove(percent as i32);
    }

    pub fn get_ranged_attack_speed_mod(&self) -> f64 {
        self.ranged_attack_speed.modifier()
    }

    pub fn increase_ranged_attack_speed(&mut self, percent: u32) {
        self.ranged_attack_speed.add(percent as i32);
    }

    pub fn decrease_ranged_attack_speed(&mut self, percent: u32) {
        self.ranged_attack_speed.remove(percent as i32);
    }

    pub fn get_casting_speed_mod(&self) -> f64 {
        self.casting_speed.modifier()
    }

    pub fn increase_casting_speed_mod(&mut self, percent: u32) {
        self.casting_speed.add(percent as i32);
    }

    pub fn decrease_casting_speed_mod(&mut self, percent: u32) {
        self.casting_speed.remove(percent as i32);
    }

    /// The buff that currently makes casts instant, if any. The caller consumes a charge from it.
    pub fn casting_time_suppression_buff(&self) -> Option<BuffId> {
        self.casting_time_suppression_buffs.last().copied()
    }

    pub fn casting_time_suppressed(&self) -> bool {
        !self.casting_time_suppression_buffs.is_empty()
    }

    /// # Panics
    /// Panics if the buff is already registered.
    pub fn suppress_casting_time(&mut self, buff: BuffId) {
        assert!(
            !self.casting_time_suppression_buffs.contains(&buff),
            "Tried to add the same cast time suppression buff multiple times"
        );
        self.casting_time_suppression_buffs.push(buff);
    }

    /// # Panics
    /// Panics if the buff is not registered.
    pub fn return_casting_time(&mut self, buff: BuffId) {
        let index = self
            .casting_time_suppression_buffs
            .iter()
            .position(|&stored| stored == buff)
            .unwrap_or_else(|| panic!("Failed to remove casting time suppression buff {buff:?}"));
        self.casting_time_suppression_buffs.remove(index);
    }

    pub fn get_casting_speed_flat_reduction(&self) -> u32 {
        self.casting_speed_flat_reduction
    }

    pub fn increase_casting_speed_flat_reduction(&mut self, value: u32) {
        self.casting_speed_flat_reduction += value;
    }

    pub fn decrease_casting_speed_flat_reduction(&mut self, value: u32) {
        self.casting_speed_flat_reduction = sub_checked(
            self.casting_speed_flat_reduction,
            value,
            "flat casting speed",
        );
    }

    /// Mainhand swing time in seconds (2.0 unarmed).
    pub fn get_mh_wpn_speed(&self, ctx: &StatContext) -> f64 {
        ctx.mainhand.map(|weapon| weapon.speed).unwrap_or(2.0) / self.melee_attack_speed.modifier()
    }

    /// Offhand swing time in seconds (300 when no offhand weapon is equipped).
    pub fn get_oh_wpn_speed(&self, ctx: &StatContext) -> f64 {
        ctx.offhand
            .map(|weapon| weapon.speed / self.melee_attack_speed.modifier())
            .unwrap_or(300.0)
    }

    /// Ranged shot time in seconds (300 when no ranged weapon is equipped).
    pub fn get_ranged_wpn_speed(&self, ctx: &StatContext) -> f64 {
        ctx.ranged
            .map(|weapon| weapon.speed / self.ranged_attack_speed.modifier())
            .unwrap_or(300.0)
    }

    // ---------------------------------------------------------------- stat multipliers

    pub fn add_total_stat_mod(&mut self, percent: i32) {
        self.add_agility_mod(percent);
        self.add_intellect_mod(percent);
        self.add_spirit_mod(percent);
        self.add_stamina_mod(percent);
        self.add_strength_mod(percent);
    }

    pub fn remove_total_stat_mod(&mut self, percent: i32) {
        self.remove_agility_mod(percent);
        self.remove_intellect_mod(percent);
        self.remove_spirit_mod(percent);
        self.remove_stamina_mod(percent);
        self.remove_strength_mod(percent);
    }

    pub fn add_agility_mod(&mut self, percent: i32) {
        self.agility.add(percent);
    }

    pub fn remove_agility_mod(&mut self, percent: i32) {
        self.agility.remove(percent);
    }

    pub fn add_intellect_mod(&mut self, percent: i32) {
        self.intellect.add(percent);
    }

    pub fn remove_intellect_mod(&mut self, percent: i32) {
        self.intellect.remove(percent);
    }

    pub fn add_spirit_mod(&mut self, percent: i32) {
        self.spirit.add(percent);
    }

    pub fn remove_spirit_mod(&mut self, percent: i32) {
        self.spirit.remove(percent);
    }

    pub fn add_stamina_mod(&mut self, percent: i32) {
        self.stamina.add(percent);
    }

    pub fn remove_stamina_mod(&mut self, percent: i32) {
        self.stamina.remove(percent);
    }

    pub fn add_strength_mod(&mut self, percent: i32) {
        self.strength.add(percent);
    }

    pub fn remove_strength_mod(&mut self, percent: i32) {
        self.strength.remove(percent);
    }

    pub fn add_armor_mod(&mut self, percent: i32) {
        self.armor.add(percent);
    }

    pub fn remove_armor_mod(&mut self, percent: i32) {
        self.armor.remove(percent);
    }

    // ---------------------------------------------------------------- weapon damage bonuses

    pub fn get_mh_weapon_damage_bonus(&self, ctx: &StatContext) -> u32 {
        self.mh_weapon_dmg_bonus
            + ctx.equipment.get_flat_weapon_damage()
            + ctx.equipment.get_mh_weapon_damage()
    }

    pub fn increase_mh_weapon_damage_bonus(&mut self, value: u32) {
        self.mh_weapon_dmg_bonus += value;
    }

    pub fn decrease_mh_weapon_damage_bonus(&mut self, value: u32) {
        self.mh_weapon_dmg_bonus =
            sub_checked(self.mh_weapon_dmg_bonus, value, "mh weapon damage bonus");
    }

    pub fn get_oh_weapon_damage_bonus(&self, ctx: &StatContext) -> u32 {
        self.oh_weapon_dmg_bonus
            + ctx.equipment.get_flat_weapon_damage()
            + ctx.equipment.get_oh_weapon_damage()
    }

    pub fn increase_oh_weapon_damage_bonus(&mut self, value: u32) {
        self.oh_weapon_dmg_bonus += value;
    }

    pub fn decrease_oh_weapon_damage_bonus(&mut self, value: u32) {
        self.oh_weapon_dmg_bonus =
            sub_checked(self.oh_weapon_dmg_bonus, value, "oh weapon damage bonus");
    }

    pub fn get_ranged_weapon_damage_bonus(&self, ctx: &StatContext) -> u32 {
        self.ranged_weapon_dmg_bonus + ctx.equipment.get_ranged_weapon_damage()
    }

    pub fn increase_ranged_weapon_damage_bonus(&mut self, value: u32) {
        self.ranged_weapon_dmg_bonus += value;
    }

    pub fn decrease_ranged_weapon_damage_bonus(&mut self, value: u32) {
        self.ranged_weapon_dmg_bonus = sub_checked(
            self.ranged_weapon_dmg_bonus,
            value,
            "ranged weapon damage bonus",
        );
    }

    pub fn get_flat_physical_damage_bonus(&self) -> u32 {
        self.physical_flat_dmg_bonus
    }

    pub fn increase_flat_physical_damage_bonus(&mut self, value: u32) {
        self.physical_flat_dmg_bonus += value;
    }

    pub fn decrease_flat_physical_damage_bonus(&mut self, value: u32) {
        self.physical_flat_dmg_bonus = sub_checked(
            self.physical_flat_dmg_bonus,
            value,
            "flat physical damage bonus",
        );
    }

    // ---------------------------------------------------------------- mana

    pub fn get_mp5(&self, ctx: &StatContext) -> u32 {
        self.base_stats.get_mp5() + ctx.equipment.get_mp5()
    }

    pub fn increase_mp5(&mut self, value: u32) {
        self.base_stats.increase_mp5(value);
    }

    pub fn decrease_mp5(&mut self, value: u32) {
        self.base_stats.decrease_mp5(value);
    }

    pub fn get_hp5(&self, ctx: &StatContext) -> u32 {
        self.base_stats.get_hp5() + ctx.equipment.get_hp5()
    }

    pub fn increase_hp5(&mut self, value: u32) {
        self.base_stats.increase_hp5(value);
    }

    pub fn decrease_hp5(&mut self, value: u32) {
        self.base_stats.decrease_hp5(value);
    }

    pub fn get_mana_skill_reduction(&self) -> u32 {
        self.mana_skill_reduction
    }

    pub fn increase_mana_skill_reduction(&mut self, value: u32) {
        self.mana_skill_reduction += value;
    }

    pub fn decrease_mana_skill_reduction(&mut self, value: u32) {
        self.mana_skill_reduction =
            sub_checked(self.mana_skill_reduction, value, "mana skill reduction");
    }

    // ---------------------------------------------------------------- spells

    pub fn get_spell_hit_chance(&self, ctx: &StatContext, school: MagicSchool) -> u32 {
        self.base_stats.get_spell_hit_chance(school) + ctx.equipment.get_spell_hit_chance(school)
    }

    pub fn increase_spell_hit(&mut self, value: u32) {
        self.base_stats.increase_spell_hit(value);
    }

    pub fn decrease_spell_hit(&mut self, value: u32) {
        self.base_stats.decrease_spell_hit(value);
    }

    pub fn increase_spell_hit_for_school(&mut self, school: MagicSchool, value: u32) {
        self.base_stats.increase_spell_hit_for_school(school, value);
    }

    pub fn decrease_spell_hit_for_school(&mut self, school: MagicSchool, value: u32) {
        self.base_stats.decrease_spell_hit_for_school(school, value);
    }

    pub fn get_spell_crit_chance(&self, ctx: &StatContext, school: MagicSchool) -> u32 {
        let equip_effect = self.base_stats.get_spell_crit_chance(school)
            + ctx.equipment.get_spell_crit_chance(school);
        let crit_from_target = ctx.target.spell_crit[school as usize];
        let crit_from_int = (f64::from(self.get_intellect(ctx))
            / ctx.class.intellect_per_percent_spell_crit
            * 100.0)
            .round() as u32;

        (crit_from_int + equip_effect + crit_from_target).saturating_sub(self.crit_penalty)
    }

    pub fn increase_spell_crit(&mut self, value: u32) {
        self.base_stats.increase_spell_crit(value);
    }

    pub fn decrease_spell_crit(&mut self, value: u32) {
        self.base_stats.decrease_spell_crit(value);
    }

    pub fn increase_spell_crit_for_school(&mut self, school: MagicSchool, value: u32) {
        self.base_stats
            .increase_spell_crit_for_school(school, value);
    }

    pub fn decrease_spell_crit_for_school(&mut self, school: MagicSchool, value: u32) {
        self.base_stats
            .decrease_spell_crit_for_school(school, value);
    }

    /// Spell damage from gear, buffs, creature-type bonuses and target debuffs.
    pub fn get_spell_damage(&self, ctx: &StatContext, school: MagicSchool) -> u32 {
        let creature = ctx.target.creature_type;
        self.base_stats.get_spell_damage(school)
            + self.base_stats.get_spell_damage_against_type(creature)
            + ctx.equipment.get_spell_damage(school)
            + ctx.equipment.get_spell_damage_against_type(creature)
            + ctx.target.spell_damage[school as usize]
    }

    pub fn increase_base_spell_damage(&mut self, value: u32) {
        self.base_stats.increase_base_spell_damage(value);
    }

    pub fn decrease_base_spell_damage(&mut self, value: u32) {
        self.base_stats.decrease_base_spell_damage(value);
    }

    pub fn increase_spell_damage_vs_school(&mut self, value: u32, school: MagicSchool) {
        self.base_stats
            .increase_spell_damage_vs_school(value, school);
    }

    pub fn decrease_spell_damage_vs_school(&mut self, value: u32, school: MagicSchool) {
        self.base_stats
            .decrease_spell_damage_vs_school(value, school);
    }

    pub fn increase_spell_damage_vs_type(&mut self, creature: CreatureType, value: u32) {
        self.base_stats
            .increase_spell_damage_against_type(creature, value);
    }

    pub fn decrease_spell_damage_vs_type(&mut self, creature: CreatureType, value: u32) {
        self.base_stats
            .decrease_spell_damage_against_type(creature, value);
    }

    /// Target resistance after spell penetration, never negative.
    pub fn get_target_resistance(&self, ctx: &StatContext, school: MagicSchool) -> u32 {
        let delta = ctx.target.resistances[school as usize]
            - self.get_spell_penetration(ctx, school) as i32;
        delta.max(0) as u32
    }

    pub fn get_spell_penetration(&self, ctx: &StatContext, school: MagicSchool) -> u32 {
        ctx.equipment.get_spell_penetration(school) + self.base_stats.get_spell_penetration(school)
    }

    pub fn increase_spell_penetration(&mut self, school: MagicSchool, value: u32) {
        self.base_stats.increase_spell_penetration(school, value);
    }

    pub fn decrease_spell_penetration(&mut self, school: MagicSchool, value: u32) {
        self.base_stats.decrease_spell_penetration(school, value);
    }

    /// Damage multiplier for a magic school: own buffs, target debuffs and creature-type talents.
    pub fn get_magic_school_damage_mod(&self, ctx: &StatContext, school: MagicSchool) -> f64 {
        self.magic_school_damage[school as usize].modifier()
            * ctx.target.magic_school_damage_mod[school as usize]
            * self.magic_damage_per_creature[ctx.target.creature_type.index()].modifier()
    }

    pub fn increase_magic_school_damage_mod_all(&mut self, percent: u32) {
        for school in MagicSchool::MAGIC {
            self.increase_magic_school_damage_mod(percent, school);
        }
    }

    pub fn decrease_magic_school_damage_mod_all(&mut self, percent: u32) {
        for school in MagicSchool::MAGIC {
            self.decrease_magic_school_damage_mod(percent, school);
        }
    }

    pub fn increase_magic_school_damage_mod(&mut self, percent: u32, school: MagicSchool) {
        self.magic_school_damage[school as usize].add(percent as i32);
    }

    pub fn decrease_magic_school_damage_mod(&mut self, percent: u32, school: MagicSchool) {
        self.magic_school_damage[school as usize].remove(percent as i32);
    }

    pub fn increase_magic_damage_mod_vs_type(&mut self, creature: CreatureType, percent: i32) {
        self.magic_damage_per_creature[creature.index()].add(percent);
    }

    pub fn decrease_magic_damage_mod_vs_type(&mut self, creature: CreatureType, percent: i32) {
        self.magic_damage_per_creature[creature.index()].remove(percent);
    }

    // ---------------------------------------------------------------- generic stat changes

    /// Applies a stat change from a buff/proc/use effect. Values are in internal units
    /// (`100` = 1% for chances, percent for speeds, flat otherwise).
    ///
    /// Attack speed changes only update the modifier; the character must re-time pending swings.
    pub fn increase_stat(&mut self, stat: ItemStat, value: u32) {
        self.change_stat(stat, value, StatChange::Increase);
    }

    /// Reverts [`CharacterStats::increase_stat`].
    pub fn decrease_stat(&mut self, stat: ItemStat, value: u32) {
        self.change_stat(stat, value, StatChange::Decrease);
    }

    fn change_stat(&mut self, stat: ItemStat, value: u32, change: StatChange) {
        let inc = change == StatChange::Increase;
        let signed = value as i32;
        let base = &mut self.base_stats;

        match stat {
            ItemStat::Agility => {
                base.change_u32(inc, Stats::increase_agility, Stats::decrease_agility, value)
            }
            ItemStat::Intellect => base.change_u32(
                inc,
                Stats::increase_intellect,
                Stats::decrease_intellect,
                value,
            ),
            ItemStat::Spirit => {
                base.change_u32(inc, Stats::increase_spirit, Stats::decrease_spirit, value)
            }
            ItemStat::Stamina => {
                base.change_u32(inc, Stats::increase_stamina, Stats::decrease_stamina, value)
            }
            ItemStat::Strength => base.change_u32(
                inc,
                Stats::increase_strength,
                Stats::decrease_strength,
                value,
            ),
            ItemStat::ManaPer5 => {
                base.change_u32(inc, Stats::increase_mp5, Stats::decrease_mp5, value)
            }
            ItemStat::HealthPer5 => {
                base.change_u32(inc, Stats::increase_hp5, Stats::decrease_hp5, value)
            }
            ItemStat::ManaSkillReduction => {
                if inc {
                    self.increase_mana_skill_reduction(value);
                } else {
                    self.decrease_mana_skill_reduction(value);
                }
            }
            ItemStat::SpellDamage => base.change_u32(
                inc,
                Stats::increase_base_spell_damage,
                Stats::decrease_base_spell_damage,
                value,
            ),
            ItemStat::SpellDamageArcane
            | ItemStat::SpellDamageFire
            | ItemStat::SpellDamageFrost
            | ItemStat::SpellDamageHoly
            | ItemStat::SpellDamageNature
            | ItemStat::SpellDamageShadow => {
                let school = stat.magic_school().expect("school stat");
                if inc {
                    base.increase_spell_damage_vs_school(value, school);
                } else {
                    base.decrease_spell_damage_vs_school(value, school);
                }
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
                if inc {
                    base.increase_spell_damage_against_type(creature, value);
                } else {
                    base.decrease_spell_damage_against_type(creature, value);
                }
            }
            ItemStat::SpellCritChance => base.change_u32(
                inc,
                Stats::increase_spell_crit,
                Stats::decrease_spell_crit,
                value,
            ),
            ItemStat::SpellHitChance => base.change_u32(
                inc,
                Stats::increase_spell_hit,
                Stats::decrease_spell_hit,
                value,
            ),
            ItemStat::SpellPenetration => {
                for school in MagicSchool::MAGIC {
                    if inc {
                        base.increase_spell_penetration(school, value);
                    } else {
                        base.decrease_spell_penetration(school, value);
                    }
                }
            }
            ItemStat::BlockValue => base.change_i32(
                inc,
                Stats::increase_block_value,
                Stats::decrease_block_value,
                signed,
            ),
            ItemStat::Armor => {
                base.change_i32(inc, Stats::increase_armor, Stats::decrease_armor, signed)
            }
            ItemStat::Defense => base.change_i32(
                inc,
                Stats::increase_defense,
                Stats::decrease_defense,
                signed,
            ),
            ItemStat::DodgeChance => {
                base.change_f64(inc, Stats::increase_dodge, Stats::decrease_dodge, value)
            }
            ItemStat::ParryChance => {
                base.change_f64(inc, Stats::increase_parry, Stats::decrease_parry, value)
            }
            ItemStat::BlockChance => base.change_f64(
                inc,
                Stats::increase_block_chance,
                Stats::decrease_block_chance,
                value,
            ),
            ItemStat::AllResistance => {
                for school in MagicSchool::MAGIC {
                    if inc {
                        base.increase_resistance(school, signed);
                    } else {
                        base.decrease_resistance(school, signed);
                    }
                }
            }
            ItemStat::ArcaneResistance
            | ItemStat::FireResistance
            | ItemStat::FrostResistance
            | ItemStat::HolyResistance
            | ItemStat::NatureResistance
            | ItemStat::ShadowResistance => {
                let school = stat.magic_school().expect("resistance stat");
                if inc {
                    base.increase_resistance(school, signed);
                } else {
                    base.decrease_resistance(school, signed);
                }
            }
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
                if inc {
                    self.increase_wpn_skill(weapon_type, value);
                } else {
                    self.decrease_wpn_skill(weapon_type, value);
                }
            }
            ItemStat::HitChance => {
                base.change_u32(
                    inc,
                    Stats::increase_ranged_hit,
                    Stats::decrease_ranged_hit,
                    value,
                );
                base.change_u32(
                    inc,
                    Stats::increase_melee_hit,
                    Stats::decrease_melee_hit,
                    value,
                );
            }
            ItemStat::RangedHitChance => {
                base.change_u32(
                    inc,
                    Stats::increase_ranged_hit,
                    Stats::decrease_ranged_hit,
                    value,
                );
            }
            ItemStat::CritChance => {
                base.change_u32(
                    inc,
                    Stats::increase_ranged_crit,
                    Stats::decrease_ranged_crit,
                    value,
                );
                if inc {
                    self.increase_melee_aura_crit(value);
                } else {
                    self.decrease_melee_aura_crit(value);
                }
            }
            ItemStat::AttackSpeed => {
                if inc {
                    self.increase_ranged_attack_speed(value);
                    self.increase_melee_attack_speed(value);
                } else {
                    self.decrease_ranged_attack_speed(value);
                    self.decrease_melee_attack_speed(value);
                }
            }
            ItemStat::MeleeAttackSpeed => {
                if inc {
                    self.increase_melee_attack_speed(value);
                } else {
                    self.decrease_melee_attack_speed(value);
                }
            }
            ItemStat::RangedAttackSpeed => {
                if inc {
                    self.increase_ranged_attack_speed(value);
                } else {
                    self.decrease_ranged_attack_speed(value);
                }
            }
            ItemStat::CastingSpeed => {
                if inc {
                    self.increase_casting_speed_mod(value);
                } else {
                    self.decrease_casting_speed_mod(value);
                }
            }
            ItemStat::AttackPower => {
                base.change_u32(
                    inc,
                    Stats::increase_base_melee_ap,
                    Stats::decrease_base_melee_ap,
                    value,
                );
                base.change_u32(
                    inc,
                    Stats::increase_base_ranged_ap,
                    Stats::decrease_base_ranged_ap,
                    value,
                );
            }
            ItemStat::MeleeAttackPower => base.change_u32(
                inc,
                Stats::increase_base_melee_ap,
                Stats::decrease_base_melee_ap,
                value,
            ),
            ItemStat::RangedAttackPower => base.change_u32(
                inc,
                Stats::increase_base_ranged_ap,
                Stats::decrease_base_ranged_ap,
                value,
            ),
            ItemStat::FeralAttackPower => base.change_u32(
                inc,
                Stats::increase_base_feral_ap,
                Stats::decrease_base_feral_ap,
                value,
            ),
            ItemStat::AttackPowerBeast
            | ItemStat::AttackPowerDemon
            | ItemStat::AttackPowerDragonkin
            | ItemStat::AttackPowerElemental
            | ItemStat::AttackPowerGiant
            | ItemStat::AttackPowerHumanoid
            | ItemStat::AttackPowerMechanical
            | ItemStat::AttackPowerUndead => {
                let creature = stat.creature_type().expect("creature stat");
                if inc {
                    self.increase_ap_vs_type(creature, value);
                } else {
                    self.decrease_ap_vs_type(creature, value);
                }
            }
            ItemStat::WeaponDamage => {
                if inc {
                    self.increase_mh_weapon_damage_bonus(value);
                    self.increase_oh_weapon_damage_bonus(value);
                } else {
                    self.decrease_mh_weapon_damage_bonus(value);
                    self.decrease_oh_weapon_damage_bonus(value);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatChange {
    Increase,
    Decrease,
}

impl Stats {
    fn change_u32(
        &mut self,
        inc: bool,
        add: fn(&mut Stats, u32),
        sub: fn(&mut Stats, u32),
        value: u32,
    ) {
        if inc {
            add(self, value);
        } else {
            sub(self, value);
        }
    }

    fn change_i32(
        &mut self,
        inc: bool,
        add: fn(&mut Stats, i32),
        sub: fn(&mut Stats, i32),
        value: i32,
    ) {
        if inc {
            add(self, value);
        } else {
            sub(self, value);
        }
    }

    /// Chances given in hundredths of a percent are stored as fractions.
    fn change_f64(
        &mut self,
        inc: bool,
        add: fn(&mut Stats, f64),
        sub: fn(&mut Stats, f64),
        value: u32,
    ) {
        let fraction = f64::from(value) / 10_000.0;
        if inc {
            add(self, fraction);
        } else {
            sub(self, fraction);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const EPS: f64 = 0.0001;

    /// Level 60 Orc Warrior against a level 63 Dragonkin, as in `TestCharacterStats`.
    struct Fixture {
        stats: CharacterStats,
        equipment: Stats,
        race: RaceStats,
        class: ClassStatRules,
        mechanics: Mechanics,
        target: TargetStatView,
        mainhand: Option<WeaponProfile>,
        offhand: Option<WeaponProfile>,
        ranged: Option<WeaponProfile>,
        attack_mode: AttackMode,
    }

    impl Fixture {
        fn orc_warrior() -> Self {
            let mut stats = CharacterStats::new();
            stats.increase_strength(100);
            stats.increase_agility(60);
            stats.increase_stamina(90);
            stats.increase_intellect(10);
            stats.increase_spirit(25);
            stats.increase_melee_ap(160);
            stats.increase_melee_base_crit(200);

            let mut target = TargetStatView::default();
            for school in MagicSchool::MAGIC {
                target.resistances[school as usize] =
                    if school == MagicSchool::Holy { 0 } else { 70 };
            }

            Self {
                stats,
                equipment: Stats::new(),
                race: RaceStats {
                    strength: 23,
                    agility: 17,
                    stamina: 22,
                    intellect: 17,
                    spirit: 23,
                    axe_skill_bonus: 5,
                    ..RaceStats::default()
                },
                class: ClassStatRules {
                    agility_per_percent_crit: 20.0,
                    intellect_per_percent_spell_crit: f64::MAX,
                    melee_ap_per_strength: 2,
                    melee_ap_per_agility: 0,
                    ranged_ap_per_agility: 0,
                },
                mechanics: Mechanics::new(63),
                target,
                mainhand: None,
                offhand: None,
                ranged: None,
                attack_mode: AttackMode::MeleeAttack,
            }
        }

        fn ctx(&self) -> StatContext<'_> {
            StatContext {
                equipment: &self.equipment,
                race: &self.race,
                class: &self.class,
                clvl: 60,
                mechanics: &self.mechanics,
                mainhand: self.mainhand,
                offhand: self.offhand,
                ranged: self.ranged,
                attack_mode: self.attack_mode,
                druid_feral_form: false,
                target: &self.target,
            }
        }
    }

    fn weapon(weapon_type: WeaponType, speed: f64) -> Option<WeaponProfile> {
        Some(WeaponProfile { weapon_type, speed })
    }

    #[test]
    fn values_after_initialization() {
        let f = Fixture::orc_warrior();
        let stats = &f.stats;
        assert_abs_diff_eq!(
            stats.get_total_physical_damage_mod(&f.ctx()),
            1.0,
            epsilon = EPS
        );
        assert_abs_diff_eq!(stats.get_melee_attack_speed_mod(), 1.0, epsilon = EPS);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 1.0, epsilon = EPS);
        assert_abs_diff_eq!(stats.get_spell_damage_taken_mod(), 1.0, epsilon = EPS);
    }

    #[test]
    fn base_attributes_include_race_and_class() {
        let f = Fixture::orc_warrior();
        let ctx = f.ctx();
        assert_eq!(f.stats.get_strength(&ctx), 123);
        assert_eq!(f.stats.get_agility(&ctx), 77);
        assert_eq!(f.stats.get_stamina(&ctx), 112);
        assert_eq!(f.stats.get_intellect(&ctx), 27);
        assert_eq!(f.stats.get_spirit(&ctx), 48);
        // 160 base + 2 * 123 strength.
        assert_eq!(f.stats.get_melee_ap(&ctx), 406);
        assert_eq!(f.stats.get_ranged_ap(&ctx), 0);
        // 77 agi / 20 = 3.85% + 2% base.
        assert_eq!(f.stats.get_mh_crit_chance(&ctx), 585);
        assert_eq!(f.stats.get_oh_crit_chance(&ctx), 0);
        assert_eq!(f.stats.get_armor(&ctx), 154);
    }

    #[test]
    fn attack_speed_multipliers_stack_multiplicatively() {
        let mut f = Fixture::orc_warrior();
        let stats = &mut f.stats;
        let expect = |stats: &CharacterStats, value: f64| {
            assert_abs_diff_eq!(stats.get_melee_attack_speed_mod(), value, epsilon = EPS);
        };

        stats.increase_melee_attack_speed(10);
        expect(stats, 1.10);
        stats.increase_melee_attack_speed(20);
        expect(stats, 1.32);
        stats.increase_melee_attack_speed(50);
        expect(stats, 1.98);
        stats.increase_melee_attack_speed(100);
        expect(stats, 3.96);
        stats.decrease_melee_attack_speed(20);
        expect(stats, 3.30);
        stats.decrease_melee_attack_speed(10);
        expect(stats, 3.00);
        stats.decrease_melee_attack_speed(100);
        expect(stats, 1.50);
        stats.decrease_melee_attack_speed(50);
        expect(stats, 1.00);
    }

    #[test]
    fn melee_and_ranged_attack_speed_modifiers_are_independent() {
        let mut f = Fixture::orc_warrior();
        let stats = &mut f.stats;
        stats.increase_melee_attack_speed(30);
        stats.increase_ranged_attack_speed(20);
        assert_abs_diff_eq!(stats.get_melee_attack_speed_mod(), 1.3, epsilon = EPS);
        assert_abs_diff_eq!(stats.get_ranged_attack_speed_mod(), 1.2, epsilon = EPS);
        stats.decrease_melee_attack_speed(30);
        assert_abs_diff_eq!(stats.get_melee_attack_speed_mod(), 1.0, epsilon = EPS);
        assert_abs_diff_eq!(stats.get_ranged_attack_speed_mod(), 1.2, epsilon = EPS);
        stats.decrease_ranged_attack_speed(20);
        assert_abs_diff_eq!(stats.get_ranged_attack_speed_mod(), 1.0, epsilon = EPS);
    }

    #[test]
    fn physical_damage_multipliers_stack_multiplicatively() {
        let mut f = Fixture::orc_warrior();
        let expect = |f: &Fixture, value: f64| {
            assert_abs_diff_eq!(
                f.stats.get_total_physical_damage_mod(&f.ctx()),
                value,
                epsilon = EPS
            );
        };

        f.stats.increase_total_phys_dmg_mod(10);
        expect(&f, 1.10);
        f.stats.increase_total_phys_dmg_mod(20);
        expect(&f, 1.32);
        f.stats.increase_total_phys_dmg_mod(50);
        expect(&f, 1.98);
        f.stats.increase_total_phys_dmg_mod(100);
        expect(&f, 3.96);
        f.stats.decrease_total_phys_dmg_mod(20);
        expect(&f, 3.30);
        f.stats.decrease_total_phys_dmg_mod(10);
        expect(&f, 3.00);
        f.stats.decrease_total_phys_dmg_mod(100);
        expect(&f, 1.50);
        f.stats.decrease_total_phys_dmg_mod(50);
        expect(&f, 1.00);
    }

    #[test]
    fn damage_taken_multipliers_stack_multiplicatively() {
        let mut f = Fixture::orc_warrior();
        let stats = &mut f.stats;

        stats.add_phys_damage_taken_mod(-10);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 0.90, epsilon = EPS);
        stats.add_phys_damage_taken_mod(-20);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 0.72, epsilon = EPS);
        stats.add_phys_damage_taken_mod(-50);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 0.36, epsilon = EPS);
        stats.remove_phys_damage_taken_mod(-20);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 0.45, epsilon = EPS);
        stats.remove_phys_damage_taken_mod(-10);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 0.50, epsilon = EPS);
        stats.remove_phys_damage_taken_mod(-50);
        assert_abs_diff_eq!(stats.get_physical_damage_taken_mod(), 1.00, epsilon = EPS);

        stats.add_spell_damage_taken_mod(-10);
        stats.add_spell_damage_taken_mod(-20);
        assert_abs_diff_eq!(stats.get_spell_damage_taken_mod(), 0.72, epsilon = EPS);
        stats.remove_spell_damage_taken_mod(-10);
        stats.remove_spell_damage_taken_mod(-20);
        assert_abs_diff_eq!(stats.get_spell_damage_taken_mod(), 1.00, epsilon = EPS);
    }

    #[test]
    fn damage_bonuses_vs_creature_type() {
        let mut f = Fixture::orc_warrior();
        assert_eq!(f.target.creature_type, CreatureType::Dragonkin);

        f.stats.increase_dmg_vs_type(CreatureType::Dragonkin, 0.01);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.01,
            epsilon = EPS
        );
        f.stats.increase_dmg_vs_type(CreatureType::Dragonkin, 0.1);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.11,
            epsilon = EPS
        );
        f.stats.decrease_dmg_vs_type(CreatureType::Dragonkin, 0.05);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.06,
            epsilon = EPS
        );
        f.stats.decrease_dmg_vs_type(CreatureType::Dragonkin, 0.06);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.0,
            epsilon = EPS
        );

        f.target.creature_type = CreatureType::Beast;
        f.stats.increase_dmg_vs_type(CreatureType::Dragonkin, 0.5);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.0,
            epsilon = EPS
        );
    }

    #[test]
    fn ap_bonuses_vs_creature_type() {
        let mut f = Fixture::orc_warrior();
        let base_melee_ap = f.stats.get_melee_ap(&f.ctx());
        let base_ranged_ap = f.stats.get_ranged_ap(&f.ctx());

        f.stats.increase_ap_vs_type(CreatureType::Dragonkin, 100);
        assert_eq!(f.stats.get_melee_ap(&f.ctx()), base_melee_ap + 100);
        assert_eq!(f.stats.get_ranged_ap(&f.ctx()), base_ranged_ap + 100);
        f.stats.decrease_ap_vs_type(CreatureType::Dragonkin, 100);
        assert_eq!(f.stats.get_melee_ap(&f.ctx()), base_melee_ap);
        assert_eq!(f.stats.get_ranged_ap(&f.ctx()), base_ranged_ap);
    }

    #[test]
    fn ap_multipliers() {
        let mut f = Fixture::orc_warrior();
        f.stats.increase_ranged_ap(50);
        let base_melee_ap = f.stats.get_melee_ap(&f.ctx());
        let base_ranged_ap = f.stats.get_ranged_ap(&f.ctx());

        f.stats.add_ap_multiplier(100);
        assert_eq!(f.stats.get_melee_ap(&f.ctx()), base_melee_ap * 2);
        assert_eq!(f.stats.get_ranged_ap(&f.ctx()), base_ranged_ap * 2);
        f.stats.remove_ap_multiplier(100);
        assert_eq!(f.stats.get_melee_ap(&f.ctx()), base_melee_ap);
        assert_eq!(f.stats.get_ranged_ap(&f.ctx()), base_ranged_ap);
    }

    #[test]
    fn physical_damage_mod_depends_on_attack_mode() {
        let mut f = Fixture::orc_warrior();
        f.mainhand = weapon(WeaponType::Axe, 2.6);
        f.ranged = weapon(WeaponType::Bow, 2.9);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.0,
            epsilon = EPS
        );

        f.stats
            .increase_total_phys_dmg_for_weapon_type(WeaponType::Axe, 10);
        f.stats
            .increase_total_phys_dmg_for_weapon_type(WeaponType::Bow, 20);

        f.attack_mode = AttackMode::MeleeAttack;
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.1,
            epsilon = EPS
        );
        f.attack_mode = AttackMode::RangedAttack;
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.2,
            epsilon = EPS
        );
        f.attack_mode = AttackMode::MagicAttack;
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.0,
            epsilon = EPS
        );

        f.attack_mode = AttackMode::MeleeAttack;
        f.stats
            .decrease_total_phys_dmg_for_weapon_type(WeaponType::Axe, 10);
        assert_abs_diff_eq!(
            f.stats.get_total_physical_damage_mod(&f.ctx()),
            1.0,
            epsilon = EPS
        );
    }

    #[test]
    fn crit_dmg_mod_affected_by_creature_type() {
        let mut f = Fixture::orc_warrior();
        assert_abs_diff_eq!(
            f.stats.get_melee_ability_crit_dmg_mod(&f.ctx()),
            2.0,
            epsilon = EPS
        );
        assert_abs_diff_eq!(f.stats.get_spell_crit_dmg_mod(&f.ctx()), 1.5, epsilon = EPS);
        assert_abs_diff_eq!(
            f.stats.get_ranged_ability_crit_dmg_mod(&f.ctx()),
            2.0,
            epsilon = EPS
        );

        f.stats
            .increase_crit_dmg_vs_type(CreatureType::Dragonkin, 1);
        f.target.creature_type = CreatureType::Humanoid;
        assert_abs_diff_eq!(
            f.stats.get_melee_ability_crit_dmg_mod(&f.ctx()),
            2.0,
            epsilon = EPS
        );
        assert_abs_diff_eq!(f.stats.get_spell_crit_dmg_mod(&f.ctx()), 1.5, epsilon = EPS);
        assert_abs_diff_eq!(
            f.stats.get_ranged_ability_crit_dmg_mod(&f.ctx()),
            2.0,
            epsilon = EPS
        );

        f.target.creature_type = CreatureType::Dragonkin;
        assert_abs_diff_eq!(
            f.stats.get_melee_ability_crit_dmg_mod(&f.ctx()),
            2.01,
            epsilon = EPS
        );
        assert_abs_diff_eq!(
            f.stats.get_spell_crit_dmg_mod(&f.ctx()),
            1.51,
            epsilon = EPS
        );
        assert_abs_diff_eq!(
            f.stats.get_ranged_ability_crit_dmg_mod(&f.ctx()),
            2.01,
            epsilon = EPS
        );

        f.stats
            .decrease_crit_dmg_vs_type(CreatureType::Dragonkin, 1);
        assert_abs_diff_eq!(
            f.stats.get_melee_ability_crit_dmg_mod(&f.ctx()),
            2.0,
            epsilon = EPS
        );

        f.stats.increase_melee_ability_crit_dmg_mod(0.1);
        assert_abs_diff_eq!(
            f.stats.get_melee_ability_crit_dmg_mod(&f.ctx()),
            2.1,
            epsilon = EPS
        );
    }

    #[test]
    fn spell_school_damage_mods() {
        let mut f = Fixture::orc_warrior();
        let holy = |f: &Fixture| {
            f.stats
                .get_magic_school_damage_mod(&f.ctx(), MagicSchool::Holy)
        };
        assert_abs_diff_eq!(holy(&f), 1.0, epsilon = EPS);
        f.stats
            .increase_magic_school_damage_mod(10, MagicSchool::Holy);
        assert_abs_diff_eq!(holy(&f), 1.1, epsilon = EPS);
        f.stats
            .increase_magic_school_damage_mod(20, MagicSchool::Holy);
        assert_abs_diff_eq!(holy(&f), 1.32, epsilon = EPS);
        f.stats
            .decrease_magic_school_damage_mod(10, MagicSchool::Holy);
        assert_abs_diff_eq!(holy(&f), 1.2, epsilon = EPS);
        f.stats
            .decrease_magic_school_damage_mod(20, MagicSchool::Holy);
        assert_abs_diff_eq!(holy(&f), 1.0, epsilon = EPS);

        f.stats.increase_magic_school_damage_mod_all(10);
        assert_abs_diff_eq!(holy(&f), 1.1, epsilon = EPS);
        assert_abs_diff_eq!(
            f.stats
                .get_magic_school_damage_mod(&f.ctx(), MagicSchool::Physical),
            1.0,
            epsilon = EPS
        );
        f.stats.decrease_magic_school_damage_mod_all(10);
        assert_abs_diff_eq!(holy(&f), 1.0, epsilon = EPS);
    }

    #[test]
    fn no_negative_target_resistances_with_spell_pen_bonuses() {
        let mut f = Fixture::orc_warrior();
        let resistance = |f: &Fixture, school| f.stats.get_target_resistance(&f.ctx(), school);

        assert_eq!(resistance(&f, MagicSchool::Holy), 0);
        f.stats.increase_spell_penetration(MagicSchool::Holy, 10);
        assert_eq!(resistance(&f, MagicSchool::Holy), 0);
        f.stats.decrease_spell_penetration(MagicSchool::Holy, 10);

        for school in [
            MagicSchool::Arcane,
            MagicSchool::Fire,
            MagicSchool::Frost,
            MagicSchool::Nature,
            MagicSchool::Shadow,
        ] {
            assert_eq!(resistance(&f, school), 70);
            f.stats.increase_spell_penetration(school, 80);
            assert_eq!(resistance(&f, school), 0);
            f.stats.decrease_spell_penetration(school, 10);
            assert_eq!(resistance(&f, school), 0);
            f.stats.decrease_spell_penetration(school, 50);
            assert_eq!(resistance(&f, school), 50);
        }
    }

    #[test]
    fn spell_damage_includes_relevant_sources() {
        let mut f = Fixture::orc_warrior();
        let creature = f.target.creature_type;
        let fire = |f: &Fixture| f.stats.get_spell_damage(&f.ctx(), MagicSchool::Fire);
        let initial = fire(&f);

        f.stats.increase_base_spell_damage(100);
        assert_eq!(fire(&f), initial + 100);
        f.stats.decrease_base_spell_damage(100);
        assert_eq!(fire(&f), initial);

        f.stats
            .increase_spell_damage_vs_school(100, MagicSchool::Fire);
        assert_eq!(fire(&f), initial + 100);
        f.stats
            .decrease_spell_damage_vs_school(100, MagicSchool::Fire);
        assert_eq!(fire(&f), initial);

        f.stats.increase_spell_damage_vs_type(creature, 100);
        assert_eq!(fire(&f), initial + 100);
        f.stats.decrease_spell_damage_vs_type(creature, 100);
        assert_eq!(fire(&f), initial);

        f.equipment.increase_base_spell_damage(30);
        f.target.spell_damage[MagicSchool::Fire as usize] = 20;
        assert_eq!(fire(&f), initial + 50);
    }

    #[test]
    fn magic_damage_includes_target_mods() {
        let mut f = Fixture::orc_warrior();
        f.target.creature_type = CreatureType::Undead;
        let holy = |f: &Fixture| {
            f.stats
                .get_magic_school_damage_mod(&f.ctx(), MagicSchool::Holy)
        };

        assert_abs_diff_eq!(holy(&f), 1.0, epsilon = EPS);
        f.stats
            .increase_magic_damage_mod_vs_type(CreatureType::Undead, 10);
        assert_abs_diff_eq!(holy(&f), 1.1, epsilon = EPS);
        f.stats
            .increase_magic_damage_mod_vs_type(CreatureType::Undead, 20);
        assert_abs_diff_eq!(holy(&f), 1.32, epsilon = EPS);
        f.stats
            .decrease_magic_damage_mod_vs_type(CreatureType::Undead, 10);
        assert_abs_diff_eq!(holy(&f), 1.2, epsilon = EPS);
        f.stats
            .decrease_magic_damage_mod_vs_type(CreatureType::Undead, 20);
        assert_abs_diff_eq!(holy(&f), 1.0, epsilon = EPS);

        f.target.magic_school_damage_mod[MagicSchool::Holy as usize] = 1.5;
        assert_abs_diff_eq!(holy(&f), 1.5, epsilon = EPS);
    }

    #[test]
    fn weapon_skill_from_level_race_equipment_and_talents() {
        let mut f = Fixture::orc_warrior();
        assert_eq!(f.stats.get_mh_wpn_skill(&f.ctx()), 300);

        f.mainhand = weapon(WeaponType::Axe, 2.6);
        assert_eq!(f.stats.get_mh_wpn_skill(&f.ctx()), 305);

        f.mainhand = weapon(WeaponType::TwohandAxe, 3.6);
        f.equipment.increase_weapon_skill(WeaponType::TwohandAxe, 3);
        f.stats.increase_wpn_skill(WeaponType::TwohandAxe, 2);
        assert_eq!(f.stats.get_mh_wpn_skill(&f.ctx()), 310);
        f.stats.decrease_wpn_skill(WeaponType::TwohandAxe, 2);
        assert_eq!(f.stats.get_mh_wpn_skill(&f.ctx()), 308);

        f.offhand = weapon(WeaponType::Sword, 1.8);
        assert_eq!(f.stats.get_oh_wpn_skill(&f.ctx()), 300);
        f.ranged = weapon(WeaponType::Gun, 2.8);
        f.race.gun_skill_bonus = 5;
        assert_eq!(f.stats.get_ranged_wpn_skill(&f.ctx()), 305);
    }

    #[test]
    #[should_panic(expected = "has no effect for weapon type Shield")]
    fn weapon_skill_for_shield_panics() {
        let mut f = Fixture::orc_warrior();
        f.stats.increase_wpn_skill(WeaponType::Shield, 1);
    }

    #[test]
    fn crit_chance_sources_and_suppression() {
        let mut f = Fixture::orc_warrior();
        let base = f.stats.get_mh_crit_chance(&f.ctx());

        // Aura crit is suppressed by 1.8% against a +3 target: 5% -> 3.2%.
        f.stats.increase_melee_aura_crit(500);
        assert_eq!(f.stats.get_mh_crit_chance(&f.ctx()), base + 320);
        f.stats.decrease_melee_aura_crit(500);

        // Base crit (talents) is not suppressed.
        f.stats.increase_melee_base_crit(500);
        assert_eq!(f.stats.get_mh_crit_chance(&f.ctx()), base + 500);
        f.stats.decrease_melee_base_crit(500);

        // Weapon type crit only applies to the matching hand.
        f.mainhand = weapon(WeaponType::Axe, 2.6);
        f.offhand = weapon(WeaponType::Sword, 1.8);
        f.stats.increase_crit_for_weapon_type(WeaponType::Axe, 500);
        assert_eq!(f.stats.get_mh_crit_chance(&f.ctx()), base + 320);
        assert_eq!(f.stats.get_oh_crit_chance(&f.ctx()), base);
        f.stats.decrease_crit_for_weapon_type(WeaponType::Axe, 500);

        f.stats.increase_crit_penalty(base + 1000);
        assert_eq!(f.stats.get_mh_crit_chance(&f.ctx()), 0);
    }

    #[test]
    fn weapon_speeds_include_haste() {
        let mut f = Fixture::orc_warrior();
        assert_abs_diff_eq!(f.stats.get_mh_wpn_speed(&f.ctx()), 2.0, epsilon = EPS);
        assert_abs_diff_eq!(f.stats.get_oh_wpn_speed(&f.ctx()), 300.0, epsilon = EPS);
        assert_abs_diff_eq!(f.stats.get_ranged_wpn_speed(&f.ctx()), 300.0, epsilon = EPS);

        f.mainhand = weapon(WeaponType::Sword, 2.6);
        f.offhand = weapon(WeaponType::Dagger, 1.5);
        f.ranged = weapon(WeaponType::Bow, 2.8);
        f.stats.increase_melee_attack_speed(30);
        f.stats.increase_ranged_attack_speed(15);
        assert_abs_diff_eq!(f.stats.get_mh_wpn_speed(&f.ctx()), 2.0, epsilon = EPS);
        assert_abs_diff_eq!(f.stats.get_oh_wpn_speed(&f.ctx()), 1.5 / 1.3, epsilon = EPS);
        assert_abs_diff_eq!(
            f.stats.get_ranged_wpn_speed(&f.ctx()),
            2.8 / 1.15,
            epsilon = EPS
        );
    }

    #[test]
    fn stat_multipliers_apply_to_attributes() {
        let mut f = Fixture::orc_warrior();
        let ctx_strength = |f: &Fixture| f.stats.get_strength(&f.ctx());
        assert_eq!(ctx_strength(&f), 123);
        f.stats.add_total_stat_mod(10);
        assert_eq!(ctx_strength(&f), 135);
        assert_eq!(f.stats.get_agility(&f.ctx()), 85);
        f.stats.remove_total_stat_mod(10);
        assert_eq!(ctx_strength(&f), 123);

        f.stats.add_armor_mod(50);
        f.equipment.increase_armor(1000);
        // (1000 * 1.5) + 77 agi * 2
        assert_eq!(f.stats.get_armor(&f.ctx()), 1654);
    }

    #[test]
    fn generic_stat_changes_round_trip() {
        let mut f = Fixture::orc_warrior();
        let before_ap = f.stats.get_melee_ap(&f.ctx());
        let before_crit = f.stats.get_mh_crit_chance(&f.ctx());

        f.stats.increase_stat(ItemStat::Strength, 20);
        assert_eq!(f.stats.get_melee_ap(&f.ctx()), before_ap + 40);
        f.stats.increase_stat(ItemStat::AttackPowerDragonkin, 200);
        assert_eq!(f.stats.get_melee_ap(&f.ctx()), before_ap + 240);
        f.stats.increase_stat(ItemStat::AttackSpeed, 30);
        assert_abs_diff_eq!(f.stats.get_melee_attack_speed_mod(), 1.3, epsilon = EPS);
        assert_abs_diff_eq!(f.stats.get_ranged_attack_speed_mod(), 1.3, epsilon = EPS);
        f.stats.increase_stat(ItemStat::CritChance, 200);
        assert_eq!(f.stats.get_mh_crit_chance(&f.ctx()), before_crit + 20);
        f.stats.increase_stat(ItemStat::HitChance, 100);
        assert_eq!(f.stats.get_melee_hit_chance(&f.ctx()), 100);
        f.stats.increase_stat(ItemStat::WeaponDamage, 5);
        assert_eq!(f.stats.get_mh_weapon_damage_bonus(&f.ctx()), 5);
        f.stats.increase_stat(ItemStat::Defense, 10);
        assert_eq!(f.stats.base_stats().get_defense(), 10);
        f.stats.increase_stat(ItemStat::DodgeChance, 100);
        assert_abs_diff_eq!(f.stats.base_stats().get_dodge_chance(), 0.01, epsilon = EPS);
        f.stats.increase_stat(ItemStat::AllResistance, 10);
        assert_eq!(f.stats.base_stats().get_resistance(MagicSchool::Fire), 10);

        f.stats.decrease_stat(ItemStat::AllResistance, 10);
        f.stats.decrease_stat(ItemStat::DodgeChance, 100);
        f.stats.decrease_stat(ItemStat::Defense, 10);
        f.stats.decrease_stat(ItemStat::WeaponDamage, 5);
        f.stats.decrease_stat(ItemStat::HitChance, 100);
        f.stats.decrease_stat(ItemStat::CritChance, 200);
        f.stats.decrease_stat(ItemStat::AttackSpeed, 30);
        f.stats.decrease_stat(ItemStat::AttackPowerDragonkin, 200);
        f.stats.decrease_stat(ItemStat::Strength, 20);

        assert_eq!(f.stats.get_melee_ap(&f.ctx()), before_ap);
        assert_eq!(f.stats.get_mh_crit_chance(&f.ctx()), before_crit);
        assert_eq!(f.stats.get_melee_hit_chance(&f.ctx()), 0);
        assert_abs_diff_eq!(f.stats.get_melee_attack_speed_mod(), 1.0, epsilon = EPS);
        assert_eq!(
            f.stats.base_stats(),
            &Fixture::orc_warrior().stats.base_stats
        );
    }

    #[test]
    fn casting_time_suppression_buffs() {
        let mut stats = CharacterStats::new();
        assert!(!stats.casting_time_suppressed());
        stats.suppress_casting_time(BuffId(1));
        stats.suppress_casting_time(BuffId(2));
        assert_eq!(stats.casting_time_suppression_buff(), Some(BuffId(2)));
        stats.return_casting_time(BuffId(2));
        assert_eq!(stats.casting_time_suppression_buff(), Some(BuffId(1)));
        stats.return_casting_time(BuffId(1));
        assert!(!stats.casting_time_suppressed());
    }

    #[test]
    #[should_panic(expected = "Failed to remove multiplicative effect")]
    fn removing_unknown_multiplicative_effect_panics() {
        let mut stats = CharacterStats::new();
        stats.decrease_total_phys_dmg_mod(10);
    }
}
