//! The spell runtime. Port of `Spells/Spell.*` on the client table model.
//!
//! A [`Spell`] is one [`SpellRecord`] (a spell id with everything the `Spell*` tables say about
//! it) plus its [`SpellOverride`] and runtime state: its direct effects, its cooldown handles
//! (own and category), the handle of the buff its aura effects form, the periodic state of that
//! buff, the cast in progress and the result of the last cast. Everything it needs from the
//! character, engine, target and buffs goes through the [`SpellHost`] trait (an extension of
//! [`EffectHost`]) implemented by the spell context in Phase 4; what the character has to do
//! afterwards (statistics, proc checks, triggered spells) comes back in a [`CastReport`].
//!
//! What the tables decide (see `data/SPELL_INSTRUCTIONS.md` §1.7): the global cooldown
//! (`StartRecoveryCategory` 133), the own and category cooldowns, the stance requirement
//! (`SpellShapeshift`), the aura-state gates (`SpellAuraRestrictions`: execute range, "after a
//! dodge / parry / block", "while enraged"), the equipped-item requirement, the power cost (rage
//! in tenths, combo points), on-next-swing (`Attributes_0`), passives and the cast time. What the
//! overrides add: threat, the swing-timer flags, scripts, `IGNORED`.
//!
//! Spells with a cast time (port of `Spells/CastingTimeRequirer.*`) split [`Spell::perform`]
//! in two: it starts the cast (cooldown, GCD, `CastComplete` event tagged with a cast id) and
//! [`Spell::complete_cast`] runs the effects when the event arrives.

use std::sync::Arc;

use crate::buff::{Buff, BuffApplication};
use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult};
use crate::cooldown::{add_gcd_event, CooldownControl};
use crate::effect::{ChainState, Dependency, Effect, EffectHost};
use crate::engine::{Engine, EventKind};
use crate::ids::{BuffId, CharId, CooldownId, InstanceId, SpellId};
use crate::mechanics::Mechanics;
use crate::proc::ProcSource;
use crate::resource::ResourceType;
use crate::spell::dbc::{AuraState, AuraType, PowerType, SpellEffectName, SpellModOp};
use crate::spell::overrides::{
    EventScript, Overrides, ProcHitMask, ScriptKind, SimFlag, SpellOverride, ThreatOverride,
};
use crate::spell::periodic::{Periodic, PeriodicKind, TickReport};
use crate::spell::record::{EffectRecord, EquippedItems, SpellDb, SpellRecord};
use crate::spell::{SpellResult, SpellStatus};
use crate::stance::Stance;

/// Tolerance when comparing the cooldown's ready time with the current time.
const COOLDOWN_EPSILON: f64 = 0.0001;

/// Fraction of the cost refunded when a `DISCOUNT_POWER_ON_MISS` attack is missed, dodged or
/// parried. The tables only flag the spells; the amount is Classic's.
pub const POWER_REFUND_ON_MISS: f64 = 0.8;

/// What a spell needs from the world beyond what its effects need. Port of the `Character`,
/// `CharacterSpells`, `Engine`, `EnabledBuffs` and `SimSettings` calls made by `Spell.cpp`.
pub trait SpellHost: EffectHost {
    fn character_id(&self) -> CharId;
    /// The resource the character spends and gains (which of an aura's per-power
    /// `PERIODIC_ENERGIZE` effects ticks).
    fn resource_type(&self) -> ResourceType;
    fn engine(&self) -> &Engine;
    fn engine_mut(&mut self) -> &mut Engine;
    /// Configured encounter length in seconds (execute range is derived from it).
    fn combat_length(&self) -> f64;

    fn on_global_cooldown(&self) -> bool;
    /// Length of the global cooldown in seconds.
    fn global_cooldown(&self) -> f64;
    fn start_global_cooldown(&mut self);
    fn on_stance_cooldown(&self) -> bool;
    fn start_stance_cooldown(&mut self);
    fn cast_in_progress(&self) -> bool;
    /// Marks a cast as in progress and returns its id. Port of `CharacterSpells::start_cast`.
    fn start_cast(&mut self) -> u32;
    /// Ends the cast with `cast_id` (the character schedules its reaction). Port of
    /// `CharacterSpells::complete_cast`.
    fn complete_cast(&mut self, cast_id: u32);
    fn casting_speed_mod(&self) -> f64;
    /// Flat cast time reduction in milliseconds.
    fn casting_speed_flat_reduction(&self) -> u32;
    /// Pauses auto attacks (`STOPS_ATTACK_DURING_CAST`).
    fn stop_attack(&mut self);
    fn start_attack(&mut self);
    /// Restarts both swing timers (`RESETS_SWING_TIMERS`).
    fn reset_swing_timers(&mut self);
    /// Queues `spell` to replace the next mainhand swing (on-next-swing spells); the character
    /// updates its white miss chance since a queued swing does not suffer the dual-wield penalty.
    fn queue_next_swing(&mut self, spell: SpellId);
    /// Clears the queued next-swing spell.
    fn cancel_next_swing(&mut self);
    /// The spell queued to replace the next mainhand swing, if any.
    fn queued_next_swing(&self) -> Option<SpellId>;
    fn stance(&self) -> Stance;
    /// Whether the equipped items satisfy a `SpellEquippedItems` requirement.
    fn equipped_item_matches(&self, requirement: &EquippedItems) -> bool;
    /// Whether the character is in `state` (`SpellAuraRestrictions.CasterAuraState`:
    /// `DEFENSIVE` after a dodge / parry / block, `ENRAGED` while an enrage is active, ...).
    fn caster_aura_state(&self, state: AuraState) -> bool;
    /// Whether the target is in `state` (states other than "below 20 %", which the spell
    /// derives from the encounter length).
    fn target_aura_state(&self, state: AuraState) -> bool;
    /// Whether the buff of spell `spell` (by game id) is active on its unit.
    fn aura_active(&self, spell: u32) -> bool;

    fn lose_resource(&mut self, resource: ResourceType, amount: u32);
    /// Gives back a fractional `amount` of a cost already paid (a refund on miss). Rage keeps
    /// the tenths; whole-point resources round.
    fn refund_resource(&mut self, resource: ResourceType, amount: f64);

    fn cooldown(&self, id: CooldownId) -> &CooldownControl;
    fn cooldown_mut(&mut self, id: CooldownId) -> &mut CooldownControl;

    fn buff(&self, id: BuffId) -> &Buff;
    fn buff_mut(&mut self, id: BuffId) -> &mut Buff;
    /// Applies the buff and, if it became active, its aura effects on every affected unit
    /// (cancelling any evicted debuff).
    fn apply_buff(&mut self, id: BuffId) -> BuffApplication;
    /// Cancels the buff, removing its aura effects if it was active.
    fn cancel_buff(&mut self, id: BuffId) -> bool;
    /// Enables the buff and registers it with the enabled buffs (assigning its instance id).
    fn enable_buff(&mut self, id: BuffId);
    /// Disables the buff and unregisters it.
    fn disable_buff(&mut self, id: BuffId);

    /// Casts spell `spell` (by game id) now, as a trigger of another spell: `TRIGGER_SPELL`
    /// effects, proc payloads, periodic triggers. `trigger_value` is the triggering effect's
    /// value (Deep Wounds' bleed size). Returns the report, or `None` if the character does not
    /// have the spell. The host records statistics but does not run the report's proc sources:
    /// the caller folds them into its own report ([`CastReport::all_proc_sources`]).
    fn trigger_spell(&mut self, spell: u32, trigger_value: Option<f64>) -> Option<CastReport>;
    /// Replaces the value of effect `index` of spell `spell` (`TRIGGER_WITH_VALUE`: Flurry's
    /// talent rank into its haste buff).
    fn set_spell_effect_value(&mut self, spell: u32, index: u32, value: f64);

    fn target_armor(&self) -> i32;
    /// Damage a blocked attack loses (the target's block value).
    fn target_block_value(&self) -> u32;
    fn total_physical_damage_mod(&self) -> f64;
    fn flat_physical_damage_bonus(&self) -> u32;
    fn melee_ability_crit_dmg_mod(&self) -> f64;
    fn total_threat_mod(&self) -> f64;
    /// Average base mainhand damage, without attack power (Deep Wounds bleeds for a share of it).
    fn avg_mh_weapon_damage(&self) -> f64;

    /// Whether ability `spell` also strikes with the off hand now: an `OFFHAND_COPY` aura
    /// names it and the character is dual wielding.
    fn offhand_copy_active(&self, spell: u32) -> bool;
    /// The resources gained when ability `spell` is used (`GAIN_RESOURCE_ON_USE`).
    fn resources_on_use(&self, spell: u32) -> Vec<(ResourceType, u32)>;
    /// Rolls an off-hand melee ability on the special attack table (off-hand weapon skill and
    /// crit chance), like [`EffectHost::roll_melee_ability`] for the main hand.
    fn roll_offhand_melee_ability(
        &mut self,
        included: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult;
    /// Random off-hand damage including attack power, `normalized` to the weapon type's
    /// standard speed or at the weapon's own speed.
    fn random_oh_weapon_dmg(&mut self, normalized: bool) -> f64;
    /// The off-hand damage multiplier (0.5, raised by Dual Wield Specialization).
    fn offhand_penalty(&self) -> f64;
}

/// The attack outcome of one cast, for the spell statistics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackOutcome {
    pub result: PhysicalAttackResult,
    /// Final damage after modifiers and armor (0 for avoided attacks).
    pub damage: u32,
    /// Threat generated, including innate threat.
    pub threat: f64,
    /// Seconds the cast occupied (the global cooldown for GCD spells).
    pub execution_time: f64,
}

/// The off-hand strike of a cast (Whirlwind with Raging Blows): its own attack and its own proc
/// event.
#[derive(Debug, Clone, PartialEq)]
pub struct OffhandStrike {
    pub attack: AttackOutcome,
    /// Proc sources to run after the cast's own, as a separate event.
    pub proc_sources: Vec<ProcSource>,
}

/// What happened during [`Spell::perform`]. Port of the `StatisticsSpell` / `StatisticsResource`
/// updates and `proc_sources_to_attempt` of `Spell.cpp`, returned instead of pushed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CastReport {
    pub result: SpellResult,
    /// The cost charged for this cast in displayed units (before any refund on miss).
    pub resource_cost: u32,
    /// The resource actually taken, net of a refund on miss.
    pub resource_lost: f64,
    /// Attack outcome, if the spell rolled on the attack table.
    pub attack: Option<AttackOutcome>,
    /// Resources gained by the spell's effects.
    pub resource_gained: Vec<(ResourceType, u32)>,
    /// How the buff was applied, if the spell has one.
    pub buff: Option<BuffApplication>,
    /// Proc sources to run after the cast. Port of `proc_sources_to_attempt`.
    pub proc_sources: Vec<ProcSource>,
    /// The reports of the spells this cast triggered (`TRIGGER_SPELL`), by game id.
    pub triggered: Vec<(u32, CastReport)>,
    /// The off-hand strike, when the ability also strikes with the off hand (`OFFHAND_COPY`).
    pub offhand: Option<OffhandStrike>,
    /// The spell has a cast time and only started casting; the rest of the report is empty
    /// until [`Spell::complete_cast`].
    pub cast_started: bool,
    /// The spell replaces the next mainhand swing and was queued; the rest of the report is
    /// empty until [`Spell::perform_on_swing`].
    pub queued: bool,
}

impl CastReport {
    /// The proc sources of this cast and of every spell it triggered, in order.
    pub fn all_proc_sources(&self) -> Vec<ProcSource> {
        let mut sources = self.proc_sources.clone();
        for (_, triggered) in &self.triggered {
            sources.extend(triggered.all_proc_sources());
        }
        sources
    }

    /// The total damage of this cast (off-hand strike included) and of every spell it
    /// triggered.
    pub fn total_damage(&self) -> u32 {
        self.attack.map_or(0, |a| a.damage)
            + self.offhand.as_ref().map_or(0, |o| o.attack.damage)
            + self
                .triggered
                .iter()
                .map(|(_, t)| t.total_damage())
                .sum::<u32>()
    }
}

/// The data a [`Spell`] is built from: its record and what the overrides say about it.
#[derive(Debug, Clone)]
pub struct SpellSetup {
    pub record: Arc<SpellRecord>,
    /// The spell's override (an empty one when the file has none).
    pub overrides: SpellOverride,
    /// The proc hit mask after the file defaults.
    pub hit_mask: ProcHitMask,
    /// The record of the aura a `DEEP_WOUNDS_BLEED` script ticks with (`params.duration_spell`).
    pub bleed_aura: Option<Arc<SpellRecord>>,
}

impl SpellSetup {
    /// The setup of spell `id` in `db`, if the db has it.
    pub fn from_db(db: &SpellDb, id: u32) -> Option<SpellSetup> {
        let record = Arc::clone(db.get(id)?);
        let overrides = db
            .overrides()
            .get(id)
            .cloned()
            .unwrap_or_else(|| SpellOverride::new(id));
        let bleed_aura = overrides
            .effects
            .iter()
            .find(|e| e.script == ScriptKind::DeepWoundsBleed)
            .and_then(|e| e.params.duration_spell)
            .and_then(|spell| db.get(spell).map(Arc::clone));
        Some(SpellSetup {
            record,
            hit_mask: db.overrides().proc_hit_mask(id),
            overrides,
            bleed_aura,
        })
    }

    /// A setup with no override, for building spells in code and tests.
    pub fn plain(record: SpellRecord) -> SpellSetup {
        let id = record.id;
        let defaults = Overrides::new();
        SpellSetup {
            record: Arc::new(record),
            overrides: SpellOverride::new(id),
            hit_mask: defaults.proc_hit_mask(id),
            bleed_aura: None,
        }
    }

    /// A setup with `overrides` applied to `record`.
    pub fn with_overrides(record: SpellRecord, overrides: &Overrides) -> SpellSetup {
        let id = record.id;
        SpellSetup {
            record: Arc::new(record),
            overrides: overrides
                .get(id)
                .cloned()
                .unwrap_or_else(|| SpellOverride::new(id)),
            hit_mask: overrides.proc_hit_mask(id),
            bleed_aura: None,
        }
    }

    pub fn with_bleed_aura(mut self, aura: SpellRecord) -> SpellSetup {
        self.bleed_aura = Some(Arc::new(aura));
        self
    }

    pub fn has_sim_flag(&self, flag: SimFlag) -> bool {
        self.overrides.has_sim_flag(flag)
    }

    /// Whether the spell needs a buff: it has aura effects, or a bleed script with an aura.
    pub fn has_buff(&self) -> bool {
        self.record.applies_aura() || self.bleed_aura.is_some()
    }

    /// The record the spell's buff is built from: the bleed aura for a bleed payload, otherwise
    /// the spell itself.
    pub fn buff_record(&self) -> &Arc<SpellRecord> {
        self.bleed_aura.as_ref().unwrap_or(&self.record)
    }
}

/// One spell of a character. Port of `Spell`.
#[derive(Debug, Clone)]
pub struct Spell {
    setup: SpellSetup,
    cooldown: Option<CooldownId>,
    category_cooldown: Option<CooldownId>,
    marker_buff: Option<BuffId>,
    /// The direct (non-aura) effects.
    effects: Vec<Effect>,
    instance_id: Option<InstanceId>,

    enabled: bool,
    last_result: SpellResult,
    /// The spell's handle in its character's spell list; needed for `DotTick` events.
    id: Option<SpellId>,
    /// Tick state of the buff's periodic aura, if it has one.
    periodic: Option<Periodic>,
    /// Id of the cast in progress. Port of `CastingTimeRequirer::cast_id`.
    cast_id: Option<u32>,
    /// The value handed over by the effect that triggered this spell (proc payloads).
    trigger_value: Option<f64>,
}

impl Spell {
    /// Builds the spell. `cooldown` is required when the record has an own cooldown (or a proc
    /// internal cooldown), `category_cooldown` when it has a category cooldown, and
    /// `marker_buff` when [`SpellSetup::has_buff`].
    ///
    /// The spell starts disabled; the owner calls [`Spell::enable`] (talents enable the spells
    /// they grant). In C++ this happened in the constructor, which had the character at hand.
    ///
    /// # Panics
    /// Panics if a required handle is missing.
    pub fn new(
        setup: SpellSetup,
        cooldown: Option<CooldownId>,
        category_cooldown: Option<CooldownId>,
        marker_buff: Option<BuffId>,
    ) -> Self {
        let record = &setup.record;
        assert!(
            cooldown.is_some() || Self::own_cooldown_ms(record) == 0,
            "{} ({}) has a cooldown but no cooldown control was provided",
            record.name,
            record.id
        );
        assert!(
            category_cooldown.is_some() || record.cooldown.category_recovery_ms == 0,
            "{} ({}) has a category cooldown but no category control was provided",
            record.name,
            record.id
        );
        assert!(
            marker_buff.is_some() || !setup.has_buff(),
            "{} ({}) applies auras but no marker buff was provided",
            record.name,
            record.id
        );
        let cannot_crit = setup.has_sim_flag(SimFlag::CannotCrit);
        let mut effects: Vec<Effect> = record
            .effects
            .iter()
            .filter(|e| !e.is_apply_aura())
            .map(|e| {
                Effect::new(
                    e,
                    record,
                    setup.overrides.effect_script(e.index).copied(),
                    cannot_crit,
                )
            })
            .collect();
        // The first direct effect rolls the attack, whatever its table index (pruning may have
        // removed the effects before it); the rest reuse that roll.
        for (position, effect) in effects.iter_mut().enumerate() {
            effect.set_dependency(if position == 0 {
                Dependency::Independent
            } else {
                Dependency::PartialSuccess
            });
        }

        // Periodic auras: a periodic aura effect on the buff, or a bleed script.
        let periodic = if let Some(aura) = &setup.bleed_aura {
            let period = aura
                .effects
                .iter()
                .find(|e| e.is_apply_aura() && e.aura_period_ms > 0)
                .map(|e| f64::from(e.aura_period_ms) / 1000.0)
                .unwrap_or_else(|| {
                    panic!(
                        "{} ({}) is a bleed but its aura {} has no periodic effect",
                        record.name, record.id, aura.id
                    )
                });
            Some(Periodic::new(None, period))
        } else {
            // One periodic aura effect, or several `PERIODIC_ENERGIZE` effects on one period,
            // one per power type (Essence of the Red: mana, rage and energy), of which the one
            // for the character's resource ticks.
            let mut periodic: Option<(Vec<usize>, u32)> = None;
            for (index, effect) in record
                .effects
                .iter()
                .filter(|e| e.is_apply_aura())
                .enumerate()
            {
                let script = setup.overrides.effect_script(effect.index);
                let Some(ms) = crate::spell::periodic::period_ms(effect, script) else {
                    continue;
                };
                match &mut periodic {
                    None => periodic = Some((vec![index], ms)),
                    Some((indices, period)) => {
                        let energize = |e: &EffectRecord| e.aura == AuraType::PeriodicEnergize;
                        let first = record
                            .effects
                            .iter()
                            .filter(|e| e.is_apply_aura())
                            .nth(indices[0])
                            .expect("indexed above");
                        assert!(
                            energize(first) && energize(effect) && *period == ms,
                            "{} ({}) has more than one periodic aura effect",
                            record.name,
                            record.id
                        );
                        indices.push(index);
                    }
                }
            }
            periodic.map(|(indices, ms)| Periodic::with_effects(indices, f64::from(ms) / 1000.0))
        };

        Spell {
            setup,
            cooldown,
            category_cooldown,
            marker_buff,
            effects,
            instance_id: None,
            enabled: false,
            last_result: SpellResult::Undetermined,
            id: None,
            periodic,
            cast_id: None,
            trigger_value: None,
        }
    }

    /// The own cooldown in milliseconds: `RecoveryTime`, or the proc internal cooldown of a
    /// passive without one.
    pub fn own_cooldown_ms(record: &SpellRecord) -> u32 {
        if record.cooldown.recovery_ms > 0 {
            record.cooldown.recovery_ms
        } else if record.is_passive() {
            record.aura_options.proc_category_recovery_ms
        } else {
            0
        }
    }

    // --- Static properties ---

    pub fn record(&self) -> &Arc<SpellRecord> {
        &self.setup.record
    }

    pub fn setup(&self) -> &SpellSetup {
        &self.setup
    }

    pub fn overrides(&self) -> &SpellOverride {
        &self.setup.overrides
    }

    /// The spell's game id (`SpellName.ID`).
    pub fn game_id(&self) -> u32 {
        self.setup.record.id
    }

    pub fn name(&self) -> &str {
        &self.setup.record.name
    }

    /// The rank number from the record's subtext, 1 for unranked spells.
    pub fn rank(&self) -> u32 {
        self.setup.record.rank_number().unwrap_or(1)
    }

    /// The resource the spell costs, if any (health costs do not count).
    pub fn resource_type(&self) -> Option<ResourceType> {
        self.setup
            .record
            .power
            .iter()
            .find_map(|p| ResourceType::from_power_type(p.power_type))
    }

    /// The combo points the spell costs (`PowerType` 4).
    pub fn combo_point_cost(&self) -> u32 {
        self.setup.record.power_cost(PowerType::ComboPoints).max(0) as u32
    }

    pub fn has_sim_flag(&self, flag: SimFlag) -> bool {
        self.setup.has_sim_flag(flag)
    }

    pub fn is_passive(&self) -> bool {
        self.setup.record.is_passive()
    }

    /// Whether the sim leaves the spell unused (`IGNORED`).
    pub fn is_ignored(&self) -> bool {
        self.setup.overrides.is_ignored()
    }

    /// Whether the spell triggers the global cooldown.
    pub fn triggers_gcd(&self) -> bool {
        self.setup.record.triggers_gcd()
    }

    /// Whether the spell changes stance (it has a `MOD_SHAPESHIFT` aura).
    pub fn is_stance_spell(&self) -> bool {
        self.setup
            .record
            .effects
            .iter()
            .any(|e| e.is_apply_aura() && e.aura == AuraType::ModShapeshift)
    }

    /// The stance the spell puts the character in, if it is a stance spell.
    pub fn stance(&self) -> Option<Stance> {
        self.setup
            .record
            .effects
            .iter()
            .find(|e| e.is_apply_aura() && e.aura == AuraType::ModShapeshift)
            .and_then(|e| Stance::from_form(e.shapeshift_form()))
    }

    /// The hidden passive carrying a stance's numbers, from the overrides.
    pub fn stance_passive(&self) -> Option<u32> {
        self.setup.overrides.stance_passive
    }

    /// The event reactions the overrides attach to the spell.
    pub fn event_scripts(&self) -> &[EventScript] {
        &self.setup.overrides.on_event
    }

    /// The hidden spells this spell casts as payloads: its `trigger_spell`s and the spells
    /// its scripts name (`TRIGGER_WITH_VALUE`, event reactions). They are enabled and disabled
    /// together with this spell.
    pub fn payload_spells(&self) -> Vec<u32> {
        let mut ids = self.setup.record.trigger_spells();
        let scripted = self
            .setup
            .overrides
            .effects
            .iter()
            .filter_map(|script| script.params.spell)
            .chain(
                self.setup
                    .overrides
                    .on_event
                    .iter()
                    .filter_map(|script| script.params.spell),
            );
        for id in scripted {
            if id != self.game_id() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    pub fn threat_override(&self) -> ThreatOverride {
        self.setup.overrides.threat.unwrap_or_default()
    }

    pub fn cooldown_id(&self) -> Option<CooldownId> {
        self.cooldown
    }

    pub fn category_cooldown_id(&self) -> Option<CooldownId> {
        self.category_cooldown
    }

    pub fn marker_buff(&self) -> Option<BuffId> {
        self.marker_buff
    }

    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    pub fn effects_mut(&mut self) -> &mut Vec<Effect> {
        &mut self.effects
    }

    /// Whether the spell has a cast time (as opposed to being instant).
    pub fn has_cast_time(&self) -> bool {
        self.setup.record.cast_time_ms > 0
    }

    /// Whether this spell's cast is in progress.
    pub fn is_casting(&self) -> bool {
        self.cast_id.is_some()
    }

    /// Whether the spell replaces the next mainhand swing (Heroic Strike, Cleave).
    pub fn is_on_next_swing(&self) -> bool {
        self.setup.record.is_on_next_swing()
    }

    /// Whether this on-next-swing spell is queued for the next mainhand swing.
    pub fn is_queued(&self, host: &impl SpellHost) -> bool {
        self.is_on_next_swing() && self.id.is_some() && host.queued_next_swing() == self.id
    }

    /// The cast time in seconds after modifiers, haste and flat reductions. Port of
    /// `CastingTimeRequirer::get_cast_time`.
    pub fn cast_time(&self, host: &impl SpellHost) -> f64 {
        let record = &self.setup.record;
        let ms = host.spell_modifiers().apply(
            record.class_options.as_ref(),
            SpellModOp::ChangeCastTime,
            f64::from(record.cast_time_ms),
        );
        let flat_reduction = f64::from(host.casting_speed_flat_reduction()) / 1000.0;
        let after_mod = ms.max(0.0) / 1000.0 / host.casting_speed_mod();
        (after_mod - flat_reduction).max(0.0)
    }

    /// Crit chance added by modifiers (`CRIT_CHANCE`), hundredths of a percent.
    pub fn crit_chance_bonus(&self, host: &impl SpellHost) -> u32 {
        let record = &self.setup.record;
        (host
            .spell_modifiers()
            .flat(record.class_options.as_ref(), SpellModOp::CritChance)
            * 100.0)
            .round()
            .max(0.0) as u32
    }

    /// The proc chance of a passive: `SpellAuraOptions.ProcChance` in percent (0 and 101 mean
    /// "always"), modified by `PROC_CHANCE`, as a fraction. The aura's own value is the payload
    /// (Deep Wounds' bleed percent, Flurry's haste), never the chance.
    pub fn proc_chance(&self, host: &impl SpellHost) -> f64 {
        let record = &self.setup.record;
        let chance = record.aura_options.proc_chance;
        let from_effect = self
            .setup
            .overrides
            .proc
            .and_then(|p| p.chance_effect)
            .and_then(|index| {
                let buff = host.buff(self.marker_buff?);
                buff.effects
                    .iter()
                    .find(|e| e.index() == index)
                    .map(Effect::value)
            });
        let from_override = self.setup.overrides.proc.and_then(|p| p.chance);
        let percent = match from_effect.or(from_override) {
            Some(percent) => percent,
            None if chance == 0 || chance > 100 => 100.0,
            None => f64::from(chance),
        };
        let percent = host.spell_modifiers().apply(
            record.class_options.as_ref(),
            SpellModOp::ProcChance,
            percent,
        );
        (percent / 100.0).clamp(0.0, 1.0)
    }

    pub fn last_result(&self) -> SpellResult {
        self.last_result
    }

    pub fn id(&self) -> Option<SpellId> {
        self.id
    }

    /// Sets the spell's handle in its character's spell list (required for periodic spells).
    pub fn set_id(&mut self, id: SpellId) {
        self.id = Some(id);
    }

    /// The periodic aura state, for spells whose buff ticks.
    pub fn periodic(&self) -> Option<&Periodic> {
        self.periodic.as_ref()
    }

    pub fn is_periodic(&self) -> bool {
        self.periodic.is_some()
    }

    /// Multiplier on all damage from `HEALING_AND_DAMAGE` modifiers (Improved Revenge).
    pub fn damage_mod(&self, host: &impl SpellHost) -> f64 {
        host.spell_modifiers().multiplier(
            self.setup.record.class_options.as_ref(),
            SpellModOp::HealingAndDamage,
        )
    }

    /// Multiplier on periodic damage from `PERIODIC_HEALING_AND_DAMAGE` modifiers (Improved
    /// Rend), on top of [`Spell::damage_mod`].
    pub fn periodic_damage_mod(&self, host: &impl SpellHost) -> f64 {
        self.damage_mod(host)
            * host.spell_modifiers().multiplier(
                self.setup.record.class_options.as_ref(),
                SpellModOp::PeriodicHealingAndDamage,
            )
    }

    /// Multiplier on the crit damage bonus from `CRIT_DAMAGE_AND_HEALING` modifiers (Impale):
    /// the bonus part of the crit multiplier grows by the percentage.
    pub fn crit_damage_mod(&self, host: &impl SpellHost) -> f64 {
        let base = host.melee_ability_crit_dmg_mod();
        let bonus = host.spell_modifiers().pct(
            self.setup.record.class_options.as_ref(),
            SpellModOp::CritDamageAndHealing,
        );
        1.0 + (base - 1.0) * (1.0 + bonus / 100.0)
    }

    pub fn instance_id(&self) -> Option<InstanceId> {
        self.instance_id
    }

    pub fn set_instance_id(&mut self, id: InstanceId) {
        self.instance_id = Some(id);
    }

    /// The value the triggering effect handed over, if the spell was triggered.
    pub fn trigger_value(&self) -> Option<f64> {
        self.trigger_value
    }

    pub fn set_trigger_value(&mut self, value: Option<f64>) {
        self.trigger_value = value;
    }

    /// Whether the character has learned this rank (`SpellLevels.BaseLevel`).
    pub fn is_rank_learned(&self, host: &impl SpellHost) -> bool {
        host.caster_level() >= self.setup.record.learn_level()
    }

    // --- Cooldown / cost ---

    /// The own cooldown length in seconds after `COOLDOWN` modifiers.
    pub fn cooldown_seconds(&self, host: &impl SpellHost) -> f64 {
        let record = &self.setup.record;
        let ms = host.spell_modifiers().apply(
            record.class_options.as_ref(),
            SpellModOp::Cooldown,
            f64::from(Self::own_cooldown_ms(record)),
        );
        ms.max(0.0) / 1000.0
    }

    /// The category cooldown length in seconds after `COOLDOWN` modifiers, which the server
    /// applies to both cooldowns (Improved Slam shortens Slam's category cooldown).
    pub fn category_cooldown_seconds(&self, host: &impl SpellHost) -> f64 {
        let record = &self.setup.record;
        let ms = host.spell_modifiers().apply(
            record.class_options.as_ref(),
            SpellModOp::Cooldown,
            f64::from(record.cooldown.category_recovery_ms),
        );
        ms.max(0.0) / 1000.0
    }

    pub fn last_used(&self, host: &impl SpellHost) -> f64 {
        self.cooldown
            .into_iter()
            .chain(self.category_cooldown)
            .map(|id| host.cooldown(id).last_used)
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// The time the spell is next usable: the later of its own and its category cooldown.
    pub fn next_use(&self, host: &impl SpellHost) -> f64 {
        self.cooldown
            .into_iter()
            .chain(self.category_cooldown)
            .map(|id| host.cooldown(id).next_use())
            .fold(f64::NEG_INFINITY, f64::max)
    }

    pub fn cooldown_remaining(&self, host: &impl SpellHost) -> f64 {
        (self.next_use(host) - host.engine().current_time()).max(0.0)
    }

    /// The cost of a cast in displayed units after `POWER_COST` modifiers (Improved Heroic
    /// Strike: −10 stored = −1 rage). Port of `Spell::get_resource_cost`.
    pub fn resource_cost(&self, host: &impl SpellHost) -> u32 {
        let record = &self.setup.record;
        let Some(resource) = self.resource_type() else {
            return 0;
        };
        let stored = f64::from(record.power_cost(resource.power_type()));
        let modifiers = host.spell_modifiers();
        let class = record.class_options.as_ref();
        let stored = (stored + modifiers.flat(class, SpellModOp::PowerCost0))
            * modifiers.multiplier(class, SpellModOp::PowerCostPct);
        resource.from_stored_amount(stored.max(0.0))
    }

    // --- Enable / disable ---

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables the spell and its buff. A passive's aura goes up right away (permanent buffs)
    /// when its equipment / stance conditions hold.
    ///
    /// # Panics
    /// Panics if already enabled.
    pub fn enable(&mut self, host: &mut impl SpellHost) {
        assert!(
            !self.enabled,
            "Tried to enable an already enabled spell '{}'",
            self.name()
        );
        self.enabled = true;
        if let Some(id) = self.marker_buff {
            // Party buffs and shared debuffs are enabled when registered with the raid.
            if !host.buff(id).is_enabled() {
                host.enable_buff(id);
            }
        }
        self.reevaluate_passive(host);
    }

    pub fn disable(&mut self, host: &mut impl SpellHost) {
        if let Some(id) = self.marker_buff {
            host.cancel_buff(id);
            let buff = host.buff(id);
            if buff.is_enabled() && !buff.kind().is_raid_shared() {
                host.disable_buff(id);
            }
        }
        self.enabled = false;
    }

    /// Whether the spell strikes with the shield rather than the main hand (Shield Slam, Shield
    /// Bash: `SpellEquippedItems` asks for a shield). Such attacks count as off-hand attacks
    /// and do not trigger main-hand procs (Windfury).
    pub fn is_offhand_attack(&self) -> bool {
        self.setup
            .record
            .equipped_items
            .is_some_and(|items| items.requires_shield())
    }

    /// Whether the equipment and stance conditions of the record hold.
    pub fn conditions_hold(&self, host: &impl SpellHost) -> bool {
        let record = &self.setup.record;
        host.stance().allowed_by_mask(record.shapeshift_mask)
            && record
                .equipped_items
                .as_ref()
                .is_none_or(|items| host.equipped_item_matches(items))
    }

    /// Applies or cancels a passive's aura according to its conditions (called after enabling,
    /// and by the character when the equipment or stance changes).
    pub fn reevaluate_passive(&mut self, host: &mut impl SpellHost) {
        if !self.enabled || !self.is_passive() {
            return;
        }
        let Some(id) = self.marker_buff else {
            return;
        };
        let active = host.buff(id).is_active();
        if self.conditions_hold(host) {
            if !active {
                let application = host.apply_buff(id);
                if self.id.is_some() {
                    self.on_buff_applied(application, host);
                }
            }
        } else if active {
            host.cancel_buff(id);
        }
    }

    // --- Status ---

    /// Whether the spell can be cast now, and if not why. Port of `Spell::get_spell_status`.
    pub fn status(&self, host: &impl SpellHost) -> SpellStatus {
        let record = &self.setup.record;
        if !self.enabled {
            return SpellStatus::NotEnabled;
        }
        if self.is_ignored() {
            return SpellStatus::NotSupported;
        }
        if self.triggers_gcd() && host.on_global_cooldown() {
            return SpellStatus::OnGcd;
        }
        if host.cast_in_progress() {
            return SpellStatus::CastInProgress;
        }
        let now = host.engine().current_time();
        if self.next_use(host) - now > COOLDOWN_EPSILON {
            return SpellStatus::OnCooldown;
        }
        if let Some(resource) = self.resource_type() {
            if host.resource_level(resource) < self.resource_cost(host) {
                return SpellStatus::InsufficientResources;
            }
        }
        if host.combo_points() < self.combo_point_cost() {
            return SpellStatus::InsufficientComboPoints;
        }
        // Both normal GCD spells and stance changes wait for the stance cooldown (C++).
        if (self.triggers_gcd() || self.is_stance_spell()) && host.on_stance_cooldown() {
            return SpellStatus::OnStanceCooldown;
        }
        if !host.stance().allowed_by_mask(record.shapeshift_mask) {
            return SpellStatus::in_stance(host.stance());
        }
        // A stance spell does nothing while in its stance (the C++ stance spells' own check).
        if self.stance().is_some_and(|stance| stance == host.stance()) {
            return SpellStatus::in_stance(host.stance());
        }
        let restrictions = &record.aura_restrictions;
        match restrictions.target_aura_state {
            AuraState::None => {}
            AuraState::Wounded20Percent
            | AuraState::Wounded25Percent
            | AuraState::Wounded35Percent => {
                let fraction = match restrictions.target_aura_state {
                    AuraState::Wounded20Percent => 0.2,
                    AuraState::Wounded25Percent => 0.25,
                    _ => 0.35,
                };
                let combat_length = host.combat_length();
                let time_remaining = combat_length - now;
                if time_remaining / combat_length > fraction {
                    return SpellStatus::NotInExecuteRange;
                }
            }
            state => {
                if !host.target_aura_state(state) {
                    return SpellStatus::BuffInactive;
                }
            }
        }
        if restrictions.caster_aura_state != AuraState::None
            && !host.caster_aura_state(restrictions.caster_aura_state)
        {
            return SpellStatus::BuffInactive;
        }
        if restrictions.caster_aura_spell != 0 && !host.aura_active(restrictions.caster_aura_spell)
        {
            return SpellStatus::BuffInactive;
        }
        if restrictions.target_aura_spell != 0 && !host.aura_active(restrictions.target_aura_spell)
        {
            return SpellStatus::BuffInactive;
        }
        if let Some(items) = &record.equipped_items {
            if !host.equipped_item_matches(items) {
                return SpellStatus::IncorrectWeaponType;
            }
        }
        SpellStatus::Available
    }

    // --- Casting ---

    /// Casts the spell. Port of `Spell::perform` + `Spell::spell_effect`.
    ///
    /// # Panics
    /// Panics if the character cannot afford the spell (callers check [`Spell::status`] first).
    pub fn perform(&mut self, host: &mut impl SpellHost) -> CastReport {
        let cost = self.resource_cost(host);
        if let Some(resource) = self.resource_type() {
            assert!(
                host.resource_level(resource) >= cost,
                "Tried to perform '{}' but has insufficient resource {} (requires {cost})",
                self.name(),
                host.resource_level(resource)
            );
        }

        self.last_result = SpellResult::Undetermined;
        let report = CastReport {
            resource_cost: cost,
            ..CastReport::default()
        };

        self.start_cooldown(host);
        let character = host.character_id();
        if self.triggers_gcd() {
            let gcd = self.global_cooldown(host);
            if add_gcd_event(host.engine_mut(), character, gcd) {
                host.start_global_cooldown();
            }
        }
        if self.is_stance_spell() {
            assert!(
                !host.on_stance_cooldown(),
                "Spell {} already on stance cooldown when starting another",
                self.name()
            );
            host.start_stance_cooldown();
        }

        if self.has_cast_time() {
            return self.start_cast(host, report);
        }
        if self.is_on_next_swing() {
            return self.queue(host, report);
        }
        self.execute(host, report)
    }

    /// Casts the spell as another spell's effect (a proc's payload, an item's chance on hit):
    /// no cost, no global cooldown, no cast time.
    pub fn perform_triggered(&mut self, host: &mut impl SpellHost) -> CastReport {
        self.last_result = SpellResult::Undetermined;
        self.start_cooldown(host);
        self.execute(host, CastReport::default())
    }

    /// Starts the own and category cooldowns with their player-action events (a proc's internal
    /// cooldown when the proc fires without performing the spell).
    pub fn start_cooldown(&self, host: &mut impl SpellHost) {
        let now = host.engine().current_time();
        let character = host.character_id();
        if let Some(id) = self.cooldown {
            let duration = self.cooldown_seconds(host);
            if duration > 0.0 {
                host.cooldown_mut(id).start_for(now, duration);
                host.engine_mut()
                    .add_event_in(duration, EventKind::PlayerAction { character });
            }
        }
        if let Some(id) = self.category_cooldown {
            let duration = self.category_cooldown_seconds(host);
            if duration > 0.0 {
                host.cooldown_mut(id).start_for(now, duration);
                host.engine_mut()
                    .add_event_in(duration, EventKind::PlayerAction { character });
            }
        }
    }

    /// The global cooldown this cast triggers, after `START_COOLDOWN` modifiers (Improved Slam).
    pub fn global_cooldown(&self, host: &impl SpellHost) -> f64 {
        let record = &self.setup.record;
        let ms = host.spell_modifiers().apply(
            record.class_options.as_ref(),
            SpellModOp::StartCooldown,
            host.global_cooldown() * 1000.0,
        );
        ms.max(0.0) / 1000.0
    }

    /// Queues the spell for the next mainhand swing: its marker buff marks it as queued. Port
    /// of `HeroicStrike::spell_effect`.
    fn queue(&mut self, host: &mut impl SpellHost, mut report: CastReport) -> CastReport {
        let id = self
            .id
            .unwrap_or_else(|| panic!("on-next-swing spell {} has no id", self.name()));
        if let Some(marker) = self.marker_buff {
            report.buff = Some(host.apply_buff(marker));
        }
        host.queue_next_swing(id);
        report.queued = true;
        report
    }

    /// The mainhand swing the spell was queued for lands: un-queues it and runs its effects.
    /// The caller has checked [`Spell::status`] and completed the swing timer. Port of
    /// `HeroicStrike::calculate_damage`.
    pub fn perform_on_swing(&mut self, host: &mut impl SpellHost) -> CastReport {
        self.cancel(host);
        host.cancel_next_swing();
        self.last_result = SpellResult::Undetermined;
        let report = CastReport {
            resource_cost: self.resource_cost(host),
            ..CastReport::default()
        };
        self.execute(host, report)
    }

    /// Starts casting: `CastComplete` is scheduled after the cast time. Port of
    /// `CastingTimeRequirer::start_cast` plus the `Slam::spell_effect` attack handling.
    fn start_cast(&mut self, host: &mut impl SpellHost, mut report: CastReport) -> CastReport {
        let cast_id = host.start_cast();
        self.cast_id = Some(cast_id);
        if self.has_sim_flag(SimFlag::StopsAttackDuringCast) {
            host.stop_attack();
        }
        if self.has_sim_flag(SimFlag::CancelsNextSwingQueue) {
            host.cancel_next_swing();
        }
        let id = self
            .id
            .unwrap_or_else(|| panic!("cast-time spell {} has no id", self.name()));
        let (character, cast_time) = (host.character_id(), self.cast_time(host));
        host.engine_mut().add_event_in(
            cast_time,
            EventKind::CastComplete {
                character,
                spell: id,
                cast_id,
            },
        );
        report.cast_started = true;
        report
    }

    /// Handles a `CastComplete` event: runs the effects of the cast started with `cast_id`.
    /// Returns `None` for a stale cast id. Port of `CastingTimeRequirer::complete_cast` plus
    /// `Slam::complete_cast_effect`'s swing timer handling.
    pub fn complete_cast(&mut self, cast_id: u32, host: &mut impl SpellHost) -> Option<CastReport> {
        if self.cast_id != Some(cast_id) {
            return None;
        }
        self.cast_id = None;
        host.complete_cast(cast_id);
        if self.has_sim_flag(SimFlag::ResetsSwingTimers) {
            host.reset_swing_timers();
        }
        if self.has_sim_flag(SimFlag::StopsAttackDuringCast) {
            host.start_attack();
        }
        let report = CastReport {
            resource_cost: self.resource_cost(host),
            ..CastReport::default()
        };
        Some(self.execute(host, report))
    }

    /// Runs the effect chain, applies the buff, pays the cost, collects the damage and casts
    /// the triggered spells. The second half of `Spell::spell_effect`.
    fn execute(&mut self, host: &mut impl SpellHost, mut report: CastReport) -> CastReport {
        let cost = report.resource_cost;
        let resource = self.resource_type();

        let mut first_roll = None;
        let mut innate_threat = self.threat_override().flat;
        let mut triggers = Vec::new();
        let mut consumes_all = false;
        if self.effects.is_empty() {
            // Nothing that must succeed (a pure buff): apply the buff immediately.
            self.last_result = SpellResult::Success;
        } else {
            let mut chain = ChainState {
                result: SpellResult::Undetermined,
                previous: None,
                resource_cost: cost,
                extra_crit: self.crit_chance_bonus(host),
            };
            // The effects are taken out so the host can be borrowed mutably alongside them.
            let mut effects = std::mem::take(&mut self.effects);
            for (position, effect) in effects.iter_mut().enumerate() {
                let outcome = effect.perform(host, &chain);
                if position == 0 {
                    first_roll = effect.last_result;
                    chain.previous = first_roll;
                }
                chain.result = chain.result.merge(outcome.success);
                if let Some(rolled) = outcome.rolled {
                    match rolled {
                        PhysicalAttackResult::Miss => {
                            report.proc_sources.push(ProcSource::MeleeMiss)
                        }
                        PhysicalAttackResult::Dodge => {
                            report.proc_sources.push(ProcSource::MeleeDodge)
                        }
                        PhysicalAttackResult::Parry => {
                            report.proc_sources.push(ProcSource::MeleeParry)
                        }
                        _ => {}
                    }
                }
                if let Some(gained) = outcome.resource_gained {
                    report.resource_gained.push(gained);
                }
                if outcome.success {
                    innate_threat += outcome.threat;
                    if let Some(trigger) = outcome.trigger {
                        triggers.push(trigger);
                    }
                    consumes_all |= outcome.consumes_all_resource;
                }
            }
            self.effects = effects;
            self.last_result = chain.result;
        }

        // The marker buff of an on-next-swing spell only marks it as queued.
        let marker = self.marker_buff.filter(|_| !self.is_on_next_swing());
        if let Some(id) = marker {
            if self.last_result.applies_buff() {
                let application = host.apply_buff(id);
                report.buff = Some(application);
                self.on_buff_applied(application, host);
            }
        }

        // Resource loss and damage.
        if self.last_result == SpellResult::Failure {
            // The full cost is paid; `DISCOUNT_POWER_ON_MISS` spells get most of it back when
            // the attack is missed, dodged or parried.
            let refund = match first_roll {
                Some(
                    PhysicalAttackResult::Miss
                    | PhysicalAttackResult::Dodge
                    | PhysicalAttackResult::Parry,
                ) if self.setup.record.refunds_power_on_miss() => {
                    f64::from(cost) * POWER_REFUND_ON_MISS
                }
                _ => 0.0,
            };
            if let Some(resource) = resource {
                host.lose_resource(resource, cost);
                if refund > 0.0 {
                    host.refund_resource(resource, refund);
                }
            }
            report.resource_lost = f64::from(cost) - refund;
            if let Some(result) = first_roll {
                report.attack = Some(AttackOutcome {
                    result,
                    damage: 0,
                    threat: 0.0,
                    execution_time: self.execution_time(host),
                });
            }
        } else {
            let lost = if consumes_all {
                resource.map_or(cost, |r| host.resource_level(r))
            } else {
                cost
            };
            if let Some(resource) = resource {
                host.lose_resource(resource, lost);
            }
            report.resource_lost = f64::from(lost);
            // Combo points (Overpower's dodge marker) are spent by a successful cast.
            if self.combo_point_cost() > 0 {
                host.spend_combo_points();
            }
            report.attack =
                self.collect_damage(host, first_roll, innate_threat, &mut report.proc_sources);
        }

        if self.last_result != SpellResult::Failure {
            for (resource, amount) in host.resources_on_use(self.game_id()) {
                let gained = host.gain_resource(resource, amount);
                if gained > 0 {
                    report.resource_gained.push((resource, gained));
                }
            }
        }

        if host.offhand_copy_active(self.game_id()) {
            report.offhand = self.offhand_strike(host);
        }

        for trigger in triggers {
            if let Some(triggered) = host.trigger_spell(trigger, None) {
                report.triggered.push((trigger, triggered));
            }
        }

        assert!(
            self.last_result != SpellResult::Undetermined,
            "Spell {} result undetermined",
            self.name()
        );
        report.result = self.last_result;
        report
    }

    /// Sums the damage of the effects, applies crit / modifiers / armor and produces the attack
    /// outcome. Port of the damage aggregation at the end of `Spell::spell_effect`, going through
    /// `damage_after_modifiers` like the hardcoded C++ spells did.
    fn collect_damage(
        &mut self,
        host: &mut impl SpellHost,
        first_roll: Option<PhysicalAttackResult>,
        innate_threat: f64,
        proc_sources: &mut Vec<ProcSource>,
    ) -> Option<AttackOutcome> {
        let mut raw_damage = 0.0;
        for effect in &mut self.effects {
            raw_damage += effect.damage_dealt;
            effect.damage_dealt = 0.0;
        }
        raw_damage *= self.damage_mod(host);
        // A spell without a roll (Sunder Armor's threat, a pure buff) reports an outcome only
        // when it did something worth counting.
        let result = match first_roll {
            Some(result) => result,
            None if raw_damage > 0.0 || innate_threat != 0.0 => PhysicalAttackResult::Hit,
            None => return None,
        };

        let crit = matches!(
            result,
            PhysicalAttackResult::Critical | PhysicalAttackResult::BlockCritical
        );
        let damage = match result {
            PhysicalAttackResult::Miss
            | PhysicalAttackResult::Dodge
            | PhysicalAttackResult::Parry => {
                return Some(AttackOutcome {
                    result,
                    damage: 0,
                    threat: 0.0,
                    execution_time: self.execution_time(host),
                });
            }
            _ if crit => self.damage_after_modifiers(host, raw_damage) * self.crit_damage_mod(host),
            _ => self.damage_after_modifiers(host, raw_damage),
        };
        // A landed melee ability is reported as a main-hand ability (`melee_mh_yellow_hit_effect`)
        // unless it strikes with the shield (Shield Slam: an off-hand attack, which procs
        // nothing main-hand like Windfury); a crit additionally by its result, for the crit-only
        // procs. See `ProcSource::from_masks`.
        if first_roll.is_some() {
            if !self.is_offhand_attack() {
                proc_sources.push(ProcSource::MainhandSpell);
            }
            if crit {
                proc_sources.push(ProcSource::MeleeCritical);
            }
        }
        let damage = match result {
            PhysicalAttackResult::Block | PhysicalAttackResult::BlockCritical => {
                damage - f64::from(host.target_block_value())
            }
            _ => damage,
        };
        let damage = damage.round().max(0.0) as u32;
        let threat = (f64::from(damage) + innate_threat)
            * host.total_threat_mod()
            * self.threat_override().modifier;
        Some(AttackOutcome {
            result,
            damage,
            threat,
            execution_time: self.execution_time(host),
        })
    }

    /// The off-hand strike of an ability with an off-hand copy (Raging Blows' Whirlwind). It
    /// rolls on its own, whatever the main hand did, with the off-hand weapon skill and crit
    /// chance; the weapon damage effects deal the off-hand weapon's damage times the off-hand
    /// penalty, then the modifiers, crit bonus, armor and block of a main-hand hit apply. Only
    /// the weapon damage effects are copied. `None` for a spell without one.
    fn offhand_strike(&mut self, host: &mut impl SpellHost) -> Option<OffhandStrike> {
        use SpellEffectName as E;
        let is_weapon_damage = |kind| {
            matches!(
                kind,
                E::NormalizedWeaponDmg
                    | E::WeaponDamage
                    | E::WeaponDamageNoschool
                    | E::WeaponPercentDamage
            )
        };
        let weapon = self.effects.iter().find(|e| is_weapon_damage(e.kind()))?;
        let (included, can_crit) = (weapon.included_outcomes(), weapon.can_crit());
        let extra_crit = self.crit_chance_bonus(host);
        let result = host.roll_offhand_melee_ability(included, extra_crit, can_crit);

        let mut proc_sources = Vec::new();
        let avoided = match result {
            PhysicalAttackResult::Miss => Some(ProcSource::MeleeMiss),
            PhysicalAttackResult::Dodge => Some(ProcSource::MeleeDodge),
            PhysicalAttackResult::Parry => Some(ProcSource::MeleeParry),
            _ => None,
        };
        if let Some(source) = avoided {
            proc_sources.push(source);
            let attack = AttackOutcome {
                result,
                damage: 0,
                threat: 0.0,
                execution_time: 0.0,
            };
            return Some(OffhandStrike {
                attack,
                proc_sources,
            });
        }

        let mut raw_damage = 0.0;
        for effect in &self.effects {
            let value = effect.effective_value(&*host);
            raw_damage += match effect.kind() {
                E::NormalizedWeaponDmg => host.random_oh_weapon_dmg(true) + value,
                E::WeaponDamage | E::WeaponDamageNoschool => {
                    host.random_oh_weapon_dmg(false) + value
                }
                E::WeaponPercentDamage => host.random_oh_weapon_dmg(false) * value / 100.0,
                _ => 0.0,
            };
        }
        raw_damage *= host.offhand_penalty() * self.damage_mod(host);

        let crit = matches!(
            result,
            PhysicalAttackResult::Critical | PhysicalAttackResult::BlockCritical
        );
        let mut damage = self.damage_after_modifiers(host, raw_damage);
        proc_sources.push(ProcSource::OffhandSpell);
        if crit {
            damage *= self.crit_damage_mod(host);
            proc_sources.push(ProcSource::MeleeCritical);
        }
        if matches!(
            result,
            PhysicalAttackResult::Block | PhysicalAttackResult::BlockCritical
        ) {
            damage -= f64::from(host.target_block_value());
        }
        let damage = damage.round().max(0.0) as u32;
        let threat = f64::from(damage) * host.total_threat_mod() * self.threat_override().modifier;
        Some(OffhandStrike {
            attack: AttackOutcome {
                result,
                damage,
                threat,
                execution_time: 0.0,
            },
            proc_sources,
        })
    }

    /// The damage done multiplier of the spell's school: the physical one (Death Wish, Enrage)
    /// for a physical spell, none otherwise (magic damage modifiers are not ported), as in
    /// [`Spell::damage_after_modifiers`].
    fn school_damage_mod(&self, host: &impl SpellHost) -> f64 {
        let school = self.setup.record.school_mask;
        if !school.is_empty() && !school.is_physical() {
            return 1.0;
        }
        host.total_physical_damage_mod()
    }

    /// Port of `Spell::damage_after_modifiers`. The physical damage modifiers and armor only
    /// apply to physical spells: the damage of another school (an item's Nature proc) lands as
    /// it is, resistances and magic damage modifiers not being ported.
    pub fn damage_after_modifiers(&self, host: &impl SpellHost, damage: f64) -> f64 {
        let school = self.setup.record.school_mask;
        if !school.is_empty() && !school.is_physical() {
            return damage;
        }
        let armor_reduction =
            1.0 - Mechanics::reduction_from_armor(host.target_armor(), host.caster_level());
        (damage * host.total_physical_damage_mod() + f64::from(host.flat_physical_damage_bonus()))
            * armor_reduction
    }

    fn execution_time(&self, host: &impl SpellHost) -> f64 {
        if self.has_cast_time() {
            return self.cast_time(host);
        }
        if self.triggers_gcd() {
            host.global_cooldown()
        } else {
            0.0
        }
    }

    /// Cancels the marker buff (and, for on-next-swing spells, the queue). Port of
    /// `Spell::cancel` / `HeroicStrike::cancel`.
    pub fn cancel(&mut self, host: &mut impl SpellHost) {
        if let Some(id) = self.marker_buff {
            host.cancel_buff(id);
        }
        if self.is_on_next_swing() {
            host.cancel_next_swing();
        }
    }

    /// Start of a combat iteration. Port of `Spell::reset`.
    pub fn reset(&mut self, host: &mut impl SpellHost) {
        if let Some(id) = self.cooldown {
            host.cooldown_mut(id).reset();
        }
        if let Some(id) = self.category_cooldown {
            host.cooldown_mut(id).reset();
        }
        self.last_result = SpellResult::Undetermined;
        self.cast_id = None;
        self.trigger_value = None;
        if let Some(periodic) = &mut self.periodic {
            periodic.reset_state();
        }
    }

    // --- Periodic ---

    /// Starts or re-arms the tick chain after the marker buff was applied. Port of the
    /// `start_ticking` / `new_application_effect` / `refresh_effect` calls in
    /// `SpellPeriodic::spell_effect`.
    fn on_buff_applied(&mut self, application: BuffApplication, host: &mut impl SpellHost) {
        let Some(kind) = self.periodic_kind(host) else {
            return;
        };
        let periodic = self.periodic.as_mut().expect("kind implies periodic");
        match application {
            BuffApplication::Applied { .. } => {
                let id = self.id.unwrap_or_else(|| {
                    panic!("periodic spell {} has no id", self.setup.record.name)
                });
                periodic.start(id, host, &kind);
            }
            BuffApplication::Refreshed { .. } => periodic.refresh(&kind),
            BuffApplication::NotApplied => {}
        }
    }

    /// The periodic behaviour as currently defined by the buff's effect (re-read so talent rank
    /// values and modifiers apply), or by the bleed script.
    pub fn periodic_kind(&self, host: &impl SpellHost) -> Option<PeriodicKind> {
        let periodic = self.periodic.as_ref()?;
        if let Some(aura) = &self.setup.bleed_aura {
            let script = self
                .effects
                .iter()
                .find(|e| e.script_kind() == Some(ScriptKind::DeepWoundsBleed))?;
            let percent = self.trigger_value.unwrap_or_else(|| script.value());
            let duration = f64::from(aura.finite_duration_ms()?) / 1000.0;
            return Some(PeriodicKind::weapon_damage(percent, duration, periodic.tick_rate()).0);
        }
        let buff = host.buff(self.marker_buff?);
        let kinds = periodic.effect_indices().iter().filter_map(|&index| {
            let effect = buff.effects.get(index)?;
            PeriodicKind::from_effect(effect, buff.duration(), host).map(|(kind, _)| kind)
        });
        if periodic.effect_indices().len() == 1 {
            return kinds.into_iter().next();
        }
        let resource = host.resource_type();
        kinds.into_iter().find(|kind| {
            matches!(kind, PeriodicKind::ResourceGain { resource: gained, .. } if *gained == resource)
        })
    }

    /// Handles a `DotTick` event for this spell. Port of `SpellPeriodic::perform_periodic`.
    /// A `PERIODIC_TRIGGER_SPELL` tick casts its spell through the host.
    pub fn perform_periodic(
        &mut self,
        application_id: u32,
        host: &mut impl SpellHost,
    ) -> Option<TickReport> {
        if !self.enabled {
            return None;
        }
        let (id, marker) = (self.id?, self.marker_buff?);
        let kind = self.periodic_kind(host)?;
        let buff = host.buff(marker);
        let (active, expired_at) = (buff.is_active(), buff.expired_at());
        // The damage done modifiers apply once, to the tick: a bleed's base (Deep Wounds'
        // weapon damage) is taken before any of them.
        let damage_mod = self.periodic_damage_mod(host) * self.school_damage_mod(host);
        let cost = self.resource_cost(host);
        let report = self.periodic.as_mut()?.tick(
            application_id,
            id,
            host,
            &kind,
            active,
            expired_at,
            damage_mod,
            cost,
        )?;
        if let Some(trigger) = report.trigger {
            host.trigger_spell(trigger, None);
        }
        Some(report)
    }

    // --- Talent rank values ---

    /// Replaces the base points of effect `index` (direct or aura) with `value`: the talent's
    /// rank value from the curve tables, or a proc's `TRIGGER_WITH_VALUE`. An active buff is
    /// re-applied so the new value takes effect. Port of the `increase_talent_rank_effect`
    /// value substitution.
    pub fn set_effect_value(&mut self, host: &mut impl SpellHost, index: u32, value: f64) {
        if let Some(effect) = self.effects.iter_mut().find(|e| e.index() == index) {
            effect.set_value(value);
            return;
        }
        let Some(id) = self.marker_buff else {
            return;
        };
        let reapply = host.buff(id).is_active() && !self.is_periodic();
        if reapply {
            host.cancel_buff(id);
        }
        if let Some(effect) = host
            .buff_mut(id)
            .effects
            .iter_mut()
            .find(|e| e.index() == index)
        {
            effect.set_value(value);
        }
        if reapply {
            host.apply_buff(id);
        }
    }

    /// Restores the table values of every effect.
    pub fn reset_effect_values(&mut self, host: &mut impl SpellHost) {
        for effect in &mut self.effects {
            effect.reset_value();
        }
        let Some(id) = self.marker_buff else {
            return;
        };
        let reapply = host.buff(id).is_active() && !self.is_periodic();
        if reapply {
            host.cancel_buff(id);
        }
        for effect in &mut host.buff_mut(id).effects {
            effect.reset_value();
        }
        if reapply {
            host.apply_buff(id);
        }
    }
}

/// Spell power coefficient of a direct-damage spell from its cast time. Port of
/// `CastingTimeRequirer::spell_coefficient_from_casting_time` (with the `1500 / 3500` integer
/// division of the C++ evaluated as a real division).
pub fn spell_coefficient_from_casting_time(casting_time_ms: u32, level_req: u32) -> f64 {
    if casting_time_ms < 1500 {
        return 1500.0 / 3500.0;
    }
    if casting_time_ms > 3500 {
        return 1.0;
    }
    let base = f64::from(casting_time_ms) / 3500.0;
    if level_req >= 20 {
        return base;
    }
    (base - f64::from(20 - level_req) * 0.0375).max(0.0)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parity;
