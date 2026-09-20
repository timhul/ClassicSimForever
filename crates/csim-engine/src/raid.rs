//! The raid: the characters in their parties, the buffs they share and the world they act in.
//! Port of `Raid/RaidControl.*`.
//!
//! [`RaidControl`] owns what the C++ raid control owned — the [`Engine`], the [`Target`], the
//! characters in their 8 × 5 places — plus the [`SharedBuffRegistry`] of party buffs and
//! raid-shared debuffs. A character acts through a [`CharacterContext`] that borrows the
//! character together with the engine, the target and the registry
//! ([`RaidControl::with_character`]); the raid control routes the engine's events to their
//! characters ([`RaidControl::dispatch`]) and runs the start / end of an iteration for all of
//! them.
//!
//! **Party buffs.** One character casts a party buff (Battle Shout), every member of its party
//! gains the auras. A context only borrows its own character, so it applies the auras to that
//! character and notes the change with the registry ([`PartyAuraChange`]); the raid control
//! repeats it on the other members before handing control back
//! ([`RaidControl::propagate_party_auras`]), the equivalent of the C++
//! `RaidControl::apply_party_buff` loop. The registry remembers who holds the auras, so a
//! removal only reaches the members that received the application. Statistics (the C++
//! `raid_statistics`) arrive in Phase 5.

use std::collections::BTreeMap;

use crate::buff::Buff;
use crate::character::context::CharacterContext;
use crate::character::Character;
use crate::character_spells::{PartyAuraChange, SharedBuffs};
use crate::engine::{Engine, Event, EventKind};
use crate::ids::{CharId, InstanceId, SharedBuffId};
use crate::target::Target;

/// Parties in a raid.
pub const PARTIES: u8 = 8;
/// Members per party.
pub const PARTY_SIZE: u8 = 5;

/// Why a character could not be placed in the raid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RaidError {
    #[error("party {party} member {member} is outside the {PARTIES} x {PARTY_SIZE} raid")]
    PlaceOutOfBounds { party: u8, member: u8 },
    #[error("party {party} member {member} is already taken")]
    PlaceTaken { party: u8, member: u8 },
    #[error("the raid is full")]
    RaidFull,
    #[error(
        "the character was built for party {party} member {member}, not the place it was added at"
    )]
    PlaceMismatch { party: u8, member: u8 },
}

/// The buffs shared between characters: party buffs (one instance per party) and raid-shared
/// debuffs (one instance for the raid), by canonical name. Port of `RaidControl`'s
/// `shared_party_buffs` / `shared_raid_buffs`.
#[derive(Debug, Default)]
pub struct SharedBuffRegistry {
    buffs: Vec<Buff>,
    party: [BTreeMap<String, SharedBuffId>; PARTIES as usize],
    raid: BTreeMap<String, SharedBuffId>,
    next_instance_id: u32,
    pending: Vec<PartyAuraChange>,
    /// Per shared buff, the characters holding its aura effects and how many times they were
    /// applied to them (once per stack). Only party buffs have holders.
    holders: Vec<Vec<(CharId, u32)>>,
}

impl SharedBuffRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every shared buff, in registration order (`SharedBuffId` order).
    pub fn buffs(&self) -> &[Buff] {
        &self.buffs
    }

    /// The instance ids of the raid's buffs come from the raid's own counter
    /// ([`InstanceId::for_raid`]). Port of `RaidControl::next_instance_id`.
    pub fn next_instance_id(&mut self) -> InstanceId {
        let id = self.next_instance_id;
        self.next_instance_id += 1;
        InstanceId::for_raid(id)
    }

    fn add(&mut self, mut buff: Buff) -> SharedBuffId {
        assert!(
            buff.is_enabled(),
            "Tried to register shared buff {} but it is not enabled",
            buff.canonical_name()
        );
        if buff.instance_id().is_none() {
            let id = self.next_instance_id();
            buff.set_instance_id(id);
        }
        let id = SharedBuffId(self.buffs.len() as u32);
        self.buffs.push(buff);
        self.holders.push(Vec::new());
        id
    }

    /// Takes the party aura changes noted since the last call.
    pub fn take_party_aura_changes(&mut self) -> Vec<PartyAuraChange> {
        std::mem::take(&mut self.pending)
    }

    /// The characters holding the aura effects of shared buff `id`, with their application
    /// counts.
    pub fn holders(&self, id: SharedBuffId) -> &[(CharId, u32)] {
        &self.holders[id.index()]
    }

    /// Records one application of buff `id`'s auras to `character`.
    fn add_holder(&mut self, id: SharedBuffId, character: CharId) {
        let holders = &mut self.holders[id.index()];
        match holders.iter_mut().find(|(c, _)| *c == character) {
            Some((_, count)) => *count += 1,
            None => holders.push((character, 1)),
        }
    }

    /// Records one removal of buff `id`'s auras from `character`; `false` if it held none.
    fn remove_holder(&mut self, id: SharedBuffId, character: CharId) -> bool {
        let holders = &mut self.holders[id.index()];
        let Some(index) = holders.iter().position(|(c, _)| *c == character) else {
            return false;
        };
        holders[index].1 -= 1;
        if holders[index].1 == 0 {
            holders.swap_remove(index);
        }
        true
    }
}

impl SharedBuffs for SharedBuffRegistry {
    fn shared_party_buff(&self, canonical_name: &str, party: u8) -> Option<SharedBuffId> {
        self.party[usize::from(party)].get(canonical_name).copied()
    }

    fn register_shared_party_buff(&mut self, buff: Buff, party: u8) -> SharedBuffId {
        assert!(
            party < PARTIES,
            "Register party buff: party {party} out of bounds"
        );
        let name = buff.canonical_name().to_string();
        assert!(
            !self.party[usize::from(party)].contains_key(&name),
            "Tried to register already registered party buff {name}"
        );
        let id = self.add(buff);
        self.party[usize::from(party)].insert(name, id);
        id
    }

    fn shared_raid_buff(&self, canonical_name: &str) -> Option<SharedBuffId> {
        self.raid.get(canonical_name).copied()
    }

    fn register_shared_raid_buff(&mut self, buff: Buff) -> SharedBuffId {
        let name = buff.canonical_name().to_string();
        assert!(
            !self.raid.contains_key(&name),
            "Tried to register already registered raid buff {name}"
        );
        let id = self.add(buff);
        self.raid.insert(name, id);
        id
    }

    fn shared_buff(&self, id: SharedBuffId) -> &Buff {
        &self.buffs[id.index()]
    }

    fn shared_buff_mut(&mut self, id: SharedBuffId) -> &mut Buff {
        &mut self.buffs[id.index()]
    }

    fn note_party_aura_change(&mut self, change: PartyAuraChange) {
        self.pending.push(change);
    }
}

/// The raid and its world. See the module documentation.
#[derive(Debug)]
pub struct RaidControl {
    engine: Engine,
    target: Target,
    shared: SharedBuffRegistry,
    /// Indexed by `CharId`.
    characters: Vec<Character>,
    /// `places[party][member]`.
    places: [[Option<CharId>; PARTY_SIZE as usize]; PARTIES as usize],
}

impl RaidControl {
    /// An empty raid facing `target`.
    pub fn new(target: Target) -> Self {
        RaidControl {
            engine: Engine::new(),
            target,
            shared: SharedBuffRegistry::new(),
            characters: Vec::new(),
            places: [[None; PARTY_SIZE as usize]; PARTIES as usize],
        }
    }

    // ---------------------------------------------------------------- world

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn target_mut(&mut self) -> &mut Target {
        &mut self.target
    }

    pub fn shared_buffs(&self) -> &SharedBuffRegistry {
        &self.shared
    }

    pub fn shared_buffs_mut(&mut self) -> &mut SharedBuffRegistry {
        &mut self.shared
    }

    // ---------------------------------------------------------------- places

    /// The id the next character added gets.
    pub fn next_char_id(&self) -> CharId {
        CharId(u8::try_from(self.characters.len()).expect("at most 40 characters"))
    }

    /// The first free place, filling party 1 before party 2 and so on. Port of
    /// `RaidControl::auto_assign_character_to_group`.
    pub fn free_place(&self) -> Option<(u8, u8)> {
        self.places.iter().enumerate().find_map(|(party, members)| {
            members
                .iter()
                .position(Option::is_none)
                .map(|member| (party as u8, member as u8))
        })
    }

    /// Adds the character `build(id, party, member)` at the first free place.
    pub fn add_character(
        &mut self,
        build: impl FnOnce(CharId, u8, u8) -> Character,
    ) -> Result<CharId, RaidError> {
        let (party, member) = self.free_place().ok_or(RaidError::RaidFull)?;
        self.add_character_at(party, member, build)
    }

    /// Adds the character `build(id, party, member)` at the given place, which must be free
    /// (the C++ replaced the occupant; a raid is built once here). Port of
    /// `RaidControl::assign_character_to_place`.
    pub fn add_character_at(
        &mut self,
        party: u8,
        member: u8,
        build: impl FnOnce(CharId, u8, u8) -> Character,
    ) -> Result<CharId, RaidError> {
        if party >= PARTIES || member >= PARTY_SIZE {
            return Err(RaidError::PlaceOutOfBounds { party, member });
        }
        if self.places[usize::from(party)][usize::from(member)].is_some() {
            return Err(RaidError::PlaceTaken { party, member });
        }
        let id = self.next_char_id();
        let character = build(id, party, member);
        if character.id() != id || character.party() != party || character.party_member() != member
        {
            return Err(RaidError::PlaceMismatch {
                party: character.party(),
                member: character.party_member(),
            });
        }
        self.places[usize::from(party)][usize::from(member)] = Some(id);
        self.characters.push(character);
        Ok(id)
    }

    pub fn len(&self) -> usize {
        self.characters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.characters.is_empty()
    }

    pub fn char_ids(&self) -> impl Iterator<Item = CharId> {
        (0..self.characters.len()).map(|i| CharId(i as u8))
    }

    pub fn characters(&self) -> &[Character] {
        &self.characters
    }

    pub fn character(&self, id: CharId) -> &Character {
        &self.characters[id.index()]
    }

    pub fn character_mut(&mut self, id: CharId) -> &mut Character {
        &mut self.characters[id.index()]
    }

    /// The character at a place, if any.
    pub fn character_at(&self, party: u8, member: u8) -> Option<CharId> {
        self.places
            .get(usize::from(party))?
            .get(usize::from(member))
            .copied()
            .flatten()
    }

    /// The members of `party`, in member order.
    pub fn party_members(&self, party: u8) -> impl Iterator<Item = CharId> + '_ {
        self.places
            .get(usize::from(party))
            .into_iter()
            .flat_map(|members| members.iter().flatten().copied())
    }

    // ---------------------------------------------------------------- acting

    /// The context of character `id`: the character with the engine, the target and the
    /// shared buffs. Party aura changes it makes are propagated by the next
    /// [`RaidControl::propagate_party_auras`]; [`RaidControl::with_character`] does that for
    /// you.
    pub fn context(&mut self, id: CharId) -> CharacterContext<'_, SharedBuffRegistry> {
        CharacterContext::new(
            &mut self.characters[id.index()],
            &mut self.engine,
            &mut self.target,
            &mut self.shared,
        )
    }

    /// Runs `f` with the context of character `id`, then propagates the party aura changes.
    pub fn with_character<R>(
        &mut self,
        id: CharId,
        f: impl FnOnce(&mut CharacterContext<'_, SharedBuffRegistry>) -> R,
    ) -> R {
        let result = f(&mut self.context(id));
        self.propagate_party_auras();
        result
    }

    /// Applies (or removes) the auras of the party buffs whose change a context noted on the
    /// party's other members: an application reaches every member of the party, a removal
    /// every holder of the auras. The noting character already did its own part. Port of
    /// `RaidControl::apply_party_buff` / `remove_party_buff`.
    pub fn propagate_party_auras(&mut self) {
        loop {
            let changes = self.shared.take_party_aura_changes();
            if changes.is_empty() {
                return;
            }
            for change in changes {
                let effects = self.shared.shared_buff(change.buff).effects.clone();
                let affected: Vec<CharId> = if change.apply {
                    self.party_members(change.party).collect()
                } else {
                    self.shared
                        .holders(change.buff)
                        .iter()
                        .map(|(id, _)| *id)
                        .collect()
                };
                for id in affected {
                    if change.apply {
                        self.shared.add_holder(change.buff, id);
                    } else if !self.shared.remove_holder(change.buff, id) {
                        continue;
                    }
                    if id != change.by {
                        self.context(id).change_aura_effects(&effects, change.apply);
                    }
                }
            }
        }
    }

    /// Routes an event to its character (`EncounterEnd` ends combat for everyone). Returns
    /// `false` for events nobody handles yet (`PlayerAction` is the rotation's, Phase 5;
    /// `IncomingDamage` is tank mode's).
    pub fn dispatch(&mut self, event: &Event) -> bool {
        match event.kind.character() {
            Some(id) => self.with_character(id, |ctx| ctx.handle_event(event)),
            None => {
                debug_assert!(matches!(event.kind, EventKind::EncounterEnd));
                self.engine.end_combat();
                true
            }
        }
    }

    /// Pops and dispatches events until the queue is empty. Port of `Engine::run`.
    pub fn run(&mut self) {
        while let Some(event) = self.engine.next_event() {
            self.dispatch(&event);
        }
    }

    // ---------------------------------------------------------------- lifecycle

    /// Before a set of iterations: the engine, every character and, through them, the shared
    /// buffs. Port of `RaidControl::prepare_set_of_combat_iterations` plus the per-character
    /// loop of `SimControl::run_sim`.
    pub fn prepare_set_of_combat_iterations(&mut self) {
        self.engine.prepare_set_of_iterations();
        for id in self.char_ids().collect::<Vec<_>>() {
            self.with_character(id, |ctx| ctx.prepare_set_of_combat_iterations());
        }
    }

    /// End of an iteration: every character is reset (the first member holding a shared buff
    /// resets it, stripping its auras from the whole party) and the target must be clean.
    /// Port of `RaidControl::reset` plus the per-character loop of `SimControl::run_sim`.
    pub fn reset(&mut self) {
        for id in self.char_ids().collect::<Vec<_>>() {
            self.with_character(id, |ctx| ctx.reset());
        }
        self.target.check_clean();
    }
}

#[cfg(test)]
mod tests;
