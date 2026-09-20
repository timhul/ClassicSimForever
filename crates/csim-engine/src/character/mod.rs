//! The character. Port of `Character/Character.*` and the class-agnostic parts of
//! `Class/Warrior/Warrior.*`.
//!
//! [`Character`] owns everything that belongs to one player: its class definition
//! ([`ClassSpec`], data), race, stats, equipment, spell registry, spell modifiers, combat roll,
//! resource, stance and the cooldown timestamps. It never touches the engine, the target or the
//! raid: methods that need "now" take it as a parameter, and everything that needs the world
//! (casting, swinging, buff application, stance passives) goes through
//! [`CharacterContext`](context::CharacterContext), which borrows the character together with
//! the engine, the target and the raid and implements the host traits the spell runtime was
//! written against.
//!
//! The C++ virtual class methods (`get_agi_needed_for_one_percent_phys_crit`,
//! `get_weapon_proficiencies_for_slot`, `global_cooldown`, ...) read the [`ClassSpec`]; the
//! Warrior overrides (`is_dual_wielding` while Heroic Strike is queued, the Tactical Mastery
//! rage remainder on a stance change, rage from damage dealt) are keyed on the resource and
//! the queued swing rather than the class, so they apply to any class that shares the mechanic.

pub mod class;
pub mod context;

use std::sync::Arc;

use crate::attack_mode::AttackMode;
use crate::buff::external::GeneralBuffs;
use crate::character_spells::CharacterSpells;
use crate::combat_roll::{CombatRoll, RollContext};
use crate::equipment::Equipment;
use crate::faction::{Faction, PlayerClass};
use crate::ids::{CharId, SpellId};
use crate::item::{EquipmentDb, EquipmentSlot, WeaponSlot, WeaponType};
use crate::phase::Phase;
use crate::race::{Race, RaceSpec};
use crate::resource::{Resource, ResourceType};
use crate::rng::Random;
use crate::spell::auto_attack::rage_gained_from_damage;
use crate::spell::modifiers::SpellModifiers;
use crate::spell::{AutoAttack, Hand};
use crate::stance::Stance;
use crate::stats::{CharacterStats, ClassStatRules, RaceStats, StatContext, TargetStatView};
use crate::talent::CharacterTalents;

pub use class::{ClassBaseStats, ClassDb, ClassSpec, ClassSpecError, StatOffsets, StatRules};

/// Simulation settings the character reads. Port of the `SimSettings` / ruleset queries made
/// from `Character` and `CombatRoll`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimParams {
    /// Encounter length in seconds.
    pub combat_length: f64,
    /// Whether glancing blows occur (the Loatheb ruleset turns them off).
    pub glancing_blows: bool,
}

impl Default for SimParams {
    fn default() -> Self {
        Self {
            combat_length: 300.0,
            glancing_blows: true,
        }
    }
}

/// Seconds a stance change locks stance changes and GCD abilities. `Character::stance_cooldown`.
pub const STANCE_COOLDOWN: f64 = 1.0;
/// How long after avoiding an incoming attack the `DEFENSIVE` aura state (Revenge) lasts.
pub const DEFENSIVE_STATE_DURATION: f64 = 5.0;
/// Tolerance when comparing the global cooldown with the current time (C++ `action_ready`).
const GCD_EPSILON: f64 = 0.0001;

/// A stance spell and the hidden passive that carries the stance's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StanceLink {
    pub spell: SpellId,
    /// Game id of the `stance_passive` override, if any.
    pub passive: Option<u32>,
}

/// One player character.
#[derive(Debug)]
pub struct Character {
    id: CharId,
    class: Arc<ClassSpec>,
    race: Race,
    race_stats: RaceStats,
    /// `class.stat_rules` in the form the stats module reads.
    stat_rules: ClassStatRules,
    clvl: u32,
    stats: CharacterStats,
    equipment: Equipment,
    spells: CharacterSpells,
    /// The external buffs the character is offered (`CharacterContext::add_external_buffs`).
    general_buffs: GeneralBuffs,
    modifiers: SpellModifiers,
    /// The talent setups, once attached (`CharacterContext::set_talents`).
    talents: Option<CharacterTalents>,
    roll: CombatRoll,
    /// Rolls weapon damage between the weapon's min and max (the C++ `Weapon::random`).
    dmg_roll: Random,
    resource: Resource,
    sim: SimParams,

    stance: Stance,
    /// The stance spells learned, by the stance they put the character in.
    stance_spells: Vec<(Stance, StanceLink)>,
    next_gcd: f64,
    next_stance_cd: f64,
    next_trinket_cd: f64,
    /// End of the `DEFENSIVE` aura state window.
    defensive_until: f64,
    combo_points: u32,
    tanking: bool,
    party: u8,
    member: u8,

    /// Rage kept on a stance change (Tactical Mastery). Port of `stance_rage_remainder`.
    stance_rage_retained: u32,
    /// Off-hand damage bonus in percent (Dual Wield Specialization).
    offhand_damage_percent: i32,
    /// Off-hand rage generation bonus in percent (`OFFHAND_RAGE_PERCENT`).
    offhand_rage_percent: i32,
    /// Extra main-hand attacks granted by `ADD_EXTRA_ATTACKS` and not yet performed.
    pending_extra_attacks: u32,
    /// Cached roll context, to refresh the attack tables only when it changes.
    last_roll_context: Option<RollContext>,

    rotation_name: String,
    player_name: String,
}

impl Character {
    /// Creates a level 60 character with the class base stats and the race attributes.
    ///
    /// # Panics
    /// Panics if the race is not available to the class.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: CharId,
        class: Arc<ClassSpec>,
        race: &RaceSpec,
        equipment_db: Arc<EquipmentDb>,
        phase: Phase,
        sim: SimParams,
        target_level: u32,
        party: u8,
        member: u8,
    ) -> Self {
        assert!(
            class.race_available(race.race),
            "Race {:?} not available to {:?}",
            race.race,
            class.class
        );
        let faction = race.race.faction();
        let mut stats = CharacterStats::new();
        let base = class.base_stats;
        stats.increase_strength(base.strength);
        stats.increase_agility(base.agility);
        stats.increase_stamina(base.stamina);
        stats.increase_intellect(base.intellect);
        stats.increase_spirit(base.spirit);
        stats.increase_melee_ap(base.melee_ap);
        stats.increase_ranged_ap(base.ranged_ap);
        stats.increase_melee_base_crit(base.melee_crit);
        let mut resource = Resource::new(class.resource);
        if let Some(mana) = resource.as_mana_mut() {
            mana.set_base_mana(base.mana);
        }
        let seed = u64::from(id.0) + 1;
        let mut character = Character {
            id,
            equipment: Equipment::new(equipment_db, phase, faction, class.class),
            spells: CharacterSpells::new(id, seed),
            general_buffs: GeneralBuffs::new(),
            modifiers: SpellModifiers::new(),
            talents: None,
            roll: CombatRoll::new(target_level),
            dmg_roll: Random::new(0, 1),
            resource,
            sim,
            stance: class.default_stance,
            stance_spells: Vec::new(),
            next_gcd: -class.global_cooldown,
            next_stance_cd: -STANCE_COOLDOWN,
            next_trinket_cd: -1.0,
            defensive_until: -1.0,
            combo_points: 0,
            tanking: false,
            party,
            member,
            stance_rage_retained: 0,
            offhand_damage_percent: 0,
            offhand_rage_percent: 0,
            pending_extra_attacks: 0,
            last_roll_context: None,
            rotation_name: String::new(),
            player_name: if party == 0 && member == 0 {
                "You".to_string()
            } else {
                format!("P{}M{}", party + 1, member + 1)
            },
            race: race.race,
            race_stats: race.race_stats(),
            stat_rules: class.stat_rules.rules(),
            clvl: 60,
            stats,
            class,
        };
        character.apply_stat_offsets(race.race, true);
        character
    }

    // ---------------------------------------------------------------- identity

    pub fn id(&self) -> CharId {
        self.id
    }

    pub fn class(&self) -> &Arc<ClassSpec> {
        &self.class
    }

    pub fn class_kind(&self) -> PlayerClass {
        self.class.class
    }

    pub fn race(&self) -> Race {
        self.race
    }

    pub fn faction(&self) -> Faction {
        self.race.faction()
    }

    /// Changes the race: the attributes and the per-race stat offsets follow. The racial
    /// spells are the context's business (`CharacterContext::set_race`).
    ///
    /// # Panics
    /// Panics if the race is not available to the class.
    pub(crate) fn set_race_stats(&mut self, race: &RaceSpec) {
        assert!(
            self.class.race_available(race.race),
            "Race {:?} not available to {:?}",
            race.race,
            self.class.class
        );
        self.apply_stat_offsets(self.race, false);
        self.race = race.race;
        self.race_stats = race.race_stats();
        self.apply_stat_offsets(self.race, true);
        self.equipment.set_faction(self.race.faction());
    }

    /// Port of `Character::set_special_statistics`, from the class data.
    fn apply_stat_offsets(&mut self, race: Race, apply: bool) {
        let offsets = self.class.stat_offsets(race);
        let mut adjust =
            |value: i32, inc: fn(&mut CharacterStats, u32), dec: fn(&mut CharacterStats, u32)| {
                let (amount, positive) = (value.unsigned_abs(), value > 0);
                if amount == 0 {
                    return;
                }
                if positive == apply {
                    inc(&mut self.stats, amount);
                } else {
                    dec(&mut self.stats, amount);
                }
            };
        adjust(
            offsets.strength,
            CharacterStats::increase_strength,
            CharacterStats::decrease_strength,
        );
        adjust(
            offsets.agility,
            CharacterStats::increase_agility,
            CharacterStats::decrease_agility,
        );
        adjust(
            offsets.stamina,
            CharacterStats::increase_stamina,
            CharacterStats::decrease_stamina,
        );
        adjust(
            offsets.intellect,
            CharacterStats::increase_intellect,
            CharacterStats::decrease_intellect,
        );
        adjust(
            offsets.spirit,
            CharacterStats::increase_spirit,
            CharacterStats::decrease_spirit,
        );
    }

    pub fn clvl(&self) -> u32 {
        self.clvl
    }

    pub fn set_clvl(&mut self, clvl: u32) {
        self.clvl = clvl;
        self.last_roll_context = None;
    }

    pub fn party(&self) -> u8 {
        self.party
    }

    pub fn party_member(&self) -> u8 {
        self.member
    }

    pub fn player_name(&self) -> &str {
        &self.player_name
    }

    pub fn rotation_name(&self) -> &str {
        &self.rotation_name
    }

    pub fn set_rotation_name(&mut self, name: impl Into<String>) {
        self.rotation_name = name.into();
    }

    pub fn sim(&self) -> &SimParams {
        &self.sim
    }

    pub fn set_sim(&mut self, sim: SimParams) {
        self.sim = sim;
        self.last_roll_context = None;
    }

    // ---------------------------------------------------------------- components

    pub fn stats(&self) -> &CharacterStats {
        &self.stats
    }

    pub fn stats_mut(&mut self) -> &mut CharacterStats {
        &mut self.stats
    }

    pub fn race_stats(&self) -> &RaceStats {
        &self.race_stats
    }

    pub fn class_stat_rules(&self) -> &ClassStatRules {
        &self.stat_rules
    }

    pub fn equipment(&self) -> &Equipment {
        &self.equipment
    }

    pub fn equipment_mut(&mut self) -> &mut Equipment {
        self.last_roll_context = None;
        &mut self.equipment
    }

    pub fn spells(&self) -> &CharacterSpells {
        &self.spells
    }

    pub fn spells_mut(&mut self) -> &mut CharacterSpells {
        &mut self.spells
    }

    /// The external buffs the character is offered and which of them are selected.
    pub fn external_buffs(&self) -> &GeneralBuffs {
        &self.general_buffs
    }

    pub fn spell_modifiers(&self) -> &SpellModifiers {
        &self.modifiers
    }

    pub fn spell_modifiers_mut(&mut self) -> &mut SpellModifiers {
        &mut self.modifiers
    }

    /// The talent setups, if attached.
    pub fn talents(&self) -> Option<&CharacterTalents> {
        self.talents.as_ref()
    }

    /// The talent setups for bookkeeping only: rank changes made here are not applied to
    /// the spells (use the context's talent methods for that).
    pub fn talents_mut(&mut self) -> Option<&mut CharacterTalents> {
        self.talents.as_mut()
    }

    /// Attaches (or replaces) the talent setups without touching the spells; the context's
    /// `set_talents` also syncs the spells.
    pub fn set_talents_unsynced(&mut self, talents: Option<CharacterTalents>) {
        self.talents = talents;
    }

    /// Whether `spell` is granted by a talent of the attached tree (not by the trainer).
    pub fn talent_grants(&self, spell: u32) -> bool {
        self.talents.as_ref().is_some_and(|t| t.grants(spell))
    }

    pub fn roll(&self) -> &CombatRoll {
        &self.roll
    }

    pub fn roll_mut(&mut self) -> &mut CombatRoll {
        &mut self.roll
    }

    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    pub fn resource_mut(&mut self) -> &mut Resource {
        &mut self.resource
    }

    pub fn resource_type(&self) -> ResourceType {
        self.class.resource
    }

    // ---------------------------------------------------------------- stat context

    /// Everything the composite stat getters read, for one target view.
    pub fn stat_context<'a>(&'a self, target: &'a TargetStatView) -> StatContext<'a> {
        StatContext {
            equipment: self.equipment.stats(),
            race: &self.race_stats,
            class: &self.stat_rules,
            clvl: self.clvl,
            mechanics: self.roll.mechanics(),
            mainhand: self.equipment.weapon_profile(EquipmentSlot::Mainhand),
            offhand: self.equipment.weapon_profile(EquipmentSlot::Offhand),
            ranged: self.equipment.weapon_profile(EquipmentSlot::Ranged),
            attack_mode: self.spells.attack_mode(),
            druid_feral_form: matches!(self.stance, Stance::Bear | Stance::Cat),
            target,
        }
    }

    /// What the attack tables depend on. Port of the `CombatRoll` reads of the character.
    pub fn roll_context(&self, target: &TargetStatView) -> RollContext {
        RollContext {
            clvl: self.clvl,
            melee_hit_chance: self.stats.get_melee_hit_chance(&self.stat_context(target)),
            dual_wielding: self.is_dual_wielding(),
            attacking_from_behind: self.is_attacking_from_behind(),
            glancing_blows: self.sim.glancing_blows,
        }
    }

    /// Refreshes the cached attack tables when the roll context changed since the last roll
    /// (hit chance, dual wielding, tanking). The C++ `CharacterStats` pushed these changes into
    /// the roll as they happened; here the roll is refreshed lazily.
    pub fn refresh_roll_context(&mut self, target: &TargetStatView) -> RollContext {
        let ctx = self.roll_context(target);
        if self.last_roll_context != Some(ctx) {
            self.roll.update_melee_white_miss_chance(&ctx);
            self.roll.update_melee_yellow_miss_chance(&ctx);
            self.last_roll_context = Some(ctx);
        }
        ctx
    }

    // ---------------------------------------------------------------- cooldowns

    pub fn global_cooldown(&self) -> f64 {
        self.class.global_cooldown
    }

    /// Whether an action can be started at `now`: off the global cooldown (within a rounding
    /// tolerance) and not casting. Port of `Character::action_ready`.
    pub fn action_ready(&self, now: f64) -> bool {
        self.next_gcd - now < GCD_EPSILON && !self.spells.cast_in_progress()
    }

    pub fn on_global_cooldown(&self, now: f64) -> bool {
        now < self.next_gcd
    }

    /// # Panics
    /// Panics if an action is not ready (the C++ `check`).
    pub fn start_global_cooldown(&mut self, now: f64) {
        assert!(self.action_ready(now), "Action not ready");
        self.next_gcd = now + self.global_cooldown();
    }

    pub fn time_until_action_ready(&self, now: f64) -> f64 {
        (self.next_gcd - now).max(0.0)
    }

    pub fn next_gcd(&self) -> f64 {
        self.next_gcd
    }

    pub fn on_stance_cooldown(&self, now: f64) -> bool {
        now < self.next_stance_cd
    }

    /// Starts the stance cooldown. Returns the time the global cooldown was pushed to when the
    /// stance swap lag delayed it (the caller schedules the player action). Port of
    /// `Character::start_stance_cooldown`.
    pub fn start_stance_cooldown(&mut self, now: f64) -> Option<f64> {
        self.next_stance_cd = now + STANCE_COOLDOWN;
        let lagged = now + 0.5;
        if lagged > self.next_gcd {
            self.next_gcd = lagged;
            Some(lagged)
        } else {
            None
        }
    }

    pub fn start_trinket_cooldown(&mut self, now: f64, duration: f64) {
        self.next_trinket_cd = now + duration;
    }

    pub fn on_trinket_cooldown(&self, now: f64) -> bool {
        now < self.next_trinket_cd
    }

    // ---------------------------------------------------------------- stance

    pub fn stance(&self) -> Stance {
        self.stance
    }

    /// Records the stance without touching spells; `CharacterContext::swap_stance` does the
    /// rest. Port of the state part of `Character::swap_stance` + `Warrior::new_stance_effect`.
    pub(crate) fn set_stance(&mut self, stance: Stance) {
        self.stance = stance;
        if let Some(rage) = self.resource.as_rage_mut() {
            rage.retain_at_most(self.stance_rage_retained);
        }
    }

    pub fn stance_link(&self, stance: Stance) -> Option<StanceLink> {
        self.stance_spells
            .iter()
            .find(|(s, _)| *s == stance)
            .map(|(_, link)| *link)
    }

    pub(crate) fn add_stance_link(&mut self, stance: Stance, link: StanceLink) {
        self.stance_spells.retain(|(s, _)| *s != stance);
        self.stance_spells.push((stance, link));
    }

    pub fn stance_rage_retained(&self) -> u32 {
        self.stance_rage_retained
    }

    pub fn adjust_stance_rage_retained(&mut self, delta: i32) {
        let retained = i64::from(self.stance_rage_retained) + i64::from(delta);
        assert!(retained >= 0, "Underflow stance rage retained");
        self.stance_rage_retained = retained as u32;
    }

    pub fn offhand_damage_percent(&self) -> i32 {
        self.offhand_damage_percent
    }

    /// Changes the off-hand damage bonus (`MOD_OFFHAND_DAMAGE_PCT`): the off-hand penalty
    /// becomes `0.5 × (1 + percent / 100)` (Dual Wield Specialization: 0.625 at +25 %).
    pub fn adjust_offhand_damage_percent(&mut self, percent: i32) {
        self.offhand_damage_percent += percent;
        let penalty = AutoAttack::DEFAULT_OFFHAND_PENALTY
            * (1.0 + f64::from(self.offhand_damage_percent) / 100.0);
        self.spells.oh_attack_mut().set_offhand_penalty(penalty);
    }

    pub fn offhand_rage_percent(&self) -> i32 {
        self.offhand_rage_percent
    }

    pub fn adjust_offhand_rage_percent(&mut self, percent: i32) {
        self.offhand_rage_percent += percent;
    }

    pub fn pending_extra_attacks(&self) -> u32 {
        self.pending_extra_attacks
    }

    pub fn add_extra_attacks(&mut self, count: u32) {
        self.pending_extra_attacks += count;
    }

    pub(crate) fn take_extra_attack(&mut self) -> bool {
        if self.pending_extra_attacks == 0 {
            return false;
        }
        self.pending_extra_attacks -= 1;
        true
    }

    // ---------------------------------------------------------------- combat state

    pub fn combo_points(&self) -> u32 {
        self.combo_points
    }

    pub fn gain_combo_points(&mut self, amount: u32) {
        self.combo_points = (self.combo_points + amount).min(5);
    }

    pub fn spend_combo_points(&mut self) {
        self.combo_points = 0;
    }

    pub fn is_tanking(&self) -> bool {
        self.tanking
    }

    pub fn set_tanking(&mut self, tanking: bool) {
        self.tanking = tanking;
        self.last_roll_context = None;
    }

    pub fn is_attacking_from_behind(&self) -> bool {
        !self.tanking
    }

    /// Opens the `DEFENSIVE` aura state window (Revenge) after the character dodged, parried or
    /// blocked an incoming attack at `now`.
    pub fn note_avoided_incoming_attack(&mut self, now: f64) {
        self.defensive_until = now + DEFENSIVE_STATE_DURATION;
    }

    pub fn in_defensive_state(&self, now: f64) -> bool {
        now < self.defensive_until
    }

    /// Whether both hands swing: an off-hand weapon is equipped and no on-next-swing spell is
    /// queued (a queued Heroic Strike removes the dual-wield miss penalty). Port of
    /// `Warrior::is_dual_wielding`.
    pub fn is_dual_wielding(&self) -> bool {
        self.spells.queued_next_swing().is_none() && self.equipment.is_dual_wielding()
    }

    pub fn has_mainhand(&self) -> bool {
        self.equipment.mainhand().is_some()
    }

    pub fn has_offhand(&self) -> bool {
        self.equipment.offhand().is_some()
    }

    pub fn has_ranged(&self) -> bool {
        self.equipment.ranged().is_some()
    }

    // ---------------------------------------------------------------- resources

    pub fn resource_level(&self, resource: ResourceType) -> u32 {
        if resource == self.class.resource {
            self.resource.current()
        } else {
            0
        }
    }

    pub fn max_resource_level(&self, resource: ResourceType) -> u32 {
        if resource == self.class.resource {
            self.resource.max()
        } else {
            0
        }
    }

    /// Gains `amount` of the character's resource; returns what was actually gained.
    pub fn gain_resource(&mut self, resource: ResourceType, amount: u32) -> u32 {
        if resource != self.class.resource {
            return 0;
        }
        self.resource.gain(amount)
    }

    /// # Panics
    /// Panics on underflow or for a resource the class does not use.
    pub fn lose_resource(&mut self, resource: ResourceType, amount: u32, now: f64) {
        assert!(
            resource == self.class.resource,
            "{:?} does not use {resource:?}",
            self.class.class
        );
        self.resource.lose(amount, now);
    }

    /// Rage generated by `hand` dealing `damage`, or `None` for characters without rage. Port
    /// of `Warrior::rage_gained_from_dd` with the off-hand rage percentage applied.
    pub fn rage_from_damage(&self, hand: Hand, damage: f64) -> Option<u32> {
        if self.class.resource != ResourceType::Rage {
            return None;
        }
        let rage = rage_gained_from_damage(damage, self.clvl);
        Some(match hand {
            Hand::Mainhand => rage,
            Hand::Offhand => {
                let scaled = f64::from(rage) * (1.0 + f64::from(self.offhand_rage_percent) / 100.0);
                scaled.round().max(0.0) as u32
            }
        })
    }

    // ---------------------------------------------------------------- weapon damage

    /// A uniformly distributed integer in `[min, max]`.
    pub fn random_in_range(&mut self, min: f64, max: f64) -> f64 {
        let (min, max) = (min.round().max(0.0) as u32, max.round().max(0.0) as u32);
        if max <= min {
            return f64::from(min);
        }
        self.dmg_roll.set_new_range(min, max + 1);
        f64::from(self.dmg_roll.get_roll())
    }

    /// The weapon's random damage. Port of `Weapon::get_random_dmg` (`Random(min, max)`, whose
    /// upper bound is exclusive).
    fn random_weapon_dmg(&mut self, slot: EquipmentSlot) -> Option<f64> {
        let weapon = self.equipment.slot(slot)?.weapon()?;
        let (min, max) = (weapon.min_dmg(), weapon.max_dmg());
        Some(if max <= min {
            f64::from(min)
        } else {
            self.dmg_roll.set_new_range(min, max);
            f64::from(self.dmg_roll.get_roll())
        })
    }

    /// The weapon speed a normalized attack uses: 1.7 daggers, 2.4 one-handers, 3.3 two-handers,
    /// 2.8 ranged.
    pub fn normalized_speed(weapon_slot: WeaponSlot, weapon_type: WeaponType) -> f64 {
        match weapon_slot {
            WeaponSlot::Mainhand | WeaponSlot::Offhand | WeaponSlot::OneHand => {
                if weapon_type == WeaponType::Dagger {
                    1.7
                } else {
                    2.4
                }
            }
            WeaponSlot::TwoHand => 3.3,
            WeaponSlot::Ranged => 2.8,
        }
    }

    /// `damage + speed × AP / 14`. Port of `Character::get_non_normalized_dmg`.
    pub fn non_normalized_dmg(damage: f64, attack_power: u32, speed: f64) -> f64 {
        damage + speed * f64::from(attack_power) / 14.0
    }

    pub fn melee_ap(&self, target: &TargetStatView) -> u32 {
        self.stats.get_melee_ap(&self.stat_context(target))
    }

    pub fn ranged_ap(&self, target: &TargetStatView) -> u32 {
        self.stats.get_ranged_ap(&self.stat_context(target))
    }

    /// Random mainhand damage normalized to the weapon type's standard speed (unarmed: 2.0).
    /// Port of `Character::get_random_normalized_mh_dmg`.
    pub fn random_normalized_mh_dmg(&mut self, target: &TargetStatView) -> f64 {
        let ap = self.melee_ap(target);
        let bonus = f64::from(
            self.stats
                .get_mh_weapon_damage_bonus(&self.stat_context(target)),
        );
        let Some(profile) = self
            .equipment
            .mainhand()
            .map(|w| (w.weapon_slot(), w.weapon_type()))
        else {
            return Self::non_normalized_dmg(1.0, ap, 2.0);
        };
        let damage = self
            .random_weapon_dmg(EquipmentSlot::Mainhand)
            .unwrap_or(0.0)
            + bonus;
        Self::non_normalized_dmg(damage, ap, Self::normalized_speed(profile.0, profile.1))
    }

    /// Port of `Character::get_random_non_normalized_mh_dmg`.
    pub fn random_non_normalized_mh_dmg(&mut self, target: &TargetStatView) -> f64 {
        self.random_non_normalized_dmg(target, EquipmentSlot::Mainhand)
    }

    /// Port of `Character::get_random_non_normalized_oh_dmg`.
    pub fn random_non_normalized_oh_dmg(&mut self, target: &TargetStatView) -> f64 {
        self.random_non_normalized_dmg(target, EquipmentSlot::Offhand)
    }

    fn random_non_normalized_dmg(&mut self, target: &TargetStatView, slot: EquipmentSlot) -> f64 {
        let ap = self.melee_ap(target);
        let ctx = self.stat_context(target);
        let bonus = match slot {
            EquipmentSlot::Mainhand => self.stats.get_mh_weapon_damage_bonus(&ctx),
            EquipmentSlot::Offhand => self.stats.get_oh_weapon_damage_bonus(&ctx),
            _ => self.stats.get_ranged_weapon_damage_bonus(&ctx),
        };
        let Some(speed) = self.equipment.weapon_profile(slot).map(|w| w.speed) else {
            return 0.0;
        };
        let damage = self.random_weapon_dmg(slot).unwrap_or(0.0) + f64::from(bonus);
        Self::non_normalized_dmg(damage, ap, speed)
    }

    /// Average mainhand damage including attack power, rounded. Port of
    /// `Character::get_avg_mh_damage`.
    pub fn avg_mh_damage(&self, target: &TargetStatView) -> u32 {
        let ap = self.melee_ap(target);
        let ctx = self.stat_context(target);
        let Some(weapon) = self.equipment.mainhand() else {
            return Self::non_normalized_dmg(1.0, ap, 2.0).round() as u32;
        };
        let avg = (f64::from(weapon.min_dmg() + weapon.max_dmg())
            + f64::from(self.stats.get_mh_weapon_damage_bonus(&ctx)))
        .round()
            / 2.0;
        Self::non_normalized_dmg(avg.floor(), ap, weapon.speed()).round() as u32
    }

    /// Average offhand damage including attack power (0 unless dual wielding). Port of
    /// `Character::get_avg_oh_damage`.
    pub fn avg_oh_damage(&self, target: &TargetStatView) -> u32 {
        if !self.is_dual_wielding() {
            return 0;
        }
        let ap = self.melee_ap(target);
        let ctx = self.stat_context(target);
        let Some(weapon) = self.equipment.offhand() else {
            return 0;
        };
        let avg = (f64::from(weapon.min_dmg() + weapon.max_dmg())
            + f64::from(self.stats.get_oh_weapon_damage_bonus(&ctx)))
        .round()
            / 2.0;
        Self::non_normalized_dmg(avg.floor(), ap, weapon.speed()).round() as u32
    }

    pub fn weapon_skill(&self, hand: Hand, target: &TargetStatView) -> u32 {
        let ctx = self.stat_context(target);
        match hand {
            Hand::Mainhand => self.stats.get_mh_wpn_skill(&ctx),
            Hand::Offhand => self.stats.get_oh_wpn_skill(&ctx),
        }
    }

    /// Current (hasted) swing time of `hand`; `None` when the hand is empty.
    pub fn weapon_speed(&self, hand: Hand, target: &TargetStatView) -> Option<f64> {
        let ctx = self.stat_context(target);
        match hand {
            Hand::Mainhand => self
                .equipment
                .mainhand()
                .map(|_| self.stats.get_mh_wpn_speed(&ctx)),
            Hand::Offhand => self
                .equipment
                .offhand()
                .map(|_| self.stats.get_oh_wpn_speed(&ctx)),
        }
    }

    pub fn attack_mode(&self) -> AttackMode {
        self.spells.attack_mode()
    }

    // ---------------------------------------------------------------- lifecycle

    /// The state part of `Character::reset`; the context resets spells, buffs and procs and
    /// leaves the stance.
    pub(crate) fn reset_state(&mut self) {
        self.next_gcd = -self.global_cooldown();
        self.next_stance_cd = -STANCE_COOLDOWN;
        self.next_trinket_cd = -1.0;
        self.defensive_until = -1.0;
        self.combo_points = 0;
        self.pending_extra_attacks = 0;
        self.spells.reset_state();
        self.resource.reset();
    }

    /// The state part of `Character::prepare_set_of_combat_iterations`.
    pub(crate) fn prepare_set_of_combat_iterations_state(&mut self) {
        self.spells.prepare_set_of_combat_iterations();
        self.roll.drop_tables();
        self.last_roll_context = None;
    }
}

#[cfg(test)]
mod tests;
