use std::sync::Arc;

use crate::buff::external::ExternalBuffDb;
use crate::buff::{Buff, BuffApplication, BuffContext, BuffKind, ChargeUse};
use crate::character_spells::{
    AddedSpell, BuffSlot, EquipmentGrantor, EquipmentSpellKey, PartyAuraChange, SharedBuffs,
    SpellHandle,
};
use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult};
use crate::cooldown::CooldownControl;
use crate::effect::{Effect, EffectHost};
use crate::enchant::EnchantName;
use crate::engine::{Engine, Event, EventKind};
use crate::equipment::{EnchantError, EquipChange, EquipError};
use crate::ids::{BuffId, CharId, CooldownId, ProcId, SpellId};
use crate::item::{EffectTrigger, EquipmentSlot, ItemEffect};
use crate::proc::{ProcHost, ProcSource};
use crate::resource::ResourceType;
use crate::rotation::{BuiltinVariable, ConditionContext, Rotation, RotationHost, RotationSpec};
use crate::rulesets::Ruleset;
use crate::spell::dbc::AuraState;
use crate::spell::modifiers::SpellModifiers;
use crate::spell::overrides::{EventScript, ScriptKind, SimFlag, SpellOverride};
use crate::spell::periodic::TickReport;
use crate::spell::record::{EquippedItems, SpellDb};
use crate::spell::{
    AutoAttack, AutoAttackHost, CastReport, Hand, Spell, SpellHost, SpellSetup, SpellStatus,
    SwingReport,
};
use crate::stance::Stance;
use crate::statistics::{ClassStatistics, EngineStatistics, RotationExecutorStatistics};
use crate::stats::{CharacterStats, TargetStatView};
use crate::talent::{CharacterTalents, RankChange};
use crate::target::Target;

use super::{Character, SimParams, StanceLink};

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
        if let Some(proc) = added.proc {
            if added.enable_now {
                self.enable_proc(proc);
            }
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

    /// Learns every spell of `db` that the character's class or race can have: class spells
    /// (any `class_mask`, including the talent-granted ones which stay disabled) and the
    /// racials whose `race_mask` names the race. The external buff auras
    /// (`SpellDb::is_learnable`) are not spells of the character.
    pub fn learn_all(&mut self, db: &SpellDb) -> Vec<AddedSpell> {
        let race = self.character.race();
        let mut ids: Vec<u32> = db
            .records()
            .into_iter()
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
        if let SpellHandle::Spell(id) = handle {
            if let Some(group) = spells.rank_group_of(id) {
                handles.extend(
                    group
                        .spells()
                        .filter(|&other| other != id)
                        .map(SpellHandle::Spell),
                );
            }
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

    /// Puts the temporary enchant `enchant` (sharpening stone, oil, Windfury Totem, ...) on the
    /// item in `slot` (`None` removes it) and registers what it grants.
    pub fn set_temp_enchant(
        &mut self,
        db: &SpellDb,
        slot: EquipmentSlot,
        enchant: Option<EnchantName>,
    ) -> Result<(), EnchantError> {
        self.character.equipment.set_temp_enchant(slot, enchant)?;
        self.sync_equipment_spells(db);
        Ok(())
    }

    /// Brings the spells the equipment grants in line with what is worn. Each is registered
    /// once per grantor (with the payloads it casts learned) and enabled; those of an item,
    /// enchant or set bonus no longer worn are disabled. Port of `Item::apply_proc`, the
    /// `EnchantProc` constructor / destructor and `SetBonusControl`, which created and destroyed
    /// them with the equipment.
    ///
    /// - An enchant's or hand-authored item's `procs` entry that names a spell is a proc.
    /// - An item's on-equip effect and a reached set bonus are passives: a proc aura (with a
    ///   `ProcTypeMask`) becomes a proc, any other aura is up while worn (stats, spell
    ///   modifiers).
    /// - An item's chance-on-hit effect is a proc of the wielding hand casting the spell at the
    ///   target ([`crate::proc::Proc::on_hit`]); one whose rate is unknown is skipped.
    /// - An item's on-use effect is a spell the rotation casts by its name, with the item's
    ///   cooldown and shared category cooldown (the trinkets' 1141) in place of the record's.
    ///   One the sim cannot run (an effect without its script, a proc aura while the buff is
    ///   up: Badge of the Swarmguard) is skipped, so it does not start the shared cooldown for
    ///   nothing.
    ///
    /// Spells missing from `db` (pruned by the export: stuns, heals, immunities, ...) and
    /// ignored ones (`IGNORED`) are skipped. The rotation is linked again afterwards.
    pub fn sync_equipment_spells(&mut self, db: &SpellDb) {
        let mut wanted = Vec::new();
        let mut uses = Vec::new();
        for grant in self.granted_equipment_spells() {
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
            if matches!(grant.kind, GrantKind::Use(_)) && !runnable_use() {
                continue;
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
            let is_proc_aura =
                record.is_passive() && record.aura_options.proc_type_mask.bits() != 0;
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
                GrantKind::Use(effect) => {
                    let setup = with_item_cooldowns(setup, &effect);
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
    fn granted_equipment_spells(&self) -> Vec<Grant> {
        let equipment = self.character.equipment();
        let mut grants = Vec::new();
        let procs = equipment
            .equipped_items()
            .map(|(slot, item)| (slot, EquipmentGrantor::ItemProc(item.id()), item.procs()))
            .chain(equipment.active_enchants().into_iter().map(|(slot, spec)| {
                (
                    slot,
                    EquipmentGrantor::Enchant(spec.name),
                    spec.procs.as_slice(),
                )
            }));
        for (slot, grantor, specs) in procs {
            for (index, spec) in specs.iter().enumerate() {
                let Some(spell) = spec.spell else {
                    continue;
                };
                let allowed = if spec.sources.is_empty() {
                    slot.default_proc_sources()
                } else {
                    spec.sources.sources(slot)
                };
                grants.push(Grant {
                    key: EquipmentSpellKey {
                        slot: Some(slot),
                        grantor,
                        index,
                    },
                    spell,
                    kind: GrantKind::Proc(allowed),
                });
            }
        }
        for (slot, item) in equipment.equipped_items() {
            for (index, effect) in item.effects().iter().enumerate() {
                let kind = match effect.trigger {
                    EffectTrigger::Use => GrantKind::Use(effect.clone()),
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
        grants
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
        let (buff, stacks) = (entry.buff, entry.stacks);
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
        let report = self.with_spell(id, |spell, ctx| spell.perform(ctx));
        self.after_cast(id, &report);
        self.perform_extra_attacks();
        report
    }

    /// Records the statistics of a completed cast of `id` and runs its proc sources.
    fn after_cast(&mut self, id: SpellId, report: &CastReport) {
        self.record_cast(id, report);
        self.run_sources(&report.all_proc_sources());
    }

    /// Records the statistics of a swing and runs its proc sources.
    fn after_swing(&mut self, report: &SwingReport) {
        self.record_swing(report);
        self.run_sources(&report.proc_sources);
    }

    /// Runs the proc checks for `sources`, then uses the charges of the buffs that react to
    /// them. The charges go after the procs: a landed swing consumes a charge when its damage
    /// lands, one batch after the procs it triggered (the `classic-warrior` wiki on Windfury
    /// Totem), so the swing that proc'd Windfury uses a charge of the aura it just applied.
    fn run_sources(&mut self, sources: &[ProcSource]) {
        self.run_proc_checks(sources);
        self.run_event_scripts(sources);
        for &source in sources {
            self.consume_charges(source);
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
                    self.character.gain_combo_points(value);
                }
                other => unreachable!("on_event script {other:?} is refused by the overrides"),
            }
        }
    }

    /// Runs the proc check for each source and returns the reports of the procs that fired.
    /// The sources of one event form one check: a proc fires at most once per event. Extra
    /// attacks the procs granted stay pending so that no proc re-fires off its own extra attack or
    /// twice in one chain of extra attacks.
    pub fn run_proc_checks(&mut self, sources: &[ProcSource]) -> Vec<(ProcId, CastReport)> {
        let before = self.character.pending_extra_attacks();
        self.character.spells.procs_mut().begin_check();
        let mut fired = Vec::new();
        for &source in sources {
            if source == ProcSource::Manual {
                continue;
            }
            fired.extend(self.with_procs(|procs, ctx| procs.run_proc_check(source, ctx)));
        }
        for (id, report) in &fired {
            let (name, rank) = {
                let spell = self.character.spells.procs().get(*id).spell();
                (spell.name().to_string(), spell.rank())
            };
            self.record_report(&name, rank, report);
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

    /// Fires proc `id` now, whatever its chance, and records it; the extra attacks it grants
    /// stay pending.
    #[cfg(test)]
    pub(crate) fn perform_proc(&mut self, id: ProcId) -> CastReport {
        let report = self.with_procs(|procs, ctx| procs.get_mut(id).perform(ctx));
        let (name, rank) = {
            let spell = self.character.spells.procs().get(id).spell();
            (spell.name().to_string(), spell.rank())
        };
        self.record_report(&name, rank, &report);
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
        let report = self.with_spell(queued, |spell, ctx| spell.perform_on_swing(ctx));
        self.after_cast(queued, &report);
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
    /// queued ([`Character::is_dual_wielding`] is false then only for the attack table).
    pub fn oh_swing(&mut self, iteration: u32) -> SwingOutcome {
        if !self.character.spells.oh_attack().attack_is_valid(iteration)
            || !self.character.spells.is_melee_attacking()
            || !self.character.equipment().is_dual_wielding()
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
        if self.character.equipment().is_dual_wielding() {
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
            let (stacks, applied, hidden) = {
                let buff = self.buff_ref(id);
                (buff.stacks(), buff.applied_at(), buff.is_hidden())
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
            if !hidden {
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
                self.after_cast(id, &report);
                self.perform_extra_attacks();
            }
        }
        self.start_attack();
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
                if let Some(report) = self.with_spell(spell, |s, ctx| s.complete_cast(cast_id, ctx))
                {
                    self.after_cast(spell, &report);
                    self.perform_extra_attacks();
                }
            }
            _ => return false,
        }
        true
    }

    /// A periodic tick of `spell`; returns the tick report if the application is current.
    pub fn dot_tick(&mut self, spell: SpellId, application_id: u32) -> Option<TickReport> {
        let report = self.with_spell(spell, |s, ctx| s.perform_periodic(application_id, ctx))?;
        self.record_tick(spell, &report);
        Some(report)
    }

    /// Uses a charge of every buff that reacts to `source`.
    pub fn consume_charges(&mut self, source: ProcSource) {
        for id in self.character.spells.charge_consumers(source) {
            let (buff, mut ctx) = self.buff_ctx(id);
            if buff.use_charge(&mut ctx) == ChargeUse::Removed {
                self.remove_auras(id);
                self.record_buff_removed(id);
            }
        }
    }

    // ---------------------------------------------------------------- statistics

    /// Records the attack outcome and resource gains of a cast of `id` (not of the spells it
    /// triggered: those were recorded when they were cast through [`SpellHost::trigger_spell`]).
    fn record_cast(&mut self, id: SpellId, report: &CastReport) {
        let (name, rank) = {
            let spell = self.character.spells.spell(id);
            (spell.name().to_string(), spell.rank())
        };
        self.record_report(&name, rank, report);
    }

    /// Records a cast report under `name` / `rank`.
    fn record_report(&mut self, name: &str, rank: u32, report: &CastReport) {
        let statistics = &mut self.character.statistics;
        if let Some(attack) = &report.attack {
            statistics
                .spell(name, rank)
                .record_attack(attack, f64::from(report.resource_cost));
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
        statistics.spell(name, 1).record_attack(&report.attack, 0.0);
        if let Some(rage) = report.rage_gained {
            statistics
                .resource(name, 1)
                .add_gain(ResourceType::Rage, rage);
        }
    }

    /// Records a periodic tick of `id`: its damage as a hit, its resource gain.
    fn record_tick(&mut self, id: SpellId, report: &TickReport) {
        let (name, rank) = {
            let spell = self.character.spells.spell(id);
            (spell.name().to_string(), spell.rank())
        };
        let statistics = &mut self.character.statistics;
        if report.damage > 0 || report.threat > 0.0 {
            statistics.spell(&name, rank).record_tick(
                report.damage,
                report.threat,
                report.resource_cost,
                report.execution_time,
            );
        }
        if let Some((resource, amount)) = report.resource_gained {
            statistics.resource(&name, rank).add_gain(resource, amount);
        }
    }

    /// Records the application that just ended for a removed buff.
    fn record_buff_removed(&mut self, id: BuffId) {
        let buff = self.buff_ref(id);
        if buff.is_hidden() {
            return;
        }
        let uptime = buff.expired_at() - buff.applied_at();
        self.record_buff_uptime(id, uptime);
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
        let statistics = &mut self.character.statistics;
        for (name, attempts, procs) in counts {
            statistics.proc(&name).set_counts(attempts, procs);
        }
        statistics.set_executors(executors);
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
        if self.now() < 0.0 {
            return;
        }
        if let Some(mut rotation) = self.character.take_rotation() {
            rotation.perform(self);
            self.character.put_rotation(Some(rotation));
        }
    }

    /// Casts the rotation's precombat spells.
    /// Expected to run at T < 0, but not strictly enforced.
    pub fn run_precombat_actions(&mut self) {
        if let Some(rotation) = self.character.take_rotation() {
            rotation.run_precombat_actions(self);
            self.character.put_rotation(Some(rotation));
        }
    }

    /// Seconds before the pull the precombat actions need: the precast's cast time, else one
    /// global cooldown (also without a rotation).
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
        self.buff_ref(*buff).time_left(self.now())
    }

    fn buff_is_active(&self, buff: &BuffId) -> bool {
        self.buff_ref(*buff).is_active()
    }

    fn buff_stacks(&self, buff: &BuffId) -> u32 {
        self.buff_ref(*buff).stacks()
    }

    fn spell_cooldown_remaining(&self, spell: &SpellId) -> f64 {
        self.character.spells.spell(*spell).cooldown_remaining(self)
    }

    fn resource_level(&self, resource: ResourceType) -> u32 {
        self.character.resource_level(resource)
    }

    fn variable(&self, variable: BuiltinVariable) -> f64 {
        let now = self.now();
        let sim = self.character.sim();
        match variable {
            BuiltinVariable::TargetHealth => (sim.combat_length - now) / sim.combat_length,
            BuiltinVariable::TimeRemainingEncounter => sim.combat_length - now,
            BuiltinVariable::TimeRemainingExecute => {
                sim.combat_length * (1.0 - sim.execute_threshold) - now
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
            BuiltinVariable::ComboPoints => f64::from(self.character.combo_points()),
            BuiltinVariable::TimeRemainingGcd => self.character.time_until_action_ready(now),
        }
    }
}

impl<S: SharedBuffs> RotationHost for CharacterContext<'_, S> {
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
        self.character.spells.cast_in_progress()
    }

    fn gcd_length(&self) -> f64 {
        self.character.global_cooldown()
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

    /// Marks the character as attacking and schedules the swings of each hand.
    fn start_attack(&mut self) {
        self.character.spells_mut().start_attack();
        self.schedule_swings();
    }

    fn reset_swing_timers(&mut self) {
        self.with_auto_attack(Hand::Mainhand, |attack, ctx| {
            attack.reset_swing_timer_and_schedule(ctx)
        });
        if self.character.equipment().is_dual_wielding() {
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
        self.record_cast(id, &report);
        Some(report)
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

    fn add_player_reaction_event(&mut self) {
        let character = self.character.id();
        self.engine
            .add_event_in(0.1, EventKind::PlayerAction { character });
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
    /// An item's on-use spell, with the item effect's cooldowns.
    Use(ItemEffect),
}

/// `setup` with the cooldowns of the item effect that grants it: the item's own cooldown and
/// its shared category cooldown replace the spell record's where the item has them.
fn with_item_cooldowns(mut setup: SpellSetup, effect: &ItemEffect) -> SpellSetup {
    let record = Arc::make_mut(&mut setup.record);
    if let Some(ms) = effect.cooldown_ms {
        record.cooldown.recovery_ms = ms;
    }
    if let Some(category) = effect.category {
        record.categories.category = category;
        record.cooldown.category_recovery_ms = effect.category_cooldown_ms.unwrap_or(0);
    }
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
