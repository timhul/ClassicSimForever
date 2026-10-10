//! A one-character test world: what the Phase 4 spell context will be, built on
//! [`CharacterSpells`] and a small spell db: some of the shipped `data/spells/warrior.yaml`
//! records with their overrides, loaded from the files. Shared by the spell runtime, proc and
//! registry tests.

use std::collections::VecDeque;
use std::path::Path;

use crate::buff::{Buff, BuffApplication, BuffContext, ChargeUse};
use crate::character_spells::{AddedSpell, BuffSlot, CharacterSpells, SharedBuffs};
use crate::combat_roll::{
    IncludedOutcomes, MagicResistResult, PhysicalAttackResult, SpellResistKind, SpellRoll,
};
use crate::cooldown::CooldownControl;
use crate::data_bundle::DataBundle;
use crate::effect::EffectHost;
use crate::engine::{Engine, Event, EventKind};
use crate::ids::{BuffId, CharId, CooldownId, InstanceId, SpellId};
use crate::magic_school::MagicSchool;
use crate::proc::{ProcHost, ProcSource};
use crate::raid::SharedBuffRegistry;
use crate::resource::ResourceType;
use crate::spell::dbc::{AuraState, AuraType};
use crate::spell::modifiers::SpellModifiers;
use crate::spell::overrides::{OverrideFile, Overrides};
use crate::spell::periodic::TickReport;
use crate::spell::record::{EquippedItems, SpellDb, SpellFile};
use crate::spell::{CastReport, Hand, Spell, SpellHost, SpellStatus};
use crate::stance::Stance;
use crate::stats::CharacterStats;
use crate::target::{CreatureType, Target};

/// The spells of the test db: the shipped Warrior records (`data/spells/warrior.yaml`) of these
/// ids, with their overrides.
pub(crate) const SPELL_IDS: [u32; 34] = [
    78, 284, 23881, 20662, 26651, 11551, 25289, 2457, 2458, 21156, 7381, 11585, 25288, 2687, 29131,
    11574, 11597, 1464, 1310197, 12296, 12319, 12966, 12834, 12162, 412609, 12322, 12964, 12292,
    12723, 12282, 12290, 16493, 12862, 12163,
];

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The shipped records of [`SPELL_IDS`], cut at their edge: a `supercedes` or an effect naming a
/// spell outside them goes (Improved Slam keeps the one replacement of the test db, Slam 1464
/// -> 1310197).
fn shipped_spells() -> SpellFile {
    let path = DataBundle::repository_dir().join("spells/warrior.yaml");
    let mut file: SpellFile = serde_yaml::from_str(&read(&path)).expect("valid spell yaml");
    let outside = |id: u32| id != 0 && !SPELL_IDS.contains(&id);
    file.spells.retain(|record| SPELL_IDS.contains(&record.id));
    for record in &mut file.spells {
        if outside(record.supercedes) {
            record.supercedes = 0;
        }
        record.effects.retain(|effect| {
            let replaces_outside = effect.is_apply_aura()
                && effect.aura == AuraType::OverrideActionbarSpells
                && (outside(effect.misc_value[0] as u32) || outside(effect.base_points as u32));
            !outside(effect.trigger_spell) && !replaces_outside
        });
    }
    assert_eq!(
        file.spells.len(),
        SPELL_IDS.len(),
        "every test spell is shipped"
    );
    file
}

/// The shipped overrides (`data/spells/overrides/warrior.yaml`) of [`SPELL_IDS`].
fn shipped_overrides() -> OverrideFile {
    let path = DataBundle::repository_dir().join("spells/overrides/warrior.yaml");
    let mut file: OverrideFile = serde_yaml::from_str(&read(&path)).expect("valid override yaml");
    file.overrides
        .retain(|spell_override| SPELL_IDS.contains(&spell_override.id));
    file
}

/// The test spell db: the shipped records of [`SPELL_IDS`] with their overrides.
pub(crate) fn db() -> SpellDb {
    db_with(|_| {})
}

/// The test spell db with `edit` applied to the records first.
pub(crate) fn db_with(edit: impl FnOnce(&mut SpellFile)) -> SpellDb {
    let mut file = shipped_spells();
    edit(&mut file);
    let overrides = shipped_overrides();
    let mut db = SpellDb::new();
    db.add_file(file).expect("valid records");
    let mut all = Overrides::new();
    all.add_file(overrides).expect("valid overrides");
    db.set_overrides(all);
    db.check_references().expect("consistent references");
    db
}

/// A one-character world around a [`CharacterSpells`] registry.
pub(crate) struct World {
    pub db: SpellDb,
    pub engine: Engine,
    pub target: Target,
    pub stats: CharacterStats,
    pub spells: CharacterSpells,
    pub raid: SharedBuffRegistry,
    pub modifiers: SpellModifiers,
    pub level: u32,
    pub rage: u32,
    pub combo_points: u32,
    pub stance: Stance,
    pub next_gcd: f64,
    pub next_stance_cd: f64,
    /// Forces `cast_in_progress` regardless of the registry.
    pub casting: bool,
    pub rolls: VecDeque<PhysicalAttackResult>,
    pub extra_crits: Vec<u32>,
    pub can_crits: Vec<bool>,
    pub combat_length: f64,
    pub armor: i32,
    pub block_value: u32,
    pub aura_log: Vec<String>,
    pub ticks: Vec<TickReport>,
    pub casting_speed_mod: f64,
    pub attack_log: Vec<&'static str>,
    pub completed_casts: Vec<CastReport>,
    /// Whether every `SpellEquippedItems` requirement is met.
    pub weapon_ok: bool,
    pub caster_states: Vec<AuraState>,
    pub target_states: Vec<AuraState>,
    pub mh_speed: Option<f64>,
    pub oh_speed: Option<f64>,
    /// `(spell, trigger value)` of every `trigger_spell` call.
    pub trigger_log: Vec<(u32, Option<f64>)>,
    pub extra_attacks: u32,
    pub stance_rage_retained: i32,
    pub offhand_damage_percent: i32,
    pub offhand_rage_percent: i32,
    /// `OFFHAND_COPY` abilities, once per active aura.
    pub offhand_copies: Vec<u32>,
    /// `(spell, resource, amount)` gains on use (`GAIN_RESOURCE_ON_USE`).
    pub resources_on_use: Vec<(u32, ResourceType, u32)>,
    pub actionbar_log: Vec<(u32, u32, bool)>,
    /// Rolls on the magic table, in order; a hit once they run out.
    pub spell_rolls: VecDeque<SpellRoll>,
    /// `(school, kind)` of every roll on the magic table.
    pub spell_roll_log: Vec<(MagicSchool, SpellResistKind)>,
    /// Partial resists of the periodic ticks, in order; none once they run out.
    pub periodic_resists: VecDeque<MagicResistResult>,
    /// `(school, pure DoT)` of every tick resist roll.
    pub periodic_resist_log: Vec<(MagicSchool, bool)>,
    /// Whether each periodic tick that can crit crits, in order; no crit once they run out.
    pub periodic_crits: VecDeque<bool>,
    /// The extra crit chance of every periodic crit roll.
    pub periodic_crit_log: Vec<u32>,
    /// The caster's spell damage, of every school.
    pub spell_damage: u32,
    /// The damage done multiplier of every magic school.
    pub magic_damage_mod: f64,
}

impl World {
    pub fn new() -> Self {
        Self::with_db(db())
    }

    pub fn with_db(db: SpellDb) -> Self {
        let mut engine = Engine::new();
        engine.prepare_iteration(0.0);
        World {
            db,
            engine,
            target: Target::new(63),
            stats: CharacterStats::new(),
            spells: CharacterSpells::new(CharId(0), 1),
            raid: SharedBuffRegistry::new(),
            modifiers: SpellModifiers::new(),
            level: 60,
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
            block_value: 0,
            aura_log: Vec::new(),
            ticks: Vec::new(),
            casting_speed_mod: 1.0,
            attack_log: Vec::new(),
            completed_casts: Vec::new(),
            weapon_ok: true,
            caster_states: Vec::new(),
            target_states: Vec::new(),
            mh_speed: Some(2.6),
            oh_speed: Some(1.8),
            trigger_log: Vec::new(),
            extra_attacks: 0,
            stance_rage_retained: 0,
            offhand_damage_percent: 0,
            offhand_rage_percent: 0,
            offhand_copies: Vec::new(),
            resources_on_use: Vec::new(),
            actionbar_log: Vec::new(),
            spell_rolls: VecDeque::new(),
            spell_roll_log: Vec::new(),
            periodic_resists: VecDeque::new(),
            periodic_resist_log: Vec::new(),
            periodic_crits: VecDeque::new(),
            periodic_crit_log: Vec::new(),
            spell_damage: 0,
            magic_damage_mod: 1.0,
        }
    }

    /// Adds spell `id` from the db (not enabled).
    pub fn add(&mut self, id: u32) -> AddedSpell {
        let db = std::mem::take(&mut self.db);
        let added = self.spells.add_spell(&db, id, 0, &mut self.raid);
        self.db = db;
        added
    }

    /// Adds and enables spell `id`; returns its handle (procs are enabled through the procs).
    pub fn learn(&mut self, id: u32) -> AddedSpell {
        let added = self.add(id);
        if let Some(spell) = added.spell {
            self.with_spell(spell, |spell, world| spell.enable(world));
        }
        if let Some(proc) = added.proc {
            let mut procs = self.spells.take_procs();
            procs.enable(proc, self);
            self.spells.put_procs(procs);
        }
        added
    }

    pub fn spell_id(&self, game_id: u32) -> SpellId {
        self.spells
            .spell_by_game_id(game_id)
            .unwrap_or_else(|| panic!("spell {game_id} not added"))
    }

    pub fn spell(&self, game_id: u32) -> &Spell {
        self.spells.spell(self.spell_id(game_id))
    }

    /// Runs `f` with the spell taken out of the registry.
    pub fn with_spell<R>(&mut self, id: SpellId, f: impl FnOnce(&mut Spell, &mut World) -> R) -> R {
        let mut spell = self.spells.take_spell(id);
        let result = f(&mut spell, self);
        self.spells.put_spell(id, spell);
        result
    }

    pub fn status(&self, game_id: u32) -> SpellStatus {
        self.spell(game_id).status(self)
    }

    pub fn perform(&mut self, game_id: u32) -> CastReport {
        let id = self.spell_id(game_id);
        self.with_spell(id, |spell, world| spell.perform(world))
    }

    pub fn run_proc_check(&mut self, source: ProcSource) -> Vec<(crate::ids::ProcId, CastReport)> {
        let mut procs = self.spells.take_procs();
        let reports = procs.run_proc_check(source, crate::proc::ProcTrigger::default(), self);
        self.spells.put_procs(procs);
        reports
    }

    /// Uses a charge of every buff that reacts to `source`.
    pub fn consume_charges(&mut self, source: ProcSource) {
        for id in self.spells.charge_consumers(source) {
            let (buff, mut ctx) = self.buff_ctx(id);
            if buff.use_charge(&mut ctx) == ChargeUse::Removed {
                self.remove_auras(id);
            }
        }
    }

    pub fn advance_to(&mut self, time: f64) {
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

    /// Dispatches events up to and including `until`: buff removals, dot ticks and cast
    /// completions of the registry's spells.
    pub fn run(&mut self, until: f64) {
        self.engine
            .add_event(Event::new(until, EventKind::EncounterEnd));
        while let Some(event) = self.engine.next_event() {
            match event.kind {
                EventKind::EncounterEnd => break,
                EventKind::BuffRemoval {
                    buff, iteration, ..
                } => {
                    let stacks = self.buff_ref(buff).stacks();
                    let (b, mut ctx) = self.buff_ctx(buff);
                    if b.remove(iteration, &mut ctx) {
                        for _ in 0..stacks.max(1) {
                            self.remove_auras(buff);
                        }
                    }
                }
                EventKind::DotTick {
                    spell,
                    application_id,
                    ..
                } => {
                    if let Some(tick) =
                        self.with_spell(spell, |s, world| s.perform_periodic(application_id, world))
                    {
                        self.ticks.push(tick);
                    }
                }
                EventKind::CastComplete { spell, cast_id, .. } => {
                    if let Some(report) =
                        self.with_spell(spell, |s, world| s.complete_cast(cast_id, world))
                    {
                        self.completed_casts.push(report);
                    }
                }
                _ => {}
            }
        }
    }

    fn buff_ref(&self, id: BuffId) -> &Buff {
        match self.spells.buff_slot(id) {
            BuffSlot::Owned(buff) => buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff(*handle),
        }
    }

    fn buff_ctx(&mut self, id: BuffId) -> (&mut Buff, BuffContext<'_>) {
        let buff = match self.spells.buff_slot_mut(id) {
            BuffSlot::Owned(buff) => buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff_mut(*handle),
        };
        (
            buff,
            BuffContext {
                engine: &mut self.engine,
                target: &mut self.target,
                character: CharId(0),
                buff: id,
            },
        )
    }

    fn apply_auras(&mut self, id: BuffId) {
        let effects = self.buff_ref(id).effects.clone();
        for effect in &effects {
            self.aura_log.push(format!("+{:?}", effect.aura()));
            effect.apply_aura(self, effect.record().targets_enemy());
        }
    }

    fn remove_auras(&mut self, id: BuffId) {
        let effects = self.buff_ref(id).effects.clone();
        for effect in &effects {
            self.aura_log.push(format!("-{:?}", effect.aura()));
            effect.remove_aura(self, effect.record().targets_enemy());
        }
    }
}

impl EffectHost for World {
    fn caster_level(&self) -> u32 {
        self.level
    }
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
    fn spell_damage(&self, _school: MagicSchool) -> u32 {
        self.spell_damage
    }
    fn max_health(&self) -> u32 {
        4000
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
    fn roll_spell(
        &mut self,
        school: MagicSchool,
        kind: SpellResistKind,
        _extra_crit: u32,
        _can_crit: bool,
    ) -> SpellRoll {
        self.spell_roll_log.push((school, kind));
        self.spell_rolls.pop_front().unwrap_or(SpellRoll::HIT)
    }
    fn stats_mut(&mut self) -> &mut CharacterStats {
        &mut self.stats
    }
    fn target_mut(&mut self) -> &mut Target {
        &mut self.target
    }
    fn target_creature_type(&self) -> CreatureType {
        self.target.creature_type()
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
    fn spell_modifiers(&self) -> &SpellModifiers {
        &self.modifiers
    }
    fn spell_modifiers_mut(&mut self) -> &mut SpellModifiers {
        &mut self.modifiers
    }
    fn add_extra_attacks(&mut self, count: u32) {
        self.extra_attacks += count;
    }
    fn adjust_stance_rage_retained(&mut self, delta: i32) {
        self.stance_rage_retained += delta;
    }
    fn adjust_offhand_damage_percent(&mut self, percent: i32) {
        self.offhand_damage_percent += percent;
    }
    fn adjust_offhand_rage_percent(&mut self, percent: i32) {
        self.offhand_rage_percent += percent;
    }
    fn adjust_offhand_copy(&mut self, spell: u32, apply: bool) {
        if apply {
            self.offhand_copies.push(spell);
        } else if let Some(i) = self.offhand_copies.iter().position(|&s| s == spell) {
            self.offhand_copies.remove(i);
        }
    }
    fn adjust_resource_on_use(
        &mut self,
        spell: u32,
        resource: ResourceType,
        amount: u32,
        apply: bool,
    ) {
        let entry = (spell, resource, amount);
        if apply {
            self.resources_on_use.push(entry);
        } else if let Some(i) = self.resources_on_use.iter().position(|&e| e == entry) {
            self.resources_on_use.remove(i);
        }
    }
    fn override_actionbar_spell(&mut self, replaced: u32, replacement: u32, apply: bool) {
        self.actionbar_log.push((replaced, replacement, apply));
        self.spells
            .apply_actionbar_override(replaced, replacement, apply);
    }
}

impl SpellHost for World {
    fn character_id(&self) -> CharId {
        CharId(0)
    }
    fn resource_type(&self) -> ResourceType {
        ResourceType::Rage
    }
    fn engine(&self) -> &Engine {
        &self.engine
    }
    fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
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
    fn cast_in_progress(&self) -> bool {
        self.casting || self.spells.cast_in_progress()
    }
    fn running_to_target(&self) -> bool {
        self.spells.running_to_target()
    }
    fn start_cast(&mut self, running_to_target: bool) -> u32 {
        self.spells.start_cast(running_to_target)
    }
    fn complete_cast(&mut self, cast_id: u32) {
        self.spells.complete_cast(cast_id);
    }
    fn casting_speed_mod(&self) -> f64 {
        self.casting_speed_mod
    }
    fn casting_speed_flat_reduction(&self) -> u32 {
        0
    }
    fn stop_attack(&mut self) {
        self.attack_log.push("stop");
        self.spells.stop_attack();
    }
    fn start_attack(&mut self) {
        self.attack_log.push("start");
        self.spells.start_attack();
    }
    fn reset_swing_timers(&mut self) {
        self.attack_log.push("reset");
    }
    fn queue_next_swing(&mut self, spell: SpellId, marker: Option<BuffId>) {
        if self.spells.queued_next_swing() != Some(spell) {
            self.cancel_next_swing();
        }
        self.attack_log.push("queue");
        self.spells.queue_next_swing(spell, marker);
    }
    fn cancel_next_swing(&mut self) {
        let marker = self.spells.queued_next_swing_marker();
        if self.spells.cancel_next_swing().is_some() {
            self.attack_log.push("unqueue");
        }
        if let Some(marker) = marker {
            self.cancel_buff(marker);
        }
    }
    fn queued_next_swing(&self) -> Option<SpellId> {
        self.spells.queued_next_swing()
    }
    fn stance(&self) -> Stance {
        self.stance
    }
    fn equipped_item_matches(&self, _: &EquippedItems) -> bool {
        self.weapon_ok
    }
    fn caster_aura_state(&self, state: AuraState) -> bool {
        self.caster_states.contains(&state)
    }
    fn target_aura_state(&self, state: AuraState) -> bool {
        self.target_states.contains(&state)
    }
    fn aura_active(&self, spell: u32) -> bool {
        self.spells
            .buff_ids()
            .map(|id| self.buff_ref(id))
            .any(|buff| buff.spell() == spell && buff.is_active())
    }
    fn lose_resource(&mut self, _: ResourceType, amount: u32) {
        self.rage -= amount;
    }
    /// Whole rage only: the refund rounds.
    fn refund_resource(&mut self, _: ResourceType, amount: f64) {
        self.rage = (self.rage + amount.round() as u32).min(100);
    }
    fn cooldown(&self, id: CooldownId) -> &CooldownControl {
        self.spells.cooldowns().get(id)
    }
    fn cooldown_mut(&mut self, id: CooldownId) -> &mut CooldownControl {
        self.spells.cooldowns_mut().get_mut(id)
    }
    fn buff(&self, id: BuffId) -> &Buff {
        self.buff_ref(id)
    }
    fn buff_mut(&mut self, id: BuffId) -> &mut Buff {
        self.buff_ctx(id).0
    }
    /// Aura effects are applied once per stack (Sunder Armor's −450 armor × 5).
    fn apply_buff(&mut self, id: BuffId) -> BuffApplication {
        let before = self.buff_ref(id).stacks();
        let (buff, mut ctx) = self.buff_ctx(id);
        let application = buff.apply(&mut ctx);
        match application {
            BuffApplication::Applied { .. } => self.apply_auras(id),
            BuffApplication::Refreshed { stacks } if stacks > before => self.apply_auras(id),
            _ => {}
        }
        application
    }
    fn cancel_buff(&mut self, id: BuffId) -> bool {
        let stacks = self.buff_ref(id).stacks();
        let (buff, mut ctx) = self.buff_ctx(id);
        let cancelled = buff.cancel(&mut ctx);
        if cancelled {
            for _ in 0..stacks.max(1) {
                self.remove_auras(id);
            }
        }
        cancelled
    }
    fn enable_buff(&mut self, id: BuffId) {
        if self.spells.owned_buff(id).is_some() {
            self.spells.enable_buff(id);
        } else {
            let buff = self.buff_ctx(id).0;
            buff.set_instance_id(InstanceId(id.0 + 100));
            buff.enable();
        }
    }
    fn disable_buff(&mut self, id: BuffId) {
        self.spells.disable_buff(id);
    }
    fn trigger_spell(&mut self, spell: u32, trigger_value: Option<f64>) -> Option<CastReport> {
        self.trigger_log.push((spell, trigger_value));
        let id = self.spells.spell_by_game_id(spell)?;
        Some(self.with_spell(id, |s, world| {
            s.set_trigger_value(trigger_value);
            s.perform_triggered(world)
        }))
    }
    fn set_spell_effect_value(&mut self, spell: u32, index: u32, value: f64) {
        if let Some(id) = self.spells.spell_by_game_id(spell) {
            self.with_spell(id, |s, world| s.set_effect_value(world, index, value));
        } else if let Some(id) = self.spells.proc_by_game_id(spell) {
            let mut procs = self.spells.take_procs();
            procs
                .get_mut(id)
                .spell_mut()
                .set_effect_value(self, index, value);
            self.spells.put_procs(procs);
        }
    }
    fn target_armor(&self) -> i32 {
        self.armor
    }
    fn target_block_value(&self) -> u32 {
        self.block_value
    }
    fn total_physical_damage_mod(&self) -> f64 {
        1.0
    }
    fn magic_school_damage_mod(&self, _school: MagicSchool) -> f64 {
        self.magic_damage_mod
    }
    fn flat_physical_damage_bonus(&self) -> u32 {
        0
    }
    fn melee_ability_crit_dmg_mod(&self) -> f64 {
        2.0
    }
    fn roll_periodic_resist(&mut self, school: MagicSchool, pure_dot: bool) -> MagicResistResult {
        self.periodic_resist_log.push((school, pure_dot));
        self.periodic_resists
            .pop_front()
            .unwrap_or(MagicResistResult::NoResist)
    }
    fn roll_periodic_crit(&mut self, extra_crit: u32) -> bool {
        self.periodic_crit_log.push(extra_crit);
        self.periodic_crits.pop_front().unwrap_or(false)
    }
    fn total_threat_mod(&self) -> f64 {
        self.stats.get_total_threat_mod()
    }
    fn avg_mh_weapon_damage(&self) -> f64 {
        200.0
    }
    fn offhand_copy_active(&self, spell: u32) -> bool {
        self.offhand_copies.contains(&spell) && self.oh_speed.is_some()
    }
    fn is_dual_wielding(&self) -> bool {
        self.oh_speed.is_some()
    }
    fn resources_on_use(&self, spell: u32) -> Vec<(ResourceType, u32)> {
        self.resources_on_use
            .iter()
            .filter(|&&(s, _, _)| s == spell)
            .map(|&(_, resource, amount)| (resource, amount))
            .collect()
    }
    fn roll_offhand_melee_ability(
        &mut self,
        _: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult {
        self.extra_crits.push(extra_crit);
        self.can_crits.push(can_crit);
        self.rolls.pop_front().expect("no roll queued")
    }
    /// Half the main-hand values, so an off-hand strike is told apart.
    fn random_oh_weapon_dmg(&mut self, normalized: bool) -> f64 {
        if normalized { 150.0 } else { 200.0 }
    }
    fn offhand_penalty(&self) -> f64 {
        0.5
    }
}

impl ProcHost for World {
    fn base_weapon_speed(&self, hand: Hand) -> Option<f64> {
        match hand {
            Hand::Mainhand => self.mh_speed,
            Hand::Offhand => self.oh_speed,
        }
    }

    fn hand_weapon_matches(&self, hand: Hand, _: &EquippedItems) -> bool {
        self.weapon_ok && self.base_weapon_speed(hand).is_some()
    }

    fn aura_effect_value(&self, _: u32, _: u32) -> Option<f64> {
        None
    }
}
