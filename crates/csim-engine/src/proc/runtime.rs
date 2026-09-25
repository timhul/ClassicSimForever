//! The proc runtime. Port of `Spells/Proc.*`, `Spells/ProcPPM.*` and `Character/EnabledProcs.*`
//! on the client table model.
//!
//! A [`Proc`] wraps a passive [`Spell`] whose `SpellAuraOptions.ProcTypeMask` says which events it
//! reacts to (the C++ `Proc : Spell` inheritance as composition) and adds the proc roll: the
//! record's `ProcChance` or its `SpellProcsPerMinute` rate based on the triggering weapon's speed.
//! The passive's aura effects are its payload: a `PROC_TRIGGER_SPELL` aura casts its trigger spell
//! with the aura's value (Deep Wounds → 12162 with the bleed percent), a `DUMMY` aura with a
//! `TRIGGER_WITH_VALUE` script hands its value to another spell's effect before casting it
//! (Flurry's rank into its haste buff). The passive's conditions (`SpellEquippedItems`,
//! `SpellShapeshift`) are the conditions of its aura: the proc only fires while the aura is up.
//! [`EnabledProcs`] owns the procs of one character and runs the proc checks with the C++
//! re-entrancy guard.
//!
//! Deviation from C++: the internal cooldown (`ProcCategoryRecovery`, the spell's cooldown
//! control) is enforced — the C++ `EnabledProcs::run_proc_check` performed a proc without
//! checking its cooldown control.

use std::collections::HashSet;

use crate::ids::ProcId;
use crate::proc::ProcSource;
use crate::rng::Random;
use crate::spell::dbc::{AuraType, SpellModOp};
use crate::spell::overrides::ScriptKind;
use crate::spell::{CastReport, Hand, Spell, SpellHost, SpellResult};

/// Rolls are out of 10 000 (100 = 1%).
pub const PROC_ROLL_RANGE: u32 = 10_000;

/// What a proc needs from the world beyond what its spell needs.
pub trait ProcHost: SpellHost {
    /// Base speed of the weapon in `hand`, without haste; `None` when the hand is empty.
    fn base_weapon_speed(&self, hand: Hand) -> Option<f64>;
}

/// How often a proc fires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcRate {
    /// `SpellAuraOptions.ProcChance`, read from the spell at roll time (modifiers apply).
    Chance,
    /// `SpellProcsPerMinute.BaseProcRate`: chance = ppm × speed of the triggering hand / 60.
    Ppm(f64),
}

/// One payload of a proc: what its aura effects do when it fires.
#[derive(Debug, Clone, PartialEq)]
enum Payload {
    /// `PROC_TRIGGER_SPELL`: cast `spell` with the aura's value as trigger value.
    Trigger { spell: u32, value: f64 },
    /// `TRIGGER_WITH_VALUE`: set effect `effect` of `spell` to the aura's value, then cast it.
    TriggerWithValue { spell: u32, effect: u32, value: f64 },
}

/// A passive spell with a proc chance. Port of `Proc` / `ProcPPM`.
#[derive(Debug, Clone)]
pub struct Proc {
    spell: Spell,
    rate: ProcRate,
    sources: Vec<ProcSource>,
    random: Random,
    current_source: Option<ProcSource>,
    attempts: u32,
    procs: u32,
}

impl Proc {
    /// Builds the proc for a passive spell: its sources from `ProcTypeMask` and the override's
    /// hit mask (narrowed to the override's hand, if any), its rate from `ProcChance` /
    /// `SpellProcsPerMinute`.
    ///
    /// # Panics
    /// Panics if the spell is not passive, has no proc sources, or has neither effects nor a
    /// marker buff (the C++ constructor checks).
    pub fn new(spell: Spell, seed: u64) -> Self {
        let record = spell.record();
        assert!(spell.is_passive(), "{} is not a passive spell", record.name);
        let mut sources =
            ProcSource::from_masks(record.aura_options.proc_type_mask, spell.setup().hit_mask);
        if let Some(hand) = spell.setup().overrides.proc.and_then(|p| p.hand) {
            sources.retain(|source| source.hand() == hand);
        }
        assert!(
            !sources.is_empty(),
            "Proc {} ({}) has no proc sources",
            record.name,
            record.id
        );
        assert!(
            !spell.effects().is_empty() || spell.marker_buff().is_some(),
            "No effects or marker buff for proc {} ({})",
            record.name,
            record.id
        );
        let rate = if record.aura_options.ppm > 0.0 {
            ProcRate::Ppm(f64::from(record.aura_options.ppm))
        } else {
            ProcRate::Chance
        };
        Proc {
            spell,
            rate,
            sources,
            random: Random::from_seed(0, PROC_ROLL_RANGE, seed),
            current_source: None,
            attempts: 0,
            procs: 0,
        }
    }

    pub fn spell(&self) -> &Spell {
        &self.spell
    }

    pub fn spell_mut(&mut self) -> &mut Spell {
        &mut self.spell
    }

    pub fn name(&self) -> &str {
        self.spell.name()
    }

    /// The spell's game id.
    pub fn game_id(&self) -> u32 {
        self.spell.game_id()
    }

    pub fn rate(&self) -> ProcRate {
        self.rate
    }

    pub fn sources(&self) -> &[ProcSource] {
        &self.sources
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.random.set_gen_from_seed(seed);
    }

    /// The source that triggered the current proc check.
    pub fn current_source(&self) -> Option<ProcSource> {
        self.current_source
    }

    /// Proc attempts in the current iteration set.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Successful procs in the current iteration set.
    pub fn procs(&self) -> u32 {
        self.procs
    }

    pub fn procs_from_source(&self, source: ProcSource) -> bool {
        self.sources.contains(&source)
    }

    /// The proc chance out of [`PROC_ROLL_RANGE`] for an event from `source`. Port of
    /// `Proc::get_proc_range` / `ProcPPM::get_proc_range`.
    pub fn proc_range(&self, source: ProcSource, host: &impl ProcHost) -> u32 {
        match self.rate {
            ProcRate::Chance => {
                (self.spell.proc_chance(host) * f64::from(PROC_ROLL_RANGE)).round() as u32
            }
            ProcRate::Ppm(ppm) => {
                let ppm = host.spell_modifiers().apply(
                    self.spell.record().class_options.as_ref(),
                    SpellModOp::ProcFrequency,
                    ppm,
                );
                let speed = host.base_weapon_speed(source.hand()).unwrap_or(0.0);
                (ppm * 100.0 / 60.0 * speed * 100.0).round() as u32
            }
        }
    }

    /// Whether the passive's conditions hold: its aura is up (the equipment and stance
    /// requirements of the record gate the aura) and, for a PPM proc, the triggering hand holds
    /// a weapon. Port of the `proc_specific_conditions_fulfilled` overrides.
    pub fn conditions_fulfilled(&self, source: ProcSource, host: &impl ProcHost) -> bool {
        if let Some(id) = self.spell.marker_buff() {
            if !host.buff(id).is_active() {
                return false;
            }
        }
        if let ProcRate::Ppm(_) = self.rate {
            if host.base_weapon_speed(source.hand()).is_none() {
                return false;
            }
        }
        true
    }

    /// Whether the proc is off its internal cooldown.
    pub fn is_ready(&self, host: &impl ProcHost) -> bool {
        self.spell.cooldown_remaining(host) <= 0.0
    }

    /// Rolls for the proc. Port of `Proc::check_proc_success` (plus the cooldown check).
    pub fn check_proc_success(&mut self, source: ProcSource, host: &impl ProcHost) -> bool {
        self.current_source = Some(source);
        self.attempts += 1;
        if !self.is_ready(host) {
            return false;
        }
        let range = self.proc_range(source, host);
        self.random.get_roll() < range && self.conditions_fulfilled(source, host)
    }

    /// The payloads of the aura effects, read from the buff so talent rank values apply.
    fn payloads(&self, host: &impl ProcHost) -> Vec<Payload> {
        let Some(id) = self.spell.marker_buff() else {
            return Vec::new();
        };
        host.buff(id)
            .effects
            .iter()
            .filter_map(|effect| {
                let record = effect.record();
                if record.is_proc_trigger() && record.trigger_spell != 0 {
                    return Some(Payload::Trigger {
                        spell: record.trigger_spell,
                        value: effect.effective_value(host),
                    });
                }
                if effect.aura() == AuraType::Dummy
                    && effect.script_kind() == Some(ScriptKind::TriggerWithValue)
                {
                    let params = &effect.script()?.params;
                    return Some(Payload::TriggerWithValue {
                        spell: params.spell?,
                        effect: params.effect?,
                        value: effect.effective_value(host),
                    });
                }
                None
            })
            .collect()
    }

    /// Performs the proc: casts the payloads of its aura effects and, if the passive has direct
    /// effects of its own, performs the spell. The internal cooldown starts. Port of
    /// `Proc::spell_effect`.
    pub fn perform(&mut self, host: &mut impl ProcHost) -> CastReport {
        self.procs += 1;
        let mut report = if self.spell.effects().is_empty() {
            self.spell.start_cooldown(host);
            CastReport {
                result: SpellResult::Success,
                ..CastReport::default()
            }
        } else {
            self.spell.perform(host)
        };
        for payload in self.payloads(host) {
            match payload {
                Payload::Trigger { spell, value } => {
                    if let Some(triggered) = host.trigger_spell(spell, Some(value)) {
                        report.triggered.push((spell, triggered));
                    }
                }
                Payload::TriggerWithValue {
                    spell,
                    effect,
                    value,
                } => {
                    host.set_spell_effect_value(spell, effect, value);
                    if let Some(triggered) = host.trigger_spell(spell, None) {
                        report.triggered.push((spell, triggered));
                    }
                }
            }
        }
        report
    }

    /// Clears the statistics counters. Port of `Proc::prepare_set_of_combat_iterations`.
    pub fn prepare_set_of_combat_iterations(&mut self) {
        self.attempts = 0;
        self.procs = 0;
    }

    pub fn reset(&mut self, host: &mut impl ProcHost) {
        self.spell.reset(host);
        self.current_source = None;
    }
}

/// The procs of one character. Port of `EnabledProcs`.
#[derive(Debug, Clone, Default)]
pub struct EnabledProcs {
    procs: Vec<Proc>,
    enabled: Vec<ProcId>,
    procced: HashSet<ProcId>,
    checks_in_progress: u32,
    /// Checks left open by [`Self::hold_check`] for the extra attacks they granted.
    held_checks: u32,
}

impl EnabledProcs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a proc (not yet enabled). Port of the proc creation in `CharacterSpells`.
    pub fn add_proc(&mut self, proc: Proc) -> ProcId {
        let id = ProcId(u32::try_from(self.procs.len()).expect("too many procs"));
        self.procs.push(proc);
        id
    }

    pub fn get(&self, id: ProcId) -> &Proc {
        &self.procs[id.index()]
    }

    pub fn get_mut(&mut self, id: ProcId) -> &mut Proc {
        &mut self.procs[id.index()]
    }

    pub fn procs(&self) -> &[Proc] {
        &self.procs
    }

    pub fn ids(&self) -> impl Iterator<Item = ProcId> {
        (0..self.procs.len()).map(|index| ProcId(index as u32))
    }

    pub fn find_by_name(&self, name: &str) -> Option<ProcId> {
        self.procs
            .iter()
            .position(|proc| proc.name() == name)
            .map(|index| ProcId(index as u32))
    }

    pub fn find_by_game_id(&self, spell: u32) -> Option<ProcId> {
        self.procs
            .iter()
            .position(|proc| proc.game_id() == spell)
            .map(|index| ProcId(index as u32))
    }

    pub fn is_enabled(&self, id: ProcId) -> bool {
        self.enabled.contains(&id)
    }

    pub fn enabled(&self) -> &[ProcId] {
        &self.enabled
    }

    /// Enables the proc's spell (its aura goes up) and adds it to the proc checks. Port of
    /// `Proc::enable_proc` via `Spell::enable`.
    pub fn enable(&mut self, id: ProcId, host: &mut impl ProcHost) {
        if !self.procs[id.index()].spell.is_enabled() {
            self.procs[id.index()].spell.enable(host);
        }
        if !self.enabled.contains(&id) {
            self.enabled.push(id);
        }
    }

    /// Disables the proc's spell and removes it from the proc checks. Port of
    /// `Proc::disable_proc`.
    pub fn disable(&mut self, id: ProcId, host: &mut impl ProcHost) {
        self.procs[id.index()].spell.disable(host);
        self.enabled.retain(|enabled| *enabled != id);
    }

    /// Excludes `id` from the next proc check (a proc that just fired and must not re-trigger
    /// itself). Port of `ignore_proc_in_next_proc_check`.
    pub fn ignore_in_next_check(&mut self, id: ProcId) {
        self.procced.insert(id);
    }

    /// Opens a check scope: the procs that fire inside it stay excluded from re-firing until
    /// the outermost scope is closed (the C++ `procs_in_progress` nesting guard, which held
    /// for the extra attacks a proc performed inside its own `perform`).
    pub fn begin_check(&mut self) {
        self.checks_in_progress += 1;
    }

    /// Closes a check scope opened by [`Self::begin_check`].
    pub fn end_check(&mut self) {
        self.checks_in_progress -= 1;
        if self.checks_in_progress == 0 {
            self.procced.clear();
        }
    }

    /// Keeps the current check scope open for the extra attacks its procs granted: the chain
    /// of extra attacks belongs to the check, so a proc fires at most once in it (Windfury
    /// never procs off its own extra attack, nor twice off a Sword Specialization chain).
    /// Closed by [`Self::release_held_checks`] once the extra attacks were performed.
    pub fn hold_check(&mut self) {
        self.held_checks += 1;
    }

    /// Closes the check scopes held for extra attacks.
    pub fn release_held_checks(&mut self) {
        while self.held_checks > 0 {
            self.held_checks -= 1;
            self.end_check();
        }
    }

    /// Runs the proc check for `source`: every enabled proc listening to it that has not
    /// already procced in this (possibly nested) check rolls and fires. Proc sources produced
    /// by the procs themselves (and by the spells they trigger) are checked recursively.
    /// Returns the cast reports of the procs that fired, for the statistics. Port of
    /// `run_proc_check`.
    ///
    /// # Panics
    /// Panics for `ProcSource::Manual`.
    pub fn run_proc_check(
        &mut self,
        source: ProcSource,
        host: &mut impl ProcHost,
    ) -> Vec<(ProcId, CastReport)> {
        assert!(
            source != ProcSource::Manual,
            "Cannot run proc effects on manually triggered proc"
        );
        self.begin_check();
        let mut reports = Vec::new();

        let candidates = self.enabled.clone();
        for id in candidates {
            if !self.procs[id.index()].procs_from_source(source) || self.procced.contains(&id) {
                continue;
            }
            if !self.procs[id.index()].check_proc_success(source, host) {
                continue;
            }
            self.procced.insert(id);
            self.fire(id, host, &mut reports);
        }

        self.end_check();
        reports
    }

    /// Performs `id`, then the proc sources it and its triggered spells produced.
    fn fire(
        &mut self,
        id: ProcId,
        host: &mut impl ProcHost,
        reports: &mut Vec<(ProcId, CastReport)>,
    ) {
        let report = self.procs[id.index()].perform(host);
        let nested = report.all_proc_sources();
        reports.push((id, report));
        for nested_source in nested {
            reports.extend(self.run_proc_check(nested_source, host));
        }
    }

    /// Start of an iteration. Port of `EnabledProcs::reset`.
    pub fn reset(&mut self, host: &mut impl ProcHost) {
        for proc in &mut self.procs {
            proc.reset(host);
        }
        self.procced.clear();
        self.checks_in_progress = 0;
        self.held_checks = 0;
    }

    pub fn prepare_set_of_combat_iterations(&mut self) {
        for proc in &mut self.procs {
            proc.prepare_set_of_combat_iterations();
        }
    }

    /// Disables every proc. Port of `EnabledProcs::clear_all`.
    pub fn clear_all(&mut self, host: &mut impl ProcHost) {
        let enabled = self.enabled.clone();
        for id in enabled {
            self.disable(id, host);
        }
    }
}

#[cfg(test)]
mod tests;
