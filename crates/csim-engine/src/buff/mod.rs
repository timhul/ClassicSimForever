//! Buffs and debuffs. Port of `Spells/Buff.*`, `SelfBuff`, `PartyBuff`, `UniqueDebuff`,
//! `SharedDebuff` and `ExternalBuff` (the state machine part; the external buff *data* and
//! registry are in [`external`]).
//!
//! The C++ class hierarchy is one struct with a [`BuffKind`]. A buff owns its timing state
//! (applied / refreshed / expired, charges, stacks, removal iteration) and its aura effects, but
//! it does not reach into the character, the raid or the statistics: every state transition
//! returns what the owner has to do next ([`BuffApplication`], [`ChargeUse`], the `bool` of
//! [`Buff::remove`] / [`Buff::cancel`]), namely apply or remove the aura effects on the affected
//! units and cancel an evicted debuff. Target debuff slots are claimed through the [`Target`] in
//! the [`BuffContext`], keyed by the buff's raid-wide [`InstanceId`].
//!
//! On the table model a spell *is* its buff: [`Buff::from_record`] takes the spell's apply-aura
//! effects, its `SpellDuration`, `SpellAuraOptions` (charges, stacks, the events that consume a
//! charge) and `Attributes_0` (hidden), and derives the [`BuffKind`] from the effects' implicit
//! targets. The buffs other players and consumables provide are in [`external`].

pub mod external;

use crate::effect::Effect;
use crate::engine::{Engine, EventKind};
use crate::ids::{BuffId, CharId, InstanceId};
use crate::proc::ProcSource;
use crate::spell::overrides::{Overrides, ProcHitMask, SimFlag};
use crate::spell::record::SpellRecord;
use crate::target::{Priority, Target};

/// Which units a buff affects and how it is registered. Port of the `Buff` subclasses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuffKind {
    /// Affects the owning character only.
    SelfBuff,
    /// Affects every member of `party`; one instance is shared by the party's casters.
    PartyBuff { party: u8 },
    /// A debuff on the target owned by one character.
    UniqueDebuff,
    /// A debuff on the target shared by the whole raid (Sunder Armor).
    SharedDebuff,
    /// A raid/consumable buff (or target debuff) configured by the user rather than cast
    /// ([`external`]). Applied when selected and kept across iterations: [`Buff::reset`] and
    /// [`Buff::initialize`] leave it alone, and it never claims a debuff slot.
    External,
}

impl BuffKind {
    /// The kind of the buff a spell record applies, from its aura effects' implicit targets:
    /// party / raid area auras are party buffs (raid buffs, which the C++ code did not support
    /// either, are treated as party buffs), auras on the enemy are debuffs (shared by the raid
    /// when `debuff_shared` says so, or by default when the debuff stacks, like Sunder Armor),
    /// everything else is a self buff. `None` for spells without aura effects.
    pub fn from_record(
        record: &SpellRecord,
        party: u8,
        debuff_shared: Option<bool>,
    ) -> Option<BuffKind> {
        let auras: Vec<_> = record
            .effects
            .iter()
            .filter(|e| e.is_apply_aura())
            .collect();
        if auras.is_empty() {
            return None;
        }
        if auras.iter().any(|e| e.targets_group()) {
            return Some(BuffKind::PartyBuff { party });
        }
        if auras.iter().any(|e| e.targets_enemy()) {
            let shared = debuff_shared.unwrap_or(record.aura_options.max_stacks > 1);
            return Some(if shared {
                BuffKind::SharedDebuff
            } else {
                BuffKind::UniqueDebuff
            });
        }
        Some(BuffKind::SelfBuff)
    }

    pub fn is_debuff(self) -> bool {
        matches!(self, BuffKind::UniqueDebuff | BuffKind::SharedDebuff)
    }

    /// Whether the buff is registered with the raid control rather than one character (its
    /// `disable` is a no-op, as in C++ where these were disabled with the raid control).
    pub fn is_raid_shared(self) -> bool {
        matches!(self, BuffKind::PartyBuff { .. } | BuffKind::SharedDebuff)
    }
}

/// What [`Buff::refresh`] does with the remaining duration. Resolves the C++ TODO in
/// `Buff::refresh_buff`, which updated `refreshed` without scheduling a new removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RefreshPolicy {
    /// A refresh restarts the duration (default).
    #[default]
    ExtendDuration,
    /// A refresh only adds a stack; the buff still expires at the original time (armor
    /// penetration procs without `extend_duration_on_proc`).
    KeepDuration,
}

/// Outcome of [`Buff::apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuffApplication {
    /// The buff is disabled or (for debuffs) the target rejected it.
    NotApplied,
    /// The buff went from inactive to active: apply its aura effects to the affected units. For
    /// debuffs, `evicted` is a lower-priority debuff that lost its slot and must be cancelled.
    Applied { evicted: Option<InstanceId> },
    /// The buff was already active and has been refreshed; `stacks` is the new stack count.
    Refreshed { stacks: u32 },
}

/// Outcome of [`Buff::use_charge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeUse {
    /// The buff was not active.
    Inactive,
    /// A charge was used and some remain.
    ChargeUsed { remaining: u32 },
    /// The last charge was used and the buff has been removed: remove its aura effects.
    Removed,
}

/// Everything a buff needs from its surroundings to change state.
pub struct BuffContext<'a> {
    pub engine: &'a mut Engine,
    pub target: &'a mut Target,
    /// The character whose buff list the buff lives in (used for the removal event).
    pub character: CharId,
    /// The buff's handle in that list (used for the removal event).
    pub buff: BuffId,
}

impl BuffContext<'_> {
    fn now(&self) -> f64 {
        self.engine.current_time()
    }
}

/// A buff or debuff. Port of `Buff`.
#[derive(Debug, Clone, PartialEq)]
pub struct Buff {
    name: String,
    canonical_name: String,
    icon: Option<String>,
    kind: BuffKind,
    hidden: bool,
    /// `None` = permanent.
    base_duration: Option<f64>,
    /// Talent modification of the duration (`increase_buff_duration_percent`).
    duration_percent: i32,
    base_charges: u32,
    max_stacks: u32,
    priority: Priority,
    refresh_policy: RefreshPolicy,
    /// The spell the buff belongs to (`SpellName.ID`), 0 for buffs made in code.
    spell: u32,
    /// The events that use up one charge (`SpellAuraOptions.ProcTypeMask` of a charged aura:
    /// Flurry loses a charge per landed swing).
    charge_sources: Vec<ProcSource>,
    /// The aura effects, owned so talent rank values can be substituted.
    pub effects: Vec<Effect>,

    instance_id: Option<InstanceId>,
    enabled: bool,
    current_charges: u32,
    current_stacks: u32,
    iteration: u32,
    applied: f64,
    refreshed: f64,
    expired: f64,
    active: bool,
    uptime: f64,
}

impl Buff {
    /// The name shared buffs are registered under: `"Name (spell id)"`.
    pub fn canonical_name_for(name: &str, spell: u32) -> String {
        format!("{name} ({spell})")
    }

    /// A buff not defined by a spell spec (hidden marker buffs, item/enchant buffs). The name
    /// doubles as canonical name, as in the C++ `SelfBuff`/`UniqueDebuff` constructors.
    pub fn new(
        name: &str,
        icon: Option<&str>,
        kind: BuffKind,
        duration: Option<f64>,
        base_charges: u32,
    ) -> Self {
        Buff {
            name: name.to_string(),
            canonical_name: name.to_string(),
            icon: icon.map(str::to_string),
            kind,
            hidden: false,
            base_duration: duration,
            duration_percent: 0,
            base_charges,
            max_stacks: 1,
            priority: Priority::Invalid,
            refresh_policy: RefreshPolicy::default(),
            spell: 0,
            charge_sources: Vec::new(),
            effects: Vec::new(),
            instance_id: None,
            enabled: false,
            current_charges: 0,
            current_stacks: 0,
            iteration: 0,
            applied: 0.0,
            refreshed: 0.0,
            expired: 0.0,
            active: false,
            uptime: 0.0,
        }
    }

    /// The buff a spell record applies: its aura effects with the spell's duration, charges,
    /// stacks and hidden flag. `kind` comes from [`BuffKind::from_record`]; the debuff priority
    /// comes from the overrides (`Mid` by default).
    pub fn from_record(record: &SpellRecord, kind: BuffKind, overrides: &Overrides) -> Self {
        let duration = if record.is_permanent() || record.duration_ms.is_none() {
            None
        } else {
            record.finite_duration_ms().map(|ms| f64::from(ms) / 1000.0)
        };
        let mut buff = Buff::new(
            &record.name,
            None,
            kind,
            duration,
            record.aura_options.proc_charges,
        );
        buff.canonical_name = Buff::canonical_name_for(&record.name, record.id);
        buff.spell = record.id;
        buff.hidden = record.is_hidden();
        buff.max_stacks = record.aura_options.max_stacks.max(1);
        if kind.is_debuff() {
            buff.set_priority(
                overrides
                    .debuff_priority(record.id)
                    .unwrap_or(Priority::Mid),
            );
        }
        if record.aura_options.proc_charges > 0 {
            buff.charge_sources =
                ProcSource::from_masks(record.aura_options.proc_type_mask, ProcHitMask::LANDED);
        }
        let cannot_crit = overrides.has_sim_flag(record.id, SimFlag::CannotCrit);
        buff.effects = record
            .effects
            .iter()
            .filter(|e| e.is_apply_aura())
            .map(|e| {
                Effect::new(
                    e,
                    record,
                    overrides.effect_script(record.id, e.index).copied(),
                    cannot_crit,
                )
            })
            .collect();
        buff
    }

    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Renames the buff (external buffs carry the registry's name, not the aura spell's); the
    /// canonical name follows.
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self.canonical_name = if self.spell == 0 {
            name.to_string()
        } else {
            Buff::canonical_name_for(name, self.spell)
        };
        self
    }

    /// Replaces the duration (`None` = permanent).
    pub fn with_duration(mut self, duration: Option<f64>) -> Self {
        self.base_duration = duration;
        self
    }

    pub fn with_max_stacks(mut self, max_stacks: u32) -> Self {
        self.max_stacks = max_stacks.max(1);
        self
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.set_priority(priority);
        self
    }

    pub fn with_refresh_policy(mut self, policy: RefreshPolicy) -> Self {
        self.refresh_policy = policy;
        self
    }

    fn set_priority(&mut self, priority: Priority) {
        assert!(
            !self.kind.is_debuff() || priority != Priority::Invalid,
            "Cannot create debuff {} with invalid priority",
            self.name
        );
        self.priority = priority;
    }

    // --- Static properties ---

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn canonical_name(&self) -> &str {
        &self.canonical_name
    }

    pub fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    pub fn kind(&self) -> BuffKind {
        self.kind
    }

    pub fn is_debuff(&self) -> bool {
        self.kind.is_debuff()
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Duration in seconds including talent modifications; `None` = permanent.
    pub fn duration(&self) -> Option<f64> {
        self.base_duration
            .map(|base| base * (1.0 + f64::from(self.duration_percent) / 100.0))
    }

    pub fn is_permanent(&self) -> bool {
        self.base_duration.is_none()
    }

    /// Lengthens the duration by `percent` of the base duration (Booming Voice).
    pub fn increase_duration_percent(&mut self, percent: i32) {
        self.duration_percent += percent;
    }

    pub fn decrease_duration_percent(&mut self, percent: i32) {
        self.duration_percent -= percent;
    }

    pub fn base_charges(&self) -> u32 {
        self.base_charges
    }

    pub fn max_stacks(&self) -> u32 {
        self.max_stacks
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }

    pub fn refresh_policy(&self) -> RefreshPolicy {
        self.refresh_policy
    }

    /// The spell the buff belongs to (0 for buffs made in code).
    pub fn spell(&self) -> u32 {
        self.spell
    }

    /// The events that use up one charge.
    pub fn charge_sources(&self) -> &[ProcSource] {
        &self.charge_sources
    }

    /// Whether a `source` event uses up one charge of this buff.
    pub fn consumes_charge_on(&self, source: ProcSource) -> bool {
        self.charge_sources.contains(&source)
    }

    /// The name under which statistics are collected (party buffs are per party).
    pub fn statistics_name(&self) -> String {
        match self.kind {
            BuffKind::PartyBuff { party } => format!("{} (party {})", self.name, party + 1),
            _ => self.name.clone(),
        }
    }

    // --- Enable / disable / identity ---

    pub fn instance_id(&self) -> Option<InstanceId> {
        self.instance_id
    }

    /// Sets the raid-wide identity; done once when the buff is added to the enabled buffs.
    pub fn set_instance_id(&mut self, id: InstanceId) {
        self.instance_id = Some(id);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// # Panics
    /// Panics if the buff is already enabled.
    pub fn enable(&mut self) {
        assert!(
            !self.enabled,
            "Tried to enable an already enabled buff '{}'",
            self.name
        );
        self.enabled = true;
    }

    /// Disables a character-owned buff. A no-op for party buffs and shared debuffs, which live
    /// as long as the raid control.
    ///
    /// # Panics
    /// Panics if a character-owned buff is already disabled.
    pub fn disable(&mut self) {
        if self.kind.is_raid_shared() {
            return;
        }
        assert!(
            self.enabled,
            "Tried to disable an already disabled buff '{}'",
            self.name
        );
        self.enabled = false;
    }

    // --- Runtime state ---

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn charges(&self) -> u32 {
        self.current_charges
    }

    pub fn stacks(&self) -> u32 {
        self.current_stacks
    }

    /// Time the buff was last applied (from inactive).
    pub fn applied_at(&self) -> f64 {
        self.applied
    }

    /// Time the buff last expired or was cancelled.
    pub fn expired_at(&self) -> f64 {
        self.expired
    }

    /// Total active time in the current iteration (including the current activation, if it has
    /// ended; ongoing activations are counted on removal).
    pub fn uptime(&self) -> f64 {
        self.uptime
    }

    /// Seconds until the buff expires at `now`; `f64::MAX` for permanent buffs, 0 when inactive.
    pub fn time_left(&self, now: f64) -> f64 {
        if !self.active {
            return 0.0;
        }
        match self.duration() {
            None => f64::MAX,
            Some(duration) => self.refreshed + duration - now,
        }
    }

    /// The iteration a removal event must carry to be valid.
    pub fn removal_iteration(&self) -> u32 {
        self.iteration
    }

    // --- Transitions ---

    /// Applies the buff, or refreshes it if it is active. Port of `Buff::apply_buff`.
    pub fn apply(&mut self, ctx: &mut BuffContext) -> BuffApplication {
        if !self.enabled {
            return BuffApplication::NotApplied;
        }

        let now = ctx.now();
        let outcome = if self.active {
            self.add_stack();
            BuffApplication::Refreshed {
                stacks: self.current_stacks,
            }
        } else {
            let evicted = match self.claim_debuff_slot(ctx) {
                Ok(evicted) => evicted,
                Err(()) => return BuffApplication::NotApplied,
            };
            self.current_stacks = 1;
            self.applied = now;
            BuffApplication::Applied { evicted }
        };

        self.current_charges = self.base_charges;
        self.refreshed = now;
        self.active = true;
        self.schedule_removal(ctx);
        outcome
    }

    /// Refreshes an active buff (adds a stack; extends the duration per [`RefreshPolicy`]).
    /// Returns `false` if the buff is disabled or inactive. Port of `Buff::refresh_buff`.
    pub fn refresh(&mut self, ctx: &mut BuffContext) -> bool {
        if !self.enabled || !self.active {
            return false;
        }
        self.add_stack();
        if self.refresh_policy == RefreshPolicy::ExtendDuration {
            self.refreshed = ctx.now();
            self.schedule_removal(ctx);
        }
        true
    }

    /// Handles a `BuffRemoval` event. Returns `true` if the buff was removed (stale events for
    /// an earlier application are ignored): remove its aura effects. Port of `Buff::remove_buff`.
    pub fn remove(&mut self, iteration: u32, ctx: &mut BuffContext) -> bool {
        if iteration != self.iteration || !self.active {
            return false;
        }
        self.force_remove(ctx);
        true
    }

    /// Removes the buff if it is active; returns whether it was. Port of `Buff::cancel_buff`.
    pub fn cancel(&mut self, ctx: &mut BuffContext) -> bool {
        if !self.active {
            return false;
        }
        self.force_remove(ctx);
        true
    }

    /// Uses one charge, removing the buff when the last one is used. Port of `Buff::use_charge`.
    ///
    /// # Panics
    /// Panics if the buff is active without charges.
    pub fn use_charge(&mut self, ctx: &mut BuffContext) -> ChargeUse {
        if !self.active {
            return ChargeUse::Inactive;
        }
        assert!(
            self.current_charges > 0,
            "Attempted to use charge of '{}' but it has no charges",
            self.name
        );
        self.current_charges -= 1;
        if self.current_charges == 0 {
            self.force_remove(ctx);
            ChargeUse::Removed
        } else {
            ChargeUse::ChargeUsed {
                remaining: self.current_charges,
            }
        }
    }

    /// End-of-iteration reset: removes the buff if active and returns the iteration's total
    /// uptime (for the statistics of non-hidden buffs), then clears the runtime state.
    /// Port of `Buff::reset`; the caller removes aura effects if `was_active`. External buffs
    /// stay applied across iterations (the C++ kept them out of the enabled buffs).
    pub fn reset(&mut self, ctx: &mut BuffContext) -> BuffReset {
        if self.kind == BuffKind::External {
            return BuffReset {
                was_active: false,
                uptime: 0.0,
            };
        }
        let was_active = self.cancel(ctx);
        let uptime = self.uptime;
        self.initialize();
        BuffReset { was_active, uptime }
    }

    /// Clears the runtime state without touching the enabled flag. Port of `Buff::initialize`
    /// (also what `prepare_set_of_combat_iterations` does for every buff kind). A no-op for
    /// external buffs, whose aura effects are applied for as long as they are selected.
    pub fn initialize(&mut self) {
        if self.kind == BuffKind::External {
            return;
        }
        self.current_charges = 0;
        self.current_stacks = 0;
        self.iteration = 0;
        self.applied = 0.0;
        self.refreshed = 0.0;
        self.expired = 0.0;
        self.uptime = 0.0;
        self.active = false;
    }

    fn add_stack(&mut self) {
        if self.current_stacks < self.max_stacks {
            self.current_stacks += 1;
        }
    }

    /// Claims a debuff slot for debuffs; returns the evicted debuff, or `Err` if rejected.
    fn claim_debuff_slot(&self, ctx: &mut BuffContext) -> Result<Option<InstanceId>, ()> {
        if !self.kind.is_debuff() {
            return Ok(None);
        }
        let id = self.instance_id.unwrap_or_else(|| {
            panic!("Debuff '{}' has no instance id", self.name);
        });
        ctx.target.add_debuff(id, self.priority).map_err(|_| ())
    }

    fn schedule_removal(&mut self, ctx: &mut BuffContext) {
        if let Some(duration) = self.duration() {
            self.iteration += 1;
            ctx.engine.add_event_in(
                duration,
                EventKind::BuffRemoval {
                    character: ctx.character,
                    buff: ctx.buff,
                    iteration: self.iteration,
                },
            );
        }
    }

    fn force_remove(&mut self, ctx: &mut BuffContext) {
        if self.active && self.kind.is_debuff() {
            if let Some(id) = self.instance_id {
                ctx.target.remove_debuff(id);
            }
        }
        self.expired = ctx.now();
        self.active = false;
        self.current_stacks = 0;
        self.uptime += self.expired - self.applied;
    }
}

/// Outcome of [`Buff::reset`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuffReset {
    /// The buff was active and has been removed: remove its aura effects.
    pub was_active: bool,
    /// Total active time during the iteration.
    pub uptime: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Event, EventType};
    use crate::spell::dbc::{AuraType, ImplicitTarget, SpellEffectName};
    use crate::spell::overrides::SpellOverride;
    use crate::spell::record::{AuraOptions, EffectRecord};

    struct World {
        engine: Engine,
        target: Target,
    }

    impl World {
        fn new(start_at: f64) -> Self {
            let mut engine = Engine::new();
            engine.prepare_iteration(start_at);
            World {
                engine,
                target: Target::new(63),
            }
        }

        fn ctx(&mut self) -> BuffContext<'_> {
            BuffContext {
                engine: &mut self.engine,
                target: &mut self.target,
                character: CharId(0),
                buff: BuffId(4),
            }
        }

        fn advance_to(&mut self, time: f64) {
            self.engine.add_event(Event::new(
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

        fn pending_removals(&self) -> Vec<(f64, u32)> {
            let mut queue = self.engine.queue().clone();
            let mut removals = Vec::new();
            while let Some(event) = queue.pop() {
                if let EventKind::BuffRemoval { iteration, .. } = event.kind {
                    removals.push((event.time, iteration));
                }
            }
            removals
        }
    }

    fn self_buff(duration: Option<f64>, charges: u32) -> Buff {
        let mut buff = Buff::new("Flurry", None, BuffKind::SelfBuff, duration, charges);
        buff.enable();
        buff
    }

    #[test]
    fn disabled_buff_is_not_applied() {
        let mut world = World::new(0.0);
        let mut buff = Buff::new("X", None, BuffKind::SelfBuff, Some(10.0), 0);
        assert_eq!(buff.apply(&mut world.ctx()), BuffApplication::NotApplied);
        assert!(!buff.is_active());
        buff.enable();
        assert_eq!(
            buff.apply(&mut world.ctx()),
            BuffApplication::Applied { evicted: None }
        );
        assert!(buff.is_active());
    }

    #[test]
    fn apply_sets_state_and_schedules_removal() {
        let mut world = World::new(0.0);
        world.advance_to(5.0);
        let mut buff = self_buff(Some(15.0), 3);

        assert_eq!(
            buff.apply(&mut world.ctx()),
            BuffApplication::Applied { evicted: None }
        );
        assert_eq!(buff.charges(), 3);
        assert_eq!(buff.stacks(), 1);
        assert_eq!(buff.applied_at(), 5.0);
        assert_eq!(buff.time_left(10.0), 10.0);
        assert_eq!(buff.removal_iteration(), 1);
        assert_eq!(world.pending_removals(), vec![(20.0, 1)]);
        let removal = world.engine.peek().unwrap();
        assert_eq!(
            removal.kind,
            EventKind::BuffRemoval {
                character: CharId(0),
                buff: BuffId(4),
                iteration: 1
            }
        );

        // Reapplying refreshes: charges reset, new removal event, old one becomes stale.
        world.advance_to(8.0);
        buff.use_charge(&mut world.ctx());
        assert_eq!(
            buff.apply(&mut world.ctx()),
            BuffApplication::Refreshed { stacks: 1 }
        );
        assert_eq!(buff.charges(), 3);
        assert_eq!(buff.applied_at(), 5.0);
        assert_eq!(buff.time_left(8.0), 15.0);
        assert_eq!(world.pending_removals(), vec![(20.0, 1), (23.0, 2)]);

        assert!(!buff.remove(1, &mut world.ctx()));
        assert!(buff.is_active());
        world.advance_to(23.0);
        assert!(buff.remove(2, &mut world.ctx()));
        assert!(!buff.is_active());
        assert_eq!(buff.stacks(), 0);
        assert_eq!(buff.expired_at(), 23.0);
        assert_eq!(buff.uptime(), 18.0);
        assert_eq!(buff.time_left(23.0), 0.0);
        assert!(!buff.remove(2, &mut world.ctx()));
    }

    #[test]
    fn permanent_buffs_never_schedule_removal() {
        let mut world = World::new(0.0);
        let mut buff = self_buff(None, 0);
        assert!(buff.is_permanent());
        buff.apply(&mut world.ctx());
        assert!(world.engine.peek().is_none());
        assert_eq!(buff.time_left(1000.0), f64::MAX);
        assert_eq!(buff.removal_iteration(), 0);
        world.advance_to(30.0);
        assert!(buff.cancel(&mut world.ctx()));
        assert!(!buff.cancel(&mut world.ctx()));
        assert_eq!(buff.uptime(), 30.0);
    }

    #[test]
    fn charges_remove_buff_when_depleted() {
        let mut world = World::new(0.0);
        let mut buff = self_buff(Some(15.0), 3);
        assert_eq!(buff.use_charge(&mut world.ctx()), ChargeUse::Inactive);
        buff.apply(&mut world.ctx());
        assert_eq!(
            buff.use_charge(&mut world.ctx()),
            ChargeUse::ChargeUsed { remaining: 2 }
        );
        assert_eq!(
            buff.use_charge(&mut world.ctx()),
            ChargeUse::ChargeUsed { remaining: 1 }
        );
        assert_eq!(buff.use_charge(&mut world.ctx()), ChargeUse::Removed);
        assert!(!buff.is_active());
        assert_eq!(buff.charges(), 0);
        assert_eq!(buff.use_charge(&mut world.ctx()), ChargeUse::Inactive);
    }

    #[test]
    #[should_panic(expected = "no charges")]
    fn using_a_charge_of_a_chargeless_buff_panics() {
        let mut world = World::new(0.0);
        let mut buff = self_buff(Some(15.0), 0);
        buff.apply(&mut world.ctx());
        buff.use_charge(&mut world.ctx());
    }

    #[test]
    fn stacks_are_capped_at_max() {
        let mut world = World::new(0.0);
        let mut buff = self_buff(Some(10.0), 0).with_max_stacks(3);
        assert_eq!(buff.max_stacks(), 3);
        buff.apply(&mut world.ctx());
        for expected in [2, 3, 3] {
            assert_eq!(
                buff.apply(&mut world.ctx()),
                BuffApplication::Refreshed { stacks: expected }
            );
        }
        assert_eq!(
            Buff::new("x", None, BuffKind::SelfBuff, None, 0).max_stacks(),
            1
        );
    }

    #[test]
    fn refresh_policy_controls_duration_extension() {
        let mut world = World::new(0.0);
        let mut extend = self_buff(Some(10.0), 0).with_max_stacks(5);
        let mut keep = self_buff(Some(10.0), 0)
            .with_max_stacks(5)
            .with_refresh_policy(RefreshPolicy::KeepDuration);
        assert!(!extend.refresh(&mut world.ctx()));

        extend.apply(&mut world.ctx());
        keep.apply(&mut world.ctx());
        world.advance_to(4.0);
        assert!(extend.refresh(&mut world.ctx()));
        assert!(keep.refresh(&mut world.ctx()));
        assert_eq!(extend.stacks(), 2);
        assert_eq!(keep.stacks(), 2);
        assert_eq!(extend.time_left(4.0), 10.0);
        assert_eq!(keep.time_left(4.0), 6.0);
        assert_eq!(extend.removal_iteration(), 2);
        assert_eq!(keep.removal_iteration(), 1);
        assert_eq!(
            world.pending_removals(),
            vec![(10.0, 1), (10.0, 1), (14.0, 2)]
        );
    }

    #[test]
    fn debuffs_claim_target_slots() {
        let mut world = World::new(0.0);
        let mut debuff = Buff::new("Rend", None, BuffKind::UniqueDebuff, Some(21.0), 0)
            .with_priority(Priority::Mid);
        debuff.set_instance_id(InstanceId(7));
        debuff.enable();
        assert!(debuff.is_debuff());

        assert_eq!(
            debuff.apply(&mut world.ctx()),
            BuffApplication::Applied { evicted: None }
        );
        assert!(world.target.has_debuff(InstanceId(7)));
        assert_eq!(world.target.debuff_count(), 1);

        world.advance_to(21.0);
        assert!(debuff.remove(1, &mut world.ctx()));
        assert!(!world.target.has_debuff(InstanceId(7)));
        world.target.check_clean();
    }

    #[test]
    fn debuffs_report_evictions_and_rejections() {
        let mut world = World::new(0.0);
        for i in 0..Target::DEBUFF_LIMIT {
            world
                .target
                .add_debuff(InstanceId(100 + i as u32), Priority::Trash)
                .unwrap();
        }

        let mut high = Buff::new("Sunder", None, BuffKind::SharedDebuff, None, 0)
            .with_priority(Priority::High);
        high.set_instance_id(InstanceId(1));
        high.enable();
        assert_eq!(
            high.apply(&mut world.ctx()),
            BuffApplication::Applied {
                evicted: Some(InstanceId(100))
            }
        );

        let mut trash = Buff::new("Trash", None, BuffKind::UniqueDebuff, Some(5.0), 0)
            .with_priority(Priority::Trash);
        trash.set_instance_id(InstanceId(2));
        trash.enable();
        assert_eq!(trash.apply(&mut world.ctx()), BuffApplication::NotApplied);
        assert!(!trash.is_active());
        assert!(world.engine.peek().is_none());
    }

    #[test]
    #[should_panic(expected = "invalid priority")]
    fn debuff_with_invalid_priority_panics_on_apply() {
        let mut world = World::new(0.0);
        let mut debuff = Buff::new("Bad", None, BuffKind::UniqueDebuff, None, 0);
        debuff.set_instance_id(InstanceId(1));
        debuff.enable();
        debuff.apply(&mut world.ctx());
    }

    #[test]
    #[should_panic(expected = "invalid priority")]
    fn setting_an_invalid_priority_on_a_debuff_panics() {
        let _ = Buff::new("Bad", None, BuffKind::UniqueDebuff, None, 0)
            .with_priority(Priority::Invalid);
    }

    #[test]
    #[should_panic(expected = "no instance id")]
    fn debuff_without_instance_id_panics_on_apply() {
        let mut world = World::new(0.0);
        let mut debuff =
            Buff::new("Rend", None, BuffKind::UniqueDebuff, None, 0).with_priority(Priority::Low);
        debuff.enable();
        debuff.apply(&mut world.ctx());
    }

    #[test]
    fn enable_and_disable_follow_the_kind() {
        let mut buff = Buff::new("x", None, BuffKind::SelfBuff, None, 0);
        assert!(!buff.is_enabled());
        buff.enable();
        assert!(buff.is_enabled());
        buff.disable();
        assert!(!buff.is_enabled());

        let mut party = Buff::new("x", None, BuffKind::PartyBuff { party: 2 }, None, 0);
        party.enable();
        party.disable();
        assert!(party.is_enabled());
        assert_eq!(party.statistics_name(), "x (party 3)");
        assert!(party.kind().is_raid_shared());
    }

    #[test]
    #[should_panic(expected = "already enabled")]
    fn enabling_twice_panics() {
        let mut buff = Buff::new("x", None, BuffKind::SelfBuff, None, 0);
        buff.enable();
        buff.enable();
    }

    #[test]
    #[should_panic(expected = "already disabled")]
    fn disabling_twice_panics() {
        let mut buff = Buff::new("x", None, BuffKind::SelfBuff, None, 0);
        buff.disable();
    }

    #[test]
    fn reset_reports_uptime_and_clears_state() {
        let mut world = World::new(0.0);
        let mut buff = self_buff(Some(10.0), 2);
        buff.apply(&mut world.ctx());
        world.advance_to(10.0);
        assert!(buff.remove(1, &mut world.ctx()));
        world.advance_to(12.0);
        buff.apply(&mut world.ctx());
        world.advance_to(15.0);

        assert_eq!(
            buff.reset(&mut world.ctx()),
            BuffReset {
                was_active: true,
                uptime: 13.0
            }
        );
        assert!(!buff.is_active());
        assert_eq!(buff.uptime(), 0.0);
        assert_eq!(buff.charges(), 0);
        assert_eq!(buff.removal_iteration(), 0);
        assert!(buff.is_enabled());
        assert_eq!(
            buff.reset(&mut world.ctx()),
            BuffReset {
                was_active: false,
                uptime: 0.0
            }
        );
    }

    fn aura(index: u32, aura: AuraType, points: f32, target: ImplicitTarget) -> EffectRecord {
        let mut effect = EffectRecord::new(index, SpellEffectName::ApplyAura);
        effect.aura = aura;
        effect.base_points = points;
        effect.implicit_target = [target, ImplicitTarget::None];
        effect
    }

    #[test]
    fn buffs_are_built_from_spell_records() {
        let mut shout = SpellRecord::new(5242, "Battle Shout");
        shout.duration_ms = Some(180_000);
        shout.effects.push(aura(
            0,
            AuraType::ModAttackPower,
            21.0,
            ImplicitTarget::UnitCasterAreaParty,
        ));
        let overrides = Overrides::new();
        let kind = BuffKind::from_record(&shout, 3, None).unwrap();
        assert_eq!(kind, BuffKind::PartyBuff { party: 3 });
        let buff = Buff::from_record(&shout, kind, &overrides);
        assert_eq!(buff.name(), "Battle Shout");
        assert_eq!(buff.canonical_name(), "Battle Shout (5242)");
        assert_eq!(buff.spell(), 5242);
        assert_eq!(buff.duration(), Some(180.0));
        assert_eq!(buff.max_stacks(), 1);
        assert_eq!(buff.effects.len(), 1);
        assert_eq!(buff.effects[0].aura(), AuraType::ModAttackPower);
        assert_eq!(buff.effects[0].value(), 21.0);
        assert!(!buff.is_hidden());
        assert!(!buff.is_enabled());
        assert!(buff.charge_sources().is_empty());

        let mut sunder = SpellRecord::new(11597, "Sunder Armor");
        sunder.duration_ms = Some(30_000);
        sunder.aura_options = AuraOptions {
            max_stacks: 5,
            ..AuraOptions::default()
        };
        sunder.effects.push(aura(
            0,
            AuraType::ModResistance,
            -450.0,
            ImplicitTarget::UnitTargetEnemy,
        ));
        sunder
            .effects
            .push(EffectRecord::new(1, SpellEffectName::Threat));
        let kind = BuffKind::from_record(&sunder, 0, None).unwrap();
        assert_eq!(
            kind,
            BuffKind::SharedDebuff,
            "stacking debuffs are raid-wide"
        );
        assert_eq!(
            BuffKind::from_record(&sunder, 0, Some(false)),
            Some(BuffKind::UniqueDebuff)
        );
        let mut overrides = Overrides::new();
        let mut spell_override = SpellOverride::new(11597);
        spell_override.debuff_priority = Some(Priority::High);
        overrides.add(spell_override).unwrap();
        let buff = Buff::from_record(&sunder, kind, &overrides);
        assert_eq!(buff.priority(), Priority::High);
        assert_eq!(buff.max_stacks(), 5);
        assert!(buff.is_debuff());
        assert_eq!(buff.effects.len(), 1, "direct effects stay with the spell");

        let mut rend = SpellRecord::new(11574, "Rend");
        rend.duration_ms = Some(21_000);
        rend.effects.push(aura(
            0,
            AuraType::PeriodicDamage,
            21.0,
            ImplicitTarget::UnitTargetEnemy,
        ));
        let kind = BuffKind::from_record(&rend, 0, None).unwrap();
        assert_eq!(kind, BuffKind::UniqueDebuff);
        assert_eq!(
            Buff::from_record(&rend, kind, &Overrides::new()).priority(),
            Priority::Mid
        );

        let mut flurry = SpellRecord::new(12966, "Flurry");
        flurry.attributes[0] = 0x40000;
        flurry.duration_ms = Some(15_000);
        flurry.aura_options = AuraOptions {
            proc_charges: 3,
            proc_chance: 100,
            proc_type_mask: crate::spell::dbc::ProcFlags::DEAL_MELEE_SWING,
            ..AuraOptions::default()
        };
        flurry.effects.push(aura(
            0,
            AuraType::ModMeleeHaste3,
            30.0,
            ImplicitTarget::UnitCaster,
        ));
        let kind = BuffKind::from_record(&flurry, 0, None).unwrap();
        assert_eq!(kind, BuffKind::SelfBuff);
        let buff = Buff::from_record(&flurry, kind, &Overrides::new());
        assert_eq!(buff.base_charges(), 3);
        assert_eq!(
            buff.charge_sources(),
            [ProcSource::MainhandSwing, ProcSource::OffhandSwing]
        );
        assert!(buff.consumes_charge_on(ProcSource::OffhandSwing));
        assert!(!buff.consumes_charge_on(ProcSource::MeleeCritical));

        let mut stance = SpellRecord::new(2458, "Berserker Stance");
        stance.duration_ms = Some(-1);
        stance.attributes[0] = 0x80;
        stance.effects.push(aura(
            0,
            AuraType::ModShapeshift,
            0.0,
            ImplicitTarget::UnitCaster,
        ));
        let buff = Buff::from_record(&stance, BuffKind::SelfBuff, &Overrides::new());
        assert!(buff.is_permanent());
        assert!(buff.is_hidden());

        let plain = SpellRecord::new(78, "Heroic Strike");
        assert_eq!(BuffKind::from_record(&plain, 0, None), None);
    }

    #[test]
    fn duration_talents_scale_the_base_duration() {
        let mut world = World::new(0.0);
        let mut buff = self_buff(Some(120.0), 0);
        buff.increase_duration_percent(10);
        assert_eq!(buff.duration(), Some(132.0));
        buff.apply(&mut world.ctx());
        assert_eq!(buff.time_left(0.0), 132.0);
        assert_eq!(world.pending_removals(), vec![(132.0, 1)]);
        buff.decrease_duration_percent(10);
        assert_eq!(buff.duration(), Some(120.0));
        assert!(Buff::new("x", None, BuffKind::SelfBuff, None, 0)
            .duration()
            .is_none());
    }

    #[test]
    fn removal_events_use_the_context_handles() {
        let mut world = World::new(-3.0);
        let mut buff = self_buff(Some(2.0), 0);
        buff.apply(&mut world.ctx());
        let event = world.engine.next_event().unwrap();
        assert_eq!(event.time, -1.0);
        assert_eq!(event.kind.event_type(), EventType::BuffRemoval);
    }
}
