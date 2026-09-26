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
//! An item's chance-on-hit spell (`ItemEffect` trigger 2, Thunderfury) is a proc of its own
//! kind ([`Proc::on_hit`]): the spell is the payload itself, cast at the target when the
//! wielding hand lands a hit, at the rate the overrides give (the server's item data).
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
use crate::spell::record::EquippedItems;
use crate::spell::{CastReport, Hand, Spell, SpellHost, SpellResult};

/// Rolls are out of 10 000 (100 = 1%).
pub const PROC_ROLL_RANGE: u32 = 10_000;

/// What a proc needs from the world beyond what its spell needs.
pub trait ProcHost: SpellHost {
    /// Base speed of the weapon in `hand`, without haste; `None` when the hand is empty.
    fn base_weapon_speed(&self, hand: Hand) -> Option<f64>;

    /// Whether the weapon in `hand` satisfies a `SpellEquippedItems` requirement; `false` when
    /// the hand is empty.
    fn hand_weapon_matches(&self, hand: Hand, requirement: &EquippedItems) -> bool;

    /// The current value of effect `effect` of spell `spell`'s aura (a talent's rank value),
    /// `None` when the character does not have the spell.
    fn aura_effect_value(&self, spell: u32, effect: u32) -> Option<f64>;
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
    /// `PROC_TRIGGER_SPELL` (or a `TRIGGER_SPELL` script): cast `spell`, with the aura's value
    /// as trigger value when the aura carries one.
    Trigger { spell: u32, value: Option<f64> },
    /// `TRIGGER_WITH_VALUE`: set effect `effect` of `spell` to the aura's value, then cast it.
    TriggerWithValue { spell: u32, effect: u32, value: f64 },
}

/// What a proc's spell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcKind {
    /// A passive whose aura effects are the payloads (`PROC_TRIGGER_SPELL`, scripted `DUMMY`);
    /// it only fires while its aura is up.
    Aura,
    /// An item's chance-on-hit spell: the proc casts the spell itself.
    OnHit,
}

/// A passive spell with a proc chance. Port of `Proc` / `ProcPPM`.
#[derive(Debug, Clone)]
pub struct Proc {
    spell: Spell,
    kind: ProcKind,
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
        let sources = Self::sources_of(&spell);
        Self::build(spell, sources, seed)
    }

    /// The sources a passive proc spell listens to: its `ProcTypeMask` and hit mask, narrowed
    /// to the override's hand. Empty for a proc on events the sim does not have (a killing
    /// blow).
    ///
    /// # Panics
    /// Panics if the spell is not passive.
    pub fn sources_of(spell: &Spell) -> Vec<ProcSource> {
        let mut sources = Self::record_sources(spell);
        if let Some(hand) = spell.setup().overrides.proc.and_then(|p| p.hand) {
            sources.retain(|source| source.hand() == hand);
        }
        sources
    }

    /// Builds the proc an equipped item or enchant grants: the record says what the proc does
    /// and how often, the equipment slot which attacks trigger it (`allowed`, from
    /// [`crate::item::EquipmentSlot::default_proc_sources`] or the spec's own flags), so that a
    /// weapon enchant only procs off its own hand. Port of the `EnchantProc` constructor and
    /// `Item::add_default_proc_sources`. Returns `None` when the record and the slot have no
    /// trigger in common (a main-hand only proc on a trinket).
    pub fn for_equipment(spell: Spell, allowed: &[ProcSource], seed: u64) -> Option<Self> {
        let mut sources = Self::record_sources(&spell);
        sources.retain(|source| allowed.contains(source));
        if sources.is_empty() {
            return None;
        }
        Some(Self::build(spell, sources, seed))
    }

    /// Builds the proc of an item's chance-on-hit spell (`ItemEffect` trigger 2): the spell is
    /// the payload, cast at the target when one of `allowed` (the wielding hand's landed
    /// swings and abilities) fires. The rate is the override's chance or procs per minute, else
    /// the record's procs per minute or a `ProcChance` of 1–100 %. Returns `None` when the rate
    /// is unknown — the payload's `ProcChance` 101 means "handled by the effect", the server's
    /// item data holds the real rate — or when `allowed` is empty.
    pub fn on_hit(spell: Spell, allowed: &[ProcSource], seed: u64) -> Option<Self> {
        let record = spell.record();
        let table_chance = (1..=100).contains(&record.aura_options.proc_chance);
        let override_rate = spell.setup().overrides.proc.is_some_and(|p| p.has_rate());
        if !(override_rate || record.aura_options.ppm > 0.0 || table_chance) || allowed.is_empty() {
            return None;
        }
        let mut proc = Self::build(spell, allowed.to_vec(), seed);
        proc.kind = ProcKind::OnHit;
        Some(proc)
    }

    /// The sources the record's `ProcTypeMask` and hit mask name.
    fn record_sources(spell: &Spell) -> Vec<ProcSource> {
        let record = spell.record();
        assert!(spell.is_passive(), "{} is not a passive spell", record.name);
        ProcSource::from_masks(record.aura_options.proc_type_mask, spell.setup().hit_mask)
    }

    fn build(spell: Spell, sources: Vec<ProcSource>, seed: u64) -> Self {
        let record = spell.record();
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
        let override_ppm = spell.setup().overrides.proc.and_then(|p| p.ppm);
        let rate = match override_ppm {
            Some(ppm) => ProcRate::Ppm(ppm),
            None if record.aura_options.ppm > 0.0 => {
                ProcRate::Ppm(f64::from(record.aura_options.ppm))
            }
            None => ProcRate::Chance,
        };
        Proc {
            spell,
            kind: ProcKind::Aura,
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

    pub fn kind(&self) -> ProcKind {
        self.kind
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
                // A hidden aura enabled by another aura fires with that aura's value.
                let enabled_value = self
                    .spell
                    .setup()
                    .enabled_by
                    .filter(|e| e.target_effect.is_none())
                    .map(|e| host.aura_effect_value(e.spell, e.effect).unwrap_or(0.0));
                let chance = self.spell.proc_chance_with(host, enabled_value);
                (chance * f64::from(PROC_ROLL_RANGE)).round() as u32
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
    /// requirements of the record gate the aura), a weapon requirement holds for the hand of
    /// the triggering attack (a sword-only proc never fires off an off-hand axe), one rank of
    /// the override's `target_aura` is up (Bloodthrill: your Rend on the target) and, for a PPM
    /// proc, the triggering hand holds a weapon. Port of the
    /// `proc_specific_conditions_fulfilled` overrides.
    pub fn conditions_fulfilled(&self, source: ProcSource, host: &impl ProcHost) -> bool {
        // An on-hit spell's marker buff is what it applies (Thunderfury's debuff), not a
        // condition.
        if let (ProcKind::Aura, Some(id)) = (self.kind, self.spell.marker_buff()) {
            if !host.buff(id).is_active() {
                return false;
            }
        }
        let hand_source = matches!(
            source,
            ProcSource::MainhandSwing
                | ProcSource::OffhandSwing
                | ProcSource::MainhandSpell
                | ProcSource::OffhandSpell
        );
        if let Some(items) = &self.spell.record().equipped_items {
            if hand_source
                && items.class == EquippedItems::WEAPON
                && !host.hand_weapon_matches(source.hand(), items)
            {
                return false;
            }
        }
        let target_auras = &self.spell.setup().target_aura_ranks;
        if !target_auras.is_empty() && !target_auras.iter().any(|&id| host.aura_active(id)) {
            return false;
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
                        value: Some(effect.effective_value(host)),
                    });
                }
                // A `PROC_TRIGGER_SPELL` without a trigger spell (Windfury Totem's party aura
                // keeps the payload id in its base points) is scripted like a `DUMMY` aura.
                if effect.aura() != AuraType::Dummy && !record.is_proc_trigger() {
                    return None;
                }
                match effect.script_kind()? {
                    ScriptKind::TriggerWithValue => {
                        let params = &effect.script()?.params;
                        Some(Payload::TriggerWithValue {
                            spell: params.spell?,
                            effect: params.effect?,
                            value: effect.effective_value(host),
                        })
                    }
                    // The server-side script: the value is the payload's id, not a number the
                    // payload wants.
                    ScriptKind::TriggerSpell => Some(Payload::Trigger {
                        spell: effect.script()?.params.spell?,
                        value: None,
                    }),
                    _ => None,
                }
            })
            .collect()
    }

    /// Performs the proc: casts the payloads of its aura effects and, if the passive has direct
    /// effects of its own, performs the spell. The internal cooldown starts. Port of
    /// `Proc::spell_effect`.
    pub fn perform(&mut self, host: &mut impl ProcHost) -> CastReport {
        self.procs += 1;
        if self.kind == ProcKind::OnHit {
            return self.spell.perform_triggered(host);
        }
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
                    if let Some(triggered) = host.trigger_spell(spell, value) {
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

    /// Re-seeds every proc's roll, in registration order, with the seeds `next` hands out.
    pub fn reseed(&mut self, mut next: impl FnMut() -> u64) {
        for proc in &mut self.procs {
            proc.set_seed(next());
        }
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
