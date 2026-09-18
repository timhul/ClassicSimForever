//! The proc runtime. Port of `Spells/Proc.*`, `Spells/ProcPPM.*` and `Character/EnabledProcs.*`.
//!
//! A [`Proc`] wraps a passive [`Spell`] (the C++ `Proc : Spell` inheritance as composition) and
//! adds the proc roll: a flat chance or a PPM rate based on a weapon's speed, the sources it
//! triggers on, and the data-driven equivalents of the C++ `proc_specific_conditions_fulfilled`
//! overrides (weapon type on the triggering side, a required buff). [`EnabledProcs`] owns the
//! procs of one character and runs the proc checks with the C++ re-entrancy guard.
//!
//! Deviation from C++: the internal cooldown (the spell group's `cooldown`) is enforced — the
//! C++ `EnabledProcs::run_proc_check` performed a proc without checking its cooldown control.

use std::collections::HashSet;

use crate::ids::ProcId;
use crate::item::WeaponType;
use crate::proc::ProcSource;
use crate::rng::Random;
use crate::spell::{CastReport, Hand, ProcSpec, Spell, SpellHost};

/// Rolls are out of 10 000 (100 = 1%).
pub const PROC_ROLL_RANGE: u32 = 10_000;

/// What a proc needs from the world beyond what its spell needs.
pub trait ProcHost: SpellHost {
    fn weapon_type(&self, hand: Hand) -> Option<WeaponType>;
    /// Base speed of the weapon in `hand`, without haste.
    fn base_weapon_speed(&self, hand: Hand) -> Option<f64>;
}

/// How often a proc fires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcRate {
    /// Fraction per eligible event.
    Flat(f64),
    /// Procs per minute of the weapon in `hand`: chance = ppm × speed / 60.
    Ppm { ppm: f64, hand: Hand },
}

/// A passive spell with a proc chance. Port of `Proc` / `ProcPPM`.
#[derive(Debug, Clone)]
pub struct Proc {
    spell: Spell,
    rate: ProcRate,
    sources: Vec<ProcSource>,
    requires_weapon_type: Vec<WeaponType>,
    requires_buff: Option<String>,
    /// Procs performed without their own roll whenever this one procs.
    linked: Vec<ProcId>,
    random: Random,
    current_source: Option<ProcSource>,
    attempts: u32,
    procs: u32,
}

impl Proc {
    /// Builds the proc for a passive spell from its group's flags and `proc` spec.
    ///
    /// # Panics
    /// Panics if the spell is not passive, has no proc sources, or has neither effects nor a
    /// marker buff (the C++ constructor checks).
    pub fn new(spell: Spell, seed: u64) -> Self {
        let group = spell.group();
        assert!(spell.is_passive(), "{} is not a passive spell", group.name);
        let sources = group.proc_sources();
        assert!(
            !sources.is_empty(),
            "Proc {} has no proc sources",
            group.name
        );
        assert!(
            !spell.effects().is_empty() || spell.marker_buff().is_some(),
            "No effects or marker buff for proc {}",
            group.name
        );
        let spec = group.proc.clone().unwrap_or_else(|| ProcSpec {
            rate: 1.0,
            ppm: false,
            hand: None,
            sources: Vec::new(),
            requires_weapon_type: Vec::new(),
            requires_buff: None,
            linked: Vec::new(),
        });
        let rate = if spec.ppm {
            ProcRate::Ppm {
                ppm: spec.rate,
                hand: spec
                    .hand
                    .expect("PPM procs need a hand (validated at load)"),
            }
        } else {
            ProcRate::Flat(spec.rate)
        };
        Proc {
            spell,
            rate,
            sources,
            requires_weapon_type: spec.requires_weapon_type,
            requires_buff: spec.requires_buff,
            linked: Vec::new(),
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

    pub fn rate(&self) -> ProcRate {
        self.rate
    }

    pub fn sources(&self) -> &[ProcSource] {
        &self.sources
    }

    pub fn linked(&self) -> &[ProcId] {
        &self.linked
    }

    /// Links `proc` so it is performed whenever this one procs.
    pub fn add_linked(&mut self, proc: ProcId) {
        if !self.linked.contains(&proc) {
            self.linked.push(proc);
        }
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

    /// The proc chance out of [`PROC_ROLL_RANGE`]. A talent `set_proc_rate` overrides the spec
    /// rate. Port of `Proc::get_proc_range` / `ProcPPM::get_proc_range`.
    pub fn proc_range(&self, host: &impl ProcHost) -> u32 {
        if let Some(rate) = self.spell.proc_rate() {
            return (rate * f64::from(PROC_ROLL_RANGE)).round() as u32;
        }
        match self.rate {
            ProcRate::Flat(rate) => (rate * f64::from(PROC_ROLL_RANGE)).round() as u32,
            ProcRate::Ppm { ppm, hand } => {
                let speed = host.base_weapon_speed(hand).unwrap_or(0.0);
                (ppm * 100.0 / 60.0 * speed * 100.0).round() as u32
            }
        }
    }

    /// Whether the data-driven conditions hold for a proc triggered by `source`. Port of the
    /// `proc_specific_conditions_fulfilled` overrides.
    pub fn conditions_fulfilled(&self, source: ProcSource, host: &impl ProcHost) -> bool {
        if !self.requires_weapon_type.is_empty() {
            let hand = match source {
                ProcSource::OffhandSwing => Hand::Offhand,
                _ => Hand::Mainhand,
            };
            let ok = host
                .weapon_type(hand)
                .is_some_and(|weapon_type| self.requires_weapon_type.contains(&weapon_type));
            if !ok {
                return false;
            }
        }
        if let Some(buff) = &self.requires_buff {
            if !host.buff_is_active_by_name(buff) {
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
        let range = self.proc_range(host);
        self.random.get_roll() < range && self.conditions_fulfilled(source, host)
    }

    /// Performs the proc's spell. Port of `Proc::spell_effect` (linked procs are performed by
    /// [`EnabledProcs`]).
    pub fn perform(&mut self, host: &mut impl ProcHost) -> CastReport {
        self.procs += 1;
        self.spell.perform(host)
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

    pub fn is_enabled(&self, id: ProcId) -> bool {
        self.enabled.contains(&id)
    }

    pub fn enabled(&self) -> &[ProcId] {
        &self.enabled
    }

    /// Enables the proc's spell and adds it to the proc checks. Port of `Proc::enable_proc` via
    /// `Spell::enable`.
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

    /// Runs the proc check for `source`: every enabled proc listening to it that has not
    /// already procced in this (possibly nested) check rolls, and fires with its linked procs.
    /// Proc sources produced by the procs themselves are checked recursively. Returns the cast
    /// reports of the procs that fired, for the statistics. Port of `run_proc_check`.
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
        self.checks_in_progress += 1;
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

        self.checks_in_progress -= 1;
        if self.checks_in_progress == 0 {
            self.procced.clear();
        }
        reports
    }

    /// Performs `id` and its linked procs, then the proc sources they produced.
    fn fire(
        &mut self,
        id: ProcId,
        host: &mut impl ProcHost,
        reports: &mut Vec<(ProcId, CastReport)>,
    ) {
        let report = self.procs[id.index()].perform(host);
        let nested: Vec<ProcSource> = report.proc_sources.clone();
        reports.push((id, report));
        for nested_source in nested {
            reports.extend(self.run_proc_check(nested_source, host));
        }
        let linked = self.procs[id.index()].linked.clone();
        for linked_id in linked {
            if self.is_enabled(linked_id) && self.procced.insert(linked_id) {
                self.fire(linked_id, host, reports);
            }
        }
    }

    /// Start of an iteration. Port of `EnabledProcs::reset`.
    pub fn reset(&mut self, host: &mut impl ProcHost) {
        for proc in &mut self.procs {
            proc.reset(host);
        }
        self.procced.clear();
        self.checks_in_progress = 0;
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
mod tests {
    use super::*;
    use crate::buff::{Buff, BuffApplication, BuffContext, BuffKind, ChargeUse};
    use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult};
    use crate::cooldown::{CooldownControl, CooldownRegistry};
    use crate::effect::EffectHost;
    use crate::engine::{Engine, Event, EventKind};
    use crate::ids::{BuffId, CharId, CooldownId, InstanceId};
    use crate::phase::Phase;
    use crate::resource::ResourceType;
    use crate::spell::{SpellDb, SpellFileSpec, SpellGroupSpec};
    use crate::stance::Stance;
    use crate::stats::CharacterStats;
    use crate::target::Target;
    use std::sync::Arc;

    struct World {
        engine: Engine,
        target: Target,
        stats: CharacterStats,
        cooldowns: CooldownRegistry,
        buffs: Vec<Buff>,
        rage: u32,
        mainhand: Option<(WeaponType, f64)>,
        offhand: Option<(WeaponType, f64)>,
        rage_gains: Vec<u32>,
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
                rage: 50,
                mainhand: Some((WeaponType::Sword, 2.6)),
                offhand: Some((WeaponType::Axe, 1.8)),
                rage_gains: Vec::new(),
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

        fn proc(&mut self, group: &Arc<SpellGroupSpec>, seed: u64) -> Proc {
            let cooldown = Some(self.cooldowns.new_cooldown_for_group(group));
            let rank_spec = group.rank(1).unwrap();
            let marker = rank_spec.buff.as_ref().map(|spec| {
                let kind = BuffKind::from_spec(spec, 0).unwrap();
                let id = BuffId(self.buffs.len() as u32);
                self.buffs.push(Buff::from_spec(group, rank_spec, kind));
                id
            });
            Proc::new(Spell::new(Arc::clone(group), 1, cooldown, marker), seed)
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

        fn apply_auras(&mut self, id: BuffId) {
            let effects = self.buffs[id.index()].effects.clone();
            for effect in &effects {
                effect.apply_aura(self);
            }
        }

        fn remove_auras(&mut self, id: BuffId) {
            let effects = self.buffs[id.index()].effects.clone();
            for effect in &effects {
                effect.remove_aura(self);
            }
        }
    }

    impl EffectHost for World {
        fn combo_points(&self) -> u32 {
            0
        }
        fn gain_combo_points(&mut self, _: u32) {}
        fn spend_combo_points(&mut self) {}
        fn resource_level(&self, _: ResourceType) -> u32 {
            self.rage
        }
        fn gain_resource(&mut self, _: ResourceType, amount: u32) -> u32 {
            let before = self.rage;
            self.rage = (self.rage + amount).min(100);
            self.rage_gains.push(self.rage - before);
            self.rage - before
        }
        fn melee_ap(&self) -> u32 {
            0
        }
        fn random_in_range(&mut self, min: f64, max: f64) -> f64 {
            (min + max) / 2.0
        }
        fn random_normalized_mh_dmg(&mut self) -> f64 {
            100.0
        }
        fn random_non_normalized_mh_dmg(&mut self) -> f64 {
            100.0
        }
        fn roll_melee_ability(&mut self, _: IncludedOutcomes, _: u32) -> PhysicalAttackResult {
            PhysicalAttackResult::Hit
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
        fn swap_stance(&mut self, _: Stance) {}
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
            Phase::Naxxramas
        }
        fn combat_length(&self) -> f64 {
            300.0
        }
        fn on_global_cooldown(&self) -> bool {
            false
        }
        fn global_cooldown(&self) -> f64 {
            1.5
        }
        fn start_global_cooldown(&mut self) {}
        fn on_stance_cooldown(&self) -> bool {
            false
        }
        fn start_stance_cooldown(&mut self) {}
        fn on_trinket_cooldown(&self) -> bool {
            false
        }
        fn cast_in_progress(&self) -> bool {
            false
        }
        fn stance(&self) -> Stance {
            Stance::Battle
        }
        fn offhand_weapon_type(&self) -> Option<WeaponType> {
            self.offhand.map(|(kind, _)| kind)
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
            0
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
            1.0
        }
    }

    impl ProcHost for World {
        fn weapon_type(&self, hand: Hand) -> Option<WeaponType> {
            match hand {
                Hand::Mainhand => self.mainhand.map(|(kind, _)| kind),
                Hand::Offhand => self.offhand.map(|(kind, _)| kind),
            }
        }
        fn base_weapon_speed(&self, hand: Hand) -> Option<f64> {
            match hand {
                Hand::Mainhand => self.mainhand.map(|(_, speed)| speed),
                Hand::Offhand => self.offhand.map(|(_, speed)| speed),
            }
        }
    }

    fn db() -> SpellDb {
        let file: SpellFileSpec = serde_yaml::from_str(
            r#"
spell_groups:
  - name: Unbridled Wrath
    causes_gcd: none
    requires_talent: Unbridled Wrath
    flags: [PASSIVE_SPELL]
    proc: { rate: 0.0, sources: [MAINHAND_SWING, OFFHAND_SWING] }
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 1 }]
    modified_by_talent:
      - { talent: Unbridled Wrath, rank: 1, type: set_proc_rate, value: 0.08 }
      - { talent: Unbridled Wrath, rank: 5, type: set_proc_rate, value: 0.40 }
  - name: Sword Specialization
    causes_gcd: none
    flags: [PASSIVE_SPELL]
    proc:
      rate: 1.0
      sources: [MAINHAND_SPELL, MAINHAND_SWING, OFFHAND_SWING]
      requires_weapon_type: [SWORD, TWOHAND_SWORD]
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 5 }]
  - name: Flurry
    causes_gcd: none
    flags: [PASSIVE_SPELL, ON_MELEE_CRIT]
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: self
          duration: 15
          base_charges: 3
          effects: [{ name: APPLY_AURA_MOD_MELEE_ATTACK_SPEED, value: 30 }]
  - name: Crusader
    causes_gcd: none
    flags: [PASSIVE_SPELL]
    proc: { rate: 1.0, ppm: true, hand: mainhand, sources: [MAINHAND_SWING] }
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 2 }]
  - name: Ready Proc
    causes_gcd: none
    flags: [PASSIVE_SPELL]
    proc: { rate: 1.0, sources: [MAINHAND_SWING], requires_buff: Flurry }
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 3 }]
  - name: Cooled Proc
    causes_gcd: none
    cooldown: 10
    flags: [PASSIVE_SPELL]
    proc: { rate: 1.0, sources: [MAINHAND_SWING] }
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 7 }]
  - name: Not A Proc
    causes_gcd: none
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: GAIN_RESOURCE_RAGE, value: 7 }]
"#,
        )
        .unwrap();
        let mut db = SpellDb::new();
        db.add_file(file).unwrap();
        db
    }

    #[test]
    fn proc_ranges_come_from_spec_talents_and_weapon_speed() {
        let db = db();
        let mut world = World::new();

        let mut wrath = world.proc(db.get("Unbridled Wrath").unwrap(), 1);
        assert_eq!(wrath.rate(), ProcRate::Flat(0.0));
        assert_eq!(
            wrath.sources(),
            &[ProcSource::MainhandSwing, ProcSource::OffhandSwing]
        );
        assert_eq!(wrath.proc_range(&world), 0);
        wrath
            .spell_mut()
            .increase_talent_rank(&mut world, "Unbridled Wrath", 1);
        assert_eq!(wrath.proc_range(&world), 800);
        wrath
            .spell_mut()
            .decrease_talent_rank(&mut world, "Unbridled Wrath", 0);
        assert_eq!(wrath.proc_range(&world), 0);

        let crusader = world.proc(db.get("Crusader").unwrap(), 1);
        assert_eq!(
            crusader.rate(),
            ProcRate::Ppm {
                ppm: 1.0,
                hand: Hand::Mainhand
            }
        );
        // 1 PPM on a 2.6 speed weapon: 2.6 / 60 = 4.33%.
        assert_eq!(crusader.proc_range(&world), 433);
        world.mainhand = None;
        assert_eq!(crusader.proc_range(&world), 0);

        let flurry = world.proc(db.get("Flurry").unwrap(), 1);
        assert_eq!(flurry.rate(), ProcRate::Flat(1.0));
        assert_eq!(flurry.sources(), &[ProcSource::MeleeCritical]);
        assert_eq!(flurry.proc_range(&world), PROC_ROLL_RANGE);
    }

    #[test]
    #[should_panic(expected = "not a passive spell")]
    fn non_passive_spells_cannot_be_procs() {
        let db = db();
        let mut world = World::new();
        let _ = world.proc(db.get("Not A Proc").unwrap(), 1);
    }

    #[test]
    fn conditions_check_the_triggering_weapon_and_required_buffs() {
        let db = db();
        let mut world = World::new();
        let sword = world.proc(db.get("Sword Specialization").unwrap(), 1);
        assert!(sword.conditions_fulfilled(ProcSource::MainhandSwing, &world));
        assert!(sword.conditions_fulfilled(ProcSource::MainhandSpell, &world));
        assert!(!sword.conditions_fulfilled(ProcSource::OffhandSwing, &world));
        world.offhand = Some((WeaponType::Sword, 1.5));
        assert!(sword.conditions_fulfilled(ProcSource::OffhandSwing, &world));
        world.mainhand = None;
        assert!(!sword.conditions_fulfilled(ProcSource::MainhandSwing, &world));

        let ready = world.proc(db.get("Ready Proc").unwrap(), 1);
        assert!(!ready.conditions_fulfilled(ProcSource::MainhandSwing, &world));
        let flurry = world.proc(db.get("Flurry").unwrap(), 1);
        let mut procs = EnabledProcs::new();
        let flurry = procs.add_proc(flurry);
        procs.enable(flurry, &mut world);
        procs.run_proc_check(ProcSource::MeleeCritical, &mut world);
        assert!(ready.conditions_fulfilled(ProcSource::MainhandSwing, &world));
    }

    #[test]
    fn run_proc_check_rolls_enabled_listening_procs_and_reports_them() {
        let db = db();
        let mut world = World::new();
        let mut procs = EnabledProcs::new();
        let wrath = world.proc(db.get("Unbridled Wrath").unwrap(), 7);
        let wrath = procs.add_proc(wrath);
        let sword = world.proc(db.get("Sword Specialization").unwrap(), 8);
        let sword = procs.add_proc(sword);
        assert_eq!(procs.find_by_name("Sword Specialization"), Some(sword));
        assert_eq!(procs.ids().collect::<Vec<_>>(), vec![wrath, sword]);

        // Nothing enabled: nothing fires.
        assert!(procs
            .run_proc_check(ProcSource::MainhandSwing, &mut world)
            .is_empty());

        procs.enable(sword, &mut world);
        procs.enable(wrath, &mut world);
        assert!(procs.is_enabled(sword) && procs.is_enabled(wrath));
        procs
            .get_mut(wrath)
            .spell_mut()
            .increase_talent_rank(&mut world, "Unbridled Wrath", 5);

        // Sword spec always procs from the sword mainhand; Unbridled Wrath at 40%.
        let reports = procs.run_proc_check(ProcSource::MainhandSwing, &mut world);
        let fired: Vec<ProcId> = reports.iter().map(|(id, _)| *id).collect();
        assert!(fired.contains(&sword));
        assert_eq!(procs.get(sword).procs(), 1);
        assert_eq!(procs.get(sword).attempts(), 1);
        assert_eq!(procs.get(wrath).attempts(), 1);
        assert_eq!(
            procs.get(sword).current_source(),
            Some(ProcSource::MainhandSwing)
        );
        assert!(world.rage_gains.contains(&5));

        // The offhand is an axe: sword spec does not fire from it, and MeleeHit is not a source.
        world.rage_gains.clear();
        let reports = procs.run_proc_check(ProcSource::OffhandSwing, &mut world);
        assert!(reports.iter().all(|(id, _)| *id == wrath));
        assert!(procs
            .run_proc_check(ProcSource::MeleeHit, &mut world)
            .is_empty());
        // The offhand swing was attempted (and failed the weapon condition); MeleeHit was not.
        assert_eq!(procs.get(sword).attempts(), 2);
        assert_eq!(procs.get(sword).procs(), 1);

        // Over many swings the 40% rate shows.
        let mut fired = 0;
        for _ in 0..1000 {
            fired += procs
                .run_proc_check(ProcSource::OffhandSwing, &mut world)
                .len();
            world.rage = 50;
        }
        assert!((300..500).contains(&fired), "{fired} procs");

        procs.disable(sword, &mut world);
        assert!(!procs.is_enabled(sword));
        assert!(!procs.get(sword).spell().is_enabled());
        world.rage = 50;
        world.rage_gains.clear();
        procs.run_proc_check(ProcSource::MainhandSwing, &mut world);
        assert!(!world.rage_gains.contains(&5));
    }

    #[test]
    fn internal_cooldowns_are_enforced() {
        let db = db();
        let mut world = World::new();
        let mut procs = EnabledProcs::new();
        let cooled = world.proc(db.get("Cooled Proc").unwrap(), 1);
        let cooled = procs.add_proc(cooled);
        procs.enable(cooled, &mut world);

        assert_eq!(
            procs
                .run_proc_check(ProcSource::MainhandSwing, &mut world)
                .len(),
            1
        );
        assert!(!procs.get(cooled).is_ready(&world));
        assert!(procs
            .run_proc_check(ProcSource::MainhandSwing, &mut world)
            .is_empty());
        assert_eq!(procs.get(cooled).attempts(), 2);
        world.advance_to(10.0);
        assert_eq!(
            procs
                .run_proc_check(ProcSource::MainhandSwing, &mut world)
                .len(),
            1
        );

        procs.reset(&mut world);
        assert!(procs.get(cooled).is_ready(&world));
        assert_eq!(procs.get(cooled).procs(), 2);
        procs.prepare_set_of_combat_iterations();
        assert_eq!(procs.get(cooled).procs(), 0);
        assert_eq!(procs.get(cooled).attempts(), 0);
    }

    #[test]
    fn buff_procs_apply_their_buff() {
        let db = db();
        let mut world = World::new();
        let mut procs = EnabledProcs::new();
        let flurry = world.proc(db.get("Flurry").unwrap(), 1);
        let flurry = procs.add_proc(flurry);
        procs.enable(flurry, &mut world);
        let reports = procs.run_proc_check(ProcSource::MeleeCritical, &mut world);
        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].1.buff,
            Some(BuffApplication::Applied { evicted: None })
        );
        assert!(world.buffs[0].is_active());
        assert_eq!(world.stats.get_melee_attack_speed_mod(), 1.3);
    }

    #[test]
    fn linked_procs_fire_without_a_roll_and_procs_do_not_retrigger_themselves() {
        let db = db();
        let mut world = World::new();
        let mut procs = EnabledProcs::new();
        let sword = world.proc(db.get("Sword Specialization").unwrap(), 1);
        let sword = procs.add_proc(sword);
        let wrath = world.proc(db.get("Unbridled Wrath").unwrap(), 1);
        let wrath = procs.add_proc(wrath);
        procs.get_mut(sword).add_linked(wrath);
        procs.get_mut(sword).add_linked(wrath);
        assert_eq!(procs.get(sword).linked(), &[wrath]);
        procs.enable(sword, &mut world);
        procs.enable(wrath, &mut world);

        // Unbridled Wrath has a 0% rate but fires as a linked proc.
        let reports = procs.run_proc_check(ProcSource::MainhandSwing, &mut world);
        let fired: Vec<ProcId> = reports.iter().map(|(id, _)| *id).collect();
        assert_eq!(fired, vec![sword, wrath]);
        assert_eq!(world.rage_gains, vec![5, 1]);

        // A proc excluded from the next check does not fire.
        world.rage_gains.clear();
        procs.ignore_in_next_check(sword);
        assert!(procs
            .run_proc_check(ProcSource::MainhandSwing, &mut world)
            .is_empty());
        // ... but the exclusion only lasts for that check.
        assert_eq!(
            procs
                .run_proc_check(ProcSource::MainhandSwing, &mut world)
                .len(),
            2
        );

        procs.clear_all(&mut world);
        assert!(procs.enabled().is_empty());
    }

    #[test]
    #[should_panic(expected = "manually triggered")]
    fn manual_source_cannot_be_checked() {
        let mut world = World::new();
        let mut procs = EnabledProcs::new();
        procs.run_proc_check(ProcSource::Manual, &mut world);
    }
}
