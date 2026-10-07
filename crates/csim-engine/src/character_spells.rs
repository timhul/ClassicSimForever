//! The per-character spell registry. Port of `Character/CharacterSpells.*` and
//! `Character/EnabledBuffs.*` (the registry parts; the class spellbook, consumables and racials
//! are added by the character in Phase 4, the auto attacks in 3.12).
//!
//! `CharacterSpells` owns the character's spells, rank groups, cooldown controls, buffs and
//! procs, addressed by the typed handles. Spells are added by game id from a
//! [`SpellDb`]: [`CharacterSpells::add_spell`] builds the [`Spell`] (or the [`Proc`] for a
//! passive with a `ProcTypeMask`), its cooldown controls (own and category — spells of one
//! `SpellCategory` share one control), its marker buff (shared ones through the raid) and its
//! rank group. It is a registry, not an actor: operations that need the world
//! (`Spell::perform`, `Spell::enable`, proc checks) are run by the character's spell context,
//! which implements [`crate::spell::SpellHost`] and borrows this registry. To let a spell
//! mutate the world while it is being performed, the context takes it out with
//! [`CharacterSpells::take_spell`] and puts it back with [`CharacterSpells::put_spell`].
//!
//! Buffs shared with the raid (party buffs, shared debuffs) are owned by the raid control; the
//! character keeps a [`BuffSlot::Shared`] handle to them so every buff a character refers to
//! has a [`BuffId`] in its own list.

use std::collections::BTreeMap;

use crate::attack_mode::AttackMode;
use crate::buff::{Buff, BuffKind};
use crate::cooldown::{CooldownRegistry, category_cooldown_name};
use crate::enchant::EnchantName;
use crate::ids::{BuffId, CharId, CooldownId, InstanceId, ProcId, SharedBuffId, SpellId};
use crate::item::EquipmentSlot;
use crate::proc::{EnabledProcs, Proc, ProcSource};
use crate::spell::overrides::{Overrides, SimFlag};
use crate::spell::record::{ClassOptions, SpellDb};
use crate::spell::{AutoAttack, Hand, MAX_RANK, Spell, SpellRankGroup, SpellSetup};

/// Where a buff in a character's buff list lives.
#[derive(Debug, Clone)]
pub enum BuffSlot {
    /// Owned by this character.
    Owned(Box<Buff>),
    /// Owned by the raid control (party buff or shared debuff).
    Shared(SharedBuffId),
}

/// The raid's registry of buffs shared between characters. Port of the shared buff part of
/// `RaidControl`; implemented by [`crate::raid::SharedBuffRegistry`].
pub trait SharedBuffs {
    /// The registered party buff with `canonical_name` in `party`, if any.
    fn shared_party_buff(&self, canonical_name: &str, party: u8) -> Option<SharedBuffId>;
    /// Registers a new (enabled) party buff and returns its handle.
    fn register_shared_party_buff(&mut self, buff: Buff, party: u8) -> SharedBuffId;
    fn shared_raid_buff(&self, canonical_name: &str) -> Option<SharedBuffId>;
    fn register_shared_raid_buff(&mut self, buff: Buff) -> SharedBuffId;
    /// The shared buff behind a handle.
    fn shared_buff(&self, id: SharedBuffId) -> &Buff;
    fn shared_buff_mut(&mut self, id: SharedBuffId) -> &mut Buff;
    /// Records that a character applied or removed the aura effects of a shared party buff on
    /// itself, for the raid control to do the same on the party's other members afterwards
    /// (the C++ `RaidControl::apply_party_buff` / `remove_party_buff` loop).
    fn note_party_aura_change(&mut self, change: PartyAuraChange);
}

/// A shared party buff's aura effects were applied to (or removed from) the character `by`,
/// one of the members of `party`; the other members are still to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartyAuraChange {
    pub buff: SharedBuffId,
    pub party: u8,
    pub by: CharId,
    /// `true` for an application, `false` for a removal.
    pub apply: bool,
}

/// What [`CharacterSpells::add_spell`] created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedSpell {
    /// The spell's handle, or `None` for a proc (whose spell lives in the procs).
    pub spell: Option<SpellId>,
    /// The proc, for a passive with a `ProcTypeMask`.
    pub proc: Option<ProcId>,
    /// The marker buff, if the spell applies auras.
    pub buff: Option<BuffId>,
    /// Whether the spell joined the rank group of its name (spellbook abilities do; hidden
    /// payloads, passives and a second spell with the same name and rank do not).
    pub in_rank_group: bool,
    /// Whether the spell should be enabled right away: trainable and racial spells are; talent
    /// and rune granted ones (`SkillLineAbility.ClassMask` 0) wait for their talent. The C++
    /// constructor enabled them itself; here the context does, because enabling applies
    /// passive auras through the host.
    pub enable_now: bool,
}

/// What granted an equipment spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquipmentGrantor {
    /// An `effects` entry (`ItemEffect`) of the item worn in the slot.
    ItemEffect(u32),
    /// A proc of the permanent or temporary enchant on the item in the slot.
    Enchant(EnchantName),
    /// A bonus of the item set with this id (no slot).
    SetBonus(u32),
    /// A use effect of the consumable item with this id (no slot).
    Consumable(u32),
}

/// One spell granted by the equipment, as [`CharacterSpells`] keys it. Equipment spells are
/// not keyed by game id: the same enchant on both weapons is two procs (the slot decides which
/// attacks trigger each), and two rings with the same stat aura are two auras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EquipmentSpellKey {
    /// The slot of the item or enchant; `None` for a set bonus.
    pub slot: Option<EquipmentSlot>,
    pub grantor: EquipmentGrantor,
    /// Position in the grantor's list (`procs`, `effects`, the set's bonuses).
    pub index: usize,
}

/// Where a game id lives in a character's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellHandle {
    Spell(SpellId),
    Proc(ProcId),
}

/// One character's spells, buffs, procs and cooldowns. Port of `CharacterSpells` +
/// `EnabledBuffs`.
#[derive(Debug)]
pub struct CharacterSpells {
    character: CharId,
    spells: Vec<Option<Spell>>,
    /// Game id → handle for spells and procs.
    by_game_id: BTreeMap<u32, SpellHandle>,
    rank_groups: BTreeMap<String, SpellRankGroup>,
    /// Active `OVERRIDE_ACTIONBAR_SPELLS` substitutions: `(replaced, replacement)` game ids.
    actionbar_overrides: Vec<(u32, u32)>,
    cooldowns: CooldownRegistry,
    buffs: Vec<BuffSlot>,
    enabled_buffs: Vec<BuffId>,
    start_of_combat_buffs: Vec<BuffId>,
    procs: EnabledProcs,
    /// The spells and procs the equipment granted, kept across unequips so that re-equipping
    /// reuses them (and their statistics) instead of registering a second one.
    equipment_spells: BTreeMap<EquipmentSpellKey, SpellHandle>,
    start_of_combat_spells: Vec<SpellId>,
    /// The spells whose overrides attach event reactions (`on_event`).
    event_reactors: Vec<SpellId>,
    /// The proc of each cast buff that is a proc aura (a seal), firing while the buff is up
    /// ([`SpellSetup::has_buff_proc`]).
    buff_procs: BTreeMap<SpellId, ProcId>,
    next_instance_id: u32,
    next_proc_seed: u64,
    attack_mode: AttackMode,
    attack_mode_active: bool,
    mh_attack: AutoAttack,
    oh_attack: AutoAttack,
    /// The queued on-next-swing spell and its marker buff.
    queued_next_swing: Option<(SpellId, Option<BuffId>)>,
    cast_in_progress: bool,
    /// The cast in progress is a run to the target ([`SimFlag::RunToTarget`]).
    running_to_target: bool,
    cast_id: u32,
}

impl CharacterSpells {
    pub fn new(character: CharId, proc_seed: u64) -> Self {
        CharacterSpells {
            character,
            spells: Vec::new(),
            by_game_id: BTreeMap::new(),
            rank_groups: BTreeMap::new(),
            actionbar_overrides: Vec::new(),
            cooldowns: CooldownRegistry::new(),
            buffs: Vec::new(),
            enabled_buffs: Vec::new(),
            start_of_combat_buffs: Vec::new(),
            procs: EnabledProcs::new(),
            equipment_spells: BTreeMap::new(),
            start_of_combat_spells: Vec::new(),
            event_reactors: Vec::new(),
            next_instance_id: 0,
            next_proc_seed: proc_seed,
            buff_procs: BTreeMap::new(),
            attack_mode: AttackMode::MeleeAttack,
            attack_mode_active: false,
            mh_attack: AutoAttack::new(Hand::Mainhand),
            oh_attack: AutoAttack::new(Hand::Offhand),
            queued_next_swing: None,
            cast_in_progress: false,
            running_to_target: false,
            cast_id: 0,
        }
    }

    pub fn character(&self) -> CharId {
        self.character
    }

    /// A raid-unique instance id for this character's spells and buffs
    /// ([`InstanceId::for_character`] keeps them unique without a central allocator).
    pub fn next_instance_id(&mut self) -> InstanceId {
        let id = self.next_instance_id;
        self.next_instance_id += 1;
        InstanceId::for_character(self.character, id)
    }

    // --- Spells ---

    /// Adds spell `id` from `db`: the [`Spell`] (or [`Proc`]) with its cooldown controls, marker
    /// buff and rank group. Port of `CharacterSpells::add_spell_group` and the proc creation in
    /// the C++ constructors. Shared marker buffs are looked up in (or added to) the raid
    /// through `shared`.
    ///
    /// # Panics
    /// Panics if `db` has no spell `id`, or the spell was already added.
    pub fn add_spell(
        &mut self,
        db: &SpellDb,
        id: u32,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> AddedSpell {
        let setup = SpellSetup::from_db(db, id)
            .unwrap_or_else(|| panic!("spell {id} is not in the spell db"));
        self.add_spell_with(setup, db.overrides(), party, shared)
    }

    /// Adds a spell from its setup and the overrides its buff is built with (see
    /// [`CharacterSpells::add_spell`]).
    ///
    /// # Panics
    /// Panics if the spell was already added.
    pub fn add_spell_with(
        &mut self,
        setup: SpellSetup,
        overrides: &Overrides,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> AddedSpell {
        let record = std::sync::Arc::clone(&setup.record);
        let triggered = setup.triggered;
        assert!(
            !self.by_game_id.contains_key(&record.id),
            "{} ({}) has already been added",
            record.name,
            record.id
        );
        let buff_proc_setup = setup.has_buff_proc().then(|| SpellSetup {
            buff_proc: true,
            ..setup.clone()
        });
        let (spell, marker) = self.build_spell(setup, overrides, party, shared);
        let mut spell = spell;
        let enable_now = record.class_mask != 0 || record.race_mask != 0;

        // A proc on events the sim does not have (a killing blow) stays a plain passive.
        if spell.setup().is_proc() && !Proc::sources_of(&spell).is_empty() {
            let seed = self.next_proc_seed;
            self.next_proc_seed = self.next_proc_seed.wrapping_add(1);
            let proc = self.procs.add_proc(Proc::new(spell, seed));
            self.by_game_id.insert(record.id, SpellHandle::Proc(proc));
            return AddedSpell {
                spell: None,
                proc: Some(proc),
                buff: marker,
                in_rank_group: false,
                enable_now,
            };
        }

        let id = self.reserve_spell_id();
        spell.set_id(id);
        let ignored = spell.is_ignored();
        if spell.has_sim_flag(SimFlag::StartOfCombat) {
            self.start_of_combat_spells.push(id);
        }
        if !spell.event_scripts().is_empty() {
            self.event_reactors.push(id);
        }
        self.spells[id.index()] = Some(spell);
        self.by_game_id.insert(record.id, SpellHandle::Spell(id));
        if let Some(proc) = buff_proc_setup.and_then(|setup| self.add_buff_proc(setup, marker)) {
            self.buff_procs.insert(id, proc);
        }

        // Rank groups: what a rotation names. Only spellbook abilities join, not the payloads
        // another spell triggers; a second spell of the same name and rank stays out and is
        // reached by game id.
        let mut in_rank_group = false;
        if record.is_ability() && !ignored && !triggered {
            let rank = record.rank_number().unwrap_or(1);
            in_rank_group = match self.rank_groups.get_mut(&record.name) {
                Some(group) => group.add_rank(rank, id),
                None => {
                    self.rank_groups.insert(
                        record.name.clone(),
                        SpellRankGroup::new(&record.name, [(rank, id)]),
                    );
                    true
                }
            };
        }
        AddedSpell {
            spell: Some(id),
            proc: None,
            buff: marker,
            in_rank_group,
            enable_now,
        }
    }

    /// Adds a spell built elsewhere (auto attacks) under its own name.
    ///
    /// # Panics
    /// Panics if a group with the same name was already added.
    pub fn add_spell_direct(&mut self, mut spell: Spell) -> SpellId {
        let name = spell.name().to_string();
        assert!(
            !self.rank_groups.contains_key(&name),
            "{name} has already been added as a spell group"
        );
        let id = self.reserve_spell_id();
        spell.set_id(id);
        spell.set_instance_id(self.next_instance_id());
        if spell.has_sim_flag(SimFlag::StartOfCombat) {
            self.start_of_combat_spells.push(id);
        }
        if !spell.event_scripts().is_empty() {
            self.event_reactors.push(id);
        }
        let rank = spell.rank();
        if spell.game_id() != 0 {
            self.by_game_id
                .insert(spell.game_id(), SpellHandle::Spell(id));
        }
        self.spells[id.index()] = Some(spell);
        self.rank_groups
            .insert(name.clone(), SpellRankGroup::new(&name, [(rank, id)]));
        id
    }

    /// Registers the proc of a cast buff's proc aura: a spell of its own from the same record,
    /// with the proc's internal cooldown, that shares the buff `marker` the cast applies (the
    /// proc only fires while it is up). `None` for a proc on events the sim does not have
    /// (Cannibalize's).
    fn add_buff_proc(&mut self, setup: SpellSetup, marker: Option<BuffId>) -> Option<ProcId> {
        let cooldown = match Spell::own_cooldown_ms(&setup) {
            0 => None,
            ms => Some(
                self.cooldowns
                    .new_spell_cooldown(setup.record.id, f64::from(ms) / 1000.0),
            ),
        };
        let mut spell = Spell::new(setup, cooldown, None, marker);
        if Proc::sources_of(&spell).is_empty() {
            return None;
        }
        spell.set_instance_id(self.next_instance_id());
        let seed = self.next_proc_seed;
        self.next_proc_seed = self.next_proc_seed.wrapping_add(1);
        Some(self.procs.add_proc(Proc::new(spell, seed)))
    }

    /// The proc of spell `id`'s buff, when its buff is a proc aura (a seal's swings).
    pub fn buff_proc(&self, id: SpellId) -> Option<ProcId> {
        self.buff_procs.get(&id).copied()
    }

    fn reserve_spell_id(&mut self) -> SpellId {
        let id = SpellId(u32::try_from(self.spells.len()).expect("too many spells"));
        self.spells.push(None);
        id
    }

    /// The control shared by every spell of `category` (created on first use with the first
    /// spell's recovery time; each cast starts it for the casting spell's own length).
    fn category_cooldown(&mut self, category: u32, recovery_ms: u32) -> CooldownId {
        match self.cooldowns.id_by_name(&category_cooldown_name(category)) {
            Some(id) => id,
            None => self
                .cooldowns
                .new_category_cooldown(category, f64::from(recovery_ms) / 1000.0),
        }
    }

    /// Builds the [`Spell`] of `setup` with its cooldown controls and marker buff, without
    /// registering it anywhere.
    fn build_spell(
        &mut self,
        setup: SpellSetup,
        overrides: &Overrides,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> (Spell, Option<BuffId>) {
        let record = std::sync::Arc::clone(&setup.record);
        let cooldown = match Spell::own_cooldown_ms(&setup) {
            0 => None,
            ms => Some(
                self.cooldowns
                    .new_spell_cooldown(record.id, f64::from(ms) / 1000.0),
            ),
        };
        let category_cooldown = match record.cooldown.category_recovery_ms {
            0 => None,
            ms => Some(self.category_cooldown(record.categories.category, ms)),
        };
        let marker = self.create_marker_buff(&setup, overrides, party, shared);
        let mut spell = Spell::new(setup, cooldown, category_cooldown, marker);
        spell.set_instance_id(self.next_instance_id());
        (spell, marker)
    }

    // --- Equipment spells ---

    /// Registers the proc `key` of the equipment: a passive proc aura (an enchant's or item's
    /// proc, an on-equip or set bonus proc aura) built from `setup`, which only the sources
    /// `allowed` (what the slot lets an item proc react to) can trigger. A key already
    /// registered keeps its proc and a second call returns it unchanged. Port of the
    /// `EnchantProc` constructor and the proc creation in `Item::apply_proc`. Returns `None`
    /// when the record and the slot have no trigger in common.
    pub fn add_equipment_proc(
        &mut self,
        key: EquipmentSpellKey,
        setup: SpellSetup,
        overrides: &Overrides,
        allowed: &[ProcSource],
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> Option<ProcId> {
        self.add_keyed_proc(key, setup, overrides, party, shared, |spell, seed| {
            Proc::for_equipment(spell, allowed, seed)
        })
    }

    /// Registers the chance-on-hit spell `key` of an equipped weapon ([`Proc::on_hit`]): the
    /// wielding hand's landed attacks (`allowed`) cast it. Returns `None` when its rate is
    /// unknown.
    pub fn add_on_hit_proc(
        &mut self,
        key: EquipmentSpellKey,
        setup: SpellSetup,
        overrides: &Overrides,
        allowed: &[ProcSource],
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> Option<ProcId> {
        self.add_keyed_proc(key, setup, overrides, party, shared, |spell, seed| {
            Proc::on_hit(spell, allowed, seed)
        })
    }

    /// Registers the combat spell `key` of an item enchantment ([`Proc::combat_spell`]): the
    /// enchanted hand's landed attacks (`allowed`) cast the learned payload. Returns `None`
    /// when `setup` has no rate.
    pub fn add_combat_spell_proc(
        &mut self,
        key: EquipmentSpellKey,
        setup: SpellSetup,
        overrides: &Overrides,
        allowed: &[ProcSource],
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> Option<ProcId> {
        self.add_keyed_proc(key, setup, overrides, party, shared, |spell, seed| {
            Proc::combat_spell(spell, allowed, seed)
        })
    }

    fn add_keyed_proc(
        &mut self,
        key: EquipmentSpellKey,
        setup: SpellSetup,
        overrides: &Overrides,
        party: u8,
        shared: &mut impl SharedBuffs,
        build: impl FnOnce(Spell, u64) -> Option<Proc>,
    ) -> Option<ProcId> {
        match self.equipment_spells.get(&key) {
            Some(&SpellHandle::Proc(id)) => return Some(id),
            Some(SpellHandle::Spell(_)) => return None,
            None => {}
        }
        let (spell, _) = self.build_spell(setup, overrides, party, shared);
        let proc = build(spell, self.next_proc_seed)?;
        self.next_proc_seed = self.next_proc_seed.wrapping_add(1);
        let id = self.procs.add_proc(proc);
        self.equipment_spells.insert(key, SpellHandle::Proc(id));
        Some(id)
    }

    /// Registers the spell `key` of the equipment: a spell outside the game-id index and the
    /// rank groups. An on-equip or set bonus aura is up while it is enabled; an item's on-use
    /// spell (its `setup` carrying the item's cooldowns) is cast by name once
    /// [`CharacterSpells::name_equipment_use`] gave it a rank group. A key already registered
    /// keeps its spell.
    pub fn add_equipment_passive(
        &mut self,
        key: EquipmentSpellKey,
        setup: SpellSetup,
        overrides: &Overrides,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> SpellId {
        if let Some(&SpellHandle::Spell(id)) = self.equipment_spells.get(&key) {
            return id;
        }
        let (mut spell, _) = self.build_spell(setup, overrides, party, shared);
        let id = self.reserve_spell_id();
        spell.set_id(id);
        self.spells[id.index()] = Some(spell);
        self.equipment_spells.insert(key, SpellHandle::Spell(id));
        id
    }

    /// Makes the on-use spell `id` the one its name reaches: it gets a rank group of its own,
    /// or takes the place of another item's use spell of the same name that is no longer
    /// enabled (a trinket swapped for another with the same use). Returns `false` when the
    /// name belongs to another spell (a spellbook spell, a second item worn with the same use).
    pub fn name_equipment_use(&mut self, id: SpellId) -> bool {
        let name = self.spell(id).name().to_string();
        let Some(group) = self.rank_groups.get(&name) else {
            self.rank_groups
                .insert(name.clone(), SpellRankGroup::new(&name, [(1, id)]));
            return true;
        };
        if group.contains(id) {
            return true;
        }
        let members: Vec<SpellId> = group.spells().collect();
        let [member] = members[..] else {
            return false;
        };
        let replaceable = self
            .equipment_spells
            .values()
            .any(|&handle| handle == SpellHandle::Spell(member))
            && !self.spell(member).is_enabled();
        replaceable
            && self
                .rank_groups
                .get_mut(&name)
                .expect("looked up above")
                .replace(member, id)
    }

    /// The spell or proc registered for `key`, if any.
    pub fn equipment_spell(&self, key: EquipmentSpellKey) -> Option<SpellHandle> {
        self.equipment_spells.get(&key).copied()
    }

    /// Every registered equipment spell and proc with what granted it.
    pub fn equipment_spells(&self) -> impl Iterator<Item = (EquipmentSpellKey, SpellHandle)> + '_ {
        self.equipment_spells
            .iter()
            .map(|(&key, &handle)| (key, handle))
    }

    /// Creates and registers the marker buff of a spell, if it applies auras or replaces the
    /// next swing ([`Buff::next_swing_queue`]). Shared buffs are looked up in (or added to) the
    /// raid registry by canonical name. Port of the marker buff setup in the `Spell`
    /// constructor.
    fn create_marker_buff(
        &mut self,
        setup: &SpellSetup,
        overrides: &Overrides,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> Option<BuffId> {
        if !setup.has_buff() {
            // An on-next-swing spell's marker is up while it is queued (Heroic Strike).
            return setup.record.is_on_next_swing().then(|| {
                let buff = Buff::next_swing_queue(&setup.record);
                self.add_buff_slot(BuffSlot::Owned(Box::new(buff)))
            });
        }
        let record = setup.buff_record();
        let kind = BuffKind::from_record(record, party, overrides.debuff_shared(record.id))?;
        let buff = Buff::from_record(record, kind, overrides);
        let slot = match kind {
            BuffKind::PartyBuff { party } => {
                let handle = match shared.shared_party_buff(buff.canonical_name(), party) {
                    Some(handle) => handle,
                    None => {
                        let mut buff = buff;
                        buff.enable();
                        shared.register_shared_party_buff(buff, party)
                    }
                };
                BuffSlot::Shared(handle)
            }
            BuffKind::SharedDebuff => {
                let handle = match shared.shared_raid_buff(buff.canonical_name()) {
                    Some(handle) => handle,
                    None => {
                        let mut buff = buff;
                        buff.enable();
                        shared.register_shared_raid_buff(buff)
                    }
                };
                BuffSlot::Shared(handle)
            }
            _ => BuffSlot::Owned(Box::new(buff)),
        };
        Some(self.add_buff_slot(slot))
    }

    /// Where game id `spell` lives, if the character has it.
    pub fn handle(&self, spell: u32) -> Option<SpellHandle> {
        self.by_game_id.get(&spell).copied()
    }

    /// The handle of the (non-proc) spell with game id `spell`.
    pub fn spell_by_game_id(&self, spell: u32) -> Option<SpellId> {
        match self.by_game_id.get(&spell)? {
            SpellHandle::Spell(id) => Some(*id),
            SpellHandle::Proc(_) => None,
        }
    }

    /// The handle of the proc with game id `spell`.
    pub fn proc_by_game_id(&self, spell: u32) -> Option<ProcId> {
        match self.by_game_id.get(&spell)? {
            SpellHandle::Proc(id) => Some(*id),
            SpellHandle::Spell(_) => None,
        }
    }

    /// Whether the character has the spell with game id `spell` (as a spell or a proc).
    pub fn has_game_id(&self, spell: u32) -> bool {
        self.by_game_id.contains_key(&spell)
    }

    pub fn spell(&self, id: SpellId) -> &Spell {
        self.spells[id.index()]
            .as_ref()
            .unwrap_or_else(|| panic!("spell {id:?} is taken out or is a proc"))
    }

    pub fn spell_mut(&mut self, id: SpellId) -> &mut Spell {
        self.spells[id.index()]
            .as_mut()
            .unwrap_or_else(|| panic!("spell {id:?} is taken out or is a proc"))
    }

    /// Takes a spell out so it can be performed against a context that borrows this registry.
    ///
    /// # Panics
    /// Panics if the spell is already taken out (re-entrant casts of the same spell).
    pub fn take_spell(&mut self, id: SpellId) -> Spell {
        self.spells[id.index()]
            .take()
            .unwrap_or_else(|| panic!("spell {id:?} is already taken out or is a proc"))
    }

    /// Returns a spell taken with [`CharacterSpells::take_spell`].
    pub fn put_spell(&mut self, id: SpellId, spell: Spell) {
        assert!(
            self.spells[id.index()].is_none(),
            "spell {id:?} was not taken out"
        );
        self.spells[id.index()] = Some(spell);
    }

    /// Every spell handle (procs' spells excluded), in registration order.
    pub fn spell_ids(&self) -> impl Iterator<Item = SpellId> + '_ {
        self.spells
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .map(|(index, _)| SpellId(index as u32))
    }

    pub fn rank_group(&self, name: &str) -> Option<&SpellRankGroup> {
        self.rank_groups.get(name)
    }

    pub fn rank_groups(&self) -> impl Iterator<Item = &SpellRankGroup> {
        self.rank_groups.values()
    }

    /// The rank group that contains `spell`, if any.
    pub fn rank_group_of(&self, spell: SpellId) -> Option<&SpellRankGroup> {
        self.rank_groups
            .values()
            .find(|group| group.contains(spell))
    }

    /// Applies or removes an `OVERRIDE_ACTIONBAR_SPELLS` substitution: while applied, the rank
    /// group holding `replaced` lists `replacement` in its place (Improved Slam's ranks replace
    /// the trainable Slam ranks). Both are game ids; the character must have both as spells.
    /// Returns whether the group changed.
    pub fn apply_actionbar_override(
        &mut self,
        replaced: u32,
        replacement: u32,
        apply: bool,
    ) -> bool {
        let (Some(old), Some(new)) = (
            self.spell_by_game_id(replaced),
            self.spell_by_game_id(replacement),
        ) else {
            return false;
        };
        let (from, to) = if apply { (old, new) } else { (new, old) };
        let already = self.actionbar_overrides.contains(&(replaced, replacement));
        if apply == already {
            return false;
        }
        let Some(group) = self.rank_groups.values_mut().find(|g| g.contains(from)) else {
            return false;
        };
        group.replace(from, to);
        if apply {
            self.actionbar_overrides.push((replaced, replacement));
        } else {
            self.actionbar_overrides
                .retain(|pair| *pair != (replaced, replacement));
        }
        true
    }

    /// The active `OVERRIDE_ACTIONBAR_SPELLS` substitutions, `(replaced, replacement)`.
    pub fn actionbar_overrides(&self) -> &[(u32, u32)] {
        &self.actionbar_overrides
    }

    /// Spells performed automatically when combat starts (`START_OF_COMBAT` flag). Port of
    /// `start_of_combat_spells`.
    /// The spells (enabled or not) whose overrides attach event reactions, in registration
    /// order.
    pub fn event_reactors(&self) -> &[SpellId] {
        &self.event_reactors
    }

    pub fn start_of_combat_spells(&self) -> &[SpellId] {
        &self.start_of_combat_spells
    }

    pub fn add_start_of_combat_spell(&mut self, id: SpellId) {
        if !self.start_of_combat_spells.contains(&id) {
            self.start_of_combat_spells.push(id);
        }
    }

    pub fn remove_start_of_combat_spell(&mut self, id: SpellId) {
        self.start_of_combat_spells.retain(|spell| *spell != id);
    }

    // --- Cooldowns ---

    pub fn cooldowns(&self) -> &CooldownRegistry {
        &self.cooldowns
    }

    pub fn cooldowns_mut(&mut self) -> &mut CooldownRegistry {
        &mut self.cooldowns
    }

    // --- Procs ---

    pub fn procs(&self) -> &EnabledProcs {
        &self.procs
    }

    pub fn procs_mut(&mut self) -> &mut EnabledProcs {
        &mut self.procs
    }

    /// Takes the procs out to run a proc check against a context borrowing this registry.
    pub fn take_procs(&mut self) -> EnabledProcs {
        std::mem::take(&mut self.procs)
    }

    pub fn put_procs(&mut self, procs: EnabledProcs) {
        self.procs = procs;
    }

    // --- Buffs (port of EnabledBuffs) ---

    /// Adds a character-owned buff (not yet enabled) and returns its handle.
    pub fn add_buff(&mut self, buff: Buff) -> BuffId {
        self.add_buff_slot(BuffSlot::Owned(Box::new(buff)))
    }

    /// Adds a handle to a buff owned by the raid.
    pub fn add_shared_buff(&mut self, shared: SharedBuffId) -> BuffId {
        self.add_buff_slot(BuffSlot::Shared(shared))
    }

    /// Adds an external buff ([`BuffKind::External`]): enabled with an instance id, but kept
    /// out of the enabled buffs as the C++ `ExternalBuff` was, so it is never found by name
    /// (an external Sunder Armor must not shadow the shared debuff) and never consumes charges.
    ///
    /// # Panics
    /// Panics if the buff is not an external buff.
    pub fn add_external_buff(&mut self, mut buff: Buff) -> BuffId {
        assert!(
            buff.kind() == BuffKind::External,
            "{} is not an external buff",
            buff.name()
        );
        let instance_id = self.next_instance_id();
        buff.set_instance_id(instance_id);
        buff.enable();
        self.add_buff_slot(BuffSlot::Owned(Box::new(buff)))
    }

    fn add_buff_slot(&mut self, slot: BuffSlot) -> BuffId {
        let id = BuffId(u32::try_from(self.buffs.len()).expect("too many buffs"));
        self.buffs.push(slot);
        id
    }

    pub fn buff_slot(&self, id: BuffId) -> &BuffSlot {
        &self.buffs[id.index()]
    }

    pub fn buff_slot_mut(&mut self, id: BuffId) -> &mut BuffSlot {
        &mut self.buffs[id.index()]
    }

    /// The character-owned buff, or `None` for a shared slot.
    pub fn owned_buff(&self, id: BuffId) -> Option<&Buff> {
        match &self.buffs[id.index()] {
            BuffSlot::Owned(buff) => Some(buff),
            BuffSlot::Shared(_) => None,
        }
    }

    pub fn owned_buff_mut(&mut self, id: BuffId) -> Option<&mut Buff> {
        match &mut self.buffs[id.index()] {
            BuffSlot::Owned(buff) => Some(buff),
            BuffSlot::Shared(_) => None,
        }
    }

    pub fn buff_ids(&self) -> impl Iterator<Item = BuffId> + use<> {
        (0..self.buffs.len()).map(|index| BuffId(index as u32))
    }

    /// Enables a character-owned buff and adds it to the enabled buffs, assigning its instance
    /// id. Port of `Buff::enable_buff` + `EnabledBuffs::add_buff`.
    ///
    /// # Panics
    /// Panics for shared slots (the raid enables those) or an already enabled buff.
    pub fn enable_buff(&mut self, id: BuffId) {
        let instance_id = self.next_instance_id();
        let buff = self
            .owned_buff_mut(id)
            .unwrap_or_else(|| panic!("buff {id:?} is shared and enabled by the raid"));
        buff.enable();
        if buff.instance_id().is_none() {
            buff.set_instance_id(instance_id);
        }
        if !self.enabled_buffs.contains(&id) {
            self.enabled_buffs.push(id);
        }
    }

    /// Disables a character-owned buff and removes it from the enabled buffs. Port of
    /// `Buff::disable_buff` + `EnabledBuffs::remove_buff`.
    pub fn disable_buff(&mut self, id: BuffId) {
        if let Some(buff) = self.owned_buff_mut(id) {
            buff.disable();
        }
        self.enabled_buffs.retain(|buff| *buff != id);
        self.start_of_combat_buffs.retain(|buff| *buff != id);
    }

    pub fn is_buff_enabled(&self, id: BuffId) -> bool {
        self.enabled_buffs.contains(&id)
    }

    pub fn enabled_buffs(&self) -> &[BuffId] {
        &self.enabled_buffs
    }

    /// Buffs applied when combat starts (external buffs). Port of `start_of_combat_buffs`.
    pub fn start_of_combat_buffs(&self) -> &[BuffId] {
        &self.start_of_combat_buffs
    }

    /// # Panics
    /// Panics if the buff is not enabled.
    pub fn add_start_of_combat_buff(&mut self, id: BuffId) {
        assert!(
            self.is_buff_enabled(id),
            "Expected pre-combat buff {id:?} to be enabled"
        );
        if !self.start_of_combat_buffs.contains(&id) {
            self.start_of_combat_buffs.push(id);
        }
    }

    pub fn remove_start_of_combat_buff(&mut self, id: BuffId) {
        self.start_of_combat_buffs.retain(|buff| *buff != id);
    }

    /// The enabled, active character-owned buffs that lose a charge on `source` (Flurry on a
    /// landed swing: `SpellAuraOptions.ProcCharges` with the buff's own `ProcTypeMask`). The
    /// context uses a charge on each after the event's proc check.
    pub fn charge_consumers(&self, source: ProcSource) -> Vec<BuffId> {
        self.charge_consumers_for(source, None)
    }

    /// [`Self::charge_consumers`] for an event of a spell with `class` options (`None` for a
    /// swing): a charged spell modifier aura (Eureka!) only reacts to the spells it modifies.
    pub fn charge_consumers_for(
        &self,
        source: ProcSource,
        class: Option<&ClassOptions>,
    ) -> Vec<BuffId> {
        self.enabled_buffs
            .iter()
            .copied()
            .filter(|id| {
                self.owned_buff(*id)
                    .is_some_and(|buff| buff.is_active() && buff.consumes_charge_for(source, class))
            })
            .collect()
    }

    /// The enabled character-owned buff called `name`, one a passive does not apply first: the
    /// Flurry talent's permanent aura and the haste buff it procs are both called "Flurry", and
    /// a rotation means the haste. Port of `EnabledBuffs::get_buff_by_name`.
    pub fn owned_buff_by_name(&self, name: &str) -> Option<BuffId> {
        let named = || {
            self.enabled_buffs
                .iter()
                .copied()
                .filter(|id| self.owned_buff(*id).is_some_and(|buff| buff.name() == name))
        };
        named()
            .find(|id| self.owned_buff(*id).is_some_and(|buff| !buff.is_passive()))
            .or_else(|| named().next())
    }

    /// Resolves a buff name the way rotations and conditions refer to buffs: the enabled owned
    /// buff of the spell's highest learned rank (every Rend rank owns a debuff called "Rend";
    /// the rotation casts the highest), another enabled owned buff of that name, then a shared
    /// party buff, then a shared raid buff, then the shared buffs under the canonical
    /// `"Name (spell id)"` of the spell's highest learned rank. Port of
    /// `CharacterSpells::get_buff_by_name`.
    pub fn buff_by_name(
        &self,
        name: &str,
        party: u8,
        shared: &impl SharedBuffs,
        is_rank_learned: impl Fn(SpellId) -> bool,
    ) -> Option<BuffId> {
        let highest = self
            .rank_groups
            .get(name)
            .and_then(|group| group.get_spell_rank(MAX_RANK, &is_rank_learned))
            .and_then(|spell| self.spells[spell.index()].as_ref()?.marker_buff())
            .filter(|&id| {
                self.enabled_buffs.contains(&id)
                    && self.owned_buff(id).is_some_and(|buff| buff.name() == name)
            });
        if let Some(id) = highest.or_else(|| self.owned_buff_by_name(name)) {
            return Some(id);
        }
        let lookup = |canonical: &str| {
            shared
                .shared_party_buff(canonical, party)
                .or_else(|| shared.shared_raid_buff(canonical))
                .and_then(|handle| self.buff_id_of_shared(handle))
        };
        if let Some(id) = lookup(name) {
            return Some(id);
        }
        let group = self.rank_groups.get(name)?;
        let spell = group.get_spell_rank(MAX_RANK, is_rank_learned)?;
        let game_id = self.spells[spell.index()].as_ref()?.game_id();
        lookup(&Buff::canonical_name_for(name, game_id))
    }

    /// The character's handle for a raid-owned buff, if it refers to it.
    pub fn buff_id_of_shared(&self, handle: SharedBuffId) -> Option<BuffId> {
        self.buffs
            .iter()
            .position(|slot| matches!(slot, BuffSlot::Shared(h) if *h == handle))
            .map(|index| BuffId(index as u32))
    }

    // --- Casting ---

    pub fn cast_in_progress(&self) -> bool {
        self.cast_in_progress
    }

    /// Whether the cast in progress is a run to the target ([`SimFlag::RunToTarget`]).
    pub fn running_to_target(&self) -> bool {
        self.cast_in_progress && self.running_to_target
    }

    /// Whether a cast that holds back every other action is in progress: any but a run to the
    /// target.
    pub fn blocking_cast_in_progress(&self) -> bool {
        self.cast_in_progress && !self.running_to_target
    }

    /// Port of `CharacterSpells::start_cast`; `running_to_target` for a
    /// [`SimFlag::RunToTarget`] spell.
    ///
    /// # Panics
    /// Panics if a cast is already in progress.
    pub fn start_cast(&mut self, running_to_target: bool) -> u32 {
        assert!(
            !self.cast_in_progress,
            "Cast in progress when starting new cast"
        );
        self.cast_in_progress = true;
        self.running_to_target = running_to_target;
        self.cast_id += 1;
        self.cast_id
    }

    /// Port of `CharacterSpells::complete_cast` (the caller schedules the player's reaction).
    ///
    /// # Panics
    /// Panics if no cast is in progress or the id does not match.
    pub fn complete_cast(&mut self, cast_id: u32) {
        assert!(
            self.cast_in_progress,
            "Cast not in progress when completing cast"
        );
        assert_eq!(self.cast_id, cast_id, "Mismatched cast id");
        self.cast_in_progress = false;
        self.running_to_target = false;
    }

    // --- Attack mode ---

    pub fn attack_mode(&self) -> AttackMode {
        self.attack_mode
    }

    /// # Panics
    /// Panics while attacking.
    pub fn set_attack_mode(&mut self, attack_mode: AttackMode) {
        assert!(
            !self.attack_mode_active,
            "Attack mode active when setting new attack mode"
        );
        self.attack_mode = attack_mode;
    }

    pub fn is_attacking(&self) -> bool {
        self.attack_mode_active
    }

    pub fn is_melee_attacking(&self) -> bool {
        self.attack_mode == AttackMode::MeleeAttack && self.attack_mode_active
    }

    pub fn is_ranged_attacking(&self) -> bool {
        self.attack_mode == AttackMode::RangedAttack && self.attack_mode_active
    }

    /// Marks the character as attacking; the auto attacks (3.12) schedule the swings.
    pub fn start_attack(&mut self) {
        self.attack_mode_active = true;
    }

    pub fn stop_attack(&mut self) {
        self.attack_mode_active = false;
    }

    // --- Auto attacks ---

    pub fn mh_attack(&self) -> &AutoAttack {
        &self.mh_attack
    }

    pub fn mh_attack_mut(&mut self) -> &mut AutoAttack {
        &mut self.mh_attack
    }

    pub fn oh_attack(&self) -> &AutoAttack {
        &self.oh_attack
    }

    pub fn oh_attack_mut(&mut self) -> &mut AutoAttack {
        &mut self.oh_attack
    }

    pub fn auto_attack(&self, hand: Hand) -> &AutoAttack {
        match hand {
            Hand::Mainhand => &self.mh_attack,
            Hand::Offhand => &self.oh_attack,
        }
    }

    pub fn auto_attack_mut(&mut self, hand: Hand) -> &mut AutoAttack {
        match hand {
            Hand::Mainhand => &mut self.mh_attack,
            Hand::Offhand => &mut self.oh_attack,
        }
    }

    /// Takes an auto attack out to swing it against a context borrowing this registry.
    pub fn take_auto_attack(&mut self, hand: Hand) -> AutoAttack {
        std::mem::replace(self.auto_attack_mut(hand), AutoAttack::new(hand))
    }

    pub fn put_auto_attack(&mut self, attack: AutoAttack) {
        let hand = attack.hand();
        *self.auto_attack_mut(hand) = attack;
    }

    /// The spell queued to replace the next mainhand swing (Heroic Strike). Port of
    /// `WarriorSpells::is_heroic_strike_queued` generalised.
    pub fn queued_next_swing(&self) -> Option<SpellId> {
        self.queued_next_swing.map(|(spell, _)| spell)
    }

    /// The marker buff of the queued on-next-swing spell, up while it is queued.
    pub fn queued_next_swing_marker(&self) -> Option<BuffId> {
        self.queued_next_swing.and_then(|(_, marker)| marker)
    }

    pub fn queue_next_swing(&mut self, spell: SpellId, marker: Option<BuffId>) {
        self.queued_next_swing = Some((spell, marker));
    }

    pub fn cancel_next_swing(&mut self) -> Option<SpellId> {
        self.queued_next_swing.take().map(|(spell, _)| spell)
    }

    // --- Iteration lifecycle ---

    /// Clears the per-iteration state that does not need the world: cast bookkeeping, attack
    /// state and cooldowns. Spells, buffs and procs are reset by the context (they need the
    /// engine and target). Port of the state part of `CharacterSpells::reset`.
    pub fn reset_state(&mut self) {
        self.cast_in_progress = false;
        self.running_to_target = false;
        self.cast_id = 0;
        self.attack_mode_active = false;
        self.queued_next_swing = None;
        self.cooldowns.reset_all();
        self.mh_attack.reset();
        self.oh_attack.reset();
    }

    /// Port of the state part of `CharacterSpells::prepare_set_of_combat_iterations`.
    pub fn prepare_set_of_combat_iterations(&mut self) {
        self.reset_state();
        self.procs.prepare_set_of_combat_iterations();
        self.mh_attack.prepare_set_of_combat_iterations();
        self.oh_attack.prepare_set_of_combat_iterations();
    }
}

#[cfg(test)]
mod tests;
