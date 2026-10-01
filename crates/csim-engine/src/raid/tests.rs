//! Raid tests: Warriors from the character test fixture sharing party buffs and raid debuffs.
//! Port of the raid parts of `Test/Warrior/Spells/TestBattleShout` / `TestSunderArmor`.

use super::*;
use crate::buff::BuffKind;
use crate::character::SimParams;
use crate::character::tests::{equipment_db, race, warrior_class};
use crate::character_spells::BuffSlot;
use crate::ids::{BuffId, SpellId};
use crate::item::EquipmentSlot;
use crate::phase::Phase;
use crate::race::Race;
use crate::resource::ResourceType;
use crate::spell::record::SpellDb;
use crate::spell::test_world::db;
use crate::spell::{SpellHost, SpellResult};

const BATTLE_SHOUT_6: u32 = 11551;
const SUNDER_ARMOR: u32 = 11597;
const SWORD: u32 = 1;
/// Battle Shout r6 at level 60: 111 + 0.6 × (60 − 52) = 115.8.
const SHOUT_AP: u32 = 116;

fn warrior(id: CharId, party: u8, member: u8) -> Character {
    Character::new(
        id,
        warrior_class(),
        &race(Race::Orc),
        equipment_db(),
        Phase::MoltenCore,
        SimParams::default(),
        63,
        party,
        member,
    )
}

struct Fixture {
    raid: RaidControl,
    db: SpellDb,
}

impl Fixture {
    fn new() -> Self {
        let mut raid = RaidControl::new(Target::new(63));
        raid.engine_mut().prepare_iteration(0.0);
        Fixture { raid, db: db() }
    }

    /// Adds a Warrior at the first free place that knows Battle Shout and Sunder Armor.
    fn add_warrior(&mut self) -> CharId {
        let id = self.raid.add_character(warrior).unwrap();
        self.learn(id);
        id
    }

    fn add_warrior_at(&mut self, party: u8, member: u8) -> CharId {
        let id = self.raid.add_character_at(party, member, warrior).unwrap();
        self.learn(id);
        id
    }

    fn learn(&mut self, id: CharId) {
        self.raid
            .character_mut(id)
            .equipment_mut()
            .equip(EquipmentSlot::Mainhand, SWORD)
            .unwrap();
        let db = &self.db;
        self.raid.with_character(id, |ctx| {
            ctx.learn(db, BATTLE_SHOUT_6);
            ctx.learn(db, SUNDER_ARMOR);
        });
        let character = self.raid.character_mut(id);
        character.gain_resource(ResourceType::Rage, 100, 0.0);
        // Sunder Armor rolls on the attack table: rig every roll to a hit so casts always land.
        character.roll_mut().random_mut().set_new_range(9999, 10000);
    }

    fn spell(&self, id: CharId, game_id: u32) -> SpellId {
        self.raid
            .character(id)
            .spells()
            .spell_by_game_id(game_id)
            .unwrap()
    }

    fn cast(&mut self, id: CharId, game_id: u32) -> SpellResult {
        let spell = self.spell(id, game_id);
        self.raid.with_character(id, |ctx| ctx.cast(spell).result)
    }

    fn ap(&self, id: CharId) -> u32 {
        self.raid
            .character(id)
            .stats()
            .base_stats()
            .get_base_melee_ap()
    }

    fn base_ap(&self) -> u32 {
        warrior_class().base_stats.melee_ap
    }

    fn marker(&self, id: CharId, game_id: u32) -> BuffId {
        let spell = self.spell(id, game_id);
        self.raid
            .character(id)
            .spells()
            .spell(spell)
            .marker_buff()
            .unwrap()
    }

    fn shared_handle(&self, id: CharId, game_id: u32) -> SharedBuffId {
        match self
            .raid
            .character(id)
            .spells()
            .buff_slot(self.marker(id, game_id))
        {
            BuffSlot::Shared(handle) => *handle,
            BuffSlot::Owned(_) => panic!("{game_id} is not a shared buff"),
        }
    }

    fn shared_active(&self, id: CharId, game_id: u32) -> bool {
        self.raid
            .shared_buffs()
            .shared_buff(self.shared_handle(id, game_id))
            .is_active()
    }
}

// ---------------------------------------------------------------- places

#[test]
fn characters_fill_the_first_party_first() {
    let mut raid = RaidControl::new(Target::new(63));
    assert!(raid.is_empty());
    assert_eq!(raid.free_place(), Some((0, 0)));
    for i in 0..7u8 {
        let id = raid.add_character(warrior).unwrap();
        assert_eq!(id, CharId(i));
    }
    assert_eq!(raid.len(), 7);
    assert_eq!(raid.character(CharId(4)).party(), 0);
    assert_eq!(raid.character(CharId(4)).party_member(), 4);
    assert_eq!(raid.character(CharId(5)).party(), 1);
    assert_eq!(raid.character(CharId(5)).party_member(), 0);
    assert_eq!(raid.character(CharId(0)).player_name(), "You");
    assert_eq!(raid.character(CharId(6)).player_name(), "P2M2");
    assert_eq!(raid.character_at(1, 1), Some(CharId(6)));
    assert_eq!(raid.character_at(1, 2), None);
    assert_eq!(raid.character_at(9, 0), None);
    assert_eq!(
        raid.party_members(0).collect::<Vec<_>>(),
        (0..5).map(CharId).collect::<Vec<_>>()
    );
    assert_eq!(
        raid.party_members(1).collect::<Vec<_>>(),
        vec![CharId(5), CharId(6)]
    );
    assert_eq!(raid.free_place(), Some((1, 2)));
    assert_eq!(raid.char_ids().count(), 7);
}

#[test]
fn explicit_places_and_their_errors() {
    let mut raid = RaidControl::new(Target::new(63));
    let id = raid.add_character_at(3, 2, warrior).unwrap();
    assert_eq!(raid.character(id).party(), 3);
    assert_eq!(raid.character_at(3, 2), Some(id));
    assert_eq!(
        raid.add_character_at(3, 2, warrior),
        Err(RaidError::PlaceTaken {
            party: 3,
            member: 2
        })
    );
    assert_eq!(
        raid.add_character_at(8, 0, warrior),
        Err(RaidError::PlaceOutOfBounds {
            party: 8,
            member: 0
        })
    );
    assert_eq!(
        raid.add_character_at(0, 5, warrior),
        Err(RaidError::PlaceOutOfBounds {
            party: 0,
            member: 5
        })
    );
    assert_eq!(
        raid.add_character_at(0, 0, |id, _, _| warrior(id, 1, 1)),
        Err(RaidError::PlaceMismatch {
            party: 1,
            member: 1
        })
    );
    // The auto assignment skips the taken place.
    for _ in 0..39 {
        raid.add_character(warrior).unwrap();
    }
    assert_eq!(raid.len(), 40);
    assert_eq!(raid.free_place(), None);
    assert_eq!(raid.add_character(warrior), Err(RaidError::RaidFull));
}

// ---------------------------------------------------------------- shared buffs

#[test]
fn shared_buff_instance_ids_are_the_raids() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let b = f.add_warrior();
    let shout = f.shared_handle(a, BATTLE_SHOUT_6);
    let sunder = f.shared_handle(a, SUNDER_ARMOR);
    assert_eq!(f.raid.shared_buffs().buffs().len(), 2);
    assert_eq!(
        f.raid.shared_buffs().shared_buff(shout).instance_id(),
        Some(InstanceId::for_raid(0))
    );
    assert_eq!(
        f.raid.shared_buffs().shared_buff(sunder).instance_id(),
        Some(InstanceId::for_raid(1))
    );
    assert_eq!(
        f.raid.shared_buffs().shared_buff(shout).kind(),
        BuffKind::PartyBuff { party: 0 }
    );
    assert_eq!(
        f.raid.shared_buffs().shared_buff(sunder).kind(),
        BuffKind::SharedDebuff
    );
    // The second Warrior refers to the same instances.
    assert_eq!(f.shared_handle(b, BATTLE_SHOUT_6), shout);
    assert_eq!(f.shared_handle(b, SUNDER_ARMOR), sunder);
    // The characters' own instance ids live in their own namespaces.
    let own = f.raid.character(b).spells().spell(f.spell(b, SUNDER_ARMOR));
    assert_eq!(own.instance_id().map(|id| id.0 >> 24), Some(u32::from(b.0)));
}

#[test]
fn a_party_buff_reaches_every_member_of_the_party() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let b = f.add_warrior();
    let c = f.add_warrior_at(1, 0);
    let base = f.base_ap();
    assert_eq!(f.ap(a), base);

    assert_eq!(f.cast(a, BATTLE_SHOUT_6), SpellResult::Success);
    assert!(f.shared_active(a, BATTLE_SHOUT_6));
    assert_eq!(f.ap(a), base + SHOUT_AP, "the caster");
    assert_eq!(f.ap(b), base + SHOUT_AP, "the party member");
    assert_eq!(f.ap(c), base, "another party");
    assert_eq!(
        f.raid
            .shared_buffs()
            .holders(f.shared_handle(a, BATTLE_SHOUT_6)),
        &[(a, 1), (b, 1)]
    );

    // The party member casting it again refreshes the shared instance: no second application.
    assert_eq!(f.cast(b, BATTLE_SHOUT_6), SpellResult::Success);
    assert_eq!(f.ap(a), base + SHOUT_AP);
    assert_eq!(f.ap(b), base + SHOUT_AP);

    // The other party has its own instance.
    assert_ne!(
        f.shared_handle(c, BATTLE_SHOUT_6),
        f.shared_handle(a, BATTLE_SHOUT_6)
    );
    assert_eq!(f.cast(c, BATTLE_SHOUT_6), SpellResult::Success);
    assert_eq!(f.ap(c), base + SHOUT_AP);

    // Expiry removes it from every member.
    f.raid
        .engine_mut()
        .add_event_in(200.0, EventKind::EncounterEnd);
    f.raid.run();
    assert!(!f.shared_active(a, BATTLE_SHOUT_6));
    assert_eq!(f.ap(a), base);
    assert_eq!(f.ap(b), base);
    assert_eq!(f.ap(c), base);
}

#[test]
fn a_party_buff_cancelled_by_any_member_leaves_all_of_them() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let b = f.add_warrior();
    let base = f.base_ap();
    f.cast(a, BATTLE_SHOUT_6);
    let marker = f.marker(b, BATTLE_SHOUT_6);
    assert!(f.raid.with_character(b, |ctx| ctx.cancel_buff(marker)));
    assert_eq!(f.ap(a), base);
    assert_eq!(f.ap(b), base);
}

#[test]
fn a_member_added_after_the_shout_is_not_buffed_until_the_next_cast() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let c = f.add_warrior();
    let base = f.base_ap();
    f.cast(a, BATTLE_SHOUT_6);
    let b = f.add_warrior();
    assert_eq!(f.ap(b), base);
    f.cast(c, BATTLE_SHOUT_6);
    assert_eq!(f.ap(b), base, "a refresh does not re-apply the auras");
    f.raid
        .engine_mut()
        .add_event_in(200.0, EventKind::EncounterEnd);
    f.raid.run();
    assert_eq!(f.ap(a), base);
    assert_eq!(f.ap(b), base, "the removal only reaches the holders");
    assert!(
        f.raid
            .shared_buffs()
            .holders(f.shared_handle(a, BATTLE_SHOUT_6))
            .is_empty()
    );
    f.cast(b, BATTLE_SHOUT_6);
    assert_eq!(f.ap(a), base + SHOUT_AP);
    assert_eq!(f.ap(b), base + SHOUT_AP);
}

#[test]
fn reset_strips_the_party_buff_from_the_whole_party_and_cleans_the_target() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let b = f.add_warrior();
    let base = f.base_ap();
    f.cast(a, BATTLE_SHOUT_6);
    f.cast(b, SUNDER_ARMOR);
    let armor = f.raid.target().armor();
    assert!(f.shared_active(a, SUNDER_ARMOR));
    let sunder = f
        .raid
        .shared_buffs()
        .shared_buff(f.shared_handle(a, SUNDER_ARMOR));
    assert!(f.raid.target().has_debuff(sunder.instance_id().unwrap()));

    f.raid.reset();
    assert!(!f.shared_active(a, BATTLE_SHOUT_6));
    assert!(!f.shared_active(a, SUNDER_ARMOR));
    assert_eq!(f.ap(a), base);
    assert_eq!(f.ap(b), base);
    assert!(
        f.raid.target().armor() > armor,
        "the armor reduction is gone"
    );
    let sunder = f
        .raid
        .shared_buffs()
        .shared_buff(f.shared_handle(a, SUNDER_ARMOR));
    assert!(!f.raid.target().has_debuff(sunder.instance_id().unwrap()));

    // The next iteration works the same.
    f.raid.engine_mut().prepare_iteration(0.0);
    for id in [a, b] {
        f.raid
            .character_mut(id)
            .gain_resource(ResourceType::Rage, 100, 0.0);
    }
    f.cast(b, BATTLE_SHOUT_6);
    assert_eq!(f.ap(a), base + SHOUT_AP);
    assert_eq!(f.ap(b), base + SHOUT_AP);
}

#[test]
fn a_shared_debuff_is_one_instance_for_the_raid() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let b = f.add_warrior_at(2, 0);
    assert_eq!(
        f.shared_handle(a, SUNDER_ARMOR),
        f.shared_handle(b, SUNDER_ARMOR)
    );
    let armor = f.raid.target().armor();
    f.cast(a, SUNDER_ARMOR);
    let after_one = f.raid.target().armor();
    assert!(after_one < armor);
    f.cast(b, SUNDER_ARMOR);
    let after_two = f.raid.target().armor();
    assert_eq!(armor - after_two, 2 * (armor - after_one), "two stacks");
    let sunder = f
        .raid
        .shared_buffs()
        .shared_buff(f.shared_handle(a, SUNDER_ARMOR));
    assert_eq!(sunder.stacks(), 2);
}

// ---------------------------------------------------------------- events

#[test]
fn dispatch_routes_events_to_their_characters() {
    let mut f = Fixture::new();
    let a = f.add_warrior();
    let b = f.add_warrior();
    f.raid.prepare_set_of_combat_iterations();
    f.raid.engine_mut().prepare_iteration(0.0);
    for id in [a, b] {
        f.raid
            .engine_mut()
            .add_event_in(0.0, EventKind::EncounterStart { character: id });
    }
    f.raid
        .engine_mut()
        .add_event_in(10.0, EventKind::EncounterEnd);
    assert!(
        !f.raid
            .dispatch(&Event::new(0.0, EventKind::IncomingDamage { character: a }))
    );
    f.raid.run();
    assert!(f.raid.engine().queue().is_empty());
    assert!((f.raid.engine().current_time() - 10.0).abs() < f64::EPSILON);
    let counts = f.raid.engine().event_counts();
    assert_eq!(counts.get(crate::engine::EventType::EncounterStart), 2);
    assert_eq!(counts.get(crate::engine::EventType::EncounterEnd), 1);
    // 2.6 speed sword: swings at 0, 2.6, 5.2, 7.8 for each of the two Warriors.
    assert_eq!(counts.get(crate::engine::EventType::MainhandMeleeHit), 8);
    f.raid.reset();
}
