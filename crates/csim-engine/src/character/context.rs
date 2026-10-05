use std::sync::Arc;

use crate::buff::external::{ConsumableSpec, ExternalBuffDb};
use crate::buff::{Buff, BuffApplication, BuffContext, BuffKind, ChargeUse};
use crate::character_spells::{
    AddedSpell, BuffSlot, EquipmentGrantor, EquipmentSpellKey, PartyAuraChange, SharedBuffs,
    SpellHandle,
};
use crate::combat_log::{
    AuraChange, CombatLog, CombatLogEvent, Damage, LogSpell, LogUnit, MissType, UnitInfo,
};
use crate::combat_roll::{
    IncludedOutcomes, MagicResistResult, PhysicalAttackResult, SpellResistKind, SpellRoll,
};
use crate::cooldown::CooldownControl;
use crate::effect::{Effect, EffectHost};
use crate::enchant::EnchantName;
use crate::engine::{Engine, Event, EventKind, PLAYER_REACTION_DELAY};
use crate::equipment::{EnchantError, EquipChange, EquipError};
use crate::ids::{BuffId, CharId, CooldownId, ProcId, SpellId};
use crate::item::{EffectTrigger, EquipmentSlot, ItemEffect, WeaponType};
use crate::magic_school::MagicSchool;
use crate::proc::{ProcHost, ProcSource, ProcTrigger};
use crate::resource::ResourceType;
use crate::rotation::{
    BuiltinVariable, ConditionContext, Rotation, RotationHost, RotationSpec, Watched,
};
use crate::rulesets::Ruleset;
use crate::spell::dbc::{AuraState, SpellAttr1};
use crate::spell::modifiers::SpellModifiers;
use crate::spell::overrides::{EventScript, ProcOverride, ScriptKind, SimFlag, SpellOverride};
use crate::spell::periodic::TickReport;
use crate::spell::record::{EquippedItems, SpellDb};
use crate::spell::{
    AttackOutcome, AutoAttack, AutoAttackHost, CastReport, Hand, Spell, SpellHost, SpellResult,
    SpellSetup, SpellStatus, SwingReport,
};
use crate::stance::Stance;
use crate::statistics::{
    ClassStatistics, EngineStatistics, RotationExecutorStatistics, SkippedExecutor,
};
use crate::stats::{CharacterStats, TargetStatView};
use crate::talent::{CharacterTalents, RankChange};
use crate::target::{CreatureType, Target};

use super::{Character, RegenReactions, SimParams, StanceLink};

/// The resource statistics source of regeneration ticks.
pub const REGENERATION: &str = "Regeneration";

/// How long before now the resource is read (rotation conditions, costs, the combat log): a
/// regeneration tick landing at an instant is seen from the next instant on, while spends and
/// gains at that instant come after it (a spend at 0.0 gets its first tick at 0.1). The
/// reaction to a tick falls 0.1 s after it, on the next tick at the base rate: it sees the tick
/// it reacts to, not the one landing with it, so the player acts 0.1 s after the tick that
/// makes a spell affordable, whichever event runs the rotation at that instant.
pub const TICK_READ_LAG: f64 = 1e-6;

/// `SpellCategories.DispelType` of poisons.
const DISPEL_TYPE_POISON: u32 = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum SwingOutcome {
    /// The swing event was stale or the character is not attacking.
    Skipped,
    Swing(SwingReport),
    /// A queued on-next-swing spell (Heroic Strike) landed instead of the swing.
    NextSwingSpell(CastReport),
}

/// Why an external buff could not be toggled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExternalBuffToggleError {
    #[error("the character is not offered the external buff {0:?}")]
    NotOffered(String),
    #[error("the external buff {0:?} is not available to the {1}")]
    WrongFaction(String, &'static str),
}

/// The first rank of spell `id`: the start of its `supercedes` chain.
fn base_rank(db: &SpellDb, id: u32) -> u32 {
    let mut current = id;
    let mut hops = 0;
    const MAX_RANK_CHAIN: u32 = 32;
    while let Some(record) = db.get(current) {
        assert!(
            hops < MAX_RANK_CHAIN,
            "Cycle in data - could not find base rank for id {id}"
        );
        if record.supercedes == 0 {
            break;
        }
        current = record.supercedes;
        hops += 1;
    }
    current
}

/// The character together with everything it acts on.
pub struct CharacterContext<'a, S: SharedBuffs> {
    pub character: &'a mut Character,
    pub engine: &'a mut Engine,
    pub target: &'a mut Target,
    pub raid: &'a mut S,
}

impl<'a, S: SharedBuffs> CharacterContext<'a, S> {
    /// Whether the demands of `spell` beyond time and resource hold
    /// ([`crate::spell::Spell::requirements_status`]).
    pub fn spell_requirements(&self, spell: SpellId) -> SpellStatus {
        self.character.spells.spell(spell).requirements_status(self)
    }

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

    /// The time the resource is read at: just before now, so that a regeneration tick landing
    /// now is not seen yet (see [`TICK_READ_LAG`]).
    fn resource_read_time(&self) -> f64 {
        self.now() - TICK_READ_LAG
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

    /// Cancels the active buffs of the spells that end with buff `id` (`ends_auras`).
    fn end_linked_auras(&mut self, id: BuffId) {
        let spells = self.buff_ref(id).ends_auras().to_vec();
        if spells.is_empty() {
            return;
        }
        let linked: Vec<BuffId> = self
            .character
            .spells
            .buff_ids()
            .filter(|&other| {
                let buff = self.buff_ref(other);
                spells.contains(&buff.spell()) && buff.is_active()
            })
            .collect();
        for other in linked {
            self.cancel_buff(other);
        }
    }

    pub(crate) fn buff_ref(&self, id: BuffId) -> &Buff {
        match self.character.spells.buff_slot(id) {
            BuffSlot::Owned(buff) => buff,
            BuffSlot::Shared(handle) => self.raid.shared_buff(*handle),
        }
    }

    /// The active external debuff standing in for the character's own debuff `id` (the raid's
    /// Sunder Armor for the Warrior's): a target debuff is one aura whoever keeps it up, so
    /// while the external one is selected the own one is not applied, and conditions on it read
    /// the external one.
    fn external_stand_in(&self, id: BuffId) -> Option<BuffId> {
        let buff = self.buff_ref(id);
        if !buff.kind().is_debuff() {
            return None;
        }
        self.character
            .general_buffs
            .entries()
            .iter()
            .find(|e| e.debuff && e.spec.name == buff.name() && self.buff_ref(e.buff).is_active())
            .map(|e| e.buff)
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
        self.change_auras(id, true);
    }

    fn remove_auras(&mut self, id: BuffId) {
        self.change_auras(id, false);
    }

    /// Applies (or removes) the aura effects of buff `id` on this character. A shared party
    /// buff affects the whole party: the change is noted with the raid, which repeats it on the
    /// other members (`RaidControl::propagate_party_auras`).
    fn change_auras(&mut self, id: BuffId, apply: bool) {
        let effects = self.buff_ref(id).effects.clone();
        self.change_aura_effects(&effects, apply);
        if let BuffSlot::Shared(handle) = self.character.spells.buff_slot(id) {
            let handle = *handle;
            if let BuffKind::PartyBuff { party } = self.raid.shared_buff(handle).kind() {
                self.raid.note_party_aura_change(PartyAuraChange {
                    buff: handle,
                    party,
                    by: self.character.id(),
                    apply,
                });
            }
        }
    }

    /// Applies (or removes) aura `effects` on this character, whichever buff they belong to.
    pub(crate) fn change_aura_effects(&mut self, effects: &[Effect], apply: bool) {
        for effect in effects {
            let on_target = effect.record().targets_enemy();
            if apply {
                effect.apply_aura(self, on_target);
            } else {
                effect.remove_aura(self, on_target);
            }
        }
    }

    // ---------------------------------------------------------------- learning spells

    /// Adds spell `id` from `db` and enables it when it is a trainable / racial spell. Stance
    /// spells register their stance link. Spells a talent of the attached tree grants (the
    /// talent spell and its higher ranks, whatever their `class_mask` says) wait for the
    /// talent. Port of the spell construction in the C++ class constructors.
    pub fn learn(&mut self, db: &SpellDb, id: u32) -> AddedSpell {
        let party = self.character.party();
        let mut added = self.character.spells.add_spell(db, id, party, self.raid);
        let base = base_rank(db, id);
        let granted = self.character.talent_grants(base);
        if granted {
            added.enable_now = false;
        }
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
        if let Some(proc) = added.proc
            && added.enable_now
        {
            self.enable_proc(proc);
        }
        // A payload learned after the spell that casts it.
        if self.enabled_source_of(id) {
            self.set_payloads_enabled(&[id], true);
        }
        // A talent spell (or a rank of one) learned after its talent got points.
        if granted {
            let change = self.character.talents().and_then(|t| {
                let node = t.node_of_spell(base)?;
                let rank = t.rank(node);
                (rank > 0).then_some(RankChange {
                    node,
                    spell: base,
                    from: 0,
                    to: rank,
                })
            });
            if let Some(change) = change {
                self.apply_talent_change(change);
            }
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

    /// Learns every spell of `db` that the character's class or race can have: the spells of
    /// its class file (any `class_mask`, including the talent-granted ones which stay disabled)
    /// and of the class-independent files, where the racials must name the race in their
    /// `race_mask`. Other classes' files are skipped, and the external buff auras
    /// (`SpellDb::is_learnable`) are not spells of the character.
    pub fn learn_all(&mut self, db: &SpellDb) -> Vec<AddedSpell> {
        let race = self.character.race();
        let class = self.character.class().class;
        let mut ids: Vec<u32> = db
            .ids_of_class(Some(class))
            .iter()
            .chain(db.ids_of_class(None))
            .filter_map(|&id| db.get(id))
            .filter(|record| db.is_learnable(record.id))
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

    // ---------------------------------------------------------------- simulation settings

    /// Replaces the simulation settings and applies the ruleset: its stat change
    /// (`Character::set_sim`) and its spells ([`CharacterContext::sync_ruleset_spells`]).
    /// Port of `SimSettings::use_ruleset` / `RulesetControl::use_ruleset`.
    pub fn set_sim(&mut self, sim: SimParams, db: &SpellDb) {
        self.character.set_sim(sim);
        self.sync_ruleset_spells(db);
    }

    /// Enables the spells of the active ruleset (Essence of the Red under Vaelastrasz),
    /// learning them on first use, and disables those of the other rulesets.
    ///
    /// # Panics
    /// Panics if the active ruleset's spell is not in `db`.
    pub fn sync_ruleset_spells(&mut self, db: &SpellDb) {
        let active = self.character.sim().ruleset;
        for game_id in Ruleset::all_spells() {
            let wanted = active.spells().contains(&game_id);
            let id = match self.character.spells.spell_by_game_id(game_id) {
                Some(id) => id,
                None if wanted => self
                    .learn(db, game_id)
                    .spell
                    .unwrap_or_else(|| panic!("ruleset spell {game_id} is not a castable spell")),
                None => continue,
            };
            if wanted {
                self.enable_spell(id);
            } else {
                self.disable_spell(id);
            }
        }
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

    /// Enables proc `id`, unless the overrides mark it `IGNORED` (the other classes' Touch of
    /// the Grave): such a proc stays registered but never rolls.
    pub fn enable_proc(&mut self, id: ProcId) {
        let payloads = self.with_procs(|procs, ctx| {
            if procs.is_enabled(id) || procs.get(id).spell().is_ignored() {
                return Vec::new();
            }
            procs.enable(id, ctx);
            procs.get(id).payload_spells()
        });
        self.set_payloads_enabled(&payloads, true);
    }

    /// Disables proc `id` and the payloads no other enabled proc casts (the poison of the
    /// other hand keeps the shared Instant Poison).
    pub fn disable_proc(&mut self, id: ProcId) {
        let payloads = self.with_procs(|procs, ctx| {
            if !procs.is_enabled(id) {
                return Vec::new();
            }
            procs.disable(id, ctx);
            let mut payloads = procs.get(id).payload_spells();
            payloads.retain(|payload| {
                !procs
                    .enabled()
                    .iter()
                    .any(|&other| procs.get(other).payload_spells().contains(payload))
            });
            payloads
        });
        self.set_payloads_enabled(&payloads, false);
    }

    /// Enables / disables the learned hidden payloads among `ids` (spellbook spells are
    /// enabled by their own learning / talent, never as a payload; a hidden aura enabled by
    /// an `ENABLE_PROC` aura is one of that aura's payloads).
    fn set_payloads_enabled(&mut self, ids: &[u32], enabled: bool) {
        for &game_id in ids {
            let spells = &self.character.spells;
            let spell = spells
                .spell_by_game_id(game_id)
                .map(|id| spells.spell(id))
                .or_else(|| {
                    spells
                        .proc_by_game_id(game_id)
                        .map(|id| spells.procs().get(id).spell())
                });
            let is_payload = spell.is_some_and(|spell| {
                let record = spell.record();
                !record.is_in_spellbook()
                    || (record.acquire_method == 3 && record.class_mask == 0)
                    || spell.setup().enabled_by.is_some()
            });
            if !is_payload {
                continue;
            }
            match self.character.spells.handle(game_id) {
                Some(SpellHandle::Spell(id)) => {
                    if enabled {
                        self.enable_spell(id);
                    } else {
                        self.disable_spell(id);
                    }
                }
                Some(SpellHandle::Proc(id)) => {
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

    // ---------------------------------------------------------------- talents

    /// Attaches the talent setups and brings the spells in line with the current setup:
    /// every talent spell (and the ranks it grants) is enabled with its rank values when the
    /// talent has points and disabled otherwise. Port of the talent trees' construction in
    /// the C++ class constructors.
    pub fn set_talents(&mut self, talents: CharacterTalents) {
        self.character.set_talents_unsynced(Some(talents));
        self.sync_talents();
    }

    /// Re-applies the current setup to the spells (after learning spells, or after the
    /// talents were changed through `Character::talents_mut`).
    pub fn sync_talents(&mut self) {
        let Some(talents) = self.character.talents() else {
            return;
        };
        let changes: Vec<RankChange> = talents
            .file()
            .talents
            .iter()
            .map(|spec| RankChange {
                node: spec.node,
                spell: spec.spell,
                from: 0,
                to: talents.rank(spec.node),
            })
            .collect();
        for change in changes {
            self.apply_talent_change(change);
        }
    }

    /// Spends a point in talent `node` of the current setup and applies the new rank.
    /// Returns `false` when the rules (tier, prerequisite, budget) refuse it.
    pub fn increment_talent(&mut self, node: u32) -> bool {
        let change = self
            .character
            .talents_mut()
            .and_then(|t| t.increment_rank(node));
        self.apply_talent_changes(change)
    }

    /// Takes a point out of talent `node` and applies the new rank.
    pub fn decrement_talent(&mut self, node: u32) -> bool {
        let change = self
            .character
            .talents_mut()
            .and_then(|t| t.decrement_rank(node));
        self.apply_talent_changes(change)
    }

    /// Spends points in `node` until it is maxed or none remain.
    pub fn max_talent(&mut self, node: u32) -> bool {
        let changes = self
            .character
            .talents_mut()
            .map(|t| t.increase_to_max_rank(node))
            .unwrap_or_default();
        self.apply_talent_changes(changes)
    }

    /// Takes points out of `node` while the rules allow.
    pub fn min_talent(&mut self, node: u32) -> bool {
        let changes = self
            .character
            .talents_mut()
            .map(|t| t.decrease_to_min_rank(node))
            .unwrap_or_default();
        self.apply_talent_changes(changes)
    }

    /// Refunds every point of tab `skill_line` of the current setup.
    pub fn clear_talent_tab(&mut self, skill_line: u32) -> bool {
        let changes = self
            .character
            .talents_mut()
            .map(|t| t.clear_tab(skill_line))
            .unwrap_or_default();
        self.apply_talent_changes(changes)
    }

    /// Refunds every point of the current setup.
    pub fn clear_talents(&mut self) -> bool {
        let changes = self
            .character
            .talents_mut()
            .map(CharacterTalents::clear_all)
            .unwrap_or_default();
        self.apply_talent_changes(changes)
    }

    /// Switches to talent setup `index`: the current setup's ranks come off the spells, the
    /// new setup's go on. Port of `CharacterTalents::set_current_index`.
    pub fn switch_talent_setup(&mut self, index: usize) -> bool {
        let changes = self
            .character
            .talents_mut()
            .map(|t| t.set_current_index(index))
            .unwrap_or_default();
        self.apply_talent_changes(changes)
    }

    /// Spends points into the current setup so each `(node, rank)` reaches its rank, in
    /// order (a setup lists talents tier by tier). Returns the entries that could not be
    /// reached, with the rank they stopped at.
    pub fn spend_talent_points(&mut self, setup: &[(u32, u32)]) -> Vec<(u32, u32)> {
        let mut short = Vec::new();
        for &(node, rank) in setup {
            while self
                .character
                .talents()
                .is_some_and(|t| t.rank(node) < rank)
            {
                if !self.increment_talent(node) {
                    break;
                }
            }
            let reached = self.character.talents().map_or(0, |t| t.rank(node));
            if reached < rank {
                short.push((node, reached));
            }
        }
        short
    }

    /// Applies rank changes to all ranks of a given spell.
    pub fn apply_talent_changes(&mut self, changes: impl IntoIterator<Item = RankChange>) -> bool {
        let mut any = false;
        for change in changes {
            self.apply_talent_change(change);
            any = true;
        }
        any
    }

    /// Apply a talent effect to a specific rank of a spell.
    fn apply_talent_change(&mut self, change: RankChange) {
        let Some(values) = self
            .character
            .talents()
            .and_then(|t| t.spec(change.node))
            .map(|spec| spec.values_at(change.to))
        else {
            return;
        };
        let spell = change.spell;
        if change.to == 0 {
            for handle in self.talent_spell_handles(spell) {
                match handle {
                    SpellHandle::Spell(id) => self.disable_spell(id),
                    SpellHandle::Proc(id) => self.disable_proc(id),
                }
            }
            self.reset_spell_effect_values(spell);
            return;
        }
        for (index, value) in values {
            self.set_spell_effect_value(spell, index, value);
        }
        for handle in self.talent_spell_handles(spell) {
            match handle {
                SpellHandle::Spell(id) => self.enable_spell(id),
                SpellHandle::Proc(id) => self.enable_proc(id),
            }
        }
    }

    /// The talent spell's handle and, for an ability, the other ranks of its rank group.
    fn talent_spell_handles(&self, spell: u32) -> Vec<SpellHandle> {
        let spells = self.character.spells();
        let Some(handle) = spells.handle(spell) else {
            return Vec::new();
        };
        let mut handles = vec![handle];
        if let SpellHandle::Spell(id) = handle
            && let Some(group) = spells.rank_group_of(id)
        {
            handles.extend(
                group
                    .spells()
                    .filter(|&other| other != id)
                    .map(SpellHandle::Spell),
            );
        }
        handles
    }

    /// Restores the table values of every effect of spell `spell`.
    fn reset_spell_effect_values(&mut self, spell: u32) {
        if let Some(id) = self.character.spells().spell_by_game_id(spell) {
            self.with_spell(id, |s, ctx| s.reset_effect_values(ctx));
        } else if let Some(id) = self.character.spells().proc_by_game_id(spell) {
            self.with_procs(|procs, ctx| {
                procs.get_mut(id).spell_mut().reset_effect_values(ctx);
            });
        }
    }

    // ---------------------------------------------------------------- equipment spells

    /// Equips `item_id` in `slot` and registers what it grants.
    pub fn equip(
        &mut self,
        db: &SpellDb,
        slot: EquipmentSlot,
        item_id: u32,
    ) -> Result<EquipChange, EquipError> {
        let change = self.character.equipment.equip(slot, item_id)?;
        self.sync_equipment_spells(db);
        Ok(change)
    }

    /// Empties `slot` and disables what its item granted.
    pub fn unequip(&mut self, db: &SpellDb, slot: EquipmentSlot) -> EquipChange {
        let change = self.character.equipment.unequip(slot);
        self.sync_equipment_spells(db);
        change
    }

    /// Puts the permanent enchant `enchant` on the item in `slot` (`None` removes it) and
    /// registers what it grants.
    pub fn set_enchant(
        &mut self,
        db: &SpellDb,
        slot: EquipmentSlot,
        enchant: Option<EnchantName>,
    ) -> Result<(), EnchantError> {
        self.character.equipment.set_enchant(slot, enchant)?;
        self.sync_equipment_spells(db);
        Ok(())
    }

    /// Replaces the temporary enchants (sharpening stone, oil, Windfury Totem, poison) of the
    /// item in `slot`, at most one per group (an empty list scrapes them off), and registers
    /// what they grant.
    pub fn set_temp_enchants(
        &mut self,
        db: &SpellDb,
        slot: EquipmentSlot,
        enchants: &[EnchantName],
    ) -> Result<(), EnchantError> {
        self.character.equipment.set_temp_enchants(slot, enchants)?;
        self.sync_equipment_spells(db);
        Ok(())
    }

    /// Adds a temporary enchant to the item in `slot`, replacing the one of its group, and
    /// registers what it grants.
    pub fn add_temp_enchant(
        &mut self,
        db: &SpellDb,
        slot: EquipmentSlot,
        enchant: EnchantName,
    ) -> Result<(), EnchantError> {
        self.character.equipment.add_temp_enchant(slot, enchant)?;
        self.sync_equipment_spells(db);
        Ok(())
    }

    /// Brings the spells the equipment grants in line with what is worn. Each is registered
    /// once per grantor (with the payloads it casts learned) and enabled; those of an item,
    /// enchant or set bonus no longer worn are disabled. Port of `Item::apply_proc`, the
    /// `EnchantProc` constructor / destructor and `SetBonusControl`, which created and destroyed
    /// them with the equipment.
    ///
    /// - An enchant's `procs` entry that names a spell is a proc of the enchanted slot.
    /// - An item's on-equip effect and a reached set bonus are passives: a proc aura (with a
    ///   `ProcTypeMask`) becomes a proc, any other aura is up while worn (stats, spell
    ///   modifiers).
    /// - An item's chance-on-hit effect is a proc of the wielding hand casting the spell at the
    ///   target ([`crate::proc::Proc::on_hit`]); one whose rate is unknown is skipped.
    /// - An item's on-use effect is a spell the rotation casts by its name, with the item's
    ///   cooldown and shared category cooldown (the trinkets' 1141) in place of the record's.
    ///   One the sim cannot run (an effect without its script, a proc aura while the buff is
    ///   up: Badge of the Swarmguard) is skipped, so it does not start the shared cooldown for
    ///   nothing. Using an item does not break Stealth.
    /// - A consumable's use effect ([`Self::set_consumables`]) is the same, cast by the
    ///   consumable's name (Thistle Tea, not its spell's Restore Energy).
    ///
    /// Spells missing from `db` (pruned by the export: stuns, heals, immunities, ...) and
    /// ignored ones (`IGNORED`) are skipped. The rotation is linked again afterwards.
    pub fn sync_equipment_spells(&mut self, db: &SpellDb) {
        let mut wanted = Vec::new();
        let mut uses = Vec::new();
        for grant in self.granted_equipment_spells(db) {
            let Some(record) = db.get(grant.spell) else {
                continue;
            };
            let ignored = db
                .overrides()
                .get(grant.spell)
                .is_some_and(|o| o.sim_flags.contains(&SimFlag::Ignored));
            if ignored {
                continue;
            }
            let runnable_use = || {
                !record.is_passive()
                    && !record.is_proc_aura()
                    && db.unsupported_effects(record).is_empty()
            };
            if matches!(grant.kind, GrantKind::Use(..)) && !runnable_use() {
                continue;
            }
            // A combat spell casts the learned payload, one spell for both hands.
            if matches!(grant.kind, GrantKind::CombatSpell { .. })
                && !self.character.spells.has_game_id(grant.spell)
            {
                self.learn(db, grant.spell);
            }
            for payload in payload_spells(db, grant.spell) {
                if db.get(payload).is_some() && !self.character.spells.has_game_id(payload) {
                    self.learn(db, payload);
                }
            }
            let setup = SpellSetup::from_db(db, grant.spell).expect("the record is in the db");
            let party = self.character.party();
            let spells = &mut self.character.spells;
            let (key, overrides) = (grant.key, db.overrides());
            // A finisher proc (Revealed Flaw) has no `ProcTypeMask`: the override names its event.
            let is_proc_aura = record.is_passive()
                && (record.aura_options.proc_type_mask.bits() != 0
                    || setup.overrides.proc.is_some_and(|p| p.finisher));
            let handle = match grant.kind {
                GrantKind::Proc(allowed) => spells
                    .add_equipment_proc(key, setup, overrides, &allowed, party, self.raid)
                    .map(SpellHandle::Proc),
                GrantKind::Passive(allowed) if is_proc_aura => spells
                    .add_equipment_proc(key, setup, overrides, &allowed, party, self.raid)
                    .map(SpellHandle::Proc),
                GrantKind::Passive(_) if record.is_passive() => Some(SpellHandle::Spell(
                    spells.add_equipment_passive(key, setup, overrides, party, self.raid),
                )),
                GrantKind::Passive(_) => None,
                GrantKind::OnHit(allowed) => spells
                    .add_on_hit_proc(key, setup, overrides, &allowed, party, self.raid)
                    .map(SpellHandle::Proc),
                GrantKind::CombatSpell { sources, chance } => spells
                    .add_combat_spell_proc(
                        key,
                        combat_spell_setup(setup, chance),
                        overrides,
                        &sources,
                        party,
                        self.raid,
                    )
                    .map(SpellHandle::Proc),
                GrantKind::Use(effect, name) => {
                    let mut setup = with_item_cooldowns(setup, &effect);
                    if let Some(name) = name {
                        Arc::make_mut(&mut setup.record).name = name;
                    }
                    let id = spells.add_equipment_passive(key, setup, overrides, party, self.raid);
                    uses.push(id);
                    Some(SpellHandle::Spell(id))
                }
            };
            if let Some(handle) = handle {
                wanted.push(handle);
                self.enable_handle(handle);
            }
        }
        let stale: Vec<SpellHandle> = self
            .character
            .spells
            .equipment_spells()
            .map(|(_, handle)| handle)
            .filter(|handle| !wanted.contains(handle))
            .collect();
        for handle in stale {
            self.disable_handle(handle);
        }
        // After the stale ones are disabled, so that a new item's use takes over the name of
        // the one it replaced.
        for id in uses {
            self.character.spells.name_equipment_use(id);
        }
        self.relink_rotation();
    }

    fn enable_handle(&mut self, handle: SpellHandle) {
        match handle {
            SpellHandle::Spell(id) => self.enable_spell(id),
            SpellHandle::Proc(id) => self.enable_proc(id),
        }
    }

    fn disable_handle(&mut self, handle: SpellHandle) {
        match handle {
            SpellHandle::Spell(id) => self.disable_spell(id),
            SpellHandle::Proc(id) => self.disable_proc(id),
        }
    }

    /// The spells the equipped items, enchants and reached set bonuses grant.
    fn granted_equipment_spells(&self, db: &SpellDb) -> Vec<Grant> {
        let equipment = self.character.equipment();
        let mut grants = Vec::new();
        for (slot, spec) in equipment.active_enchants() {
            for (index, proc) in spec.procs.iter().enumerate() {
                let Some(spell) = proc.spell else {
                    continue;
                };
                grants.push(Grant {
                    key: EquipmentSpellKey {
                        slot: Some(slot),
                        grantor: EquipmentGrantor::Enchant(spec.name),
                        index,
                    },
                    spell,
                    kind: GrantKind::Proc(slot.default_proc_sources()),
                });
            }
            let enchantment = spec.enchantment.and_then(|id| db.item_enchantment(id));
            let combat_spells = enchantment.into_iter().flat_map(|e| e.combat_spells());
            for (offset, (spell, chance)) in combat_spells.enumerate() {
                grants.push(Grant {
                    key: EquipmentSpellKey {
                        slot: Some(slot),
                        grantor: EquipmentGrantor::Enchant(spec.name),
                        index: spec.procs.len() + offset,
                    },
                    spell,
                    kind: GrantKind::CombatSpell {
                        sources: slot.default_proc_sources(),
                        chance,
                    },
                });
            }
        }
        for (slot, item) in equipment.equipped_items() {
            for (index, effect) in item.effects().iter().enumerate() {
                let kind = match effect.trigger {
                    EffectTrigger::Use => GrantKind::Use(effect.clone(), None),
                    EffectTrigger::Equip => GrantKind::Passive(passive_proc_sources(Some(slot))),
                    EffectTrigger::OnHit if slot.is_weapon_slot() => {
                        GrantKind::OnHit(slot.default_proc_sources())
                    }
                    EffectTrigger::OnHit => continue,
                };
                grants.push(Grant {
                    key: EquipmentSpellKey {
                        slot: Some(slot),
                        grantor: EquipmentGrantor::ItemEffect(item.id()),
                        index,
                    },
                    spell: effect.spell,
                    kind,
                });
            }
        }
        for (set, index, bonus) in equipment.active_set_bonuses() {
            grants.push(Grant {
                key: EquipmentSpellKey {
                    slot: None,
                    grantor: EquipmentGrantor::SetBonus(set),
                    index,
                },
                spell: bonus.spell,
                kind: GrantKind::Passive(passive_proc_sources(None)),
            });
        }
        for spec in &self.character.consumables {
            let uses = db
                .consumable_item(spec.item)
                .into_iter()
                .flat_map(|i| i.uses());
            for (index, effect) in uses.enumerate() {
                grants.push(Grant {
                    key: EquipmentSpellKey {
                        slot: None,
                        grantor: EquipmentGrantor::Consumable(spec.item),
                        index,
                    },
                    spell: effect.spell,
                    kind: GrantKind::Use(effect.clone(), Some(spec.name.clone())),
                });
            }
        }
        grants
    }

    /// Replaces the items the character uses in combat (`consumables` of
    /// `data/external_buffs.yaml`: Thistle Tea) and registers their use spells, which the
    /// rotation casts by the consumable's name. The class is not checked
    /// ([`ConsumableSpec::valid_for_class`] is the setup's business).
    pub fn set_consumables(&mut self, db: &SpellDb, consumables: Vec<ConsumableSpec>) {
        self.character.consumables = consumables;
        self.sync_equipment_spells(db);
    }

    // ---------------------------------------------------------------- external buffs

    /// Offers the character the external buffs of `registry` its class can have: each becomes
    /// a permanent, hidden [`BuffKind::External`] buff built from its aura spell in `db`.
    /// Entries already offered are skipped.
    ///
    /// # Panics
    /// Panics if an entry's spell is not in `db` or applies no auras
    /// (`ExternalBuffDb::validate` catches this at load time).
    pub fn add_external_buffs(&mut self, registry: &ExternalBuffDb, db: &SpellDb) {
        let class = self.character.class_kind();
        for (spec, debuff) in registry.offered_to(class) {
            if self.character.general_buffs.get(&spec.name).is_some() {
                continue;
            }
            let record = db.get(spec.spell).unwrap_or_else(|| {
                panic!(
                    "external buff {:?}: spell {} is not in the db",
                    spec.name, spec.spell
                )
            });
            assert!(
                record.applies_aura(),
                "external buff {:?}: spell {} applies no auras",
                spec.name,
                spec.spell
            );
            let buff = Buff::from_record(record, BuffKind::External, db.overrides())
                .with_name(&spec.name)
                .with_duration(None)
                .with_hidden(true);
            let stacks = spec.applied_stacks(buff.max_stacks());
            let id = self.character.spells.add_external_buff(buff);
            self.character
                .general_buffs
                .add(spec.clone(), debuff, id, stacks);
        }
    }

    /// Selects or deselects an external buff: selecting applies its auras (once per stack,
    /// armor reductions on the target) and cancels the other buffs of its mutex group,
    /// deselecting removes them. Returns whether the buff is now selected.
    pub fn toggle_external_buff(&mut self, name: &str) -> Result<bool, ExternalBuffToggleError> {
        let selected = self.character.general_buffs.is_selected(name);
        self.set_external_buff_selected(name, !selected)
    }

    /// Selects (`true`) or deselects an external buff; see
    /// [`CharacterContext::toggle_external_buff`]. Selecting an already selected buff or
    /// deselecting an unselected one changes nothing. Returns the new selection state.
    pub fn set_external_buff_selected(
        &mut self,
        name: &str,
        selected: bool,
    ) -> Result<bool, ExternalBuffToggleError> {
        let faction = self.character.faction();
        let entry = self
            .character
            .general_buffs
            .get(name)
            .ok_or_else(|| ExternalBuffToggleError::NotOffered(name.to_string()))?;
        if !entry.spec.valid_for_faction(faction) {
            return Err(ExternalBuffToggleError::WrongFaction(
                name.to_string(),
                faction.name(),
            ));
        }
        let (buff, stacks, debuff) = (entry.buff, entry.stacks, entry.debuff);
        if selected {
            let peers: Vec<String> = self
                .character
                .general_buffs
                .mutex_peers(name)
                .iter()
                .filter(|e| e.selected)
                .map(|e| e.spec.name.clone())
                .collect();
            for peer in peers {
                self.set_external_buff_selected(&peer, false)?;
            }
            if debuff {
                // The external debuff replaces the character's own of the same name.
                let own: Vec<BuffId> = self
                    .character
                    .spells
                    .buff_ids()
                    .filter(|&id| {
                        let own = self.buff_ref(id);
                        own.kind().is_debuff() && own.name() == name
                    })
                    .collect();
                for id in own {
                    self.cancel_buff(id);
                }
            }
            self.apply_external(buff, stacks);
        } else {
            self.cancel_buff(buff);
        }
        self.character
            .general_buffs
            .get_mut(name)
            .expect("looked up above")
            .selected = selected;
        Ok(selected)
    }

    /// Applies an external buff at `stacks` stacks (each stack applies the auras once).
    fn apply_external(&mut self, buff: BuffId, stacks: u32) {
        if self.buff_ref(buff).is_active() {
            return;
        }
        for _ in 0..stacks.max(1) {
            self.apply_buff(buff);
        }
    }

    /// Deselects every external buff.
    pub fn clear_external_buffs(&mut self) {
        let selected: Vec<(String, BuffId)> = self
            .character
            .general_buffs
            .entries()
            .iter()
            .filter(|e| e.selected)
            .map(|e| (e.spec.name.clone(), e.buff))
            .collect();
        for (name, buff) in selected {
            self.cancel_buff(buff);
            self.character
                .general_buffs
                .get_mut(&name)
                .expect("listed above")
                .selected = false;
        }
    }

    /// Casts a spell: performs it, then runs the proc checks its report asks for and the
    /// extra attacks it granted.
    pub fn cast(&mut self, id: SpellId) -> CastReport {
        let mark = self.log_mark();
        let report = self.with_spell(id, |spell, ctx| spell.perform(ctx));
        self.after_cast(id, &report, mark);
        self.perform_extra_attacks();
        report
    }

    /// Records the statistics of a completed cast of `id` and runs its proc sources. The
    /// off-hand strike is part of the same cast: it does not use a second charge of a spell
    /// modifier aura (Eureka!). `mark` is where the combat log stood before the spell was
    /// performed.
    fn after_cast(&mut self, id: SpellId, report: &CastReport, mark: Option<usize>) {
        self.record_cast(id, report, mark);
        let sources = self.cast_sources(id, report);
        self.run_sources(&sources);
        if let Some(offhand) = &report.offhand {
            self.run_sources(&untriggered(&offhand.proc_sources));
        }
    }

    /// The proc sources of a cast of `id` with the spell behind each: the cast's own, those of
    /// the spells it triggered (Mutilate's strikes, with their own class options but the
    /// cast's combo points) and, when a finisher spent its combo points, the finisher event.
    fn cast_sources(&self, id: SpellId, report: &CastReport) -> Vec<(ProcSource, ProcTrigger)> {
        let record = self.character.spells.spell(id).record();
        let cast = ProcTrigger {
            class_options: record.class_options,
            combo_points_spent: report.combo_points_spent,
            awards_combo_points: record.awards_combo_points(),
        };
        let mut sources = Vec::new();
        self.collect_sources(report, cast, &mut sources);
        if report.combo_points_spent > 0 {
            sources.push((ProcSource::Finisher, cast));
        }
        sources
    }

    /// Adds the proc sources of `report` and of the spells it triggered to `sources`.
    fn collect_sources(
        &self,
        report: &CastReport,
        trigger: ProcTrigger,
        sources: &mut Vec<(ProcSource, ProcTrigger)>,
    ) {
        sources.extend(report.proc_sources.iter().map(|&source| (source, trigger)));
        for (game_id, triggered) in &report.triggered {
            let spells = &self.character.spells;
            let class_options = match spells.handle(*game_id) {
                Some(SpellHandle::Spell(id)) => spells.spell(id).record().class_options,
                Some(SpellHandle::Proc(id)) => {
                    spells.procs().get(id).spell().record().class_options
                }
                None => None,
            };
            let trigger = ProcTrigger {
                class_options,
                ..trigger
            };
            self.collect_sources(triggered, trigger, sources);
        }
    }

    /// Records the statistics of a swing and runs its proc sources.
    fn after_swing(&mut self, report: &SwingReport) {
        self.record_swing(report);
        self.run_sources(&untriggered(&report.proc_sources));
    }

    /// Runs the proc checks for `sources` and uses the charges of the buffs that react to them.
    /// The charges go after the procs of the landed hit and before those of its result (a
    /// crit): a landed swing consumes a charge when its damage lands, one batch after the procs
    /// it triggered (the `classic-warrior` wiki on Windfury Totem), so the swing that proc'd
    /// Windfury uses a charge of the aura it just applied; but a crit consumes a charge of the
    /// Flurry already up before refreshing it, and the crit that applies Flurry keeps all three
    /// charges (Classic Era combat logs).
    fn run_sources(&mut self, sources: &[(ProcSource, ProcTrigger)]) {
        self.run_proc_checks_around(sources, |ctx| ctx.use_charges(sources));
        let plain: Vec<ProcSource> = sources.iter().map(|&(source, _)| source).collect();
        self.run_event_scripts(&plain);
    }

    /// Uses the charges of the buffs that react to `sources`. A charged spell modifier aura
    /// reacts to the spells it modifies only, and loses at most one charge to one event.
    fn use_charges(&mut self, sources: &[(ProcSource, ProcTrigger)]) {
        let mut charged = Vec::new();
        for (source, trigger) in sources {
            let class = trigger.class_options;
            for id in self
                .character
                .spells
                .charge_consumers_for(*source, class.as_ref())
            {
                let spell_modifier = self
                    .character
                    .spells
                    .owned_buff(id)
                    .is_some_and(Buff::charges_require_spell_modifier);
                if spell_modifier {
                    if charged.contains(&id) {
                        continue;
                    }
                    charged.push(id);
                }
                self.use_charge(id);
            }
        }
    }

    /// Runs the overrides' event reactions (`on_event`) of the enabled spells to the sources of
    /// one event: server-side scripts the client tables do not carry (Overpower's combo point
    /// when the target dodges). Like a proc, a reaction fires at most once per event, and the
    /// ranks of one spell react once between them: Overpower's four ranks grant one combo
    /// point per dodge, not four.
    fn run_event_scripts(&mut self, sources: &[ProcSource]) {
        let spells = &self.character.spells;
        let mut seen: Vec<(&str, usize)> = Vec::new();
        let mut reactions: Vec<EventScript> = Vec::new();
        for &id in spells.event_reactors() {
            let spell = spells.spell(id);
            if !spell.is_enabled() {
                continue;
            }
            for (index, event) in spell.event_scripts().iter().enumerate() {
                if !sources.contains(&event.source) || seen.contains(&(spell.name(), index)) {
                    continue;
                }
                seen.push((spell.name(), index));
                reactions.push(*event);
            }
        }
        for event in reactions {
            match event.script {
                ScriptKind::AddComboPoints => {
                    // Validated as present and positive when the overrides were loaded.
                    let value = event.params.value.unwrap_or(0.0).round() as u32;
                    let now = self.now();
                    self.character.gain_combo_points(value, now);
                }
                other => unreachable!("on_event script {other:?} is refused by the overrides"),
            }
        }
    }

    /// Runs the proc check for each source without a spell behind it (a swing's) and returns
    /// the reports of the procs that fired; see [`Self::run_proc_checks_for`].
    pub fn run_proc_checks(&mut self, sources: &[ProcSource]) -> Vec<(ProcId, CastReport)> {
        self.run_proc_checks_for(&untriggered(sources))
    }

    /// Runs the proc check for each source with the spell behind it and returns the reports of
    /// the procs that fired. The sources of one event form one check: a proc fires at most once
    /// per event. Extra attacks the procs granted stay pending so that no proc re-fires off its
    /// own extra attack or twice in one chain of extra attacks.
    pub fn run_proc_checks_for(
        &mut self,
        sources: &[(ProcSource, ProcTrigger)],
    ) -> Vec<(ProcId, CastReport)> {
        self.run_proc_checks_around(sources, |_| {})
    }

    /// [`Self::run_proc_checks_for`], running `between` inside the check after the sources of
    /// the landed hit and before those of its critical result.
    fn run_proc_checks_around(
        &mut self,
        sources: &[(ProcSource, ProcTrigger)],
        between: impl FnOnce(&mut Self),
    ) -> Vec<(ProcId, CastReport)> {
        let before = self.character.pending_extra_attacks();
        self.character.spells.procs_mut().begin_check();
        let mut fired = Vec::new();
        let (crits, landed): (Vec<&(ProcSource, ProcTrigger)>, Vec<_>) =
            sources.iter().partition(|(source, _)| {
                matches!(
                    source,
                    ProcSource::MeleeCritical | ProcSource::SpellCritical
                )
            });
        // A crit is reported apart from its hit: it is an off-hand one when the hit was.
        let hand = if landed
            .iter()
            .any(|(source, _)| source.hand() == Hand::Offhand)
        {
            Hand::Offhand
        } else {
            Hand::Mainhand
        };
        for &(source, trigger) in landed {
            if source == ProcSource::Manual {
                continue;
            }
            self.character.set_proc_hand(source.hand());
            fired.extend(self.with_procs(|procs, ctx| procs.run_proc_check(source, trigger, ctx)));
        }
        self.character.set_proc_hand(Hand::Mainhand);
        between(self);
        self.character.set_proc_hand(hand);
        for &(source, trigger) in crits {
            fired.extend(self.with_procs(|procs, ctx| procs.run_proc_check(source, trigger, ctx)));
        }
        self.character.set_proc_hand(Hand::Mainhand);
        for (id, report) in &fired {
            let (name, rank, spell) = {
                let spell = self.character.spells.procs().get(*id).spell();
                (spell.name().to_string(), spell.rank(), log_spell(spell))
            };
            self.record_report(&name, rank, report);
            self.log_report(spell, report, None, false);
        }
        let granted_extra_attacks = self.character.pending_extra_attacks() > before;
        let procs = self.character.spells.procs_mut();
        if granted_extra_attacks {
            procs.hold_check();
        } else {
            procs.end_check();
        }
        fired
    }

    /// Fires proc `id` now, whatever its chance, and records and logs it; the extra attacks it
    /// grants stay pending.
    #[cfg(test)]
    pub(crate) fn perform_proc(&mut self, id: ProcId) -> CastReport {
        let report = self.with_procs(|procs, ctx| procs.get_mut(id).perform(ctx));
        let (name, rank, spell) = {
            let spell = self.character.spells.procs().get(id).spell();
            (spell.name().to_string(), spell.rank(), log_spell(spell))
        };
        self.record_report(&name, rank, &report);
        self.log_report(spell, &report, None, false);
        report
    }

    /// Performs the main-hand extra attacks granted so far (Windfury, Sword Specialization),
    /// including the ones those attacks grant in turn. An extra attack finishes the swing timer,
    /// so a queued on-next-swing spell that is available goes off in its place.
    /// Returns the reports of the white swings performed.
    pub fn perform_extra_attacks(&mut self) -> Vec<SwingReport> {
        let mut reports = Vec::new();
        while self.character.take_extra_attack() {
            if !self.character.has_mainhand() {
                continue;
            }
            if let Some(queued) = self.available_next_swing() {
                self.perform_next_swing(queued);
                self.with_auto_attack(Hand::Mainhand, |attack, ctx| attack.schedule_next(ctx));
                continue;
            }
            let report =
                self.with_auto_attack(Hand::Mainhand, |attack, ctx| attack.extra_attack(ctx));
            self.after_swing(&report);
            reports.push(report);
        }
        // The chain is over: the procs that granted it may fire again.
        self.character.spells.procs_mut().release_held_checks();
        reports
    }

    /// The queued on-next-swing spell, if it can go off right now.
    fn available_next_swing(&self) -> Option<SpellId> {
        let queued = self.character.spells.queued_next_swing()?;
        self.character
            .spells
            .spell(queued)
            .status(self)
            .is_available()
            .then_some(queued)
    }

    /// Lets the queued on-next-swing spell `queued` replace the main-hand swing that is due:
    /// completes the swing timer, performs the spell and runs what followed from it.
    pub(crate) fn perform_next_swing(&mut self, queued: SpellId) -> CastReport {
        let now = self.now();
        let speed = self.weapon_speed(Hand::Mainhand).unwrap_or(0.0);
        self.character
            .spells
            .mh_attack_mut()
            .complete_swing(now, speed);
        let mark = self.log_mark();
        let report = self.with_spell(queued, |spell, ctx| spell.perform_on_swing(ctx));
        self.after_cast(queued, &report, mark);
        report
    }

    // ---------------------------------------------------------------- auto attacks

    /// A main-hand swing event. A queued on-next-swing spell that is available replaces the swing.
    /// If the queued spell cannot run it is cancelled.
    pub fn mh_swing(&mut self, iteration: u32) -> SwingOutcome {
        if !self.character.spells.mh_attack().attack_is_valid(iteration)
            || !self.character.spells.is_melee_attacking()
        {
            return SwingOutcome::Skipped;
        }
        let outcome = match self.available_next_swing() {
            Some(queued) => {
                let report = self.perform_next_swing(queued);
                self.perform_extra_attacks();
                SwingOutcome::NextSwingSpell(report)
            }
            None => {
                if let Some(queued) = self.character.spells.queued_next_swing() {
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

    /// Swings `hand` now outside the swing events, without scheduling the next swing, and runs
    /// what follows from it. Port of `MainhandAttack::perform` / `OffhandAttack::perform` as the
    /// C++ tests call them.
    #[cfg(test)]
    pub(crate) fn perform_swing(&mut self, hand: Hand) -> SwingReport {
        let report = self.with_auto_attack(hand, |attack, ctx| attack.perform(ctx));
        self.after_swing(&report);
        self.perform_extra_attacks();
        report
    }

    /// An off-hand swing event. The off hand keeps swinging while a next-swing ability is
    /// queued.
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

    /// Schedules the pending swing of each attacking hand.
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
    /// fraction).
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
    /// Mastery remainder is lost.
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
        // Passives gated on a stance (Defiance in Defensive Stance) follow the change.
        self.reevaluate_passives();
        // No auto attacks in Stealth (Vanish drops them); leaving it in combat resumes them.
        if stance == Stance::Stealth {
            self.character.spells_mut().stop_attack();
        } else if old == Stance::Stealth && self.now() >= 0.0 {
            self.start_attack();
        }
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

    /// Start of an iteration. Back to caster form, cooldowns and resource cleared, every buff
    /// removed and every spell and proc reset; the passives are then re-applied for the new
    /// iteration.
    pub fn reset(&mut self) {
        self.record_regeneration();
        let stance = self.character.stance();
        if stance != Stance::Caster {
            self.character.set_stance(Stance::Caster);
            if let Some(link) = self.character.stance_link(stance) {
                self.cancel_stance_spell(link);
            }
            self.sync_stance_passives();
        }
        let now = self.now();
        let combat_length = self.character.sim().combat_length;
        for id in self.character.spells.buff_ids().collect::<Vec<_>>() {
            let (stacks, applied, unreported) = {
                let buff = self.buff_ref(id);
                // A passive's aura is not a buff: it is up whenever its conditions hold, applied
                // by the reset before the clock moves back to the pull.
                let unreported = buff.is_hidden() || buff.is_passive();
                (buff.stacks(), buff.applied_at(), unreported)
            };
            let (buff, mut ctx) = self.buff_ctx(id);
            let reset = buff.reset(&mut ctx);
            if reset.was_active {
                for _ in 0..stacks.max(1) {
                    self.remove_auras(id);
                }
            }
            // The application the end of the iteration cut short, then the iteration's share of the
            // encounter.
            if !unreported {
                if reset.was_active {
                    self.record_buff_uptime(id, now - applied);
                }
                if reset.uptime > 0.0 {
                    let (name, debuff) = {
                        let buff = self.buff_ref(id);
                        (buff.statistics_name(), buff.is_debuff())
                    };
                    self.character
                        .statistics
                        .buff(&name, debuff)
                        .add_uptime_for_encounter(reset.uptime / combat_length);
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
    /// equipment / stance changed).
    pub fn reevaluate_passives(&mut self) {
        for id in self.character.spells.spell_ids().collect::<Vec<_>>() {
            self.with_spell(id, |spell, ctx| spell.reevaluate_passive(ctx));
        }
        let enabled: Vec<ProcId> = self.character.spells.procs().enabled().to_vec();
        for id in enabled {
            self.with_procs(|procs, ctx| procs.get_mut(id).spell_mut().reevaluate_passive(ctx));
        }
    }

    /// Before a set of iterations: the statistics (cleared), the state, the buffs and the
    /// rotation, which is relinked here so that spells enabled since it was set (talents,
    /// racials, equipment) join it.
    pub fn prepare_set_of_combat_iterations(&mut self) {
        // The passives' auras are applied between sets (the reset re-applies them).
        // Initializing their buffs would mark them inactive with their effects still applied,
        // and the next reevaluation would apply them a second time: take them off first and
        // put them back once the buffs are initialized.
        let buffs: Vec<BuffId> = self.character.spells.buff_ids().collect();
        for &id in &buffs {
            let buff = self.buff_ref(id);
            if buff.is_active() && buff.kind() != BuffKind::External {
                self.cancel_buff(id);
            }
        }
        self.character.prepare_set_of_combat_iterations_state();
        for &id in &buffs {
            self.buff_ctx(id).0.initialize();
        }
        self.reevaluate_passives();
        self.relink_rotation();
        if let Some(rotation) = self.character.rotation.as_mut() {
            rotation.prepare_set_of_combat_iterations();
        }
    }

    /// Combat starts: start-of-combat buffs and spells, the auto attacks, then the rotation.
    /// The regeneration statistics count from here.
    pub fn encounter_start(&mut self) {
        let now = self.now();
        if let Some(energy) = self.character.resource_mut().as_energy_mut() {
            energy.take_regen_counters(now);
        }
        for id in self.character.spells.start_of_combat_buffs().to_vec() {
            self.apply_buff(id);
        }
        for id in self.character.spells.start_of_combat_spells().to_vec() {
            let mark = self.log_mark();
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
                self.after_cast(id, &report, mark);
                self.perform_extra_attacks();
            }
        }
        // A Rogue in Stealth starts attacking with the opener that breaks it.
        if self.character.stance() != Stance::Stealth {
            self.start_attack();
        }
        self.perform_rotation();
    }

    /// Handles an event addressed to this character; returns `false` for events it does not
    /// own (`IncomingDamage` is tank mode's).
    pub fn handle_event(&mut self, event: &Event) -> bool {
        let me = self.character.id();
        match event.kind {
            EventKind::EncounterStart { character } if character == me => {
                self.encounter_start();
            }
            EventKind::PlayerAction { character } if character == me => {
                self.perform_rotation();
            }
            EventKind::Precast { character } if character == me => {
                self.cast_precast();
            }
            EventKind::RegenReaction { character, wake } if character == me => {
                // A reaction replaced by a later plan is not handled.
                if !self.character.is_current_regen_wake(wake) {
                    return false;
                }
                // Last of its instant, whenever it was scheduled: what the other events of
                // the instant change (a swing resets the swing timer, a buff runs out) is seen
                // the same way however the reactions are planned.
                let now = self.now();
                let others_now = self.engine.queue().iter().any(|other| {
                    other.time == now && !matches!(other.kind, EventKind::RegenReaction { .. })
                });
                if others_now {
                    self.engine.add_event(*event);
                    return false;
                }
                self.character.clear_regen_wake();
                self.character.set_last_regen_reaction(now);
                self.perform_rotation();
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
                    self.record_buff_removed(buff);
                    self.end_linked_auras(buff);
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
                let mark = self.log_mark();
                if let Some(report) = self.with_spell(spell, |s, ctx| s.complete_cast(cast_id, ctx))
                {
                    self.after_cast(spell, &report, mark);
                    self.perform_extra_attacks();
                }
            }
            _ => return false,
        }
        self.plan_regen_reaction();
        true
    }

    /// Schedules the next reaction to energy regeneration (see [`RegenReactions`]), replacing
    /// the one scheduled before: whatever the event just handled changed is accounted for. The
    /// reaction is the one to the first tick that gains energy and brings what the rotation
    /// waits for ([`Rotation::next_change`]), or to the next such tick in the per-tick mode.
    fn plan_regen_reaction(&mut self) {
        let now = self.now();
        // A reaction due now has not happened yet: it plans again once it has.
        if self.character.regen_wake().is_some_and(|at| at <= now) {
            return;
        }
        let manual = self.character.manual_input();
        let wake = match (
            self.character.resource().as_energy(),
            self.character.rotation(),
        ) {
            // Played by input: every tick that gains energy while a spell is queued.
            (Some(energy), _) if manual => self.character.queued_input().and_then(|_| {
                let after_now = self.character.last_regen_reaction() == now;
                energy.next_reaction(now, after_now, now, PLAYER_REACTION_DELAY)
            }),
            (Some(energy), Some(rotation)) if now >= 0.0 => {
                let not_before = match self.character.regen_reactions() {
                    RegenReactions::EveryTick => now,
                    RegenReactions::Thresholds => {
                        let watched = Watched {
                            resource: ResourceType::Energy,
                            max: energy.max(),
                            encounter_length: self.character.sim().combat_length,
                        };
                        let next = rotation.next_change(self, watched);
                        // The first reaction that sees the tick reaching the level.
                        let by_level = next.level.map_or(f64::INFINITY, |level| {
                            now + energy.time_until(level, now) + TICK_READ_LAG
                        });
                        (now + next.delay).min(by_level)
                    }
                };
                // The reactions of this instant come last in it: one may still be ahead.
                let after_now = self.character.last_regen_reaction() == now;
                not_before
                    .is_finite()
                    .then(|| {
                        energy.next_reaction(now, after_now, not_before, PLAYER_REACTION_DELAY)
                    })
                    .flatten()
            }
            _ => None,
        };
        if wake == self.character.regen_wake() {
            return;
        }
        match wake {
            Some(at) => {
                let wake = self.character.schedule_regen_wake(at);
                let character = self.character.id();
                self.engine
                    .add_event(Event::new(at, EventKind::RegenReaction { character, wake }));
            }
            None => self.character.clear_regen_wake(),
        }
    }

    /// A periodic tick of `spell`; returns the tick report if the application is current. A
    /// tick that deals damage is a proc event of its spell (Thousand Cuts on Rupture's ticks).
    pub fn dot_tick(&mut self, spell: SpellId, application_id: u32) -> Option<TickReport> {
        let report = self.with_spell(spell, |s, ctx| s.perform_periodic(application_id, ctx))?;
        self.record_tick(spell, &report);
        if report.damage > 0 {
            let trigger = ProcTrigger {
                class_options: self.character.spells.spell(spell).record().class_options,
                ..ProcTrigger::default()
            };
            self.run_sources(&[(ProcSource::PeriodicDamage, trigger)]);
            self.perform_extra_attacks();
        }
        Some(report)
    }

    /// Uses a charge of every buff that reacts to `source`.
    pub fn consume_charges(&mut self, source: ProcSource) {
        for id in self.character.spells.charge_consumers(source) {
            self.use_charge(id);
        }
    }

    /// Uses a charge of buff `id`, removing it (every stack) when that was the last one.
    fn use_charge(&mut self, id: BuffId) {
        let stacks = self.buff_ref(id).stacks();
        let (buff, mut ctx) = self.buff_ctx(id);
        if buff.use_charge(&mut ctx) == ChargeUse::Removed {
            for _ in 0..stacks.max(1) {
                self.remove_auras(id);
            }
            self.record_buff_removed(id);
        }
    }

    // ---------------------------------------------------------------- statistics

    /// Records the attack outcome and resource gains of a cast of `id` (not of the spells it
    /// triggered: those were recorded when they were cast through [`SpellHost::trigger_spell`]).
    /// Also logs it; a cast of the character's own (`mark`, see [`Self::log_report`]) with its
    /// `SPELL_CAST_SUCCESS`.
    fn record_cast(&mut self, id: SpellId, report: &CastReport, mark: Option<usize>) {
        let (name, rank, spell, hostile) = {
            let spell = self.character.spells.spell(id);
            // An off-hand strike of its own (Mutilate's) is kept apart from the main hand's.
            let name = if spell.strikes_with_offhand() {
                format!("{} Off-Hand", spell.name())
            } else {
                spell.name().to_string()
            };
            let hostile = spell.record().effects.iter().any(|e| e.targets_enemy());
            (name, spell.rank(), log_spell(spell), hostile)
        };
        self.record_report(&name, rank, report);
        // Finishers of a class with combo points to build (not the Warrior's dodge marker).
        if report.combo_points_spent > 0 && self.character.class().max_combo_points > 1 {
            self.character
                .statistics
                .record_finisher(&name, rank, report.combo_points_spent);
        }
        self.log_report(spell, report, mark, hostile);
    }

    /// Records a cast report under `name` / `rank`.
    fn record_report(&mut self, name: &str, rank: u32, report: &CastReport) {
        let statistics = &mut self.character.statistics;
        if let Some(attack) = &report.attack {
            statistics
                .spell(name, rank)
                .record_attack(attack, f64::from(report.resource_cost));
        }
        if let Some(offhand) = &report.offhand {
            statistics
                .spell(&format!("{name} Off-Hand"), rank)
                .record_attack(&offhand.attack, 0.0);
        }
        for &(resource, amount) in &report.resource_gained {
            statistics.resource(name, rank).add_gain(resource, amount);
        }
    }

    /// Records a white swing under its hand's attack name.
    fn record_swing(&mut self, report: &SwingReport) {
        let name = match report.hand {
            Hand::Mainhand => self.character.spells.mh_attack().name(),
            Hand::Offhand => self.character.spells.oh_attack().name(),
        };
        let statistics = &mut self.character.statistics;
        statistics.spell(name, 1).record_swing(&report.attack);
        self.log_swing(report);
        let statistics = &mut self.character.statistics;
        if let Some(rage) = report.rage_gained {
            statistics
                .resource(name, 1)
                .add_fractional_gain(ResourceType::Rage, rage);
        }
    }

    /// Records the energy regenerated since the pull and the ticks lost at the cap (regeneration
    /// is not logged).
    fn record_regeneration(&mut self) {
        let now = self.now();
        let Some(energy) = self.character.resource_mut().as_energy_mut() else {
            return;
        };
        let (regenerated, lost) = energy.take_regen_counters(now);
        if regenerated == 0 && lost == 0 {
            return;
        }
        let statistics = &mut self.character.statistics;
        statistics
            .resource(REGENERATION, 1)
            .add_fractional_gain(ResourceType::Energy, regenerated as f64);
        statistics.add_lost_at_cap(ResourceType::Energy, lost as f64);
    }

    /// Records a periodic tick of `id`: its damage as a hit, its resource gain.
    fn record_tick(&mut self, id: SpellId, report: &TickReport) {
        let (name, rank, spell) = {
            let spell = self.character.spells.spell(id);
            (spell.name().to_string(), spell.rank(), log_spell(spell))
        };
        self.log_tick(spell, report);
        let statistics = &mut self.character.statistics;
        if report.damage > 0 || report.threat > 0.0 {
            statistics.spell(&name, rank).record_tick(
                report.damage,
                report.threat,
                report.resource_cost,
                report.execution_time,
                report.crit,
                report.magic.then_some(report.resist),
            );
        }
        if let Some((resource, amount)) = report.resource_gained {
            statistics.resource(&name, rank).add_gain(resource, amount);
        }
    }

    /// Records the application that just ended for a removed buff.
    fn record_buff_removed(&mut self, id: BuffId) {
        let buff = self.buff_ref(id);
        if buff.is_hidden() || buff.is_passive() {
            return;
        }
        let uptime = buff.expired_at() - buff.applied_at();
        self.record_buff_uptime(id, uptime);
        self.log_aura(id, AuraChange::Removed);
    }

    fn record_buff_uptime(&mut self, id: BuffId, uptime: f64) {
        let (name, debuff) = {
            let buff = self.buff_ref(id);
            (buff.statistics_name(), buff.is_debuff())
        };
        self.character
            .statistics
            .buff(&name, debuff)
            .add_uptime(uptime);
    }

    // ---------------------------------------------------------------- combat log

    /// Where the combat log stands, if it is recorded.
    fn log_mark(&self) -> Option<usize> {
        self.engine.combat_log().map(CombatLog::len)
    }

    fn log_me(&self) -> LogUnit {
        LogUnit::Character(self.character.id())
    }

    /// The character's advanced combat log snapshot.
    fn own_log_info(&self) -> UnitInfo {
        let view = self.target_view();
        let resource = self.character.resource();
        UnitInfo {
            attack_power: self.character.melee_ap(&view),
            armor: self
                .character
                .stats()
                .get_armor(&self.character.stat_context(&view)) as i32,
            power: Some(self.character.resource_type()),
            current_power: resource.current(self.resource_read_time()),
            max_power: resource.max(),
            level: self.character.clvl(),
        }
    }

    /// The target's advanced combat log snapshot.
    fn target_log_info(&self) -> UnitInfo {
        UnitInfo {
            armor: self.target.armor(),
            level: self.target.level(),
            ..UnitInfo::default()
        }
    }

    /// Logs a cast: its damage or miss, its off-hand strike and its power gains. A spell the
    /// character cast itself also logs `SPELL_CAST_SUCCESS` at `mark`, where the log stood before
    /// it was performed (so before what the cast caused); a proc or a triggered spell (`None`)
    /// only logs its effects, like the client. A cast that only started (a cast time, an
    /// on-next-swing queue) is logged when it completes. The cast is logged at the target when
    /// the spell attacked it or has a `hostile` effect (a landed Rupture deals no damage itself).
    fn log_report(
        &mut self,
        spell: LogSpell,
        report: &CastReport,
        mark: Option<usize>,
        hostile: bool,
    ) {
        if !self.engine.is_logging()
            || report.queued
            || report.cast_started
            || report.result == SpellResult::Undetermined
        {
            return;
        }
        let me = self.log_me();
        if let Some(mark) = mark {
            let dest = if report.attack.is_some() || hostile {
                LogUnit::Target
            } else {
                me
            };
            let cast = CombatLogEvent::SpellCastSuccess {
                spell: spell.clone(),
                info: self.own_log_info(),
            };
            self.engine.log_at(mark, me, dest, cast);
        }
        let strikes: Vec<(AttackOutcome, bool)> = report
            .attack
            .iter()
            .map(|attack| (*attack, false))
            .chain(report.offhand.iter().map(|o| (o.attack, true)))
            .collect();
        for (attack, offhand) in strikes {
            let event = match attack_damage(&attack) {
                Ok(damage) => CombatLogEvent::SpellDamage {
                    spell: spell.clone(),
                    damage,
                    info: self.target_log_info(),
                },
                Err(Some(miss)) => CombatLogEvent::SpellMissed {
                    spell: spell.clone(),
                    miss,
                    offhand,
                },
                Err(None) => continue,
            };
            self.engine.log(me, LogUnit::Target, event);
        }
        for &(power, amount) in &report.resource_gained {
            let event = CombatLogEvent::SpellEnergize {
                spell: spell.clone(),
                power,
                amount,
                periodic: false,
                info: self.own_log_info(),
            };
            self.engine.log(me, me, event);
        }
    }

    /// Logs a white swing: `SWING_DAMAGE` or `SWING_MISSED`.
    fn log_swing(&mut self, report: &SwingReport) {
        if !self.engine.is_logging() {
            return;
        }
        let hand = report.hand;
        let event = match attack_damage(&report.attack) {
            Ok(damage) => CombatLogEvent::SwingDamage {
                hand,
                damage,
                info: self.own_log_info(),
            },
            Err(Some(miss)) => CombatLogEvent::SwingMissed { hand, miss },
            Err(None) => return,
        };
        self.engine.log(self.log_me(), LogUnit::Target, event);
    }

    /// Logs a periodic tick: its damage and its power gain.
    fn log_tick(&mut self, spell: LogSpell, report: &TickReport) {
        if !self.engine.is_logging() {
            return;
        }
        let me = self.log_me();
        if report.damage > 0 {
            let event = CombatLogEvent::SpellPeriodicDamage {
                spell: spell.clone(),
                damage: Damage {
                    amount: report.damage,
                    resisted: report.resisted,
                    critical: report.crit,
                    ..Damage::default()
                },
                info: self.target_log_info(),
            };
            self.engine.log(me, LogUnit::Target, event);
        }
        if let Some((power, amount)) = report.resource_gained {
            let event = CombatLogEvent::SpellEnergize {
                spell,
                power,
                amount,
                periodic: true,
                info: self.own_log_info(),
            };
            self.engine.log(me, me, event);
        }
    }

    /// Logs a change of a visible aura that is not a passive's or a sim-only marker's; a debuff
    /// is on the target.
    fn log_aura(&mut self, id: BuffId, change: AuraChange) {
        if !self.engine.is_logging() {
            return;
        }
        let buff = self.buff_ref(id);
        if buff.is_hidden() || buff.is_passive() || !buff.is_in_combat_log() {
            return;
        }
        let debuff = buff.is_debuff();
        let spell = LogSpell {
            id: buff.spell(),
            name: buff.name().to_string(),
            school: buff.school(),
        };
        let me = self.log_me();
        let dest = if debuff { LogUnit::Target } else { me };
        let event = CombatLogEvent::SpellAura {
            spell,
            change,
            debuff,
        };
        self.engine.log(me, dest, event);
    }

    /// Copies the counters kept elsewhere into the statistics: the procs' attempts and
    /// successes, the rotation's executor statistics and the engine's event counts and
    /// elapsed time. Idempotent; called before the statistics are read or taken.
    pub fn sync_statistics(&mut self) {
        let procs = self.character.spells.procs();
        let counts: Vec<(String, u64, u64)> = procs
            .procs()
            .iter()
            .enumerate()
            .filter(|(i, proc)| procs.is_enabled(ProcId(*i as u32)) || proc.attempts() > 0)
            .map(|(_, proc)| {
                (
                    proc.name().to_string(),
                    u64::from(proc.attempts()),
                    u64::from(proc.procs()),
                )
            })
            .collect();
        let executors: Vec<RotationExecutorStatistics> = self
            .character
            .rotation
            .as_ref()
            .map(|rotation| {
                rotation
                    .active_executors()
                    .enumerate()
                    .map(|(i, executor)| RotationExecutorStatistics::from_executor(i + 1, executor))
                    .collect()
            })
            .unwrap_or_default();
        let skipped: Vec<SkippedExecutor> = self
            .character
            .rotation
            .as_ref()
            .map(|rotation| {
                rotation
                    .skipped_executors()
                    .map(|(line, executor)| SkippedExecutor::from_executor(line, executor))
                    .collect()
            })
            .unwrap_or_default();
        let statistics = &mut self.character.statistics;
        for (name, attempts, procs) in counts {
            statistics.proc(&name).set_counts(attempts, procs);
        }
        statistics.set_executors(executors);
        statistics.set_skipped_executors(skipped);
        statistics.set_engine(EngineStatistics::from_engine(self.engine));
    }

    /// Syncs and hands over the statistics of the set of iterations, leaving fresh ones
    /// behind.
    pub fn take_statistics(&mut self) -> ClassStatistics {
        self.sync_statistics();
        let combat_length = self.character.sim().combat_length;
        let fresh = ClassStatistics::new(self.character.player_name(), combat_length);
        std::mem::replace(&mut self.character.statistics, fresh)
    }

    // ---------------------------------------------------------------- equipment

    /// Whether the equipped items satisfy a `SpellEquippedItems` requirement: some equipped
    /// weapon / held item of the required item class whose subclass bit is in the mask.
    pub fn equipped_item_matches(&self, requirement: &EquippedItems) -> bool {
        if requirement.class <= 0 {
            return true;
        }
        [
            EquipmentSlot::Mainhand,
            EquipmentSlot::Offhand,
            EquipmentSlot::Ranged,
        ]
        .into_iter()
        .any(|slot| self.slot_item_matches(slot, requirement))
    }

    /// Whether the weapon / held item in `slot` satisfies a `SpellEquippedItems` requirement.
    fn slot_item_matches(&self, slot: EquipmentSlot, requirement: &EquippedItems) -> bool {
        self.character
            .equipment()
            .weapon_profile(slot)
            .is_some_and(|weapon| {
                let (item_class, subclass) = weapon.weapon_type.item_class_subclass();
                requirement.accepts(item_class, subclass)
            })
    }

    // ---------------------------------------------------------------- rotation

    /// Gives the character a rotation: builds it, links it to the spells and takes its attack
    /// mode.
    pub fn set_rotation(&mut self, spec: Arc<RotationSpec>) {
        let attack_mode = spec.attack_mode;
        let mut rotation = Rotation::new(spec);
        rotation.link(self);
        self.character.put_rotation(Some(rotation));
        self.character.spells.set_attack_mode(attack_mode);
    }

    /// Removes the rotation.
    pub fn clear_rotation(&mut self) {
        self.character.put_rotation(None);
    }

    /// Links the rotation to the spells again, after they changed (a talent, a racial, an
    /// item use came or went).
    pub fn relink_rotation(&mut self) {
        if let Some(mut rotation) = self.character.take_rotation() {
            rotation.link(self);
            self.character.put_rotation(Some(rotation));
        }
    }

    /// Evaluate player action according to current rotation.
    /// This is a no-op before combat start (T < 0).
    pub fn perform_rotation(&mut self) {
        if self.character.manual_input() {
            self.perform_input();
            return;
        }
        if self.now() < 0.0 {
            return;
        }
        if let Some(mut rotation) = self.character.take_rotation() {
            rotation.perform(self);
            self.character.put_rotation(Some(rotation));
        }
    }

    /// Casts the queued input ([`Character::queue_input`]) if it can be cast now; drops it
    /// when waiting cannot help or its time is up ([`Character::take_input_failure`]). Also
    /// before the pull: the player may act before it.
    fn perform_input(&mut self) {
        let Some(input) = self.character.queued() else {
            return;
        };
        let (until, is_macro) = (input.until, input.is_macro);
        let now = self.now();
        loop {
            let Some(input) = self.character.queued() else {
                return;
            };
            let Some(&spell) = input.spells.get(input.next) else {
                // A macro through its entries: nothing cast reports why.
                match input.first_failure {
                    Some((spell, status)) if !input.cast_any => {
                        self.character.fail_queued_input(spell, status);
                    }
                    _ => self.character.clear_queued_input(),
                }
                return;
            };
            let status = self.character.spells.spell(spell).status(self);
            if now > until {
                // Too late, even if usable by now: why it had to wait.
                let waited_on = if status.is_available() {
                    self.character.input_waiting_on()
                } else {
                    status
                };
                self.character.fail_queued_input(spell, waited_on);
                return;
            }
            if status.is_available() {
                let ends = self.character.spells.spell(spell).triggers_gcd();
                let input = self.character.queued_mut().expect("queued");
                input.next += 1;
                input.cast_any = true;
                if ends {
                    // The global cooldown it starts ends a macro: nothing after it fires.
                    self.character.clear_queued_input();
                }
                self.cast(spell);
                if let Some(rotation) = self.character.rotation_mut() {
                    rotation.record_input(now, spell);
                }
                continue;
            }
            let waits = if is_macro {
                matches!(
                    status,
                    SpellStatus::OnGcd
                        | SpellStatus::OnStanceCooldown
                        | SpellStatus::CastInProgress
                )
            } else {
                status.passes_with_time()
            };
            if waits {
                self.character.set_input_waiting_on(status);
                return;
            }
            if !is_macro {
                self.character.fail_queued_input(spell, status);
                return;
            }
            // A macro skips what it cannot cast.
            let input = self.character.queued_mut().expect("queued");
            input.first_failure.get_or_insert((spell, status));
            input.next += 1;
        }
    }

    /// Gives the initial rage (the `initial_rage` setting), casts the rotation's precombat
    /// spells, then starts its precast so that it lands at T=0:
    /// now, or by a `Precast` event when its cast time is shorter than the time left before
    /// the pull (none for a character played by input). Expected to run at T < 0, but not
    /// strictly enforced.
    pub fn run_precombat_actions(&mut self) {
        self.character.gain_initial_rage();
        if self.character.manual_input() {
            return;
        }
        let Some(mut rotation) = self.character.take_rotation() else {
            return;
        };
        rotation.run_precombat_actions(self);
        let precast = rotation.precast_spell();
        self.character.put_rotation(Some(rotation));
        if let Some(spell) = precast {
            let at = -self.spell_cast_time(spell);
            if at > self.now() {
                let character = self.character.id();
                self.engine
                    .add_event(Event::new(at, EventKind::Precast { character }));
            } else {
                self.cast_precast();
            }
        }
    }

    /// Starts the rotation's precast.
    fn cast_precast(&mut self) {
        if let Some(mut rotation) = self.character.take_rotation() {
            rotation.cast_precast(self);
            self.character.put_rotation(Some(rotation));
        }
    }

    /// Seconds before the pull the precombat actions need: one global cooldown, or the
    /// precast's cast time when longer (one global cooldown also without a rotation).
    pub fn time_required_to_run_precombat(&self) -> f64 {
        match self.character.rotation() {
            Some(rotation) => rotation.time_required_to_run_precombat(self),
            None => self.character.global_cooldown(),
        }
    }

    /// The highest learned rank of the spell `name`, or the rank asked for.
    fn spell_rank_by_name(&self, name: &str, rank: u32) -> Option<SpellId> {
        let group = self.character.spells.rank_group(name)?;
        group.get_spell_rank(rank, |id| {
            self.character.spells.spell(id).is_rank_learned(self)
        })
    }
}

/// The values rotation conditions compare.
impl<S: SharedBuffs> ConditionContext<BuffId, SpellId> for CharacterContext<'_, S> {
    fn buff_time_left(&self, buff: &BuffId) -> f64 {
        let buff = self.external_stand_in(*buff).unwrap_or(*buff);
        self.buff_ref(buff).time_left(self.now())
    }

    fn buff_is_active(&self, buff: &BuffId) -> bool {
        let buff = self.external_stand_in(*buff).unwrap_or(*buff);
        self.buff_ref(buff).is_active()
    }

    fn buff_stacks(&self, buff: &BuffId) -> u32 {
        let buff = self.external_stand_in(*buff).unwrap_or(*buff);
        self.buff_ref(buff).stacks()
    }

    fn spell_cooldown_remaining(&self, spell: &SpellId) -> f64 {
        self.character.spells.spell(*spell).cooldown_remaining(self)
    }

    fn resource_level(&self, resource: ResourceType) -> u32 {
        self.character
            .resource_level(resource, self.resource_read_time())
    }

    fn variable(&self, variable: BuiltinVariable) -> f64 {
        let now = self.now();
        let sim = self.character.sim();
        match variable {
            BuiltinVariable::TargetHealth => sim.target_health(now),
            BuiltinVariable::TimeRemainingEncounter => sim.combat_length - now,
            BuiltinVariable::TimeRemainingExecute => {
                sim.combat_length * (1.0 - sim.execute_threshold()) - now
            }
            BuiltinVariable::TimeSinceSwing => {
                now - self.character.spells.mh_attack().last_used().max(0.0)
            }
            BuiltinVariable::TimeRemainingSwing => {
                self.character.spells.mh_attack().time_until_next_swing(now)
            }
            // Ranged auto attacks are not simulated: no shot was ever fired.
            BuiltinVariable::TimeSinceAutoShot => now.max(0.0),
            BuiltinVariable::MeleeAp => f64::from(self.character.melee_ap(&self.target_view())),
            BuiltinVariable::ComboPoints => f64::from(self.character.combo_points(now)),
            BuiltinVariable::TimeRemainingGcd => self.character.time_until_action_ready(now),
        }
    }

    fn target_creature_type(&self) -> CreatureType {
        self.target.creature_type()
    }
}

impl<S: SharedBuffs> RotationHost for CharacterContext<'_, S> {
    fn now(&self) -> f64 {
        self.engine.current_time()
    }

    fn spell_by_name(&self, name: &str, rank: u32) -> Option<SpellId> {
        self.spell_rank_by_name(name, rank)
    }

    fn buff_by_name(&self, name: &str) -> Option<BuffId> {
        let party = self.character.party();
        self.character
            .spells
            .buff_by_name(name, party, self.raid, |id| {
                self.character.spells.spell(id).is_rank_learned(self)
            })
    }

    fn spell_is_enabled(&self, spell: SpellId) -> bool {
        self.character.spells.spell(spell).is_enabled()
    }

    fn missing_talent(&self, spell: SpellId) -> Option<String> {
        let talents = self.character.talents()?;
        let spells = &self.character.spells;
        let ranks: Vec<SpellId> = match spells.rank_group_of(spell) {
            Some(group) => group.spells().collect(),
            None => vec![spell],
        };
        ranks.into_iter().find_map(|id| {
            let node = talents.node_of_spell(spells.spell(id).game_id())?;
            (talents.rank(node) == 0).then(|| {
                talents.spec(node).map_or_else(
                    || spells.spell(id).name().to_string(),
                    |spec| spec.name.clone(),
                )
            })
        })
    }

    fn talent_rank(&self, name: &str) -> Option<u32> {
        let talents = self.character.talents()?;
        talents
            .node_of_name(name, None)
            .map(|node| talents.rank(node))
    }

    fn spell_has_cast_time(&self, spell: SpellId) -> bool {
        self.character.spells.spell(spell).has_cast_time()
    }

    fn spell_cast_time(&self, spell: SpellId) -> f64 {
        self.character.spells.spell(spell).cast_time(self)
    }

    fn spell_status(&self, spell: SpellId) -> SpellStatus {
        self.character.spells.spell(spell).status(self)
    }

    fn cast_spell(&mut self, spell: SpellId) {
        self.cast(spell);
    }

    fn is_casting(&self) -> bool {
        self.character.spells.blocking_cast_in_progress()
    }

    fn gcd_length(&self) -> f64 {
        self.character.global_cooldown()
    }

    fn spell_cost(&self, spell: SpellId) -> u32 {
        self.character.spells.spell(spell).resource_cost(self)
    }
}

impl<S: SharedBuffs> EffectHost for CharacterContext<'_, S> {
    fn caster_level(&self) -> u32 {
        self.character.clvl()
    }

    fn combo_points(&self) -> u32 {
        self.character.combo_points(self.now())
    }

    fn gain_combo_points(&mut self, amount: u32) {
        let now = self.now();
        self.character.gain_combo_points(amount, now);
    }

    fn spend_combo_points(&mut self) {
        self.character.spend_combo_points();
    }

    fn resource_level(&self, resource: ResourceType) -> u32 {
        self.character
            .resource_level(resource, self.resource_read_time())
    }

    /// Every gain wakes the player up, of any resource and from any source
    /// (`Character::add_player_reaction_event` after `Warrior::gain_rage`,
    /// `Rogue::gain_energy`, ...).
    fn gain_resource(&mut self, resource: ResourceType, amount: u32) -> u32 {
        let now = self.now();
        let gained = self.character.gain_resource(resource, amount, now);
        if gained > 0 {
            self.add_player_reaction_event();
        }
        gained
    }

    fn adjust_power_regen_percent(&mut self, resource: ResourceType, percent: i32) {
        let now = self.now();
        self.character
            .adjust_power_regen_percent(resource, percent, now);
    }

    fn adjust_max_power(&mut self, resource: ResourceType, amount: i32) {
        let now = self.now();
        self.character.adjust_max_power(resource, amount, now);
    }

    fn melee_ap(&self) -> u32 {
        self.character.melee_ap(&self.target_view())
    }

    fn max_health(&self) -> u32 {
        self.character.max_health(&self.target_view())
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
            self.character.stats().get_mh_ability_crit_chance(&stat_ctx) + extra_crit
        } else {
            0
        };
        self.character.roll_mut().get_melee_ability_result(
            &roll_ctx,
            Hand::Mainhand,
            skill,
            crit,
            included,
        )
    }

    fn roll_spell(
        &mut self,
        school: MagicSchool,
        kind: SpellResistKind,
        extra_crit: u32,
        can_crit: bool,
    ) -> SpellRoll {
        let view = self.target_view();
        let roll_ctx = self.character.magic_roll_context(&view, school);
        let crit = if can_crit {
            let stat_ctx = self.character.stat_context(&view);
            self.character
                .stats()
                .get_spell_crit_chance(&stat_ctx, school)
                + extra_crit
        } else {
            0
        };
        self.character
            .roll_mut()
            .get_spell_ability_result(&roll_ctx, school, crit, kind)
    }

    fn stats_mut(&mut self) -> &mut CharacterStats {
        self.character.stats_mut()
    }

    fn target_mut(&mut self) -> &mut Target {
        self.target
    }
    fn target_creature_type(&self) -> CreatureType {
        self.target.creature_type()
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

    fn leave_stance(&mut self, stance: Stance) {
        if self.character.stance() == stance {
            CharacterContext::swap_stance(self, Stance::Caster);
        }
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

    fn has_two_hand_weapon(&self) -> bool {
        self.character.equipment().has_two_hand_weapon()
    }

    fn mainhand_weapon_type(&self) -> Option<WeaponType> {
        self.character
            .equipment()
            .weapon_profile(EquipmentSlot::Mainhand)
            .map(|weapon| weapon.weapon_type)
    }

    /// One of the character's own active debuffs is a poison (`DispelType` 4).
    fn target_poisoned_by_caster(&self) -> bool {
        let spells = self.character.spells();
        spells.buff_ids().any(|id| {
            let buff = self.buff_ref(id);
            buff.is_debuff()
                && buff.is_active()
                && spells.spell_by_game_id(buff.spell()).is_some_and(|spell| {
                    spells.spell(spell).record().categories.dispel_type == DISPEL_TYPE_POISON
                })
        })
    }

    fn adjust_offhand_copy(&mut self, spell: u32, apply: bool) {
        self.character.adjust_offhand_copy(spell, apply);
    }

    fn adjust_resource_on_use(
        &mut self,
        spell: u32,
        resource: ResourceType,
        amount: u32,
        apply: bool,
    ) {
        self.character
            .adjust_resource_on_use(spell, resource, amount, apply);
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

    fn resource_type(&self) -> ResourceType {
        self.character.resource_type()
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

    fn target_start_health(&self) -> f64 {
        self.character.sim().target_start_health
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

    /// The stance swap lag pushes the global cooldown forward slightly. The player action is
    /// scheduled for then.
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

    fn running_to_target(&self) -> bool {
        self.character.spells().running_to_target()
    }

    fn start_cast(&mut self, running_to_target: bool) -> u32 {
        self.character.spells_mut().start_cast(running_to_target)
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

    /// Marks the character as attacking and schedules the swings of each hand.
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

    fn queue_next_swing(&mut self, spell: SpellId, marker: Option<BuffId>) {
        if self.character.spells().queued_next_swing() != Some(spell) {
            // Cleave replacing a queued Heroic Strike.
            SpellHost::cancel_next_swing(self);
        }
        self.character.spells_mut().queue_next_swing(spell, marker);
    }

    fn cancel_next_swing(&mut self) {
        let marker = self.character.spells().queued_next_swing_marker();
        self.character.spells_mut().cancel_next_swing();
        if let Some(marker) = marker {
            self.cancel_buff(marker);
        }
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

    fn weapon_in_hand_matches(&self, hand: Hand, requirement: &EquippedItems) -> bool {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        self.slot_item_matches(slot, requirement)
    }

    fn attacking_from_behind(&self) -> bool {
        self.character.is_attacking_from_behind()
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

    /// A refund is a gain like any other: the player reacts to it.
    fn refund_resource(&mut self, resource: ResourceType, amount: f64) {
        let now = self.now();
        if self.character.refund_resource(resource, amount, now) > 0.0 {
            self.add_player_reaction_event();
        }
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
        if self.external_stand_in(id).is_some() {
            return BuffApplication::NotApplied;
        }
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
        match application {
            BuffApplication::NotApplied => {}
            BuffApplication::Applied { .. } => self.log_aura(id, AuraChange::Applied),
            BuffApplication::Refreshed { stacks } if stacks > before => {
                self.log_aura(id, AuraChange::AppliedDose(stacks));
            }
            BuffApplication::Refreshed { .. } => self.log_aura(id, AuraChange::Refresh),
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
            self.record_buff_removed(id);
            self.end_linked_auras(id);
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
        let report = self.with_spell(id, |s, ctx| {
            s.set_trigger_value(trigger_value);
            s.perform_triggered(ctx)
        });
        self.record_cast(id, &report, None);
        Some(report)
    }

    fn trigger_strike(&mut self, spell: u32, damage_mod: f64) -> Option<CastReport> {
        let id = self.character.spells().spell_by_game_id(spell)?;
        let report = self.with_spell(id, |s, ctx| {
            s.set_strike(Some(damage_mod));
            let report = s.perform_triggered(ctx);
            s.set_strike(None);
            report
        });
        self.record_cast(id, &report, None);
        Some(report)
    }

    fn reset_cooldowns(&mut self, matches: &dyn Fn(&crate::spell::SpellRecord) -> bool) {
        let spells = &self.character.spells;
        let cooldowns: Vec<CooldownId> = spells
            .spell_ids()
            .map(|id| spells.spell(id))
            .filter(|spell| matches(spell.record()))
            .flat_map(Spell::cooldown_ids)
            .collect();
        for id in cooldowns {
            self.character
                .spells_mut()
                .cooldowns_mut()
                .get_mut(id)
                .reset();
        }
    }

    fn set_spell_effect_value(&mut self, spell: u32, index: u32, value: f64) {
        let spells = self.character.spells();
        let script = if let Some(id) = spells.spell_by_game_id(spell) {
            let script = spells
                .spell(id)
                .setup()
                .overrides
                .effect_script(index)
                .copied();
            self.with_spell(id, |s, ctx| s.set_effect_value(ctx, index, value));
            script
        } else if let Some(id) = spells.proc_by_game_id(spell) {
            let script = spells
                .procs()
                .get(id)
                .spell()
                .setup()
                .overrides
                .effect_script(index)
                .copied();
            self.with_procs(|procs, ctx| {
                procs
                    .get_mut(id)
                    .spell_mut()
                    .set_effect_value(ctx, index, value);
            });
            script
        } else {
            None
        };
        // The hidden aura an `ENABLE_AURA` effect enables carries its value (a talent's rank).
        if let Some(script) = script.filter(|s| s.script == ScriptKind::EnableAura)
            && let (Some(target), Some(effect)) = (script.params.spell, script.params.effect)
        {
            self.set_spell_effect_value(target, effect, value);
        }
    }

    fn target_armor(&self) -> i32 {
        self.target.armor()
    }

    fn armor_penetration_percent(&self, hand: Hand) -> u32 {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        self.character
            .stats()
            .get_armor_penetration_percent(self.character.equipment().weapon_profile(slot))
    }

    fn target_block_value(&self) -> u32 {
        self.target.block_value()
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

    fn spell_crit_dmg_mod(&self) -> f64 {
        let view = self.target_view();
        self.character
            .stats()
            .get_spell_crit_dmg_mod(&self.character.stat_context(&view))
    }

    fn roll_periodic_resist(&mut self, school: MagicSchool, pure_dot: bool) -> MagicResistResult {
        let view = self.target_view();
        let roll_ctx = self.character.magic_roll_context(&view, school);
        self.character
            .roll_mut()
            .get_periodic_resist_result(&roll_ctx, school, pure_dot)
    }

    fn roll_periodic_crit(&mut self, extra_crit: u32) -> bool {
        self.roll_melee_ability(IncludedOutcomes::NONE, extra_crit, true)
            == PhysicalAttackResult::Critical
    }

    fn total_threat_mod(&self) -> f64 {
        self.character.stats().get_total_threat_mod()
    }

    fn avg_weapon_damage(&self, hand: Hand) -> f64 {
        let target = self.target_view();
        match hand {
            Hand::Mainhand => self.character.avg_mh_weapon_damage(&target),
            Hand::Offhand => self.character.avg_oh_weapon_damage(&target),
        }
    }

    fn proc_hand(&self) -> Hand {
        self.character.proc_hand()
    }

    fn offhand_copy_active(&self, spell: u32) -> bool {
        self.character.has_offhand_copy(spell) && self.character.is_dual_wielding()
    }

    fn is_dual_wielding(&self) -> bool {
        self.character.is_dual_wielding()
    }

    fn resources_on_use(&self, spell: u32) -> Vec<(ResourceType, u32)> {
        self.character.resources_on_use(spell).collect()
    }

    fn roll_offhand_melee_ability(
        &mut self,
        included: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult {
        let view = self.target_view();
        let roll_ctx = self.character.refresh_roll_context(&view);
        let stat_ctx = self.character.stat_context(&view);
        let skill = self.character.stats().get_oh_wpn_skill(&stat_ctx);
        let crit = if can_crit {
            self.character.stats().get_oh_ability_crit_chance(&stat_ctx) + extra_crit
        } else {
            0
        };
        self.character.roll_mut().get_melee_ability_result(
            &roll_ctx,
            Hand::Offhand,
            skill,
            crit,
            included,
        )
    }

    fn random_oh_weapon_dmg(&mut self, normalized: bool) -> f64 {
        let view = self.target_view();
        if normalized {
            self.character.random_normalized_oh_dmg(&view)
        } else {
            self.character.random_non_normalized_oh_dmg(&view)
        }
    }

    fn offhand_penalty(&self) -> f64 {
        self.character.spells.oh_attack().offhand_penalty()
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

    fn hand_weapon_matches(&self, hand: Hand, requirement: &EquippedItems) -> bool {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        self.slot_item_matches(slot, requirement)
    }

    fn aura_effect_value(&self, spell: u32, effect: u32) -> Option<f64> {
        let spells = self.character.spells();
        let buff = match spells.handle(spell)? {
            SpellHandle::Spell(id) => spells.spell(id).marker_buff(),
            SpellHandle::Proc(id) => spells.procs().get(id).spell().marker_buff(),
        }?;
        self.buff(buff)
            .effects
            .iter()
            .find(|e| e.index() == effect)
            .map(Effect::value)
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
            .get_melee_hit_result(&roll_ctx, hand, skill, crit)
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

    fn melee_crit_dmg_mod(&self) -> f64 {
        2.0
    }

    fn gain_swing_rage(&mut self, hand: Hand, crit: bool) -> Option<f64> {
        self.character.gain_swing_rage(hand, crit)
    }

    fn add_player_reaction_event(&mut self) {
        let character = self.character.id();
        self.engine
            .add_event_in(PLAYER_REACTION_DELAY, EventKind::PlayerAction { character });
    }

    fn is_melee_attacking(&self) -> bool {
        self.character.spells().is_melee_attacking()
    }
}

/// One spell the equipment grants.
struct Grant {
    key: EquipmentSpellKey,
    spell: u32,
    kind: GrantKind,
}

/// How a granted spell runs; the sources are the attacks that may trigger it.
enum GrantKind {
    /// A proc aura (an enchant's or hand-authored item's `procs` entry).
    Proc(Vec<ProcSource>),
    /// An on-equip or set bonus spell: a proc when it is a proc aura, else an aura while worn.
    Passive(Vec<ProcSource>),
    /// A weapon's chance-on-hit spell.
    OnHit(Vec<ProcSource>),
    /// An item enchantment's combat spell (a rogue poison), cast at `chance` % on the enchanted
    /// hand's hits.
    CombatSpell {
        sources: Vec<ProcSource>,
        chance: f64,
    },
    /// An item's on-use spell, with the item effect's cooldowns; a consumable's carries the
    /// consumable's name.
    Use(ItemEffect, Option<String>),
}

/// `setup` with the cooldowns of the item effect that grants it: the item's own cooldown and
/// its shared category cooldown replace the spell record's where the item has them. Using an
/// item does not break Stealth (the server does not end it for casts from items).
fn with_item_cooldowns(mut setup: SpellSetup, effect: &ItemEffect) -> SpellSetup {
    let record = Arc::make_mut(&mut setup.record);
    record.attributes[1] |= SpellAttr1::ALLOW_WHILE_STEALTHED.bits();
    if let Some(ms) = effect.cooldown_ms {
        record.cooldown.recovery_ms = ms;
    }
    if let Some(category) = effect.category {
        record.categories.category = category;
        record.cooldown.category_recovery_ms = effect.category_cooldown_ms.unwrap_or(0);
    }
    setup
}

/// The proc spell of an item enchantment's combat spell ([`crate::proc::Proc::combat_spell`]):
/// the payload's record without its effects (the learned payload does the work, the proc only
/// keeps its name and class options for the chance modifiers), rolled at the enchantment's
/// `chance` in percent.
fn combat_spell_setup(mut setup: SpellSetup, chance: f64) -> SpellSetup {
    let record = Arc::make_mut(&mut setup.record);
    record.effects.clear();
    record.duration_ms = None;
    let proc = setup
        .overrides
        .proc
        .get_or_insert_with(ProcOverride::default);
    proc.chance = Some(chance);
    proc.ppm = None;
    setup
}

/// The attacks an on-equip or set bonus proc aura may react to: a weapon's only its own hand's
/// (Ironfoe off the main hand), anything else whatever its `ProcTypeMask` names.
fn passive_proc_sources(slot: Option<EquipmentSlot>) -> Vec<ProcSource> {
    match slot {
        Some(slot) if slot.is_weapon_slot() => slot.default_proc_sources(),
        _ => ProcSource::ALL.to_vec(),
    }
}

/// The spells the record of `spell` casts: its `EffectTriggerSpell`s and the spells its
/// override scripts name (the server-side `DUMMY` payloads). They must be learned before a proc
/// built from the record can trigger them.
fn payload_spells(db: &SpellDb, spell: u32) -> Vec<u32> {
    let mut ids = db
        .get(spell)
        .map(|r| r.trigger_spells())
        .unwrap_or_default();
    for id in db
        .overrides()
        .get(spell)
        .map(SpellOverride::referenced_spells)
        .unwrap_or_default()
    {
        if id != spell && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// `sources` with no spell behind them (a swing's).
fn untriggered(sources: &[ProcSource]) -> Vec<(ProcSource, ProcTrigger)> {
    sources
        .iter()
        .map(|&source| (source, ProcTrigger::default()))
        .collect()
}

/// The combat log's name for `spell`.
fn log_spell(spell: &Spell) -> LogSpell {
    LogSpell {
        id: spell.game_id(),
        name: spell.name().to_string(),
        school: spell.record().school_mask.bits(),
    }
}

/// The damage of an attack that landed, or how it was avoided (`None` for a hit without
/// damage, such as Sunder Armor's, which only logs its cast and its aura).
fn attack_damage(attack: &AttackOutcome) -> Result<Damage, Option<MissType>> {
    use PhysicalAttackResult as R;
    // A spell on the magic table that missed or was resisted is logged as resisted.
    if let Some(spell) = attack.spell {
        if !spell.roll.landed() {
            return Err(Some(MissType::Resist));
        }
        if attack.damage == 0 {
            return Err(None);
        }
        return Ok(Damage {
            amount: attack.damage,
            resisted: spell.resisted,
            critical: spell.roll.is_critical(),
            glancing: false,
        });
    }
    let miss = match attack.result {
        R::Miss => MissType::Miss,
        R::Dodge => MissType::Dodge,
        R::Parry => MissType::Parry,
        R::Block | R::BlockCritical if attack.damage == 0 => MissType::Block,
        _ if attack.damage == 0 => return Err(None),
        result => {
            return Ok(Damage {
                amount: attack.damage,
                resisted: 0,
                critical: matches!(result, R::Critical | R::BlockCritical),
                glancing: result == R::Glancing,
            });
        }
    };
    Err(Some(miss))
}
