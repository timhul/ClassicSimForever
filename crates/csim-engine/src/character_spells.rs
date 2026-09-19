//! The per-character spell registry. Port of `Character/CharacterSpells.*` and
//! `Character/EnabledBuffs.*` (the registry parts; the class-specific spells, consumables and
//! racials are added by the character in Phase 4, the auto attacks in 3.12).
//!
//! `CharacterSpells` owns the character's spells, rank groups, cooldown controls, buffs and
//! procs, addressed by the typed handles. It is a registry, not an actor: operations that need
//! the world (`Spell::perform`, `Spell::enable`, proc checks) are run by the character's spell
//! context, which implements [`crate::spell::SpellHost`] and borrows this registry. To let a
//! spell mutate the world while it is being performed, the context takes it out with
//! [`CharacterSpells::take_spell`] and puts it back with [`CharacterSpells::put_spell`].
//!
//! Buffs shared with the raid (party buffs, shared debuffs) are owned by the raid control; the
//! character keeps a [`BuffSlot::Shared`] handle to them so every buff a character refers to
//! has a [`BuffId`] in its own list.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::attack_mode::AttackMode;
use crate::buff::{Buff, BuffKind};
use crate::cooldown::CooldownRegistry;
use crate::ids::{BuffId, CharId, InstanceId, ProcId, SharedBuffId, SpellId};
use crate::proc::{EnabledProcs, Proc};
use crate::spell::{AutoAttack, Hand, Spell, SpellFlag, SpellGroupSpec, SpellRankGroup, MAX_RANK};

/// Where a buff in a character's buff list lives.
#[derive(Debug, Clone)]
pub enum BuffSlot {
    /// Owned by this character.
    Owned(Buff),
    /// Owned by the raid control (party buff or shared debuff).
    Shared(SharedBuffId),
}

/// The raid's registry of buffs shared between characters. Port of the shared buff part of
/// `RaidControl`; implemented by the raid control in Phase 4.
pub trait SharedBuffs {
    /// The registered party buff with `canonical_name` in `party`, if any.
    fn shared_party_buff(&self, canonical_name: &str, party: u8) -> Option<SharedBuffId>;
    /// Registers a new (enabled) party buff and returns its handle.
    fn register_shared_party_buff(&mut self, buff: Buff, party: u8) -> SharedBuffId;
    fn shared_raid_buff(&self, canonical_name: &str) -> Option<SharedBuffId>;
    fn register_shared_raid_buff(&mut self, buff: Buff) -> SharedBuffId;
}

/// What [`CharacterSpells::add_spell_group`] created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedSpellGroup {
    /// The spells of the group, lowest rank first.
    pub spells: Vec<SpellId>,
    /// The proc, for passive groups (its spell is not in `spells`).
    pub proc: Option<ProcId>,
    /// Spells and procs that should be enabled right away because the group does not require
    /// a talent (the C++ constructor enabled them itself; here the context does, because
    /// enabling registers marker buffs through the host).
    pub enable_now: bool,
}

/// One character's spells, buffs, procs and cooldowns. Port of `CharacterSpells` +
/// `EnabledBuffs`.
#[derive(Debug)]
pub struct CharacterSpells {
    character: CharId,
    spells: Vec<Option<Spell>>,
    rank_groups: BTreeMap<String, SpellRankGroup>,
    cooldowns: CooldownRegistry,
    buffs: Vec<BuffSlot>,
    enabled_buffs: Vec<BuffId>,
    start_of_combat_buffs: Vec<BuffId>,
    procs: EnabledProcs,
    start_of_combat_spells: Vec<SpellId>,
    next_instance_id: u32,
    next_proc_seed: u64,
    attack_mode: AttackMode,
    attack_mode_active: bool,
    mh_attack: AutoAttack,
    oh_attack: AutoAttack,
    queued_next_swing: Option<SpellId>,
    cast_in_progress: bool,
    cast_id: u32,
}

impl CharacterSpells {
    /// Instance ids of a character's buffs and spells are `character << 24 | counter`, keeping
    /// them unique across the raid without a central allocator.
    const INSTANCE_ID_SHIFT: u32 = 24;

    pub fn new(character: CharId, proc_seed: u64) -> Self {
        CharacterSpells {
            character,
            spells: Vec::new(),
            rank_groups: BTreeMap::new(),
            cooldowns: CooldownRegistry::new(),
            buffs: Vec::new(),
            enabled_buffs: Vec::new(),
            start_of_combat_buffs: Vec::new(),
            procs: EnabledProcs::new(),
            start_of_combat_spells: Vec::new(),
            next_instance_id: 0,
            next_proc_seed: proc_seed,
            attack_mode: AttackMode::MeleeAttack,
            attack_mode_active: false,
            mh_attack: AutoAttack::new(Hand::Mainhand),
            oh_attack: AutoAttack::new(Hand::Offhand),
            queued_next_swing: None,
            cast_in_progress: false,
            cast_id: 0,
        }
    }

    pub fn character(&self) -> CharId {
        self.character
    }

    /// A raid-unique instance id for this character's spells and buffs.
    pub fn next_instance_id(&mut self) -> InstanceId {
        let id = self.next_instance_id;
        self.next_instance_id += 1;
        InstanceId((u32::from(self.character.0) << Self::INSTANCE_ID_SHIFT) | id)
    }

    // --- Spells ---

    /// Adds every rank of a spell group (or, for a passive group, its proc). Port of
    /// `CharacterSpells::add_spell_group(SpellRankGroupSpec)` and the proc creation in the
    /// constructor. Marker buffs are created and registered here; shared ones through `shared`.
    ///
    /// # Panics
    /// Panics if a group with the same name was already added.
    pub fn add_spell_group(
        &mut self,
        group: &Arc<SpellGroupSpec>,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> AddedSpellGroup {
        assert!(
            !self.rank_groups.contains_key(&group.name),
            "{} has already been added as a spell group",
            group.name
        );
        let cooldown = Some(self.cooldowns.new_cooldown_for_group(group));
        let enable_now = group.requires_talent.is_none();

        if group.has_flag(SpellFlag::Passive) {
            let rank_spec = &group.ranks[0];
            let marker = self.create_marker_buff(group, rank_spec.rank, party, shared);
            let mut spell = Spell::new(Arc::clone(group), rank_spec.rank, cooldown, marker);
            let id = self.reserve_spell_id();
            spell.set_id(id);
            spell.set_instance_id(self.next_instance_id());
            let seed = self.next_proc_seed;
            self.next_proc_seed = self.next_proc_seed.wrapping_add(1);
            let proc = self.procs.add_proc(Proc::new(spell, seed));
            self.rank_groups.insert(
                group.name.clone(),
                SpellRankGroup::new(&group.name, [(rank_spec.rank, id)]),
            );
            return AddedSpellGroup {
                spells: Vec::new(),
                proc: Some(proc),
                enable_now,
            };
        }

        let mut ranks = Vec::with_capacity(group.ranks.len());
        for rank_spec in &group.ranks {
            let marker = self.create_marker_buff(group, rank_spec.rank, party, shared);
            let mut spell = Spell::new(Arc::clone(group), rank_spec.rank, cooldown, marker);
            let id = self.reserve_spell_id();
            spell.set_id(id);
            spell.set_instance_id(self.next_instance_id());
            if spell.has_flag(SpellFlag::StartOfCombat) {
                self.start_of_combat_spells.push(id);
            }
            self.spells[id.index()] = Some(spell);
            ranks.push((rank_spec.rank, id));
        }
        let spells = ranks.iter().map(|(_, id)| *id).collect();
        self.rank_groups
            .insert(group.name.clone(), SpellRankGroup::new(&group.name, ranks));
        AddedSpellGroup {
            spells,
            proc: None,
            enable_now,
        }
    }

    /// Adds a spell built elsewhere (auto attacks, racials) under its own name.
    ///
    /// # Panics
    /// Panics if a group with the same name was already added.
    pub fn add_spell(&mut self, mut spell: Spell) -> SpellId {
        let name = spell.name().to_string();
        assert!(
            !self.rank_groups.contains_key(&name),
            "{name} has already been added as a spell group"
        );
        let id = self.reserve_spell_id();
        spell.set_id(id);
        spell.set_instance_id(self.next_instance_id());
        if spell.has_flag(SpellFlag::StartOfCombat) {
            self.start_of_combat_spells.push(id);
        }
        let rank = spell.rank();
        self.spells[id.index()] = Some(spell);
        self.rank_groups
            .insert(name.clone(), SpellRankGroup::new(&name, [(rank, id)]));
        id
    }

    fn reserve_spell_id(&mut self) -> SpellId {
        let id = SpellId(u32::try_from(self.spells.len()).expect("too many spells"));
        self.spells.push(None);
        id
    }

    /// Creates and registers the marker buff of a rank, if the rank has one. Shared buffs are
    /// looked up in (or added to) the raid registry by canonical name. Port of the marker buff
    /// setup in the `Spell` constructor.
    fn create_marker_buff(
        &mut self,
        group: &Arc<SpellGroupSpec>,
        rank: u32,
        party: u8,
        shared: &mut impl SharedBuffs,
    ) -> Option<BuffId> {
        let rank_spec = group.rank(rank)?;
        let buff_spec = rank_spec.buff.as_ref()?;
        let kind = BuffKind::from_spec(buff_spec, party)?;
        let buff = Buff::from_spec(group, rank_spec, kind);
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
            _ => BuffSlot::Owned(buff),
        };
        Some(self.add_buff_slot(slot))
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

    /// Spells performed automatically when combat starts (`START_OF_COMBAT` flag). Port of
    /// `start_of_combat_spells`.
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
        self.add_buff_slot(BuffSlot::Owned(buff))
    }

    /// Adds a handle to a buff owned by the raid.
    pub fn add_shared_buff(&mut self, shared: SharedBuffId) -> BuffId {
        self.add_buff_slot(BuffSlot::Shared(shared))
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

    pub fn buff_ids(&self) -> impl Iterator<Item = BuffId> {
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

    /// The enabled character-owned buff called `name`. Port of `EnabledBuffs::get_buff_by_name`.
    pub fn owned_buff_by_name(&self, name: &str) -> Option<BuffId> {
        self.enabled_buffs
            .iter()
            .copied()
            .find(|id| self.owned_buff(*id).is_some_and(|buff| buff.name() == name))
    }

    /// Resolves a buff name the way rotations and conditions refer to buffs: an enabled owned
    /// buff, then a shared party buff, then a shared raid buff, then the shared buffs under the
    /// canonical `"Name (rank N)"` of the spell's highest learned rank. Port of
    /// `CharacterSpells::get_buff_by_name`.
    pub fn buff_by_name(
        &self,
        name: &str,
        party: u8,
        shared: &impl SharedBuffs,
        is_rank_learned: impl Fn(SpellId) -> bool,
    ) -> Option<BuffId> {
        if let Some(id) = self.owned_buff_by_name(name) {
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
        let rank = match self.spells[spell.index()].as_ref() {
            Some(spell) => spell.rank(),
            None => {
                let proc = self.procs.find_by_name(name)?;
                self.procs.get(proc).spell().rank()
            }
        };
        lookup(&Buff::canonical_name_for(name, rank))
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

    /// Port of `CharacterSpells::start_cast`.
    ///
    /// # Panics
    /// Panics if a cast is already in progress.
    pub fn start_cast(&mut self) -> u32 {
        assert!(
            !self.cast_in_progress,
            "Cast in progress when starting new cast"
        );
        self.cast_in_progress = true;
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
        self.queued_next_swing
    }

    pub fn queue_next_swing(&mut self, spell: SpellId) {
        self.queued_next_swing = Some(spell);
    }

    pub fn cancel_next_swing(&mut self) -> Option<SpellId> {
        self.queued_next_swing.take()
    }

    // --- Iteration lifecycle ---

    /// Clears the per-iteration state that does not need the world: cast bookkeeping, attack
    /// state and cooldowns. Spells, buffs and procs are reset by the context (they need the
    /// engine and target). Port of the state part of `CharacterSpells::reset`.
    pub fn reset_state(&mut self) {
        self.cast_in_progress = false;
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
mod tests {
    use super::*;
    use crate::spell::{SpellDb, SpellFileSpec};

    #[derive(Default)]
    struct Raid {
        buffs: Vec<Buff>,
        party: Vec<(String, u8, SharedBuffId)>,
        raid: Vec<(String, SharedBuffId)>,
    }

    impl SharedBuffs for Raid {
        fn shared_party_buff(&self, canonical_name: &str, party: u8) -> Option<SharedBuffId> {
            self.party
                .iter()
                .find(|(name, p, _)| name == canonical_name && *p == party)
                .map(|(_, _, id)| *id)
        }
        fn register_shared_party_buff(&mut self, buff: Buff, party: u8) -> SharedBuffId {
            assert!(buff.is_enabled());
            let id = SharedBuffId(self.buffs.len() as u32);
            self.party
                .push((buff.canonical_name().to_string(), party, id));
            self.buffs.push(buff);
            id
        }
        fn shared_raid_buff(&self, canonical_name: &str) -> Option<SharedBuffId> {
            self.raid
                .iter()
                .find(|(name, _)| name == canonical_name)
                .map(|(_, id)| *id)
        }
        fn register_shared_raid_buff(&mut self, buff: Buff) -> SharedBuffId {
            assert!(buff.is_enabled());
            let id = SharedBuffId(self.buffs.len() as u32);
            self.raid.push((buff.canonical_name().to_string(), id));
            self.buffs.push(buff);
            id
        }
    }

    fn db() -> SpellDb {
        let file: SpellFileSpec = serde_yaml::from_str(
            r#"
spell_groups:
  - name: Battle Shout
    causes_gcd: normal
    restricted_by_gcd: true
    ranks:
      - rank: 6
        resource: rage
        cost: 10
        buff: { unit: party, duration: 120, effects: [{ name: APPLY_AURA_MELEE_ATTACK_POWER, value: 193 }] }
      - rank: 7
        resource: rage
        cost: 10
        requires_phase: 5
        buff: { unit: party, duration: 120, effects: [{ name: APPLY_AURA_MELEE_ATTACK_POWER, value: 232 }] }
  - name: Sunder Armor
    causes_gcd: normal
    restricted_by_gcd: true
    ranks:
      - rank: 5
        resource: rage
        cost: 15
        effects: [{ name: SCHOOL_DAMAGE_PHYSICAL, value: 0 }]
        buff: { unit: target, shared: true, priority: high, duration: 30, max_stacks: 5, effects: [{ name: APPLY_AURA_MOD_ARMOR, value: -450 }] }
  - name: Death Wish
    causes_gcd: normal
    restricted_by_gcd: true
    cooldown: 180
    requires_talent: Death Wish
    ranks:
      - rank: 1
        resource: rage
        cost: 10
        buff: { unit: self, duration: 30, effects: [{ name: APPLY_AURA_MOD_DAMAGE_DONE_PHYSICAL, value: 20 }] }
  - name: Anger Management
    causes_gcd: none
    requires_talent: Anger Management
    flags: [START_OF_COMBAT]
    ranks:
      - rank: 1
        resource: rage
        buff: { unit: self, effects: [{ name: APPLY_AURA_PERIODIC_RESOURCE_GAIN_RAGE, value: 1, tick_rate: 3 }] }
  - name: Flurry
    causes_gcd: none
    requires_talent: Flurry
    flags: [PASSIVE_SPELL, ON_MELEE_CRIT]
    ranks:
      - rank: 1
        resource: rage
        buff: { unit: self, duration: 15, base_charges: 3, effects: [{ name: APPLY_AURA_MOD_MELEE_ATTACK_SPEED, value: 0 }] }
  - name: Battle Stance
    causes_gcd: stance
    restricted_by_gcd: true
    cooldown: 1
    ranks: [{ rank: 1, resource: rage, buff: { unit: self, effects: [{ name: APPLY_AURA_SHAPESHIFT_BATTLE_STANCE }] } }]
  - name: Berserker Stance
    causes_gcd: stance
    restricted_by_gcd: true
    cooldown: 1
    ranks: [{ rank: 1, resource: rage, buff: { unit: self, effects: [{ name: APPLY_AURA_SHAPESHIFT_BERSERKER_STANCE }] } }]
shared_spell_cooldowns:
  - [Battle Stance, Berserker Stance]
"#,
        )
        .unwrap();
        let mut db = SpellDb::new();
        db.add_file(file).unwrap();
        db
    }

    fn setup() -> (SpellDb, CharacterSpells, Raid) {
        (db(), CharacterSpells::new(CharId(2), 1), Raid::default())
    }

    #[test]
    fn spell_groups_create_spells_ranks_cooldowns_and_buffs() {
        let (db, mut spells, mut raid) = setup();
        let shout = spells.add_spell_group(db.get("Battle Shout").unwrap(), 0, &mut raid);
        assert_eq!(shout.spells.len(), 2);
        assert_eq!(shout.proc, None);
        assert!(shout.enable_now);
        let group = spells.rank_group("Battle Shout").unwrap();
        assert_eq!(group.rank_numbers().collect::<Vec<_>>(), vec![6, 7]);
        let rank6 = spells.spell(shout.spells[0]);
        assert_eq!(rank6.rank(), 6);
        assert_eq!(rank6.id(), Some(shout.spells[0]));
        assert_eq!(
            rank6.instance_id(),
            Some(InstanceId(2 << CharacterSpells::INSTANCE_ID_SHIFT))
        );
        assert!(rank6.cooldown_id().is_some());
        assert!(!rank6.is_enabled());

        // Both ranks refer to the same shared party buff per canonical name... no: each rank
        // has its own canonical name, so two party buffs are registered with the raid.
        assert_eq!(raid.buffs.len(), 2);
        assert!(raid.buffs.iter().all(Buff::is_enabled));
        let marker = rank6.marker_buff().unwrap();
        assert!(matches!(
            spells.buff_slot(marker),
            BuffSlot::Shared(SharedBuffId(0))
        ));
        assert_eq!(spells.buff_id_of_shared(SharedBuffId(0)), Some(marker));
        assert!(spells.owned_buff(marker).is_none());

        let death_wish = spells.add_spell_group(db.get("Death Wish").unwrap(), 0, &mut raid);
        assert!(!death_wish.enable_now);
        let marker = spells.spell(death_wish.spells[0]).marker_buff().unwrap();
        assert!(matches!(spells.buff_slot(marker), BuffSlot::Owned(_)));
        assert!(!spells.owned_buff(marker).unwrap().is_enabled());
        assert_eq!(spells.spell_ids().count(), 3);
    }

    #[test]
    fn a_second_character_reuses_shared_buffs() {
        let (db, mut first, mut raid) = setup();
        let mut second = CharacterSpells::new(CharId(3), 1);
        first.add_spell_group(db.get("Sunder Armor").unwrap(), 0, &mut raid);
        second.add_spell_group(db.get("Sunder Armor").unwrap(), 1, &mut raid);
        assert_eq!(raid.buffs.len(), 1);
        assert_eq!(raid.buffs[0].canonical_name(), "Sunder Armor (rank 5)");

        // Party buffs are per party.
        first.add_spell_group(db.get("Battle Shout").unwrap(), 0, &mut raid);
        second.add_spell_group(db.get("Battle Shout").unwrap(), 1, &mut raid);
        assert_eq!(raid.buffs.len(), 5);
        let mut third = CharacterSpells::new(CharId(4), 1);
        third.add_spell_group(db.get("Battle Shout").unwrap(), 1, &mut raid);
        assert_eq!(raid.buffs.len(), 5);
    }

    #[test]
    fn passive_groups_become_procs() {
        let (db, mut spells, mut raid) = setup();
        let flurry = spells.add_spell_group(db.get("Flurry").unwrap(), 0, &mut raid);
        assert!(flurry.spells.is_empty());
        let proc = flurry.proc.unwrap();
        assert!(!flurry.enable_now);
        assert_eq!(spells.procs().get(proc).name(), "Flurry");
        assert!(spells.rank_group("Flurry").is_some());
        assert_eq!(spells.spell_ids().count(), 0);
        let marker = spells.procs().get(proc).spell().marker_buff().unwrap();
        assert!(spells.owned_buff(marker).is_some());
    }

    #[test]
    #[should_panic(expected = "already been added")]
    fn duplicate_groups_panic() {
        let (db, mut spells, mut raid) = setup();
        spells.add_spell_group(db.get("Flurry").unwrap(), 0, &mut raid);
        spells.add_spell_group(db.get("Flurry").unwrap(), 0, &mut raid);
    }

    #[test]
    fn shared_cooldowns_share_one_control() {
        let (db, mut spells, mut raid) = setup();
        let battle = spells.add_spell_group(db.get("Battle Stance").unwrap(), 0, &mut raid);
        let berserker = spells.add_spell_group(db.get("Berserker Stance").unwrap(), 0, &mut raid);
        assert_eq!(
            spells.spell(battle.spells[0]).cooldown_id(),
            spells.spell(berserker.spells[0]).cooldown_id()
        );
        assert_eq!(spells.cooldowns().len(), 1);
    }

    #[test]
    fn spells_can_be_taken_out_and_put_back() {
        let (db, mut spells, mut raid) = setup();
        let shout = spells.add_spell_group(db.get("Battle Shout").unwrap(), 0, &mut raid);
        let id = shout.spells[0];
        let spell = spells.take_spell(id);
        assert_eq!(spell.rank(), 6);
        assert_eq!(spells.spell_ids().count(), 1);
        spells.put_spell(id, spell);
        assert_eq!(spells.spell(id).rank(), 6);
    }

    #[test]
    #[should_panic(expected = "already taken out")]
    fn taking_a_spell_twice_panics() {
        let (db, mut spells, mut raid) = setup();
        let shout = spells.add_spell_group(db.get("Battle Shout").unwrap(), 0, &mut raid);
        let _first = spells.take_spell(shout.spells[0]);
        let _second = spells.take_spell(shout.spells[0]);
    }

    #[test]
    fn start_of_combat_spells_are_collected_from_the_flag() {
        let (db, mut spells, mut raid) = setup();
        let anger = spells.add_spell_group(db.get("Anger Management").unwrap(), 0, &mut raid);
        assert_eq!(spells.start_of_combat_spells(), &[anger.spells[0]]);
        spells.remove_start_of_combat_spell(anger.spells[0]);
        assert!(spells.start_of_combat_spells().is_empty());
        spells.add_start_of_combat_spell(anger.spells[0]);
        spells.add_start_of_combat_spell(anger.spells[0]);
        assert_eq!(spells.start_of_combat_spells().len(), 1);
    }

    #[test]
    fn owned_buffs_are_enabled_and_found_by_name() {
        let (_, mut spells, _) = setup();
        let id = spells.add_buff(Buff::new(
            "Revenge Ready",
            None,
            BuffKind::SelfBuff,
            Some(5.0),
            0,
        ));
        assert!(spells.owned_buff_by_name("Revenge Ready").is_none());
        spells.enable_buff(id);
        assert!(spells.is_buff_enabled(id));
        assert_eq!(spells.owned_buff_by_name("Revenge Ready"), Some(id));
        assert_eq!(
            spells.owned_buff(id).unwrap().instance_id(),
            Some(InstanceId(2 << CharacterSpells::INSTANCE_ID_SHIFT))
        );
        spells.add_start_of_combat_buff(id);
        assert_eq!(spells.start_of_combat_buffs(), &[id]);

        spells.disable_buff(id);
        assert!(!spells.is_buff_enabled(id));
        assert!(spells.start_of_combat_buffs().is_empty());
        assert!(spells.owned_buff_by_name("Revenge Ready").is_none());
        assert_eq!(spells.buff_ids().count(), 1);
    }

    #[test]
    #[should_panic(expected = "to be enabled")]
    fn start_of_combat_buffs_must_be_enabled() {
        let (_, mut spells, _) = setup();
        let id = spells.add_buff(Buff::new("x", None, BuffKind::SelfBuff, None, 0));
        spells.add_start_of_combat_buff(id);
    }

    #[test]
    fn buff_by_name_resolves_shared_buffs_and_canonical_names() {
        let (db, mut spells, mut raid) = setup();
        spells.add_spell_group(db.get("Battle Shout").unwrap(), 0, &mut raid);
        spells.add_spell_group(db.get("Sunder Armor").unwrap(), 0, &mut raid);
        let learned_up_to_6 = |id: SpellId| spells.spell(id).rank() <= 6;
        let all = |_: SpellId| true;

        let shout6 = spells
            .buff_by_name("Battle Shout", 0, &raid, learned_up_to_6)
            .unwrap();
        assert_eq!(
            spells.buff_id_of_shared(raid.shared_party_buff("Battle Shout (rank 6)", 0).unwrap()),
            Some(shout6)
        );
        let shout7 = spells.buff_by_name("Battle Shout", 0, &raid, all).unwrap();
        assert_ne!(shout6, shout7);
        assert_eq!(
            spells.buff_by_name("Battle Shout (rank 7)", 0, &raid, all),
            Some(shout7)
        );
        assert!(spells.buff_by_name("Battle Shout", 1, &raid, all).is_none());

        let sunder = spells.buff_by_name("Sunder Armor", 0, &raid, all).unwrap();
        assert_eq!(
            spells.buff_id_of_shared(raid.shared_raid_buff("Sunder Armor (rank 5)").unwrap()),
            Some(sunder)
        );
        assert!(spells.buff_by_name("Nope", 0, &raid, all).is_none());
    }

    #[test]
    fn cast_and_attack_bookkeeping() {
        let (_, mut spells, _) = setup();
        assert!(!spells.cast_in_progress());
        assert_eq!(spells.start_cast(), 1);
        assert!(spells.cast_in_progress());
        spells.complete_cast(1);
        assert!(!spells.cast_in_progress());
        assert_eq!(spells.start_cast(), 2);
        spells.reset_state();
        assert!(!spells.cast_in_progress());
        assert_eq!(spells.start_cast(), 1);

        spells.stop_attack();
        assert_eq!(spells.attack_mode(), AttackMode::MeleeAttack);
        spells.set_attack_mode(AttackMode::RangedAttack);
        spells.start_attack();
        assert!(spells.is_ranged_attacking());
        assert!(!spells.is_melee_attacking());
        assert!(spells.is_attacking());
        spells.stop_attack();
        assert!(!spells.is_attacking());
    }

    #[test]
    #[should_panic(expected = "Cast in progress")]
    fn starting_a_cast_during_a_cast_panics() {
        let (_, mut spells, _) = setup();
        spells.start_cast();
        spells.start_cast();
    }

    #[test]
    #[should_panic(expected = "Mismatched cast id")]
    fn completing_the_wrong_cast_panics() {
        let (_, mut spells, _) = setup();
        spells.start_cast();
        spells.complete_cast(7);
    }

    #[test]
    #[should_panic(expected = "Attack mode active")]
    fn changing_attack_mode_while_attacking_panics() {
        let (_, mut spells, _) = setup();
        spells.start_attack();
        spells.set_attack_mode(AttackMode::RangedAttack);
    }

    #[test]
    fn auto_attacks_and_the_next_swing_queue() {
        let (_, mut spells, _) = setup();
        assert_eq!(spells.mh_attack().hand(), Hand::Mainhand);
        assert_eq!(spells.oh_attack().hand(), Hand::Offhand);
        spells.oh_attack_mut().set_offhand_penalty(0.6);
        let mut oh = spells.take_auto_attack(Hand::Offhand);
        assert_eq!(oh.offhand_penalty(), 0.6);
        assert_eq!(spells.oh_attack().offhand_penalty(), 0.5);
        oh.complete_swing(1.0, 1.8);
        spells.put_auto_attack(oh);
        assert_eq!(spells.auto_attack(Hand::Offhand).last_used(), 1.0);

        assert_eq!(spells.queued_next_swing(), None);
        spells.queue_next_swing(SpellId(3));
        assert_eq!(spells.queued_next_swing(), Some(SpellId(3)));
        assert_eq!(spells.cancel_next_swing(), Some(SpellId(3)));
        assert_eq!(spells.cancel_next_swing(), None);

        spells.queue_next_swing(SpellId(3));
        spells.reset_state();
        assert_eq!(spells.queued_next_swing(), None);
        assert_eq!(spells.auto_attack(Hand::Offhand).last_used(), 0.0);
    }

    #[test]
    fn procs_can_be_taken_out_and_put_back() {
        let (db, mut spells, mut raid) = setup();
        let flurry = spells.add_spell_group(db.get("Flurry").unwrap(), 0, &mut raid);
        let procs = spells.take_procs();
        assert_eq!(procs.procs().len(), 1);
        assert_eq!(spells.procs().procs().len(), 0);
        spells.put_procs(procs);
        assert_eq!(spells.procs().get(flurry.proc.unwrap()).name(), "Flurry");
    }
}
