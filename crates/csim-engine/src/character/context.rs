//! The character's world context: the [`Character`] borrowed together with the engine, the
//! target and the raid's shared buffs. This is the "spell context" the spell runtime, the
//! procs and the auto attacks were written against: it implements [`EffectHost`],
//! [`SpellHost`], [`ProcHost`] and [`AutoAttackHost`], and offers the world operations of the
//! C++ `Character` / `CharacterSpells` / `EnabledBuffs` that need more than the character's own
//! state (learning and casting spells, swinging, stance swaps, resets, event handling).
//!
//! A context is short-lived: the simulation owner builds one per event from `&mut` borrows and
//! drops it afterwards, so no back-pointers survive between events.

use crate::buff::{Buff, BuffApplication, BuffContext, ChargeUse};
use crate::character_spells::{AddedSpell, BuffSlot, SharedBuffs};
use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult};
use crate::cooldown::CooldownControl;
use crate::effect::EffectHost;
use crate::engine::{Engine, Event, EventKind};
use crate::ids::{BuffId, CharId, CooldownId, ProcId, SpellId};
use crate::item::EquipmentSlot;
use crate::proc::{ProcHost, ProcSource};
use crate::race::RaceSpec;
use crate::resource::ResourceType;
use crate::spell::dbc::AuraState;
use crate::spell::modifiers::SpellModifiers;
use crate::spell::overrides::SimFlag;
use crate::spell::periodic::TickReport;
use crate::spell::record::{EquippedItems, SpellDb};
use crate::spell::{AutoAttack, AutoAttackHost, CastReport, Hand, Spell, SpellHost, SwingReport};
use crate::stance::Stance;
use crate::stats::{CharacterStats, TargetStatView};
use crate::target::Target;

use super::{Character, StanceLink};

/// What one main-hand swing event did: the swing, or the on-next-swing spell that replaced it.
#[derive(Debug, Clone, PartialEq)]
pub enum SwingOutcome {
    /// The swing event was stale or the character is not attacking.
    Skipped,
    Swing(SwingReport),
    /// A queued on-next-swing spell (Heroic Strike) landed instead of the swing.
    NextSwingSpell(CastReport),
}

/// The character together with everything it acts on.
pub struct CharacterContext<'a, S: SharedBuffs> {
    pub character: &'a mut Character,
    pub engine: &'a mut Engine,
    pub target: &'a mut Target,
    pub raid: &'a mut S,
}

impl<'a, S: SharedBuffs> CharacterContext<'a, S> {
    pub fn new(
        character: &'a mut Character,
        engine: &'a mut Engine,
        target: &'a mut Target,
        raid: &'a mut S,
    ) -> Self {
        Self {
            character,
            engine,
            target,
            raid,
        }
    }

    fn now(&self) -> f64 {
        self.engine.current_time()
    }

    fn target_view(&self) -> TargetStatView {
        self.target.stat_view()
    }

    // ---------------------------------------------------------------- registry access

    /// Runs `f` with the spell taken out of the registry.
    pub fn with_spell<R>(&mut self, id: SpellId, f: impl FnOnce(&mut Spell, &mut Self) -> R) -> R {
        let mut spell = self.character.spells.take_spell(id);
        let result = f(&mut spell, self);
        self.character.spells.put_spell(id, spell);
        result
    }

    /// Runs `f` with the procs taken out of the registry.
    fn with_procs<R>(
        &mut self,
        f: impl FnOnce(&mut crate::proc::EnabledProcs, &mut Self) -> R,
    ) -> R {
        let mut procs = self.character.spells.take_procs();
        let result = f(&mut procs, self);
        self.character.spells.put_procs(procs);
        result
    }

    fn with_auto_attack<R>(
        &mut self,
        hand: Hand,
        f: impl FnOnce(&mut AutoAttack, &mut Self) -> R,
    ) -> R {
        let mut attack = self.character.spells.take_auto_attack(hand);
        let result = f(&mut attack, self);
        self.character.spells.put_auto_attack(attack);
        result
    }

    fn buff_ref(&self, id: BuffId) -> &Buff {
        match self.character.spells.buff_slot(id) {
            BuffSlot::Owned(buff) => buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff(*handle),
        }
    }

    fn buff_ctx(&mut self, id: BuffId) -> (&mut Buff, BuffContext<'_>) {
        let character = self.character.id();
        let buff = match self.character.spells.buff_slot_mut(id) {
            BuffSlot::Owned(buff) => &mut **buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff_mut(*handle),
        };
        (
            buff,
            BuffContext {
                engine: self.engine,
                target: self.target,
                character,
                buff: id,
            },
        )
    }

    fn apply_auras(&mut self, id: BuffId) {
        let effects = self.buff_ref(id).effects.clone();
        for effect in &effects {
            effect.apply_aura(self, effect.record().targets_enemy());
        }
    }

    fn remove_auras(&mut self, id: BuffId) {
        let effects = self.buff_ref(id).effects.clone();
        for effect in &effects {
            effect.remove_aura(self, effect.record().targets_enemy());
        }
    }

    // ---------------------------------------------------------------- learning spells

    /// Adds spell `id` from `db` and enables it when it is a trainable / racial spell. Stance
    /// spells register their stance link. Port of the spell construction in the C++ class
    /// constructors.
    pub fn learn(&mut self, db: &SpellDb, id: u32) -> AddedSpell {
        let party = self.character.party();
        let added = self.character.spells.add_spell(db, id, party, self.raid);
        if let Some(spell) = added.spell {
            let stance = {
                let spell = self.character.spells.spell(spell);
                spell.stance().map(|stance| {
                    (
                        stance,
                        StanceLink {
                            spell: spell.id().expect("registered spell has an id"),
                            passive: spell.stance_passive(),
                        },
                    )
                })
            };
            if let Some((stance, link)) = stance {
                self.character.add_stance_link(stance, link);
            }
            if added.enable_now {
                self.enable_spell(spell);
            }
        }
        if let Some(proc) = added.proc {
            if added.enable_now {
                self.enable_proc(proc);
            }
        }
        // A payload learned after the spell that casts it.
        if self.enabled_source_of(id) {
            self.set_payloads_enabled(&[id], true);
        }
        self.sync_stance_passives();
        added
    }

    /// Whether an enabled spell or proc of the registry casts `game_id` as a payload.
    fn enabled_source_of(&self, game_id: u32) -> bool {
        let spells = &self.character.spells;
        spells.spell_ids().any(|id| {
            let spell = spells.spell(id);
            spell.is_enabled() && spell.payload_spells().contains(&game_id)
        }) || spells.procs().enabled().iter().any(|&id| {
            spells
                .procs()
                .get(id)
                .spell()
                .payload_spells()
                .contains(&game_id)
        })
    }

    /// Learns every spell of `db` that the character's class or race can have: class spells
    /// (any `class_mask`, including the talent-granted ones which stay disabled) and the
    /// racials whose `race_mask` names the race.
    pub fn learn_all(&mut self, db: &SpellDb) -> Vec<AddedSpell> {
        let race = self.character.race();
        let mut ids: Vec<u32> = db
            .records()
            .into_iter()
            .filter(|record| record.race_mask == 0 || race.in_mask(record.race_mask))
            .map(|record| record.id)
            .collect();
        ids.sort_unstable();
        let mut added = Vec::new();
        for id in ids {
            if !self.character.spells.has_game_id(id) {
                added.push(self.learn(db, id));
            }
        }
        added
    }

    /// Enables a spell and the hidden payloads it casts (a trainable spell's trigger
    /// payloads, a talent's proc buff).
    pub fn enable_spell(&mut self, id: SpellId) {
        let payloads = self.with_spell(id, |spell, ctx| {
            if spell.is_enabled() {
                return Vec::new();
            }
            spell.enable(ctx);
            spell.payload_spells()
        });
        self.set_payloads_enabled(&payloads, true);
    }

    /// Disables a spell and its hidden payloads.
    pub fn disable_spell(&mut self, id: SpellId) {
        let payloads = self.with_spell(id, |spell, ctx| {
            if !spell.is_enabled() {
                return Vec::new();
            }
            spell.disable(ctx);
            spell.payload_spells()
        });
        self.set_payloads_enabled(&payloads, false);
    }

    pub fn enable_proc(&mut self, id: ProcId) {
        let payloads = self.with_procs(|procs, ctx| {
            if procs.is_enabled(id) {
                return Vec::new();
            }
            procs.enable(id, ctx);
            procs.get(id).spell().payload_spells()
        });
        self.set_payloads_enabled(&payloads, true);
    }

    pub fn disable_proc(&mut self, id: ProcId) {
        let payloads = self.with_procs(|procs, ctx| {
            if !procs.is_enabled(id) {
                return Vec::new();
            }
            procs.disable(id, ctx);
            procs.get(id).spell().payload_spells()
        });
        self.set_payloads_enabled(&payloads, false);
    }

    /// Enables / disables the learned hidden payloads among `ids` (spellbook spells are
    /// enabled by their own learning / talent, never as a payload).
    fn set_payloads_enabled(&mut self, ids: &[u32], enabled: bool) {
        for &game_id in ids {
            let is_payload = self
                .character
                .spells
                .spell_by_game_id(game_id)
                .map(|id| self.character.spells.spell(id).record().clone())
                .or_else(|| {
                    self.character.spells.proc_by_game_id(game_id).map(|id| {
                        self.character
                            .spells
                            .procs()
                            .get(id)
                            .spell()
                            .record()
                            .clone()
                    })
                })
                .is_some_and(|record| {
                    !record.is_in_spellbook()
                        || (record.acquire_method == 3 && record.class_mask == 0)
                });
            if !is_payload {
                continue;
            }
            match self.character.spells.handle(game_id) {
                Some(crate::character_spells::SpellHandle::Spell(id)) => {
                    if enabled {
                        self.enable_spell(id);
                    } else {
                        self.disable_spell(id);
                    }
                }
                Some(crate::character_spells::SpellHandle::Proc(id)) => {
                    if enabled {
                        self.enable_proc(id);
                    } else {
                        self.disable_proc(id);
                    }
                }
                None => {}
            }
        }
    }

    /// Changes the race: the racial spells of the old race are disabled, those of the new
    /// race enabled (learning them first when needed). Port of `Character::set_race` +
    /// `CharacterSpells::activate_racials`.
    pub fn set_race(&mut self, db: &SpellDb, race: &RaceSpec) {
        let old = self.character.race();
        for record in old.racials(db) {
            match self.character.spells.handle(record.id) {
                Some(crate::character_spells::SpellHandle::Spell(id)) => self.disable_spell(id),
                Some(crate::character_spells::SpellHandle::Proc(id)) => self.disable_proc(id),
                None => {}
            }
        }
        self.character.set_race_stats(race);
        for record in race.race.racials(db) {
            match self.character.spells.handle(record.id) {
                Some(crate::character_spells::SpellHandle::Spell(id)) => self.enable_spell(id),
                Some(crate::character_spells::SpellHandle::Proc(id)) => self.enable_proc(id),
                None => {
                    self.learn(db, record.id);
                }
            }
        }
    }

    // ---------------------------------------------------------------- casting

    /// Casts a spell: performs it, then runs the proc checks its report asks for and the
    /// extra attacks it granted. Port of the `Spell::perform` → `run_proc_check` flow.
    pub fn cast(&mut self, id: SpellId) -> CastReport {
        let report = self.with_spell(id, |spell, ctx| spell.perform(ctx));
        self.after_cast(&report);
        self.perform_extra_attacks();
        report
    }

    /// Runs the proc sources of a completed cast.
    fn after_cast(&mut self, report: &CastReport) {
        let sources = report.all_proc_sources();
        self.run_proc_checks(&sources);
    }

    fn after_swing(&mut self, report: &SwingReport) {
        let sources = report.proc_sources.clone();
        self.run_proc_checks(&sources);
    }

    /// Runs the proc check for each source and returns the reports of the procs that fired.
    /// Extra attacks the procs granted stay pending (see [`Self::perform_extra_attacks`]);
    /// the procs that fired are excluded from the next check so that none re-fires off its
    /// own extra attack (the C++ nesting guard).
    pub fn run_proc_checks(&mut self, sources: &[ProcSource]) -> Vec<(ProcId, CastReport)> {
        let before = self.character.pending_extra_attacks();
        let mut fired = Vec::new();
        for &source in sources {
            if source == ProcSource::Manual {
                continue;
            }
            fired.extend(self.with_procs(|procs, ctx| procs.run_proc_check(source, ctx)));
        }
        if self.character.pending_extra_attacks() > before {
            for (id, _) in &fired {
                self.character.spells.procs_mut().ignore_in_next_check(*id);
            }
        }
        fired
    }

    /// Performs the main-hand extra attacks granted so far (Windfury, Sword Specialization),
    /// including the ones those attacks grant in turn. The C++ performed them inside the
    /// granting proc; here they follow the action that produced them. Port of
    /// `MainhandAttack::extra_attack`. Returns their swing reports.
    pub fn perform_extra_attacks(&mut self) -> Vec<SwingReport> {
        let mut reports = Vec::new();
        while self.character.take_extra_attack() {
            if !self.character.has_mainhand() {
                continue;
            }
            let report =
                self.with_auto_attack(Hand::Mainhand, |attack, ctx| attack.extra_attack(ctx));
            self.after_swing(&report);
            reports.push(report);
        }
        reports
    }

    // ---------------------------------------------------------------- auto attacks

    /// A main-hand swing event. Port of `WarriorSpells::mh_auto_attack`: a queued on-next-swing
    /// spell that is available replaces the swing; a queued one that is not is cancelled.
    pub fn mh_swing(&mut self, iteration: u32) -> SwingOutcome {
        if !self.character.spells.mh_attack().attack_is_valid(iteration)
            || !self.character.spells.is_melee_attacking()
        {
            return SwingOutcome::Skipped;
        }
        let outcome = match self.character.spells.queued_next_swing() {
            Some(queued)
                if self
                    .character
                    .spells
                    .spell(queued)
                    .status(self)
                    .is_available() =>
            {
                let now = self.now();
                let speed = self.weapon_speed(Hand::Mainhand).unwrap_or(0.0);
                self.character
                    .spells
                    .mh_attack_mut()
                    .complete_swing(now, speed);
                let report = self.with_spell(queued, |spell, ctx| spell.perform_on_swing(ctx));
                self.after_cast(&report);
                self.perform_extra_attacks();
                SwingOutcome::NextSwingSpell(report)
            }
            queued => {
                if let Some(queued) = queued {
                    self.with_spell(queued, |spell, ctx| spell.cancel(ctx));
                    self.character.spells.cancel_next_swing();
                }
                let report =
                    self.with_auto_attack(Hand::Mainhand, |attack, ctx| attack.perform(ctx));
                self.after_swing(&report);
                self.perform_extra_attacks();
                SwingOutcome::Swing(report)
            }
        };
        self.with_auto_attack(Hand::Mainhand, |attack, ctx| attack.schedule_next(ctx));
        outcome
    }

    /// An off-hand swing event. Port of `WarriorSpells::oh_auto_attack`.
    pub fn oh_swing(&mut self, iteration: u32) -> SwingOutcome {
        if !self.character.spells.oh_attack().attack_is_valid(iteration)
            || !self.character.spells.is_melee_attacking()
            || !self.character.is_dual_wielding()
        {
            return SwingOutcome::Skipped;
        }
        let report = self.with_auto_attack(Hand::Offhand, |attack, ctx| attack.perform(ctx));
        self.after_swing(&report);
        self.perform_extra_attacks();
        self.with_auto_attack(Hand::Offhand, |attack, ctx| attack.schedule_next(ctx));
        SwingOutcome::Swing(report)
    }

    /// Schedules the pending swing of each attacking hand. Port of `start_melee_attack`.
    fn schedule_swings(&mut self) {
        if !self.character.spells.is_melee_attacking() || !self.character.has_mainhand() {
            return;
        }
        self.with_auto_attack(Hand::Mainhand, |attack, ctx| attack.schedule_next(ctx));
        if self.character.is_dual_wielding() {
            self.with_auto_attack(Hand::Offhand, |attack, ctx| attack.schedule_next(ctx));
        }
    }

    /// Re-times both swings after the melee attack speed changed by `haste_change` (a
    /// fraction). Port of `Character::increase_melee_attack_speed`'s swing part.
    fn retime_swings(&mut self, haste_change: f64) {
        let now = self.now();
        self.character
            .spells
            .mh_attack_mut()
            .update_next_expected_use(now, haste_change);
        self.character
            .spells
            .oh_attack_mut()
            .update_next_expected_use(now, haste_change);
        self.schedule_swings();
    }

    // ---------------------------------------------------------------- stance

    /// Puts the character in `stance`: the previous stance spell's buff is cancelled, its
    /// hidden passive disabled and the new stance's passive enabled; rage above the Tactical
    /// Mastery remainder is lost. Port of `Character::swap_stance`.
    pub fn swap_stance(&mut self, stance: Stance) {
        let old = self.character.stance();
        if old == stance {
            return;
        }
        self.character.set_stance(stance);
        if let Some(link) = self.character.stance_link(old) {
            self.cancel_stance_spell(link);
        }
        self.sync_stance_passives();
    }

    /// Cancels a stance spell's buff (unless the spell is being performed right now).
    fn cancel_stance_spell(&mut self, link: StanceLink) {
        if self.character.spells.spell_ids().any(|id| id == link.spell) {
            self.with_spell(link.spell, |spell, ctx| spell.cancel(ctx));
        }
    }

    /// Enables the hidden passive of the current stance and disables the others'.
    fn sync_stance_passives(&mut self) {
        let current = self.character.stance();
        let links: Vec<(Stance, u32)> = Stance::ALL
            .iter()
            .filter_map(|&stance| {
                self.character
                    .stance_link(stance)
                    .and_then(|link| link.passive)
                    .map(|passive| (stance, passive))
            })
            .collect();
        for (stance, passive) in links {
            let Some(id) = self.character.spells.spell_by_game_id(passive) else {
                continue;
            };
            if stance == current {
                self.enable_spell(id);
            } else {
                self.disable_spell(id);
            }
        }
    }

    // ---------------------------------------------------------------- lifecycle

    /// Start of an iteration. Port of `Character::reset`: back to caster form, cooldowns and
    /// resource cleared, every buff removed and every spell and proc reset; the passives are
    /// then re-applied for the new iteration.
    pub fn reset(&mut self) {
        let stance = self.character.stance();
        if stance != Stance::Caster {
            self.character.set_stance(Stance::Caster);
            if let Some(link) = self.character.stance_link(stance) {
                self.cancel_stance_spell(link);
            }
            self.sync_stance_passives();
        }
        for id in self.character.spells.buff_ids().collect::<Vec<_>>() {
            let stacks = self.buff_ref(id).stacks();
            let (buff, mut ctx) = self.buff_ctx(id);
            if buff.reset(&mut ctx).was_active {
                for _ in 0..stacks.max(1) {
                    self.remove_auras(id);
                }
            }
        }
        for id in self.character.spells.spell_ids().collect::<Vec<_>>() {
            self.with_spell(id, |spell, ctx| spell.reset(ctx));
        }
        self.with_procs(|procs, ctx| procs.reset(ctx));
        self.character.reset_state();
        self.reevaluate_passives();
    }

    /// Re-applies the permanent auras of the enabled passives (after a reset, or after the
    /// equipment / stance changed). Port of the C++ talents re-applying their stat changes.
    pub fn reevaluate_passives(&mut self) {
        for id in self.character.spells.spell_ids().collect::<Vec<_>>() {
            self.with_spell(id, |spell, ctx| spell.reevaluate_passive(ctx));
        }
        let enabled: Vec<ProcId> = self.character.spells.procs().enabled().to_vec();
        for id in enabled {
            self.with_procs(|procs, ctx| procs.get_mut(id).spell_mut().reevaluate_passive(ctx));
        }
    }

    /// Before a set of iterations. Port of `Character::prepare_set_of_combat_iterations`
    /// (statistics arrive in Phase 5).
    pub fn prepare_set_of_combat_iterations(&mut self) {
        self.character.prepare_set_of_combat_iterations_state();
        for id in self.character.spells.buff_ids().collect::<Vec<_>>() {
            self.buff_ctx(id).0.initialize();
        }
    }

    /// Combat starts: start-of-combat buffs and spells, then the auto attacks. Port of
    /// `EncounterStart::act` (the rotation is Phase 5).
    pub fn encounter_start(&mut self) {
        for id in self.character.spells.start_of_combat_buffs().to_vec() {
            self.apply_buff(id);
        }
        for id in self.character.spells.start_of_combat_spells().to_vec() {
            let report = self.with_spell(id, |spell, ctx| {
                if spell.is_passive() {
                    // Restart the passive's ticking at combat start.
                    if let Some(marker) = spell.marker_buff() {
                        ctx.cancel_buff(marker);
                    }
                    spell.reevaluate_passive(ctx);
                    None
                } else if spell.is_enabled() {
                    Some(spell.perform(ctx))
                } else {
                    None
                }
            });
            if let Some(report) = report {
                self.after_cast(&report);
                self.perform_extra_attacks();
            }
        }
        self.start_attack();
    }

    /// Handles an event addressed to this character; returns `false` for events it does not
    /// own (`PlayerAction` belongs to the rotation, `IncomingDamage` to tank mode).
    pub fn handle_event(&mut self, event: &Event) -> bool {
        let me = self.character.id();
        match event.kind {
            EventKind::EncounterStart { character } if character == me => {
                self.encounter_start();
            }
            EventKind::MainhandMeleeHit {
                character,
                iteration,
            } if character == me => {
                self.mh_swing(iteration);
            }
            EventKind::OffhandMeleeHit {
                character,
                iteration,
            } if character == me => {
                self.oh_swing(iteration);
            }
            EventKind::BuffRemoval {
                character,
                buff,
                iteration,
            } if character == me => {
                let stacks = self.buff_ref(buff).stacks();
                let (b, mut ctx) = self.buff_ctx(buff);
                if b.remove(iteration, &mut ctx) {
                    for _ in 0..stacks.max(1) {
                        self.remove_auras(buff);
                    }
                }
            }
            EventKind::DotTick {
                character,
                spell,
                application_id,
            } if character == me => {
                self.dot_tick(spell, application_id);
            }
            EventKind::CastComplete {
                character,
                spell,
                cast_id,
            } if character == me => {
                if let Some(report) = self.with_spell(spell, |s, ctx| s.complete_cast(cast_id, ctx))
                {
                    self.after_cast(&report);
                    self.perform_extra_attacks();
                }
            }
            _ => return false,
        }
        true
    }

    /// A periodic tick of `spell`; returns the tick report if the application is current.
    pub fn dot_tick(&mut self, spell: SpellId, application_id: u32) -> Option<TickReport> {
        self.with_spell(spell, |s, ctx| s.perform_periodic(application_id, ctx))
    }

    /// Uses a charge of every buff that reacts to `source`.
    pub fn consume_charges(&mut self, source: ProcSource) {
        for id in self.character.spells.charge_consumers(source) {
            let (buff, mut ctx) = self.buff_ctx(id);
            if buff.use_charge(&mut ctx) == ChargeUse::Removed {
                self.remove_auras(id);
            }
        }
    }

    // ---------------------------------------------------------------- equipment

    /// Whether the equipped items satisfy a `SpellEquippedItems` requirement: some equipped
    /// weapon / held item of the required item class whose subclass bit is in the mask.
    pub fn equipped_item_matches(&self, requirement: &EquippedItems) -> bool {
        if requirement.class <= 0 {
            return true;
        }
        let class = requirement.class as u32;
        [
            EquipmentSlot::Mainhand,
            EquipmentSlot::Offhand,
            EquipmentSlot::Ranged,
        ]
        .into_iter()
        .filter_map(|slot| self.character.equipment().weapon_profile(slot))
        .any(|weapon| {
            let (item_class, subclass) = weapon.weapon_type.item_class_subclass();
            item_class == class
                && (requirement.subclass_mask == 0
                    || requirement.subclass_mask & (1 << subclass) != 0)
        })
    }
}

impl<S: SharedBuffs> EffectHost for CharacterContext<'_, S> {
    fn caster_level(&self) -> u32 {
        self.character.clvl()
    }
    fn combo_points(&self) -> u32 {
        self.character.combo_points()
    }
    fn gain_combo_points(&mut self, amount: u32) {
        self.character.gain_combo_points(amount);
    }
    fn spend_combo_points(&mut self) {
        self.character.spend_combo_points();
    }
    fn resource_level(&self, resource: ResourceType) -> u32 {
        self.character.resource_level(resource)
    }
    /// Rage gains wake the player up (`Warrior::gain_rage` adds a reaction event).
    fn gain_resource(&mut self, resource: ResourceType, amount: u32) -> u32 {
        let gained = self.character.gain_resource(resource, amount);
        if resource == ResourceType::Rage && gained > 0 {
            self.add_player_reaction_event();
        }
        gained
    }
    fn melee_ap(&self) -> u32 {
        self.character.melee_ap(&self.target_view())
    }
    fn random_in_range(&mut self, min: f64, max: f64) -> f64 {
        self.character.random_in_range(min, max)
    }
    fn random_normalized_mh_dmg(&mut self) -> f64 {
        let view = self.target_view();
        self.character.random_normalized_mh_dmg(&view)
    }
    fn random_non_normalized_mh_dmg(&mut self) -> f64 {
        let view = self.target_view();
        self.character.random_non_normalized_mh_dmg(&view)
    }
    fn roll_melee_ability(
        &mut self,
        included: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult {
        let view = self.target_view();
        let roll_ctx = self.character.refresh_roll_context(&view);
        let stat_ctx = self.character.stat_context(&view);
        let skill = self.character.stats().get_mh_wpn_skill(&stat_ctx);
        let crit = if can_crit {
            self.character.stats().get_mh_crit_chance(&stat_ctx) + extra_crit
        } else {
            0
        };
        self.character
            .roll_mut()
            .get_melee_ability_result(&roll_ctx, skill, crit, included)
    }
    fn stats_mut(&mut self) -> &mut CharacterStats {
        self.character.stats_mut()
    }
    fn target_mut(&mut self) -> &mut Target {
        self.target
    }
    fn increase_melee_attack_speed(&mut self, percent: u32) {
        self.character
            .stats_mut()
            .increase_melee_attack_speed(percent);
        self.retime_swings(f64::from(percent) / 100.0);
    }
    fn decrease_melee_attack_speed(&mut self, percent: u32) {
        self.character
            .stats_mut()
            .decrease_melee_attack_speed(percent);
        self.retime_swings(-f64::from(percent) / 100.0);
    }
    fn swap_stance(&mut self, stance: Stance) {
        CharacterContext::swap_stance(self, stance);
    }
    fn spell_modifiers(&self) -> &SpellModifiers {
        self.character.spell_modifiers()
    }
    fn spell_modifiers_mut(&mut self) -> &mut SpellModifiers {
        self.character.spell_modifiers_mut()
    }
    fn add_extra_attacks(&mut self, count: u32) {
        self.character.add_extra_attacks(count);
    }
    fn adjust_stance_rage_retained(&mut self, delta: i32) {
        self.character.adjust_stance_rage_retained(delta);
    }
    fn adjust_offhand_damage_percent(&mut self, percent: i32) {
        self.character.adjust_offhand_damage_percent(percent);
    }
    fn adjust_offhand_rage_percent(&mut self, percent: i32) {
        self.character.adjust_offhand_rage_percent(percent);
    }
    fn override_actionbar_spell(&mut self, replaced: u32, replacement: u32, apply: bool) {
        self.character
            .spells_mut()
            .apply_actionbar_override(replaced, replacement, apply);
    }
}

impl<S: SharedBuffs> SpellHost for CharacterContext<'_, S> {
    fn character_id(&self) -> CharId {
        self.character.id()
    }
    fn engine(&self) -> &Engine {
        self.engine
    }
    fn engine_mut(&mut self) -> &mut Engine {
        self.engine
    }
    fn combat_length(&self) -> f64 {
        self.character.sim().combat_length
    }
    fn on_global_cooldown(&self) -> bool {
        self.character.on_global_cooldown(self.now())
    }
    fn global_cooldown(&self) -> f64 {
        self.character.global_cooldown()
    }
    fn start_global_cooldown(&mut self) {
        let now = self.now();
        self.character.start_global_cooldown(now);
    }
    fn on_stance_cooldown(&self) -> bool {
        self.character.on_stance_cooldown(self.now())
    }
    /// The stance swap lag pushes the global cooldown forward slightly; the player action is
    /// scheduled for then. Port of `Character::start_stance_cooldown`.
    fn start_stance_cooldown(&mut self) {
        let now = self.now();
        if let Some(at) = self.character.start_stance_cooldown(now) {
            let character = self.character.id();
            self.engine
                .add_event(Event::new(at, EventKind::PlayerAction { character }));
        }
    }
    fn cast_in_progress(&self) -> bool {
        self.character.spells().cast_in_progress()
    }
    fn start_cast(&mut self) -> u32 {
        self.character.spells_mut().start_cast()
    }
    fn complete_cast(&mut self, cast_id: u32) {
        self.character.spells_mut().complete_cast(cast_id);
        self.add_player_reaction_event();
    }
    fn casting_speed_mod(&self) -> f64 {
        self.character.stats().get_casting_speed_mod()
    }
    fn casting_speed_flat_reduction(&self) -> u32 {
        self.character.stats().get_casting_speed_flat_reduction()
    }
    fn stop_attack(&mut self) {
        self.character.spells_mut().stop_attack();
    }
    /// Port of `CharacterSpells::start_attack`: marks the character as attacking and schedules
    /// the swings of each hand.
    fn start_attack(&mut self) {
        self.character.spells_mut().start_attack();
        self.schedule_swings();
    }
    fn reset_swing_timers(&mut self) {
        self.with_auto_attack(Hand::Mainhand, |attack, ctx| {
            attack.reset_swing_timer_and_schedule(ctx)
        });
        if self.character.is_dual_wielding() {
            self.with_auto_attack(Hand::Offhand, |attack, ctx| {
                attack.reset_swing_timer_and_schedule(ctx)
            });
        }
    }
    fn queue_next_swing(&mut self, spell: SpellId) {
        self.character.spells_mut().queue_next_swing(spell);
    }
    fn cancel_next_swing(&mut self) {
        self.character.spells_mut().cancel_next_swing();
    }
    fn queued_next_swing(&self) -> Option<SpellId> {
        self.character.spells().queued_next_swing()
    }
    fn stance(&self) -> Stance {
        self.character.stance()
    }
    fn equipped_item_matches(&self, requirement: &EquippedItems) -> bool {
        CharacterContext::equipped_item_matches(self, requirement)
    }
    fn caster_aura_state(&self, state: AuraState) -> bool {
        match state {
            AuraState::None => true,
            AuraState::Defensive | AuraState::Defensive2 => {
                self.character.in_defensive_state(self.now())
            }
            AuraState::Enraged => {
                let spells = self.character.spells();
                spells.spell_ids().any(|id| {
                    let spell = spells.spell(id);
                    spell.has_sim_flag(SimFlag::Enrage)
                        && spell
                            .marker_buff()
                            .is_some_and(|buff| self.buff_ref(buff).is_active())
                })
            }
            _ => false,
        }
    }
    fn target_aura_state(&self, state: AuraState) -> bool {
        state == AuraState::None
    }
    fn aura_active(&self, spell: u32) -> bool {
        self.character
            .spells()
            .buff_ids()
            .map(|id| self.buff_ref(id))
            .any(|buff| buff.spell() == spell && buff.is_active())
    }
    fn lose_resource(&mut self, resource: ResourceType, amount: u32) {
        let now = self.now();
        self.character.lose_resource(resource, amount, now);
    }
    fn cooldown(&self, id: CooldownId) -> &CooldownControl {
        self.character.spells().cooldowns().get(id)
    }
    fn cooldown_mut(&mut self, id: CooldownId) -> &mut CooldownControl {
        self.character.spells_mut().cooldowns_mut().get_mut(id)
    }
    fn buff(&self, id: BuffId) -> &Buff {
        self.buff_ref(id)
    }
    fn buff_mut(&mut self, id: BuffId) -> &mut Buff {
        self.buff_ctx(id).0
    }
    /// Aura effects are applied once per stack (Sunder Armor's armor reduction × 5).
    fn apply_buff(&mut self, id: BuffId) -> BuffApplication {
        let before = self.buff_ref(id).stacks();
        let (buff, mut ctx) = self.buff_ctx(id);
        let application = buff.apply(&mut ctx);
        match application {
            BuffApplication::Applied { evicted } => {
                if let Some(evicted) = evicted {
                    let victim = self
                        .character
                        .spells()
                        .buff_ids()
                        .find(|id| self.buff_ref(*id).instance_id() == Some(evicted));
                    if let Some(victim) = victim {
                        self.cancel_buff(victim);
                    }
                }
                self.apply_auras(id);
            }
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
        if self.character.spells().owned_buff(id).is_some() {
            self.character.spells_mut().enable_buff(id);
        } else {
            self.buff_ctx(id).0.enable();
        }
    }
    fn disable_buff(&mut self, id: BuffId) {
        self.character.spells_mut().disable_buff(id);
    }
    fn trigger_spell(&mut self, spell: u32, trigger_value: Option<f64>) -> Option<CastReport> {
        let id = self.character.spells().spell_by_game_id(spell)?;
        Some(self.with_spell(id, |s, ctx| {
            s.set_trigger_value(trigger_value);
            s.perform(ctx)
        }))
    }
    fn set_spell_effect_value(&mut self, spell: u32, index: u32, value: f64) {
        if let Some(id) = self.character.spells().spell_by_game_id(spell) {
            self.with_spell(id, |s, ctx| s.set_effect_value(ctx, index, value));
        } else if let Some(id) = self.character.spells().proc_by_game_id(spell) {
            self.with_procs(|procs, ctx| {
                procs
                    .get_mut(id)
                    .spell_mut()
                    .set_effect_value(ctx, index, value);
            });
        }
    }
    fn target_armor(&self) -> i32 {
        self.target.armor()
    }
    fn total_physical_damage_mod(&self) -> f64 {
        let view = self.target_view();
        self.character
            .stats()
            .get_total_physical_damage_mod(&self.character.stat_context(&view))
    }
    fn flat_physical_damage_bonus(&self) -> u32 {
        self.character.stats().get_flat_physical_damage_bonus()
    }
    fn melee_ability_crit_dmg_mod(&self) -> f64 {
        let view = self.target_view();
        self.character
            .stats()
            .get_melee_ability_crit_dmg_mod(&self.character.stat_context(&view))
    }
    fn total_threat_mod(&self) -> f64 {
        self.character.stats().get_total_threat_mod()
    }
    fn avg_mh_damage(&self) -> f64 {
        f64::from(self.character.avg_mh_damage(&self.target_view()))
    }
}

impl<S: SharedBuffs> ProcHost for CharacterContext<'_, S> {
    fn base_weapon_speed(&self, hand: Hand) -> Option<f64> {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        self.character
            .equipment()
            .weapon_profile(slot)
            .map(|weapon| weapon.speed)
    }
}

impl<S: SharedBuffs> AutoAttackHost for CharacterContext<'_, S> {
    fn weapon_speed(&self, hand: Hand) -> Option<f64> {
        self.character.weapon_speed(hand, &self.target_view())
    }
    fn weapon_skill(&self, hand: Hand) -> u32 {
        self.character.weapon_skill(hand, &self.target_view())
    }
    fn roll_melee_hit(&mut self, hand: Hand) -> PhysicalAttackResult {
        let view = self.target_view();
        let roll_ctx = self.character.refresh_roll_context(&view);
        let stat_ctx = self.character.stat_context(&view);
        let (skill, crit) = match hand {
            Hand::Mainhand => (
                self.character.stats().get_mh_wpn_skill(&stat_ctx),
                self.character.stats().get_mh_crit_chance(&stat_ctx),
            ),
            Hand::Offhand => (
                self.character.stats().get_oh_wpn_skill(&stat_ctx),
                self.character.stats().get_oh_crit_chance(&stat_ctx),
            ),
        };
        self.character
            .roll_mut()
            .get_melee_hit_result(&roll_ctx, skill, crit)
    }
    fn glancing_blow_dmg_penalty(&mut self, weapon_skill: u32) -> f64 {
        let clvl = self.character.clvl();
        self.character
            .roll_mut()
            .get_glancing_blow_dmg_penalty(clvl, weapon_skill)
    }
    fn random_non_normalized_oh_dmg(&mut self) -> f64 {
        let view = self.target_view();
        self.character.random_non_normalized_oh_dmg(&view)
    }
    fn avg_oh_damage(&self) -> f64 {
        f64::from(self.character.avg_oh_damage(&self.target_view()))
    }
    fn melee_crit_dmg_mod(&self) -> f64 {
        2.0
    }
    fn rage_from_damage(&self, hand: Hand, damage: f64) -> Option<u32> {
        self.character.rage_from_damage(hand, damage)
    }
    /// Port of `Character::add_player_reaction_event`: the rotation runs again 0.1 s later.
    fn add_player_reaction_event(&mut self) {
        let character = self.character.id();
        self.engine
            .add_event_in(0.1, EventKind::PlayerAction { character });
    }
    fn is_melee_attacking(&self) -> bool {
        self.character.spells().is_melee_attacking()
    }
}
