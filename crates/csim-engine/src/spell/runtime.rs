//! The spell runtime. Port of `Spells/Spell.*` (the data-driven constructor path).
//!
//! A [`Spell`] is one rank of a [`SpellGroupSpec`] plus its runtime state: its effects, cooldown
//! handle, marker buff handle, talent-modified cost / cast time / crit bonus, and the result of
//! the last cast. Everything it needs from the character, engine, target and buffs goes through
//! the [`SpellHost`] trait (an extension of [`EffectHost`]) implemented by the spell context in
//! Phase 4; what the character has to do afterwards (statistics, proc checks) comes back in a
//! [`CastReport`].
//!
//! Spells with a cast time (port of `Spells/CastingTimeRequirer.*`) split [`Spell::perform`]
//! in two: it starts the cast (cooldown, GCD, `CastComplete` event tagged with a cast id) and
//! [`Spell::complete_cast`] runs the effects when the event arrives.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::buff::{Buff, BuffApplication};
use crate::combat_roll::PhysicalAttackResult;
use crate::cooldown::{add_gcd_event, CooldownControl};
use crate::effect::{ChainState, Effect, EffectHost};
use crate::engine::{Engine, EventKind};
use crate::ids::{BuffId, CharId, CooldownId, InstanceId, SpellId};
use crate::item::WeaponType;
use crate::mechanics::Mechanics;
use crate::phase::Phase;
use crate::proc::ProcSource;
use crate::resource::ResourceType;
use crate::spell::periodic::{Periodic, PeriodicKind, TickReport};
use crate::spell::{
    Comparison, EffectTarget, GcdBehavior, RestrictionSpec, SpellEffect, SpellFlag, SpellGroupSpec,
    SpellRankSpec, SpellResult, SpellStatus, TalentModification, TalentModificationSpec,
};
use crate::stance::Stance;
use crate::stats::MultiplicativeStack;

/// Tolerance when comparing the cooldown's ready time with the current time.
const COOLDOWN_EPSILON: f64 = 0.0001;

/// What a spell needs from the world beyond what its effects need. Port of the `Character`,
/// `CharacterSpells`, `Engine`, `EnabledBuffs` and `SimSettings` calls made by `Spell.cpp`.
pub trait SpellHost: EffectHost {
    fn character_id(&self) -> CharId;
    fn engine(&self) -> &Engine;
    fn engine_mut(&mut self) -> &mut Engine;
    fn clvl(&self) -> u32;
    fn phase(&self) -> Phase;
    /// Configured encounter length in seconds (execute range is derived from it).
    fn combat_length(&self) -> f64;

    fn on_global_cooldown(&self) -> bool;
    /// Length of the global cooldown in seconds.
    fn global_cooldown(&self) -> f64;
    fn start_global_cooldown(&mut self);
    fn on_stance_cooldown(&self) -> bool;
    fn start_stance_cooldown(&mut self);
    fn on_trinket_cooldown(&self) -> bool;
    fn cast_in_progress(&self) -> bool;
    /// Marks a cast as in progress and returns its id. Port of `CharacterSpells::start_cast`.
    fn start_cast(&mut self) -> u32;
    /// Ends the cast with `cast_id` (the character schedules its reaction). Port of
    /// `CharacterSpells::complete_cast`.
    fn complete_cast(&mut self, cast_id: u32);
    fn casting_speed_mod(&self) -> f64;
    /// Flat cast time reduction in milliseconds.
    fn casting_speed_flat_reduction(&self) -> u32;
    /// Whether an active buff makes suppressible casts instant.
    fn casting_time_suppressed(&self) -> bool;
    /// Pauses auto attacks (`STOPS_ATTACK_DURING_CAST`), cancelling any queued next-swing spell.
    fn stop_attack(&mut self);
    fn start_attack(&mut self);
    /// Restarts both swing timers (`RESETS_SWING_TIMERS`).
    fn reset_swing_timers(&mut self);
    /// Queues `spell` to replace the next mainhand swing (`ON_NEXT_SWING`); the character updates
    /// its white miss chance since a queued swing does not suffer the dual-wield penalty.
    fn queue_next_swing(&mut self, spell: SpellId);
    /// Clears the queued next-swing spell.
    fn cancel_next_swing(&mut self);
    fn stance(&self) -> Stance;
    fn offhand_weapon_type(&self) -> Option<WeaponType>;

    fn max_resource_level(&self, resource: ResourceType) -> u32;
    fn lose_resource(&mut self, resource: ResourceType, amount: u32);
    /// Flat cost reduction from skills (mana spells only in C++).
    fn resource_cost_reduction(&self, resource: ResourceType) -> u32;

    fn cooldown(&self, id: CooldownId) -> &CooldownControl;
    fn cooldown_mut(&mut self, id: CooldownId) -> &mut CooldownControl;

    fn buff(&self, id: BuffId) -> &Buff;
    fn buff_mut(&mut self, id: BuffId) -> &mut Buff;
    fn buff_is_active_by_name(&self, name: &str) -> bool;
    /// Applies the buff and, if it became active, its aura effects on every affected unit
    /// (cancelling any evicted debuff).
    fn apply_buff(&mut self, id: BuffId) -> BuffApplication;
    /// Cancels the buff, removing its aura effects if it was active.
    fn cancel_buff(&mut self, id: BuffId) -> bool;
    /// Enables the buff and registers it with the enabled buffs (assigning its instance id).
    fn enable_buff(&mut self, id: BuffId);
    /// Disables the buff and unregisters it.
    fn disable_buff(&mut self, id: BuffId);

    fn target_armor(&self) -> i32;
    fn total_physical_damage_mod(&self) -> f64;
    fn flat_physical_damage_bonus(&self) -> u32;
    fn melee_ability_crit_dmg_mod(&self) -> f64;
    fn total_threat_mod(&self) -> f64;
    /// Average mainhand damage including attack power (bleeds are based on it).
    fn avg_mh_damage(&self) -> f64;
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

/// What happened during [`Spell::perform`]. Port of the `StatisticsSpell` / `StatisticsResource`
/// updates and `proc_sources_to_attempt` of `Spell.cpp`, returned instead of pushed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CastReport {
    pub result: SpellResult,
    /// The cost charged for this cast (before the miss modifier).
    pub resource_cost: u32,
    /// The resource actually taken.
    pub resource_lost: u32,
    /// Attack outcome, if the spell rolled on the attack table.
    pub attack: Option<AttackOutcome>,
    /// Resources gained by the spell's effects.
    pub resource_gained: Vec<(ResourceType, u32)>,
    /// How the marker buff was applied, if the spell has one.
    pub buff: Option<BuffApplication>,
    /// Proc sources to run after the cast. Port of `proc_sources_to_attempt`.
    pub proc_sources: Vec<ProcSource>,
    /// The spell has a cast time and only started casting; the rest of the report is empty
    /// until [`Spell::complete_cast`].
    pub cast_started: bool,
    /// The spell replaces the next mainhand swing and was queued; the rest of the report is
    /// empty until [`Spell::perform_on_swing`].
    pub queued: bool,
}

/// One rank of a spell. Port of `Spell`.
#[derive(Debug, Clone)]
pub struct Spell {
    group: Arc<SpellGroupSpec>,
    rank_index: usize,
    cooldown: Option<CooldownId>,
    marker_buff: Option<BuffId>,
    effects: Vec<Effect>,
    talent_modifications: BTreeMap<String, Vec<TalentModificationSpec>>,
    instance_id: Option<InstanceId>,

    enabled: bool,
    resource_cost: u32,
    resource_cost_mod: MultiplicativeStack,
    cast_time_ms: u32,
    /// Crit chance added by talents, hundredths of a percent.
    crit_chance_bonus: u32,
    /// Proc rate set by a talent (`set_proc_rate`), read by the proc runtime.
    proc_rate: Option<f64>,
    overcap_resource_check: u32,
    /// Percent added to all damage by talents (`increase_damage_percent`).
    damage_percent: f64,
    last_result: SpellResult,
    /// The spell's handle in its character's spell list; needed for `DotTick` events.
    id: Option<SpellId>,
    /// Tick state of the marker buff's periodic aura, if it has one.
    periodic: Option<Periodic>,
    /// Id of the cast in progress. Port of `CastingTimeRequirer::cast_id`.
    cast_id: Option<u32>,
}

impl Spell {
    /// Builds the spell for `rank` of `group`. The cooldown handle is required for spells with a
    /// cooldown or a normal global cooldown; the marker buff handle for ranks with a buff.
    ///
    /// The spell starts disabled; the owner calls [`Spell::enable`] unless
    /// [`Spell::requires_talent`] is set (then the talent enables it). In C++ this happened in
    /// the constructor, which had the character at hand.
    ///
    /// # Panics
    /// Panics if the rank does not exist or a required handle is missing.
    pub fn new(
        group: Arc<SpellGroupSpec>,
        rank: u32,
        cooldown: Option<CooldownId>,
        marker_buff: Option<BuffId>,
    ) -> Self {
        let rank_index = group
            .ranks
            .iter()
            .position(|spec| spec.rank == rank)
            .unwrap_or_else(|| panic!("{} has no rank {rank}", group.name));
        let rank_spec = &group.ranks[rank_index];
        assert!(
            cooldown.is_some()
                || !(group.cooldown > 0.0 || group.causes_gcd == GcdBehavior::Normal),
            "{} rank {rank} requires a cooldown control but none was provided",
            group.name
        );
        assert!(
            marker_buff.is_some() || rank_spec.buff.is_none(),
            "{} rank {rank} has a buff but no marker buff was provided",
            group.name
        );

        let effects = rank_spec
            .effects
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, spec)| Effect::new(spec, index, &group.flags))
            .collect();
        let mut periodic = None;
        if let Some(buff) = &rank_spec.buff {
            for (index, spec) in buff.effects.iter().enumerate() {
                let effect = Effect::new(spec.clone(), index, &group.flags);
                if let Some((_, tick_rate)) = PeriodicKind::from_effect(&effect, buff.duration) {
                    assert!(
                        periodic.is_none(),
                        "{} rank {rank} has more than one periodic aura effect",
                        group.name
                    );
                    periodic = Some(Periodic::new(index, tick_rate));
                }
            }
        }
        let mut talent_modifications: BTreeMap<String, Vec<TalentModificationSpec>> =
            BTreeMap::new();
        for spec in &group.modified_by_talent {
            talent_modifications
                .entry(spec.talent.clone())
                .or_default()
                .push(spec.clone());
        }

        Spell {
            enabled: false,
            resource_cost: rank_spec.cost,
            resource_cost_mod: MultiplicativeStack::default(),
            cast_time_ms: rank_spec.cast_time_ms,
            crit_chance_bonus: 0,
            proc_rate: None,
            overcap_resource_check: 0,
            damage_percent: 0.0,
            last_result: SpellResult::Undetermined,
            id: None,
            periodic,
            cast_id: None,
            effects,
            talent_modifications,
            instance_id: None,
            group,
            rank_index,
            cooldown,
            marker_buff,
        }
    }

    // --- Static properties ---

    pub fn name(&self) -> &str {
        &self.group.name
    }

    pub fn icon(&self) -> Option<&str> {
        self.group.icon.as_deref()
    }

    pub fn group(&self) -> &Arc<SpellGroupSpec> {
        &self.group
    }

    pub fn rank_spec(&self) -> &SpellRankSpec {
        &self.group.ranks[self.rank_index]
    }

    pub fn rank(&self) -> u32 {
        self.rank_spec().rank
    }

    pub fn resource_type(&self) -> ResourceType {
        self.rank_spec().resource
    }

    pub fn has_flag(&self, flag: SpellFlag) -> bool {
        self.group.has_flag(flag)
    }

    /// The talent that must have at least one rank for the spell to be enabled.
    pub fn requires_talent(&self) -> Option<&str> {
        self.group.requires_talent.as_deref()
    }

    pub fn is_passive(&self) -> bool {
        self.has_flag(SpellFlag::Passive)
    }

    pub fn gcd_behavior(&self) -> GcdBehavior {
        self.group.causes_gcd
    }

    pub fn restricted_by_gcd(&self) -> bool {
        self.group.restricted_by_gcd
    }

    pub fn base_cooldown(&self) -> f64 {
        self.group.cooldown
    }

    pub fn cooldown_id(&self) -> Option<CooldownId> {
        self.cooldown
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

    pub fn cast_time_ms(&self) -> u32 {
        self.cast_time_ms
    }

    /// Whether the spell has a cast time (as opposed to being instant).
    pub fn has_cast_time(&self) -> bool {
        self.cast_time_ms > 0
    }

    /// Whether this spell's cast is in progress.
    pub fn is_casting(&self) -> bool {
        self.cast_id.is_some()
    }

    /// Whether the spell replaces the next mainhand swing (`ON_NEXT_SWING`, Heroic Strike).
    pub fn is_on_next_swing(&self) -> bool {
        self.has_flag(SpellFlag::OnNextSwing)
    }

    /// Whether an on-next-swing spell is queued: its marker buff is active.
    pub fn is_queued(&self, host: &impl SpellHost) -> bool {
        self.is_on_next_swing() && self.marker_buff.is_some_and(|id| host.buff(id).is_active())
    }

    /// The cast time in seconds after haste and flat reductions. Port of
    /// `CastingTimeRequirer::get_cast_time`.
    pub fn cast_time(&self, host: &impl SpellHost) -> f64 {
        let flat_reduction = f64::from(host.casting_speed_flat_reduction()) / 1000.0;
        let after_mod = f64::from(self.cast_time_ms) / 1000.0 / host.casting_speed_mod();
        (after_mod - flat_reduction).max(0.0)
    }

    pub fn crit_chance_bonus(&self) -> u32 {
        self.crit_chance_bonus
    }

    pub fn proc_rate(&self) -> Option<f64> {
        self.proc_rate
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

    /// The periodic aura state, for spells whose marker buff ticks.
    pub fn periodic(&self) -> Option<&Periodic> {
        self.periodic.as_ref()
    }

    pub fn is_periodic(&self) -> bool {
        self.periodic.is_some()
    }

    /// Multiplier on all damage from `increase_damage_percent` talents.
    pub fn damage_mod(&self) -> f64 {
        1.0 + self.damage_percent / 100.0
    }

    pub fn instance_id(&self) -> Option<InstanceId> {
        self.instance_id
    }

    pub fn set_instance_id(&mut self, id: InstanceId) {
        self.instance_id = Some(id);
    }

    /// Names of the talents that modify this spell.
    pub fn modifying_talents(&self) -> impl Iterator<Item = &str> {
        self.talent_modifications.keys().map(String::as_str)
    }

    /// Whether the character has learned this rank (level and phase requirements).
    pub fn is_rank_learned(&self, host: &impl SpellHost) -> bool {
        let spec = self.rank_spec();
        spec.level_req.is_none_or(|level| host.clvl() >= level)
            && spec.requires_phase.available_in(host.phase())
    }

    // --- Cooldown / cost ---

    pub fn last_used(&self, host: &impl SpellHost) -> f64 {
        self.cooldown
            .map_or(f64::NEG_INFINITY, |id| host.cooldown(id).last_used)
    }

    pub fn next_use(&self, host: &impl SpellHost) -> f64 {
        self.cooldown
            .map_or(f64::NEG_INFINITY, |id| host.cooldown(id).next_use())
    }

    pub fn cooldown_remaining(&self, host: &impl SpellHost) -> f64 {
        self.cooldown.map_or(0.0, |id| {
            host.cooldown(id).remaining(host.engine().current_time())
        })
    }

    /// Base cost after talent reductions, before the multiplicative modifier.
    pub fn base_resource_cost(&self) -> u32 {
        self.resource_cost
    }

    /// The cost of a cast. Port of `Spell::get_resource_cost`.
    pub fn resource_cost(&self, host: &impl SpellHost) -> u32 {
        let cost = (f64::from(self.resource_cost) * self.resource_cost_mod.modifier()).round();
        let cost = cost as u32;
        cost.saturating_sub(host.resource_cost_reduction(self.resource_type()))
    }

    pub fn increase_resource_cost_modifier(&mut self, percent: i32) {
        self.resource_cost_mod.add(percent);
    }

    pub fn decrease_resource_cost_modifier(&mut self, percent: i32) {
        self.resource_cost_mod.remove(percent);
    }

    // --- Enable / disable ---

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

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
    }

    pub fn disable(&mut self, host: &mut impl SpellHost) {
        if let Some(id) = self.marker_buff {
            let buff = host.buff(id);
            if buff.is_enabled() && !buff.kind().is_raid_shared() {
                host.disable_buff(id);
            }
        }
        self.enabled = false;
    }

    // --- Status ---

    /// Whether the spell can be cast now, and if not why. Port of `Spell::get_spell_status`.
    pub fn status(&self, host: &impl SpellHost) -> SpellStatus {
        if !self.enabled {
            return SpellStatus::NotEnabled;
        }
        if self.group.restricted_by_gcd && host.on_global_cooldown() {
            return SpellStatus::OnGcd;
        }
        if host.cast_in_progress() {
            return SpellStatus::CastInProgress;
        }
        let now = host.engine().current_time();
        if self.next_use(host) - now > COOLDOWN_EPSILON {
            return SpellStatus::OnCooldown;
        }
        let resource = self.resource_type();
        if host.resource_level(resource) < self.resource_cost(host) {
            return SpellStatus::InsufficientResources;
        }

        // Both normal and stance GCD spells wait for the stance cooldown (C++ fall-through).
        if self.group.causes_gcd != GcdBehavior::None && host.on_stance_cooldown() {
            return SpellStatus::OnStanceCooldown;
        }

        for restriction in &self.group.restrictions {
            let status = match restriction {
                RestrictionSpec::ComboPoints { value, cmp } => {
                    (!compare(f64::from(host.combo_points()), *cmp, f64::from(*value)))
                        .then_some(SpellStatus::InsufficientComboPoints)
                }
                RestrictionSpec::TargetHealthPercentage { value, cmp } => {
                    let combat_length = host.combat_length();
                    let time_remaining = combat_length - now;
                    (!compare(time_remaining / combat_length, *cmp, *value))
                        .then_some(SpellStatus::NotInExecuteRange)
                }
                RestrictionSpec::UnitStance { stance, cmp } => {
                    let current = host.stance();
                    let ok = match cmp {
                        Comparison::Eq => current == *stance,
                        _ => current != *stance,
                    };
                    (!ok).then_some(SpellStatus::in_stance(current))
                }
                RestrictionSpec::BuffActive { buff } => {
                    (!host.buff_is_active_by_name(buff)).then_some(SpellStatus::BuffInactive)
                }
                RestrictionSpec::OffhandWeaponType { weapon_type, cmp } => {
                    let current = host.offhand_weapon_type();
                    let ok = match cmp {
                        Comparison::Eq => current == Some(*weapon_type),
                        _ => current != Some(*weapon_type),
                    };
                    (!ok).then_some(SpellStatus::IncorrectWeaponType)
                }
            };
            if let Some(status) = status {
                return status;
            }
        }

        if self.overcap_resource_check > 0
            && host.max_resource_level(resource) - host.resource_level(resource)
                < self.overcap_resource_check
        {
            return SpellStatus::OvercapResource;
        }

        SpellStatus::Available
    }

    // --- Casting ---

    /// Casts the spell. Port of `Spell::perform` + `Spell::spell_effect`.
    ///
    /// # Panics
    /// Panics if the character cannot afford the spell (callers check [`Spell::status`] first).
    pub fn perform(&mut self, host: &mut impl SpellHost) -> CastReport {
        let resource = self.resource_type();
        let cost = self.resource_cost(host);
        assert!(
            host.resource_level(resource) >= cost,
            "Tried to perform '{}' but has insufficient resource {} (requires {cost})",
            self.name(),
            host.resource_level(resource)
        );

        let now = host.engine().current_time();
        if let Some(id) = self.cooldown {
            host.cooldown_mut(id).start(now);
        }
        self.last_result = SpellResult::Undetermined;
        let report = CastReport {
            resource_cost: cost,
            ..CastReport::default()
        };

        // Cooldown and global cooldown events.
        if let Some(id) = self.cooldown.filter(|_| self.group.cooldown > 0.0) {
            let character = host.character_id();
            let control = host.cooldown(id).clone();
            control.add_spell_cd_event(host.engine_mut(), character);
        }
        match self.group.causes_gcd {
            GcdBehavior::None => {}
            GcdBehavior::Normal => {
                let (character, gcd) = (host.character_id(), host.global_cooldown());
                if add_gcd_event(host.engine_mut(), character, gcd) {
                    host.start_global_cooldown();
                }
            }
            GcdBehavior::Stance => {
                assert!(
                    !host.on_stance_cooldown(),
                    "Spell {} already on stance cooldown when starting another",
                    self.name()
                );
                host.start_stance_cooldown();
            }
        }

        if self.has_cast_time() {
            return self.start_cast(host, report);
        }
        if self.is_on_next_swing() {
            return self.queue(host, report);
        }
        self.execute(host, report)
    }

    /// Queues the spell for the next mainhand swing: its marker buff marks it as queued. Port
    /// of `HeroicStrike::spell_effect`.
    fn queue(&mut self, host: &mut impl SpellHost, mut report: CastReport) -> CastReport {
        let id = self
            .id
            .unwrap_or_else(|| panic!("on-next-swing spell {} has no id", self.group.name));
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

    /// Starts casting: `CastComplete` is scheduled after the cast time unless the cast is
    /// suppressed, in which case it completes at once. Port of `CastingTimeRequirer::start_cast`
    /// plus the `Slam::spell_effect` attack handling.
    fn start_cast(&mut self, host: &mut impl SpellHost, mut report: CastReport) -> CastReport {
        let cast_id = host.start_cast();
        self.cast_id = Some(cast_id);
        if self.has_flag(SpellFlag::StopsAttackDuringCast) {
            host.stop_attack();
        }
        if self.group.suppressible_cast && host.casting_time_suppressed() {
            return self
                .complete_cast(cast_id, host)
                .expect("a cast just started completes");
        }
        let id = self
            .id
            .unwrap_or_else(|| panic!("cast-time spell {} has no id", self.group.name));
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
        if self.has_flag(SpellFlag::ResetsSwingTimers) {
            host.reset_swing_timers();
        }
        if self.has_flag(SpellFlag::StopsAttackDuringCast) {
            host.start_attack();
        }
        let report = CastReport {
            resource_cost: self.resource_cost(host),
            ..CastReport::default()
        };
        Some(self.execute(host, report))
    }

    /// Runs the effect chain, applies the buff, pays the cost and collects the damage. The
    /// second half of `Spell::spell_effect`.
    fn execute(&mut self, host: &mut impl SpellHost, mut report: CastReport) -> CastReport {
        let resource = self.resource_type();
        let cost = report.resource_cost;

        // The effect chain.
        let mut first_roll = None;
        if self.effects.is_empty() {
            // No effect that must succeed (e.g. a spell hit): apply the buff immediately.
            self.last_result = SpellResult::Success;
        } else {
            let mut chain = ChainState {
                result: SpellResult::Undetermined,
                previous: None,
                resource_cost: cost,
                extra_crit: self.crit_chance_bonus,
            };
            // The effects are taken out so the host can be borrowed mutably alongside them.
            let mut effects = std::mem::take(&mut self.effects);
            for (index, effect) in effects.iter_mut().enumerate() {
                let outcome = effect.perform(host, &chain);
                if index == 0 {
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
            // Misses cost the full amount; dodges and parries only a fraction (the C++ TODO in
            // `Spell::spell_effect`, resolved). Mana is always lost in full.
            let lost = if resource == ResourceType::Mana
                || first_roll == Some(PhysicalAttackResult::Miss)
            {
                cost
            } else {
                (f64::from(cost) * self.group.resource_miss_cost_mod).round() as u32
            };
            host.lose_resource(resource, lost);
            report.resource_lost = lost;
            if let Some(result) = first_roll {
                report.attack = Some(AttackOutcome {
                    result,
                    damage: 0,
                    threat: 0.0,
                    execution_time: self.execution_time(host),
                });
            }
        } else {
            host.lose_resource(resource, cost);
            report.resource_lost = cost;
            report.attack = self.collect_damage(host, first_roll, &mut report.proc_sources);
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
        proc_sources: &mut Vec<ProcSource>,
    ) -> Option<AttackOutcome> {
        let mut raw_damage = 0.0;
        let mut innate_threat = 0;
        for effect in &mut self.effects {
            raw_damage += effect.damage_dealt;
            effect.damage_dealt = 0.0;
            if effect.was_successful() {
                innate_threat += effect.spec.innate_threat;
            }
        }
        raw_damage *= self.damage_mod();
        let result = first_roll?;

        let damage = match result {
            PhysicalAttackResult::Critical | PhysicalAttackResult::BlockCritical => {
                proc_sources.push(ProcSource::MeleeCritical);
                self.damage_after_modifiers(host, raw_damage) * host.melee_ability_crit_dmg_mod()
            }
            PhysicalAttackResult::Hit | PhysicalAttackResult::Block => {
                proc_sources.push(ProcSource::MeleeHit);
                self.damage_after_modifiers(host, raw_damage)
            }
            PhysicalAttackResult::Glancing => {
                proc_sources.push(ProcSource::MeleeHit);
                self.damage_after_modifiers(host, raw_damage)
            }
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
        };
        let damage = damage.round().max(0.0) as u32;
        let threat = f64::from(damage + innate_threat) * host.total_threat_mod();
        Some(AttackOutcome {
            result,
            damage,
            threat,
            execution_time: self.execution_time(host),
        })
    }

    /// Port of `Spell::damage_after_modifiers`.
    pub fn damage_after_modifiers(&self, host: &impl SpellHost, damage: f64) -> f64 {
        let armor_reduction =
            1.0 - Mechanics::reduction_from_armor(host.target_armor(), host.clvl());
        (damage * host.total_physical_damage_mod() + f64::from(host.flat_physical_damage_bonus()))
            * armor_reduction
    }

    fn execution_time(&self, host: &impl SpellHost) -> f64 {
        if self.has_cast_time() {
            return f64::from(self.cast_time_ms) / 1000.0;
        }
        match self.group.causes_gcd {
            GcdBehavior::Normal => host.global_cooldown(),
            _ => 0.0,
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
        self.last_result = SpellResult::Undetermined;
        self.cast_id = None;
        if let Some(periodic) = &mut self.periodic {
            periodic.reset_state();
        }
    }

    // --- Periodic ---

    /// Starts or re-arms the tick chain after the marker buff was applied. Port of the
    /// `start_ticking` / `new_application_effect` / `refresh_effect` calls in
    /// `SpellPeriodic::spell_effect`.
    fn on_buff_applied(&mut self, application: BuffApplication, host: &mut impl SpellHost) {
        let damage_mod = self.damage_mod();
        let Some(kind) = self.periodic_kind(host) else {
            return;
        };
        let periodic = self.periodic.as_mut().expect("kind implies periodic");
        match application {
            BuffApplication::Applied { .. } => {
                let id = self
                    .id
                    .unwrap_or_else(|| panic!("periodic spell {} has no id", self.group.name));
                periodic.start(id, host, &kind, damage_mod);
            }
            BuffApplication::Refreshed { .. } => periodic.refresh(host, &kind, damage_mod),
            BuffApplication::NotApplied => {}
        }
    }

    /// The periodic behaviour as currently defined by the marker buff's effect (re-read so
    /// talent modifications of the effect apply).
    pub fn periodic_kind(&self, host: &impl SpellHost) -> Option<PeriodicKind> {
        let periodic = self.periodic.as_ref()?;
        let buff = host.buff(self.marker_buff?);
        let effect = buff.effects.get(periodic.effect_index())?;
        PeriodicKind::from_effect(effect, buff.duration()).map(|(kind, _)| kind)
    }

    /// Handles a `DotTick` event for this spell. Port of `SpellPeriodic::perform_periodic`.
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
        let (damage_mod, cost) = (self.damage_mod(), self.resource_cost);
        self.periodic.as_mut()?.tick(
            application_id,
            id,
            host,
            &kind,
            active,
            expired_at,
            damage_mod,
            cost,
        )
    }

    // --- Talents ---

    /// The talent went from `current - 1` to `current` ranks. Port of
    /// `Spell::increase_talent_rank_effect`.
    pub fn increase_talent_rank(&mut self, host: &mut impl SpellHost, talent: &str, current: u32) {
        let reapply = self.cancel_marker_buff_for_modification(host);
        if current > 1 {
            self.remove_talent_rank(host, talent, current - 1);
        }
        self.apply_talent_rank(host, talent, current);
        if reapply {
            self.reapply_marker_buff(host);
        }
    }

    /// The talent went from `current + 1` to `current` ranks. Port of
    /// `Spell::decrease_talent_rank_effect`.
    pub fn decrease_talent_rank(&mut self, host: &mut impl SpellHost, talent: &str, current: u32) {
        let reapply = self.cancel_marker_buff_for_modification(host);
        self.remove_talent_rank(host, talent, current + 1);
        if current > 0 {
            self.apply_talent_rank(host, talent, current);
        }
        if reapply {
            self.reapply_marker_buff(host);
        }
    }

    fn cancel_marker_buff_for_modification(&mut self, host: &mut impl SpellHost) -> bool {
        match self.marker_buff {
            Some(id) if host.buff(id).is_active() => {
                host.cancel_buff(id);
                true
            }
            _ => false,
        }
    }

    fn reapply_marker_buff(&mut self, host: &mut impl SpellHost) {
        if let Some(id) = self.marker_buff {
            host.apply_buff(id);
        }
    }

    fn talent_rank_spec(&self, talent: &str, rank: u32) -> Option<TalentModificationSpec> {
        self.talent_modifications
            .get(talent)?
            .iter()
            .find(|spec| spec.rank == rank)
            .cloned()
    }

    fn apply_talent_rank(&mut self, host: &mut impl SpellHost, talent: &str, rank: u32) {
        let Some(spec) = self.talent_rank_spec(talent, rank) else {
            return;
        };
        match &spec.modification {
            TalentModification::AbsoluteResourceCostReduction { value } => {
                self.resource_cost -= value;
            }
            TalentModification::IncreaseValue {
                target,
                effect,
                value,
            } => {
                if let Some(found) = self.find_effect(host, *target, *effect) {
                    found.spec.value += value;
                }
            }
            TalentModification::IncreaseValuePercent {
                target,
                effect,
                value,
            } => {
                if let Some(found) = self.find_effect(host, *target, *effect) {
                    found.spec.value += found.base_value * value / 100.0;
                }
            }
            TalentModification::AddEffect { target, effect } => {
                let flags = self.group.flags.clone();
                let effects = self.effect_list(host, *target);
                let index = effects.len();
                effects.push(Effect::from_talent(effect.clone(), index, &flags, talent));
            }
            TalentModification::IncreaseBuffDurationPercent { value } => {
                if let Some(id) = self.marker_buff {
                    host.buff_mut(id)
                        .increase_duration_percent(value.round() as i32);
                }
            }
            TalentModification::CastTimeReductionMs { value } => self.cast_time_ms -= value,
            TalentModification::IncreaseCritChance { value } => self.crit_chance_bonus += value,
            TalentModification::SetProcRate { value } => self.proc_rate = Some(*value),
            TalentModification::IncreaseDamagePercent { value } => self.damage_percent += value,
        }
    }

    fn remove_talent_rank(&mut self, host: &mut impl SpellHost, talent: &str, rank: u32) {
        let Some(spec) = self.talent_rank_spec(talent, rank) else {
            return;
        };
        match &spec.modification {
            TalentModification::AbsoluteResourceCostReduction { value } => {
                self.resource_cost += value;
            }
            TalentModification::IncreaseValue {
                target,
                effect,
                value,
            } => {
                if let Some(found) = self.find_effect(host, *target, *effect) {
                    found.spec.value -= value;
                }
            }
            TalentModification::IncreaseValuePercent {
                target,
                effect,
                value,
            } => {
                if let Some(found) = self.find_effect(host, *target, *effect) {
                    found.spec.value -= found.base_value * value / 100.0;
                }
            }
            TalentModification::AddEffect { target, .. } => {
                self.effect_list(host, *target)
                    .retain(|effect| effect.talent_as_source.as_deref() != Some(talent));
            }
            TalentModification::IncreaseBuffDurationPercent { value } => {
                if let Some(id) = self.marker_buff {
                    host.buff_mut(id)
                        .decrease_duration_percent(value.round() as i32);
                }
            }
            TalentModification::CastTimeReductionMs { value } => self.cast_time_ms += value,
            TalentModification::IncreaseCritChance { value } => self.crit_chance_bonus -= value,
            TalentModification::SetProcRate { .. } => self.proc_rate = None,
            TalentModification::IncreaseDamagePercent { value } => self.damage_percent -= value,
        }
    }

    fn effect_list<'a>(
        &'a mut self,
        host: &'a mut impl SpellHost,
        target: EffectTarget,
    ) -> &'a mut Vec<Effect> {
        match target {
            EffectTarget::Spell => &mut self.effects,
            EffectTarget::Buff => {
                let id = self
                    .marker_buff
                    .expect("talent targets the buff of a spell without one");
                &mut host.buff_mut(id).effects
            }
        }
    }

    /// The first effect of `kind` in the targeted list. Port of `Spell::find_effect` (which,
    /// as noted there, does not support several effects of the same kind).
    fn find_effect<'a>(
        &'a mut self,
        host: &'a mut impl SpellHost,
        target: EffectTarget,
        kind: SpellEffect,
    ) -> Option<&'a mut Effect> {
        self.effect_list(host, target)
            .iter_mut()
            .find(|effect| effect.kind() == kind)
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

fn compare(lhs: f64, cmp: Comparison, rhs: f64) -> bool {
    match cmp {
        Comparison::Eq => lhs == rhs,
        Comparison::Neq => lhs != rhs,
        Comparison::Less => lhs < rhs,
        Comparison::Leq => lhs <= rhs,
        Comparison::Greater => lhs > rhs,
        Comparison::Geq => lhs >= rhs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buff::{BuffContext, BuffKind, ChargeUse};
    use crate::combat_roll::IncludedOutcomes;
    use crate::cooldown::CooldownRegistry;
    use crate::engine::EventKind;
    use crate::spell::{SpellDb, SpellFileSpec};
    use crate::stats::CharacterStats;
    use crate::target::Target;
    use std::collections::VecDeque;

    /// A one-character world: what the Phase 4 spell context will be.
    struct World {
        engine: Engine,
        target: Target,
        stats: CharacterStats,
        cooldowns: CooldownRegistry,
        buffs: Vec<Buff>,
        rage: u32,
        combo_points: u32,
        stance: Stance,
        next_gcd: f64,
        next_stance_cd: f64,
        casting: bool,
        rolls: VecDeque<PhysicalAttackResult>,
        extra_crits: Vec<u32>,
        can_crits: Vec<bool>,
        combat_length: f64,
        armor: i32,
        aura_log: Vec<String>,
        next_spell_id: u32,
        ticks: Vec<TickReport>,
        cast_in_progress: bool,
        cast_id: u32,
        casting_speed_mod: f64,
        casting_time_suppressed: bool,
        attack_log: Vec<&'static str>,
        completed_casts: Vec<CastReport>,
        queued: Option<SpellId>,
    }

    impl World {
        fn new() -> Self {
            let mut engine = Engine::new();
            engine.prepare_iteration(0.0);
            World {
                engine,
                target: Target::new(63),
                stats: CharacterStats::new(),
                cooldowns: CooldownRegistry::new(),
                buffs: Vec::new(),
                rage: 100,
                combo_points: 0,
                stance: Stance::Battle,
                next_gcd: 0.0,
                next_stance_cd: 0.0,
                casting: false,
                rolls: VecDeque::new(),
                extra_crits: Vec::new(),
                can_crits: Vec::new(),
                combat_length: 300.0,
                armor: 0,
                aura_log: Vec::new(),
                next_spell_id: 0,
                ticks: Vec::new(),
                cast_in_progress: false,
                cast_id: 0,
                casting_speed_mod: 1.0,
                casting_time_suppressed: false,
                attack_log: Vec::new(),
                completed_casts: Vec::new(),
                queued: None,
            }
        }

        fn advance_to(&mut self, time: f64) {
            self.engine.add_event(crate::engine::Event::new(
                time,
                EventKind::EncounterStart {
                    character: CharId(0),
                },
            ));
            while let Some(event) = self.engine.next_event() {
                if event.time >= time {
                    break;
                }
            }
        }

        fn add_buff(&mut self, mut buff: Buff) -> BuffId {
            let id = BuffId(self.buffs.len() as u32);
            if buff.kind().is_raid_shared() {
                buff.set_instance_id(InstanceId(id.0 + 100));
                buff.enable();
            }
            self.buffs.push(buff);
            id
        }

        fn buff_ctx(&mut self, id: BuffId) -> (&mut Buff, BuffContext<'_>) {
            (
                &mut self.buffs[id.index()],
                BuffContext {
                    engine: &mut self.engine,
                    target: &mut self.target,
                    character: CharId(0),
                    buff: id,
                },
            )
        }

        fn spell(&mut self, group: &Arc<SpellGroupSpec>, rank: u32) -> Spell {
            let cooldown = Some(self.cooldowns.new_cooldown_for_group(group));
            let rank_spec = group.rank(rank).unwrap();
            let marker = rank_spec.buff.as_ref().map(|spec| {
                let kind = BuffKind::from_spec(spec, 0).unwrap();
                self.add_buff(Buff::from_spec(group, rank_spec, kind))
            });
            let mut spell = Spell::new(Arc::clone(group), rank, cooldown, marker);
            spell.set_id(SpellId(self.next_spell_id));
            self.next_spell_id += 1;
            if spell.requires_talent().is_none() {
                spell.enable(self);
            }
            spell
        }

        /// Dispatches events up to and including `until` for one spell: buff removals of its
        /// marker buff and its dot ticks.
        fn run(&mut self, spell: &mut Spell, until: f64) {
            self.engine
                .add_event(crate::engine::Event::new(until, EventKind::EncounterEnd));
            while let Some(event) = self.engine.next_event() {
                match event.kind {
                    EventKind::EncounterEnd => break,
                    EventKind::BuffRemoval {
                        buff, iteration, ..
                    } => {
                        let (b, mut ctx) = self.buff_ctx(buff);
                        if b.remove(iteration, &mut ctx) {
                            self.remove_auras(buff);
                        }
                    }
                    EventKind::DotTick {
                        spell: id,
                        application_id,
                        ..
                    } => {
                        assert_eq!(Some(id), spell.id());
                        if let Some(tick) = spell.perform_periodic(application_id, self) {
                            self.ticks.push(tick);
                        }
                    }
                    EventKind::CastComplete {
                        spell: id, cast_id, ..
                    } => {
                        assert_eq!(Some(id), spell.id());
                        if let Some(report) = spell.complete_cast(cast_id, self) {
                            self.completed_casts.push(report);
                        }
                    }
                    _ => {}
                }
            }
        }

        fn pending(&self) -> Vec<(f64, EventKind)> {
            let mut queue = self.engine.queue().clone();
            let mut events = Vec::new();
            while let Some(event) = queue.pop() {
                events.push((event.time, event.kind));
            }
            events
        }
    }

    impl EffectHost for World {
        fn combo_points(&self) -> u32 {
            self.combo_points
        }
        fn gain_combo_points(&mut self, amount: u32) {
            self.combo_points = (self.combo_points + amount).min(5);
        }
        fn spend_combo_points(&mut self) {
            self.combo_points = 0;
        }
        fn resource_level(&self, _: ResourceType) -> u32 {
            self.rage
        }
        fn gain_resource(&mut self, _: ResourceType, amount: u32) -> u32 {
            let before = self.rage;
            self.rage = (self.rage + amount).min(100);
            self.rage - before
        }
        fn melee_ap(&self) -> u32 {
            1000
        }
        fn random_in_range(&mut self, min: f64, max: f64) -> f64 {
            (min + max) / 2.0
        }
        fn random_normalized_mh_dmg(&mut self) -> f64 {
            300.0
        }
        fn random_non_normalized_mh_dmg(&mut self) -> f64 {
            400.0
        }
        fn roll_melee_ability(
            &mut self,
            _: IncludedOutcomes,
            extra_crit: u32,
            can_crit: bool,
        ) -> PhysicalAttackResult {
            self.extra_crits.push(extra_crit);
            self.can_crits.push(can_crit);
            self.rolls.pop_front().expect("no roll queued")
        }
        fn stats_mut(&mut self) -> &mut CharacterStats {
            &mut self.stats
        }
        fn target_mut(&mut self) -> &mut Target {
            &mut self.target
        }
        fn increase_melee_attack_speed(&mut self, percent: u32) {
            self.stats.increase_melee_attack_speed(percent);
        }
        fn decrease_melee_attack_speed(&mut self, percent: u32) {
            self.stats.decrease_melee_attack_speed(percent);
        }
        fn swap_stance(&mut self, stance: Stance) {
            self.stance = stance;
        }
        fn use_buff_charge(&mut self, name: &str) {
            let id = BuffId(
                self.buffs
                    .iter()
                    .position(|buff| buff.name() == name)
                    .expect("unknown buff") as u32,
            );
            let (buff, mut ctx) = self.buff_ctx(id);
            if buff.use_charge(&mut ctx) == ChargeUse::Removed {
                self.remove_auras(id);
            }
        }
    }

    impl World {
        fn apply_auras(&mut self, id: BuffId) {
            let effects = self.buffs[id.index()].effects.clone();
            for effect in &effects {
                self.aura_log.push(format!("+{:?}", effect.kind()));
                effect.apply_aura(self);
            }
        }

        fn remove_auras(&mut self, id: BuffId) {
            let effects = self.buffs[id.index()].effects.clone();
            for effect in &effects {
                self.aura_log.push(format!("-{:?}", effect.kind()));
                effect.remove_aura(self);
            }
        }
    }

    impl SpellHost for World {
        fn character_id(&self) -> CharId {
            CharId(0)
        }
        fn engine(&self) -> &Engine {
            &self.engine
        }
        fn engine_mut(&mut self) -> &mut Engine {
            &mut self.engine
        }
        fn clvl(&self) -> u32 {
            60
        }
        fn phase(&self) -> Phase {
            Phase::AhnQiraj
        }
        fn combat_length(&self) -> f64 {
            self.combat_length
        }
        fn on_global_cooldown(&self) -> bool {
            self.engine.current_time() < self.next_gcd
        }
        fn global_cooldown(&self) -> f64 {
            1.5
        }
        fn start_global_cooldown(&mut self) {
            self.next_gcd = self.engine.current_time() + 1.5;
        }
        fn on_stance_cooldown(&self) -> bool {
            self.engine.current_time() < self.next_stance_cd
        }
        fn start_stance_cooldown(&mut self) {
            self.next_stance_cd = self.engine.current_time() + 1.0;
        }
        fn on_trinket_cooldown(&self) -> bool {
            false
        }
        fn cast_in_progress(&self) -> bool {
            self.casting || self.cast_in_progress
        }
        fn start_cast(&mut self) -> u32 {
            assert!(!self.cast_in_progress, "cast in progress");
            self.cast_in_progress = true;
            self.cast_id += 1;
            self.cast_id
        }
        fn complete_cast(&mut self, cast_id: u32) {
            assert!(self.cast_in_progress, "no cast in progress");
            assert_eq!(cast_id, self.cast_id, "mismatched cast id");
            self.cast_in_progress = false;
        }
        fn casting_speed_mod(&self) -> f64 {
            self.casting_speed_mod
        }
        fn casting_speed_flat_reduction(&self) -> u32 {
            0
        }
        fn casting_time_suppressed(&self) -> bool {
            self.casting_time_suppressed
        }
        fn stop_attack(&mut self) {
            self.attack_log.push("stop");
        }
        fn start_attack(&mut self) {
            self.attack_log.push("start");
        }
        fn reset_swing_timers(&mut self) {
            self.attack_log.push("reset");
        }
        fn queue_next_swing(&mut self, spell: SpellId) {
            self.attack_log.push("queue");
            self.queued = Some(spell);
        }
        fn cancel_next_swing(&mut self) {
            if self.queued.take().is_some() {
                self.attack_log.push("unqueue");
            }
        }
        fn stance(&self) -> Stance {
            self.stance
        }
        fn offhand_weapon_type(&self) -> Option<WeaponType> {
            Some(WeaponType::Shield)
        }
        fn max_resource_level(&self, _: ResourceType) -> u32 {
            100
        }
        fn lose_resource(&mut self, _: ResourceType, amount: u32) {
            self.rage -= amount;
        }
        fn resource_cost_reduction(&self, _: ResourceType) -> u32 {
            0
        }
        fn cooldown(&self, id: CooldownId) -> &CooldownControl {
            self.cooldowns.get(id)
        }
        fn cooldown_mut(&mut self, id: CooldownId) -> &mut CooldownControl {
            self.cooldowns.get_mut(id)
        }
        fn buff(&self, id: BuffId) -> &Buff {
            &self.buffs[id.index()]
        }
        fn buff_mut(&mut self, id: BuffId) -> &mut Buff {
            &mut self.buffs[id.index()]
        }
        fn buff_is_active_by_name(&self, name: &str) -> bool {
            self.buffs
                .iter()
                .any(|buff| buff.name() == name && buff.is_active())
        }
        fn apply_buff(&mut self, id: BuffId) -> BuffApplication {
            let (buff, mut ctx) = self.buff_ctx(id);
            let application = buff.apply(&mut ctx);
            if let BuffApplication::Applied { .. } = application {
                self.apply_auras(id);
            }
            application
        }
        fn cancel_buff(&mut self, id: BuffId) -> bool {
            let (buff, mut ctx) = self.buff_ctx(id);
            let cancelled = buff.cancel(&mut ctx);
            if cancelled {
                self.remove_auras(id);
            }
            cancelled
        }
        fn enable_buff(&mut self, id: BuffId) {
            let buff = &mut self.buffs[id.index()];
            buff.enable();
            buff.set_instance_id(InstanceId(id.0 + 1));
        }
        fn disable_buff(&mut self, id: BuffId) {
            self.buffs[id.index()].disable();
        }
        fn target_armor(&self) -> i32 {
            self.armor
        }
        fn total_physical_damage_mod(&self) -> f64 {
            1.0
        }
        fn flat_physical_damage_bonus(&self) -> u32 {
            0
        }
        fn melee_ability_crit_dmg_mod(&self) -> f64 {
            2.0
        }
        fn total_threat_mod(&self) -> f64 {
            self.stats.get_total_threat_mod()
        }
        fn avg_mh_damage(&self) -> f64 {
            200.0
        }
    }

    fn db() -> SpellDb {
        let file: SpellFileSpec = serde_yaml::from_str(
            r#"
spell_groups:
  - name: Bloodthirst
    causes_gcd: normal
    restricted_by_gcd: true
    cooldown: 6
    requires_talent: Bloodthirst
    ranks:
      - rank: 1
        resource: rage
        cost: 30
        effects:
          - { name: SCHOOL_DAMAGE_PHYSICAL, value: 0, ap_dmg_mod: 0.45 }
  - name: Execute
    causes_gcd: normal
    restricted_by_gcd: true
    resource_miss_cost_mod: 0.16
    restrictions:
      - { type: TARGET_HEALTH_PERCENTAGE, value: 0.2, cmp: leq }
      - { type: UNIT_STANCE, stance: DEFENSIVE_STANCE, cmp: neq }
    ranks:
      - rank: 5
        resource: rage
        cost: 15
        effects:
          - { name: SCHOOL_DAMAGE_PHYSICAL, value: 600 }
          - { name: SCHOOL_DAMAGE_CONVERT_RAGE, value: 15 }
    modified_by_talent:
      - { talent: Improved Execute, rank: 1, type: absolute_resource_cost_reduction, value: 2 }
      - { talent: Improved Execute, rank: 2, type: absolute_resource_cost_reduction, value: 5 }
  - name: Battle Shout
    causes_gcd: normal
    restricted_by_gcd: true
    ranks:
      - rank: 6
        resource: rage
        cost: 10
        buff:
          unit: self
          duration: 120
          effects: [{ name: APPLY_AURA_MELEE_ATTACK_POWER, value: 193 }]
      - rank: 7
        resource: rage
        cost: 10
        requires_phase: 6
        level_req: 60
        buff:
          unit: self
          duration: 120
          effects: [{ name: APPLY_AURA_MELEE_ATTACK_POWER, value: 232 }]
    modified_by_talent:
      - { talent: Improved Battle Shout, rank: 1, type: increase_value_percent, target: buff, effect: APPLY_AURA_MELEE_ATTACK_POWER, value: 5 }
      - { talent: Improved Battle Shout, rank: 2, type: increase_value_percent, target: buff, effect: APPLY_AURA_MELEE_ATTACK_POWER, value: 10 }
      - { talent: Booming Voice, rank: 1, type: increase_buff_duration_percent, value: 10 }
  - name: Berserker Stance
    causes_gcd: stance
    restricted_by_gcd: true
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: self
          effects:
            - { name: APPLY_AURA_SHAPESHIFT_BERSERKER_STANCE }
            - { name: APPLY_AURA_MOD_THREAT, value: -20 }
    modified_by_talent:
      - { talent: Defiance, rank: 1, type: add_effect, target: buff, effect: { name: APPLY_AURA_MOD_THREAT, value: 3 } }
  - name: Overpower
    causes_gcd: normal
    restricted_by_gcd: true
    cooldown: 5
    restrictions:
      - { type: COMBO_POINTS, value: 0, cmp: greater }
      - { type: UNIT_STANCE, stance: BATTLE_STANCE, cmp: eq }
    flags: [CANNOT_BE_DODGED, CANNOT_BE_PARRIED, CANNOT_BE_BLOCKED]
    ranks:
      - rank: 4
        resource: rage
        cost: 5
        effects:
          - { name: NORMALIZED_WEAPON_DAMAGE, value: 35, innate_threat: 10 }
          - { name: CONSUME_COMBO_POINTS, independent: true }
    modified_by_talent:
      - { talent: Improved Overpower, rank: 1, type: increase_crit_chance, value: 2500 }
  - name: Flurry
    causes_gcd: none
    requires_talent: Flurry
    flags: [PASSIVE_SPELL, ON_MELEE_CRIT]
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: self
          duration: 15
          base_charges: 3
          effects: [{ name: APPLY_AURA_MOD_MELEE_ATTACK_SPEED, value: 0 }]
    modified_by_talent:
      - { talent: Flurry, rank: 1, type: increase_value, target: buff, effect: APPLY_AURA_MOD_MELEE_ATTACK_SPEED, value: 10 }
      - { talent: Flurry, rank: 2, type: increase_value, target: buff, effect: APPLY_AURA_MOD_MELEE_ATTACK_SPEED, value: 15 }
  - name: Flurry Consume
    causes_gcd: none
    requires_talent: Flurry
    flags: [PASSIVE_SPELL, ON_MELEE_HIT]
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: AURA_CONSUME_CHARGE, buff: Flurry }]
  - name: Revenge
    causes_gcd: normal
    restricted_by_gcd: true
    cooldown: 5
    restrictions:
      - { type: BUFF_ACTIVE, buff: Revenge Ready }
      - { type: OFFHAND_WEAPON_TYPE, weapon_type: SHIELD, cmp: eq }
    ranks:
      - rank: 6
        resource: rage
        cost: 5
        effects: [{ name: SCHOOL_DAMAGE_PHYSICAL, min: 64, max: 78, innate_threat: 315 }]
  - name: Bloodrage
    causes_gcd: none
    cooldown: 60
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 10 }]
        buff:
          unit: self
          duration: 10
          effects: [{ name: APPLY_AURA_PERIODIC_RESOURCE_GAIN_RAGE, value: 1, tick_rate: 1.0 }]
  - name: Rend
    causes_gcd: normal
    restricted_by_gcd: true
    flags: [CANNOT_CRIT]
    ranks:
      - rank: 7
        resource: rage
        cost: 10
        effects: [{ name: SCHOOL_DAMAGE_PHYSICAL, value: 0 }]
        buff:
          unit: target
          priority: trash
          duration: 21
          effects:
            - { name: APPLY_AURA_PERIODIC_DAMAGE_FROM_WEAPON, value: 147, period: 3, ticks: 7, weapon_coeff: 0.0024766667 }
    modified_by_talent:
      - { talent: Improved Rend, rank: 1, type: increase_damage_percent, value: 15 }
  - name: Deep Wounds
    causes_gcd: none
    requires_talent: Deep Wounds
    flags: [PASSIVE_SPELL, ON_MELEE_CRIT, CANNOT_MISS, CANNOT_BE_DODGED, CANNOT_BE_PARRIED, CANNOT_BE_BLOCKED]
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: target
          priority: trash
          duration: 12
          effects: [{ name: APPLY_AURA_PERIODIC_WEAPON_DAMAGE, value: 0, period: 3 }]
    modified_by_talent:
      - { talent: Deep Wounds, rank: 1, type: increase_value, target: buff, effect: APPLY_AURA_PERIODIC_WEAPON_DAMAGE, value: 20 }
      - { talent: Deep Wounds, rank: 2, type: increase_value, target: buff, effect: APPLY_AURA_PERIODIC_WEAPON_DAMAGE, value: 40 }
      - { talent: Deep Wounds, rank: 3, type: increase_value, target: buff, effect: APPLY_AURA_PERIODIC_WEAPON_DAMAGE, value: 60 }
  - name: Slam
    causes_gcd: normal
    restricted_by_gcd: true
    flags: [RESETS_SWING_TIMERS, STOPS_ATTACK_DURING_CAST]
    ranks:
      - rank: 4
        resource: rage
        cost: 15
        cast_time_ms: 1500
        effects: [{ name: NORMALIZED_WEAPON_DAMAGE, value: 87 }]
    modified_by_talent:
      - { talent: Improved Slam, rank: 1, type: cast_time_reduction_ms, value: 100 }
  - name: Heroic Strike
    causes_gcd: none
    flags: [ON_NEXT_SWING]
    ranks:
      - rank: 9
        resource: rage
        cost: 15
        effects: [{ name: WEAPON_DAMAGE, value: 157, innate_threat: 145 }]
        buff: { name: Heroic Strike Queued, unit: self, hidden: true }
  - name: Quick Cast
    causes_gcd: normal
    restricted_by_gcd: true
    suppressible_cast: true
    ranks:
      - rank: 1
        resource: rage
        cost: 5
        cast_time_ms: 2000
        effects: [{ name: SCHOOL_DAMAGE_PHYSICAL, value: 100 }]
"#,
        )
        .unwrap();
        let mut db = SpellDb::new();
        db.add_file(file).unwrap();
        db
    }

    #[test]
    fn construction_validates_handles_and_reads_the_spec() {
        let db = db();
        let mut world = World::new();
        let bloodthirst = world.spell(db.get("Bloodthirst").unwrap(), 1);
        assert_eq!(bloodthirst.name(), "Bloodthirst");
        assert_eq!(bloodthirst.rank(), 1);
        assert!(!bloodthirst.is_enabled());
        assert_eq!(bloodthirst.resource_type(), ResourceType::Rage);
        assert_eq!(bloodthirst.base_cooldown(), 6.0);
        assert_eq!(bloodthirst.effects().len(), 1);
        assert!(bloodthirst.marker_buff().is_none());
        assert!(bloodthirst.cooldown_id().is_some());
        assert_eq!(bloodthirst.resource_cost(&world), 30);

        let shout = world.spell(db.get("Battle Shout").unwrap(), 6);
        assert!(shout.is_enabled());
        assert!(shout.marker_buff().is_some());
        assert_eq!(
            shout.modifying_talents().collect::<Vec<_>>(),
            vec!["Booming Voice", "Improved Battle Shout"]
        );
        assert!(shout.is_rank_learned(&world));
        let shout7 = world.spell(db.get("Battle Shout").unwrap(), 7);
        assert!(!shout7.is_rank_learned(&world));
    }

    #[test]
    #[should_panic(expected = "requires a cooldown control")]
    fn gcd_spells_need_a_cooldown_handle() {
        let db = db();
        let _ = Spell::new(Arc::clone(db.get("Bloodthirst").unwrap()), 1, None, None);
    }

    #[test]
    #[should_panic(expected = "has a buff but no marker buff")]
    fn buff_ranks_need_a_marker_buff_handle() {
        let db = db();
        let _ = Spell::new(
            Arc::clone(db.get("Battle Shout").unwrap()),
            6,
            Some(CooldownId(0)),
            None,
        );
    }

    #[test]
    fn status_checks_in_c_plus_plus_order() {
        let db = db();
        let mut world = World::new();
        let mut bloodthirst = world.spell(db.get("Bloodthirst").unwrap(), 1);
        assert_eq!(bloodthirst.status(&world), SpellStatus::NotEnabled);
        bloodthirst.enable(&mut world);
        assert_eq!(bloodthirst.status(&world), SpellStatus::Available);

        world.next_gcd = 1.0;
        assert_eq!(bloodthirst.status(&world), SpellStatus::OnGcd);
        world.next_gcd = 0.0;
        world.casting = true;
        assert_eq!(bloodthirst.status(&world), SpellStatus::CastInProgress);
        world.casting = false;

        world.rolls.push_back(PhysicalAttackResult::Hit);
        bloodthirst.perform(&mut world);
        assert_eq!(bloodthirst.status(&world), SpellStatus::OnGcd);
        world.advance_to(1.5);
        assert_eq!(bloodthirst.status(&world), SpellStatus::OnCooldown);
        assert_eq!(bloodthirst.cooldown_remaining(&world), 4.5);
        world.advance_to(6.0);
        assert_eq!(bloodthirst.status(&world), SpellStatus::Available);

        world.rage = 29;
        assert_eq!(
            bloodthirst.status(&world),
            SpellStatus::InsufficientResources
        );
        world.rage = 30;
        world.next_stance_cd = 7.0;
        assert_eq!(bloodthirst.status(&world), SpellStatus::OnStanceCooldown);
    }

    #[test]
    fn restrictions_map_to_statuses() {
        let db = db();
        let mut world = World::new();
        let execute = world.spell(db.get("Execute").unwrap(), 5);
        assert_eq!(execute.status(&world), SpellStatus::NotInExecuteRange);
        world.advance_to(240.0);
        assert_eq!(execute.status(&world), SpellStatus::Available);
        world.stance = Stance::Defensive;
        assert_eq!(execute.status(&world), SpellStatus::InDefensiveStance);
        world.stance = Stance::Berserker;
        assert_eq!(execute.status(&world), SpellStatus::Available);

        let overpower = world.spell(db.get("Overpower").unwrap(), 4);
        assert_eq!(
            overpower.status(&world),
            SpellStatus::InsufficientComboPoints
        );
        world.combo_points = 1;
        assert_eq!(overpower.status(&world), SpellStatus::InBerserkerStance);
        world.stance = Stance::Battle;
        assert_eq!(overpower.status(&world), SpellStatus::Available);

        let revenge = world.spell(db.get("Revenge").unwrap(), 6);
        assert_eq!(revenge.status(&world), SpellStatus::BuffInactive);
        let ready = world.add_buff(Buff::new(
            "Revenge Ready",
            None,
            BuffKind::SelfBuff,
            Some(5.0),
            0,
        ));
        world.enable_buff(ready);
        world.apply_buff(ready);
        assert_eq!(revenge.status(&world), SpellStatus::Available);
    }

    #[test]
    fn perform_runs_effects_pays_cost_and_reports_damage() {
        let db = db();
        let mut world = World::new();
        world.armor = 3000;
        let mut bloodthirst = world.spell(db.get("Bloodthirst").unwrap(), 1);
        bloodthirst.enable(&mut world);
        world.rolls.push_back(PhysicalAttackResult::Critical);

        let report = bloodthirst.perform(&mut world);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(report.resource_cost, 30);
        assert_eq!(report.resource_lost, 30);
        assert_eq!(world.rage, 70);
        let expected = (450.0 * (1.0 - Mechanics::reduction_from_armor(3000, 60)) * 2.0).round();
        let attack = report.attack.unwrap();
        assert_eq!(attack.result, PhysicalAttackResult::Critical);
        assert_eq!(attack.damage, expected as u32);
        assert_eq!(attack.threat, expected);
        assert_eq!(attack.execution_time, 1.5);
        assert_eq!(report.proc_sources, vec![ProcSource::MeleeCritical]);
        assert_eq!(report.buff, None);
        assert_eq!(bloodthirst.last_result(), SpellResult::Success);

        // Cooldown started, cooldown-ready and GCD player actions scheduled.
        assert_eq!(bloodthirst.last_used(&world), 0.0);
        assert_eq!(bloodthirst.next_use(&world), 6.0);
        assert!(world.on_global_cooldown());
        let times: Vec<f64> = world.pending().into_iter().map(|(t, _)| t).collect();
        assert_eq!(times, vec![1.5, 6.0]);
    }

    #[test]
    fn dodged_spells_pay_the_reduced_cost_and_missed_spells_the_full_cost() {
        let db = db();
        let mut world = World::new();
        let mut execute = world.spell(db.get("Execute").unwrap(), 5);
        world.advance_to(290.0);

        world.rolls.push_back(PhysicalAttackResult::Dodge);
        let report = execute.perform(&mut world);
        assert_eq!(report.result, SpellResult::Failure);
        assert_eq!(report.resource_lost, 2);
        assert_eq!(world.rage, 98);
        assert_eq!(report.attack.unwrap().result, PhysicalAttackResult::Dodge);
        assert_eq!(report.attack.unwrap().damage, 0);
        assert_eq!(report.proc_sources, vec![ProcSource::MeleeDodge]);
        assert_eq!(world.extra_crits, vec![0]);

        world.rolls.push_back(PhysicalAttackResult::Miss);
        let report = execute.perform(&mut world);
        assert_eq!(report.resource_lost, 15);
        assert_eq!(world.rage, 83);
        assert_eq!(report.proc_sources, vec![ProcSource::MeleeMiss]);

        // A hit: both effects contribute damage, the dependent one reusing the roll.
        world.rolls.push_back(PhysicalAttackResult::Hit);
        let report = execute.perform(&mut world);
        assert_eq!(report.result, SpellResult::Success);
        let attack = report.attack.unwrap();
        assert_eq!(attack.damage, 600 + (83 - 15) * 15);
        assert_eq!(world.rage, 68);
        assert_eq!(report.proc_sources, vec![ProcSource::MeleeHit]);
        assert!(world.rolls.is_empty());
    }

    #[test]
    fn buff_only_spells_apply_their_buff() {
        let db = db();
        let mut world = World::new();
        let mut shout = world.spell(db.get("Battle Shout").unwrap(), 6);
        let report = shout.perform(&mut world);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(
            report.buff,
            Some(BuffApplication::Applied { evicted: None })
        );
        assert!(report.attack.is_none());
        assert!(report.proc_sources.is_empty());
        assert_eq!(world.rage, 90);
        assert_eq!(world.stats.base_stats().get_base_melee_ap(), 193);
        assert!(world.buffs[shout.marker_buff().unwrap().index()].is_active());

        let report = shout.perform(&mut world);
        assert_eq!(report.buff, Some(BuffApplication::Refreshed { stacks: 1 }));

        shout.cancel(&mut world);
        assert!(!world.buffs[0].is_active());
        assert_eq!(world.stats.base_stats().get_base_melee_ap(), 0);
    }

    #[test]
    fn stance_spells_use_the_stance_cooldown() {
        let db = db();
        let mut world = World::new();
        let mut stance = world.spell(db.get("Berserker Stance").unwrap(), 1);
        assert_eq!(stance.gcd_behavior(), GcdBehavior::Stance);
        let report = stance.perform(&mut world);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(world.stance, Stance::Berserker);
        assert!(world.on_stance_cooldown());
        assert!(!world.on_global_cooldown());
        assert_eq!(stance.status(&world), SpellStatus::OnStanceCooldown);
        assert_eq!(world.stats.get_total_threat_mod(), 0.8);
    }

    #[test]
    #[should_panic(expected = "already on stance cooldown")]
    fn stance_swap_during_stance_cooldown_panics() {
        let db = db();
        let mut world = World::new();
        let mut stance = world.spell(db.get("Berserker Stance").unwrap(), 1);
        stance.perform(&mut world);
        stance.perform(&mut world);
    }

    #[test]
    #[should_panic(expected = "insufficient resource")]
    fn performing_without_resources_panics() {
        let db = db();
        let mut world = World::new();
        world.rage = 0;
        let mut execute = world.spell(db.get("Execute").unwrap(), 5);
        execute.perform(&mut world);
    }

    #[test]
    fn enable_and_disable_manage_the_marker_buff() {
        let db = db();
        let mut world = World::new();
        let mut flurry = world.spell(db.get("Flurry").unwrap(), 1);
        let buff = flurry.marker_buff().unwrap();
        assert!(!world.buffs[buff.index()].is_enabled());
        flurry.enable(&mut world);
        assert!(world.buffs[buff.index()].is_enabled());
        assert_eq!(
            world.buffs[buff.index()].instance_id(),
            Some(InstanceId(buff.0 + 1))
        );
        flurry.disable(&mut world);
        assert!(!flurry.is_enabled());
        assert!(!world.buffs[buff.index()].is_enabled());
        // Disabling twice is harmless.
        flurry.disable(&mut world);
    }

    #[test]
    fn talents_modify_cost_values_effects_and_durations() {
        let db = db();
        let mut world = World::new();

        let mut execute = world.spell(db.get("Execute").unwrap(), 5);
        execute.increase_talent_rank(&mut world, "Improved Execute", 1);
        assert_eq!(execute.resource_cost(&world), 13);
        execute.increase_talent_rank(&mut world, "Improved Execute", 2);
        assert_eq!(execute.resource_cost(&world), 10);
        execute.decrease_talent_rank(&mut world, "Improved Execute", 1);
        assert_eq!(execute.resource_cost(&world), 13);
        execute.decrease_talent_rank(&mut world, "Improved Execute", 0);
        assert_eq!(execute.resource_cost(&world), 15);
        execute.increase_resource_cost_modifier(-20);
        assert_eq!(execute.resource_cost(&world), 12);
        execute.decrease_resource_cost_modifier(-20);
        assert_eq!(execute.resource_cost(&world), 15);

        let mut flurry = world.spell(db.get("Flurry").unwrap(), 1);
        let buff = flurry.marker_buff().unwrap();
        flurry.increase_talent_rank(&mut world, "Flurry", 1);
        assert_eq!(world.buffs[buff.index()].effects[0].value(), 10.0);
        flurry.increase_talent_rank(&mut world, "Flurry", 2);
        assert_eq!(world.buffs[buff.index()].effects[0].value(), 15.0);
        flurry.decrease_talent_rank(&mut world, "Flurry", 1);
        assert_eq!(world.buffs[buff.index()].effects[0].value(), 10.0);
        flurry.decrease_talent_rank(&mut world, "Flurry", 0);
        assert_eq!(world.buffs[buff.index()].effects[0].value(), 0.0);

        let mut shout = world.spell(db.get("Battle Shout").unwrap(), 6);
        let buff = shout.marker_buff().unwrap();
        shout.increase_talent_rank(&mut world, "Improved Battle Shout", 1);
        shout.increase_talent_rank(&mut world, "Improved Battle Shout", 2);
        assert_eq!(world.buffs[buff.index()].effects[0].value(), 193.0 * 1.1);
        shout.decrease_talent_rank(&mut world, "Improved Battle Shout", 1);
        assert!((world.buffs[buff.index()].effects[0].value() - 193.0 * 1.05).abs() < 1e-9);
        shout.increase_talent_rank(&mut world, "Booming Voice", 1);
        assert_eq!(world.buffs[buff.index()].duration(), Some(132.0));
        shout.decrease_talent_rank(&mut world, "Booming Voice", 0);
        assert_eq!(world.buffs[buff.index()].duration(), Some(120.0));
        // Unknown talents are ignored.
        shout.increase_talent_rank(&mut world, "Not A Talent", 1);

        let mut stance = world.spell(db.get("Berserker Stance").unwrap(), 1);
        let buff = stance.marker_buff().unwrap();
        stance.increase_talent_rank(&mut world, "Defiance", 1);
        assert_eq!(world.buffs[buff.index()].effects.len(), 3);
        assert_eq!(
            world.buffs[buff.index()].effects[2]
                .talent_as_source
                .as_deref(),
            Some("Defiance")
        );
        stance.decrease_talent_rank(&mut world, "Defiance", 0);
        assert_eq!(world.buffs[buff.index()].effects.len(), 2);

        let mut overpower = world.spell(db.get("Overpower").unwrap(), 4);
        overpower.increase_talent_rank(&mut world, "Improved Overpower", 1);
        assert_eq!(overpower.crit_chance_bonus(), 2500);
        world.combo_points = 1;
        world.rolls.push_back(PhysicalAttackResult::Hit);
        let report = overpower.perform(&mut world);
        assert_eq!(world.extra_crits, vec![2500]);
        assert_eq!(world.combo_points, 0);
        assert_eq!(report.attack.unwrap().damage, 335);
        assert_eq!(report.attack.unwrap().threat, 345.0);
        overpower.decrease_talent_rank(&mut world, "Improved Overpower", 0);
        assert_eq!(overpower.crit_chance_bonus(), 0);

        let mut slam = world.spell(db.get("Slam").unwrap(), 4);
        slam.increase_talent_rank(&mut world, "Improved Slam", 1);
        assert_eq!(slam.cast_time_ms(), 1400);
        slam.decrease_talent_rank(&mut world, "Improved Slam", 0);
        assert_eq!(slam.cast_time_ms(), 1500);
    }

    #[test]
    fn talent_changes_reapply_an_active_marker_buff() {
        let db = db();
        let mut world = World::new();
        let mut shout = world.spell(db.get("Battle Shout").unwrap(), 6);
        shout.perform(&mut world);
        assert_eq!(world.stats.base_stats().get_base_melee_ap(), 193);
        shout.increase_talent_rank(&mut world, "Improved Battle Shout", 1);
        assert_eq!(world.stats.base_stats().get_base_melee_ap(), 203);
        assert_eq!(
            world.aura_log,
            vec![
                "+ApplyAuraMeleeAttackPower",
                "-ApplyAuraMeleeAttackPower",
                "+ApplyAuraMeleeAttackPower"
            ]
        );
        assert!(world.buffs[0].is_active());
    }

    #[test]
    fn consume_charge_effects_reach_the_named_buff() {
        let db = db();
        let mut world = World::new();
        let mut flurry = world.spell(db.get("Flurry").unwrap(), 1);
        flurry.enable(&mut world);
        flurry.increase_talent_rank(&mut world, "Flurry", 1);
        let mut consume = world.spell(db.get("Flurry Consume").unwrap(), 1);
        consume.enable(&mut world);

        flurry.perform(&mut world);
        assert_eq!(world.stats.get_melee_attack_speed_mod(), 1.1);
        for remaining in [2, 1] {
            consume.perform(&mut world);
            assert_eq!(world.buffs[0].charges(), remaining);
        }
        consume.perform(&mut world);
        assert!(!world.buffs[0].is_active());
        assert_eq!(world.stats.get_melee_attack_speed_mod(), 1.0);
    }

    #[test]
    fn periodic_resource_gain_ticks_until_the_buff_expires() {
        let db = db();
        let mut world = World::new();
        let mut bloodrage = world.spell(db.get("Bloodrage").unwrap(), 1);
        assert!(bloodrage.is_periodic());
        assert_eq!(
            bloodrage.periodic_kind(&world),
            Some(PeriodicKind::ResourceGain {
                resource: ResourceType::Rage,
                amount: 1
            })
        );
        world.rage = 0;

        let report = bloodrage.perform(&mut world);
        assert_eq!(report.resource_gained, vec![(ResourceType::Rage, 10)]);
        assert_eq!(world.rage, 10);
        assert_eq!(bloodrage.periodic().unwrap().application_id(), 1);

        world.run(&mut bloodrage, 30.0);
        // Ticks at 1..=10 (the tick at 10 coincides with the expiry and still counts).
        assert_eq!(world.ticks.len(), 10);
        assert_eq!(world.rage, 20);
        assert!(world
            .ticks
            .iter()
            .all(|tick| tick.resource_gained == Some((ResourceType::Rage, 1)) && tick.damage == 0));
        assert!(!world.buffs[0].is_active());
        assert!(!world
            .pending()
            .iter()
            .any(|(_, kind)| matches!(kind, EventKind::DotTick { .. })));
    }

    #[test]
    fn periodic_damage_from_weapon_distributes_damage_over_the_ticks() {
        let db = db();
        let mut world = World::new();
        let mut rend = world.spell(db.get("Rend").unwrap(), 7);
        rend.increase_talent_rank(&mut world, "Improved Rend", 1);
        assert_eq!(rend.damage_mod(), 1.15);
        world.rolls.push_back(PhysicalAttackResult::Hit);

        let report = rend.perform(&mut world);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(world.can_crits, vec![false]);
        assert_eq!(report.attack.unwrap().damage, 0);
        let total = (147.0 + 200.0 * 21.0 * 0.0024766667) * 1.15;
        let periodic = rend.periodic().unwrap();
        assert_eq!(periodic.ticks_left(), 7);
        assert!((periodic.damage_remaining() - total).abs() < 1e-9);

        world.run(&mut rend, 22.0);
        assert_eq!(world.ticks.len(), 7);
        let dealt: u32 = world.ticks.iter().map(|tick| tick.damage).sum();
        assert_eq!(dealt, total.round() as u32);
        assert!((world.ticks[0].resource_cost - 10.0 / 7.0).abs() < 1e-9);
        assert!((world.ticks[0].execution_time - 1.5 / 7.0).abs() < 1e-9);
        assert_eq!(rend.periodic().unwrap().ticks_left(), 0);
        assert!(!world.buffs[0].is_active());
        world.target.check_clean();
    }

    #[test]
    fn periodic_refresh_rearms_the_damage_without_a_new_tick_chain() {
        let db = db();
        let mut world = World::new();
        let mut rend = world.spell(db.get("Rend").unwrap(), 7);
        world.rolls.push_back(PhysicalAttackResult::Hit);
        rend.perform(&mut world);
        world.run(&mut rend, 6.5);
        assert_eq!(world.ticks.len(), 2);
        assert_eq!(rend.periodic().unwrap().ticks_left(), 5);

        world.rolls.push_back(PhysicalAttackResult::Hit);
        let report = rend.perform(&mut world);
        assert_eq!(report.buff, Some(BuffApplication::Refreshed { stacks: 1 }));
        assert_eq!(rend.periodic().unwrap().ticks_left(), 7);
        assert_eq!(rend.periodic().unwrap().application_id(), 1);
        assert_eq!(world.buffs[0].time_left(6.5), 21.0);

        world.run(&mut rend, 40.0);
        assert_eq!(world.ticks.len(), 9);
        assert!(!world.buffs[0].is_active());
    }

    #[test]
    fn stale_ticks_and_ticks_after_cancel_are_ignored() {
        let db = db();
        let mut world = World::new();
        let mut rend = world.spell(db.get("Rend").unwrap(), 7);
        world.rolls.push_back(PhysicalAttackResult::Hit);
        rend.perform(&mut world);
        assert!(rend.perform_periodic(99, &mut world).is_none());

        world.run(&mut rend, 3.5);
        assert_eq!(world.ticks.len(), 1);
        rend.cancel(&mut world);
        world.run(&mut rend, 30.0);
        assert_eq!(world.ticks.len(), 1);
        assert_eq!(rend.periodic().unwrap().ticks_left(), 0);

        // A disabled spell does not tick either.
        world.rolls.push_back(PhysicalAttackResult::Hit);
        rend.perform(&mut world);
        rend.disable(&mut world);
        world.run(&mut rend, 60.0);
        assert_eq!(world.ticks.len(), 1);
    }

    #[test]
    fn periodic_weapon_damage_stacks_independent_applications() {
        let db = db();
        let mut world = World::new();
        let mut deep_wounds = world.spell(db.get("Deep Wounds").unwrap(), 1);
        deep_wounds.enable(&mut world);
        deep_wounds.increase_talent_rank(&mut world, "Deep Wounds", 1);
        deep_wounds.increase_talent_rank(&mut world, "Deep Wounds", 2);
        deep_wounds.increase_talent_rank(&mut world, "Deep Wounds", 3);
        // The kind is re-read from the buff effect, so the talent's value change applies.
        assert_eq!(
            deep_wounds.periodic_kind(&world),
            Some(PeriodicKind::WeaponDamage {
                percent: 60.0,
                ticks_per_application: 4
            })
        );

        deep_wounds.perform(&mut world);
        assert_eq!(deep_wounds.periodic().unwrap().stacks(), &[4]);
        world.run(&mut deep_wounds, 3.5);
        assert_eq!(world.ticks.len(), 1);
        assert_eq!(world.ticks[0].damage, 30);
        assert_eq!(deep_wounds.periodic().unwrap().stacks(), &[3]);

        // A second application adds an independent stack and extends the buff.
        let report = deep_wounds.perform(&mut world);
        assert_eq!(report.buff, Some(BuffApplication::Refreshed { stacks: 1 }));
        assert_eq!(deep_wounds.periodic().unwrap().stacks(), &[3, 4]);
        world.run(&mut deep_wounds, 30.0);
        // Ticks at 6, 9, 12 and 15: the chain lasts as long as the longest stack.
        assert_eq!(world.ticks.len(), 5);
        assert!(world.ticks.iter().all(|tick| tick.damage == 30));
        assert!(deep_wounds.periodic().unwrap().stacks().is_empty());
        assert!(!world.buffs[0].is_active());
        world.target.check_clean();
    }

    #[test]
    fn cast_time_spells_complete_after_the_cast_time() {
        let db = db();
        let mut world = World::new();
        let mut slam = world.spell(db.get("Slam").unwrap(), 4);
        assert!(slam.has_cast_time());
        assert_eq!(slam.cast_time(&world), 1.5);
        world.casting_speed_mod = 1.25;
        assert_eq!(slam.cast_time(&world), 1.2);

        let report = slam.perform(&mut world);
        assert!(report.cast_started);
        assert_eq!(report.result, SpellResult::Undetermined);
        assert!(report.attack.is_none());
        assert!(slam.is_casting());
        assert!(world.cast_in_progress);
        assert_eq!(world.rage, 100, "the cost is paid on completion");
        assert!(world.on_global_cooldown());
        assert_eq!(world.attack_log, vec!["stop"]);
        assert_eq!(slam.status(&world), SpellStatus::OnGcd);
        world.casting = false;
        let times: Vec<f64> = world.pending().into_iter().map(|(t, _)| t).collect();
        assert_eq!(times, vec![1.2, 1.5]);

        // A stale cast id is ignored.
        assert!(slam.complete_cast(99, &mut world).is_none());

        world.rolls.push_back(PhysicalAttackResult::Critical);
        world.run(&mut slam, 1.3);
        assert_eq!(world.completed_casts.len(), 1);
        let report = &world.completed_casts[0];
        assert!(!report.cast_started);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(report.resource_lost, 15);
        assert_eq!(world.rage, 85);
        let attack = report.attack.unwrap();
        assert_eq!(attack.damage, (300 + 87) * 2);
        assert_eq!(attack.execution_time, 1.5);
        assert!(!slam.is_casting());
        assert!(!world.cast_in_progress);
        assert_eq!(world.attack_log, vec!["stop", "reset", "start"]);
        assert_eq!(world.engine.current_time(), 1.3);
    }

    #[test]
    fn suppressed_casts_complete_immediately() {
        let db = db();
        let mut world = World::new();
        let mut quick = world.spell(db.get("Quick Cast").unwrap(), 1);
        world.casting_time_suppressed = true;
        world.rolls.push_back(PhysicalAttackResult::Hit);
        let report = quick.perform(&mut world);
        assert!(!report.cast_started);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(report.attack.unwrap().damage, 100);
        assert_eq!(report.attack.unwrap().execution_time, 2.0);
        assert!(!quick.is_casting());
        assert!(world.attack_log.is_empty());
        assert!(!world
            .pending()
            .iter()
            .any(|(_, kind)| matches!(kind, EventKind::CastComplete { .. })));

        // Slam is not suppressible.
        let mut slam = world.spell(db.get("Slam").unwrap(), 4);
        assert!(slam.perform(&mut world).cast_started);
    }

    #[test]
    fn reset_forgets_a_cast_in_progress() {
        let db = db();
        let mut world = World::new();
        let mut slam = world.spell(db.get("Slam").unwrap(), 4);
        slam.perform(&mut world);
        slam.reset(&mut world);
        assert!(!slam.is_casting());
        world.cast_in_progress = false;
        world.run(&mut slam, 5.0);
        assert!(world.completed_casts.is_empty());
    }

    #[test]
    fn on_next_swing_spells_queue_and_fire_on_the_swing() {
        let db = db();
        let mut world = World::new();
        let mut heroic = world.spell(db.get("Heroic Strike").unwrap(), 9);
        assert!(heroic.is_on_next_swing());
        assert!(!heroic.is_queued(&world));

        let report = heroic.perform(&mut world);
        assert!(report.queued);
        assert_eq!(
            report.buff,
            Some(BuffApplication::Applied { evicted: None })
        );
        assert_eq!(report.result, SpellResult::Undetermined);
        assert!(heroic.is_queued(&world));
        assert_eq!(world.queued, heroic.id());
        assert_eq!(world.rage, 100, "the cost is paid when the swing lands");
        assert!(!world.on_global_cooldown());
        assert_eq!(world.attack_log, vec!["queue"]);

        // Re-queueing refreshes the marker buff.
        assert_eq!(
            heroic.perform(&mut world).buff,
            Some(BuffApplication::Refreshed { stacks: 1 })
        );

        world.rolls.push_back(PhysicalAttackResult::Hit);
        let report = heroic.perform_on_swing(&mut world);
        assert!(!report.queued);
        assert_eq!(report.result, SpellResult::Success);
        assert_eq!(report.resource_lost, 15);
        assert_eq!(world.rage, 85);
        let attack = report.attack.unwrap();
        assert_eq!(attack.damage, 400 + 157);
        assert_eq!(attack.threat, 557.0 + 145.0);
        assert_eq!(attack.execution_time, 0.0);
        assert!(!heroic.is_queued(&world));
        assert_eq!(world.queued, None);
        assert_eq!(world.attack_log, vec!["queue", "queue", "unqueue"]);

        // Cancelling clears the queue too.
        heroic.perform(&mut world);
        heroic.cancel(&mut world);
        assert!(!heroic.is_queued(&world));
        assert_eq!(world.queued, None);
    }

    #[test]
    fn casting_time_spell_coefficient() {
        assert!((spell_coefficient_from_casting_time(1000, 60) - 1500.0 / 3500.0).abs() < 1e-12);
        assert!((spell_coefficient_from_casting_time(2500, 60) - 2500.0 / 3500.0).abs() < 1e-12);
        assert_eq!(spell_coefficient_from_casting_time(4000, 60), 1.0);
        assert!(
            (spell_coefficient_from_casting_time(3500, 12) - (1.0 - 8.0 * 0.0375)).abs() < 1e-12
        );
        assert_eq!(spell_coefficient_from_casting_time(1500, 1), 0.0);
    }

    #[test]
    fn reset_clears_cooldown_and_result() {
        let db = db();
        let mut world = World::new();
        let mut bloodthirst = world.spell(db.get("Bloodthirst").unwrap(), 1);
        bloodthirst.enable(&mut world);
        world.rolls.push_back(PhysicalAttackResult::Hit);
        bloodthirst.perform(&mut world);
        assert_eq!(bloodthirst.next_use(&world), 6.0);
        bloodthirst.reset(&mut world);
        assert_eq!(bloodthirst.next_use(&world), 0.0);
        assert_eq!(bloodthirst.last_result(), SpellResult::Undetermined);
    }
}
