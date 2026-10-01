use super::*;
use crate::ids::InstanceId;
use crate::raid::SharedBuffRegistry;
use crate::spell::record::SpellDb;
use crate::spell::test_world::db;

const HEROIC_STRIKE_1: u32 = 78;
const HEROIC_STRIKE_2: u32 = 284;
const BLOODTHIRST: u32 = 23881;
const BATTLE_SHOUT_6: u32 = 11551;
const BATTLE_SHOUT_7: u32 = 25289;
const BATTLE_STANCE: u32 = 2457;
const BERSERKER_STANCE: u32 = 2458;
const SUNDER_ARMOR: u32 = 11597;
const REND: u32 = 11574;
const ANGER_MANAGEMENT: u32 = 12296;
const FLURRY: u32 = 12319;
const FLURRY_BUFF: u32 = 12966;
const SWEEPING_STRIKES: u32 = 12292;
const SWEEPING_STRIKES_PAYLOAD: u32 = 12723;
const SLAM: u32 = 1464;
const IMPROVED_SLAM_RANK_2: u32 = 1310197;
const DEEP_WOUNDS: u32 = 12834;
const DEEP_WOUNDS_BLEED: u32 = 12162;
const BLOODRAGE: u32 = 2687;

fn setup() -> (SpellDb, CharacterSpells, SharedBuffRegistry) {
    (
        db(),
        CharacterSpells::new(CharId(2), 1),
        SharedBuffRegistry::new(),
    )
}

#[test]
fn spells_get_ids_cooldowns_buffs_and_rank_groups() {
    let (db, mut spells, mut raid) = setup();
    let shout = spells.add_spell(&db, BATTLE_SHOUT_6, 0, &mut raid);
    assert_eq!(shout.proc, None);
    assert!(shout.enable_now && shout.in_rank_group);
    let id = shout.spell.unwrap();
    let rank6 = spells.spell(id);
    assert_eq!(rank6.rank(), 6);
    assert_eq!(rank6.id(), Some(id));
    assert_eq!(
        rank6.instance_id(),
        Some(InstanceId::for_character(CharId(2), 0))
    );
    assert!(rank6.cooldown_id().is_none());
    assert!(!rank6.is_enabled());
    assert_eq!(spells.handle(BATTLE_SHOUT_6), Some(SpellHandle::Spell(id)));
    assert_eq!(spells.spell_by_game_id(BATTLE_SHOUT_6), Some(id));
    assert!(spells.has_game_id(BATTLE_SHOUT_6));
    assert!(!spells.has_game_id(BATTLE_SHOUT_7));

    // The party buff is registered with the raid; the character keeps a shared handle.
    assert_eq!(raid.buffs().len(), 1);
    assert!(raid.buffs()[0].is_enabled());
    assert_eq!(raid.buffs()[0].canonical_name(), "Battle Shout (11551)");
    let marker = shout.buff.unwrap();
    assert_eq!(rank6.marker_buff(), Some(marker));
    assert!(matches!(
        spells.buff_slot(marker),
        BuffSlot::Shared(SharedBuffId(0))
    ));
    assert_eq!(spells.buff_id_of_shared(SharedBuffId(0)), Some(marker));
    assert!(spells.owned_buff(marker).is_none());

    let rank7 = spells.add_spell(&db, BATTLE_SHOUT_7, 0, &mut raid);
    assert!(rank7.in_rank_group);
    let group = spells.rank_group("Battle Shout").unwrap();
    assert_eq!(group.rank_numbers().collect::<Vec<_>>(), vec![6, 7]);
    assert_eq!(
        spells.rank_group_of(id).map(|g| g.name()),
        Some("Battle Shout")
    );
    assert_eq!(raid.buffs().len(), 2, "each rank has its own party buff");

    // An owned marker buff, a category cooldown and an own cooldown.
    let rend = spells.add_spell(&db, REND, 0, &mut raid);
    assert!(matches!(
        spells.buff_slot(rend.buff.unwrap()),
        BuffSlot::Owned(_)
    ));
    assert!(!spells.owned_buff(rend.buff.unwrap()).unwrap().is_enabled());
    let bt = spells.add_spell(&db, BLOODTHIRST, 0, &mut raid);
    let bt_spell = spells.spell(bt.spell.unwrap());
    assert_eq!(bt.buff, None);
    assert!(bt_spell.cooldown_id().is_none());
    assert!(bt_spell.category_cooldown_id().is_some());
    let bloodrage = spells.add_spell(&db, BLOODRAGE, 0, &mut raid);
    assert!(
        spells
            .spell(bloodrage.spell.unwrap())
            .cooldown_id()
            .is_some()
    );
    assert_eq!(bloodrage.buff, None);
    assert_eq!(spells.cooldowns().len(), 2);
    assert_eq!(spells.spell_ids().count(), 5);
}

#[test]
fn a_second_character_reuses_shared_buffs() {
    let (db, mut first, mut raid) = setup();
    let mut second = CharacterSpells::new(CharId(3), 1);
    first.add_spell(&db, SUNDER_ARMOR, 0, &mut raid);
    second.add_spell(&db, SUNDER_ARMOR, 1, &mut raid);
    assert_eq!(raid.buffs().len(), 1);
    assert_eq!(raid.buffs()[0].canonical_name(), "Sunder Armor (11597)");
    assert_eq!(raid.buffs()[0].kind(), BuffKind::SharedDebuff);

    // Party buffs are per party.
    first.add_spell(&db, BATTLE_SHOUT_6, 0, &mut raid);
    second.add_spell(&db, BATTLE_SHOUT_6, 1, &mut raid);
    assert_eq!(raid.buffs().len(), 3);
    let mut third = CharacterSpells::new(CharId(4), 1);
    third.add_spell(&db, BATTLE_SHOUT_6, 1, &mut raid);
    assert_eq!(raid.buffs().len(), 3);
}

#[test]
fn passives_with_a_proc_mask_become_procs() {
    let (db, mut spells, mut raid) = setup();
    let flurry = spells.add_spell(&db, FLURRY, 0, &mut raid);
    assert!(flurry.spell.is_none());
    assert!(!flurry.enable_now && !flurry.in_rank_group);
    let proc = flurry.proc.unwrap();
    assert_eq!(spells.procs().get(proc).name(), "Flurry");
    assert_eq!(spells.handle(FLURRY), Some(SpellHandle::Proc(proc)));
    assert_eq!(spells.proc_by_game_id(FLURRY), Some(proc));
    assert_eq!(spells.spell_by_game_id(FLURRY), None);
    assert!(spells.rank_group("Flurry").is_none());
    assert_eq!(spells.spell_ids().count(), 0);
    assert!(spells.owned_buff(flurry.buff.unwrap()).is_some());

    // The haste buff of the same name is a spell of its own, outside the rank groups.
    let buff = spells.add_spell(&db, FLURRY_BUFF, 0, &mut raid);
    assert!(buff.spell.is_some() && !buff.in_rank_group);

    // Passives without a proc mask stay spells.
    let anger = spells.add_spell(&db, ANGER_MANAGEMENT, 0, &mut raid);
    assert!(anger.spell.is_some() && anger.proc.is_none());
    assert!(!anger.in_rank_group);
    assert_eq!(spells.start_of_combat_spells(), &[anger.spell.unwrap()]);
}

#[test]
#[should_panic(expected = "already been added")]
fn duplicate_spells_panic() {
    let (db, mut spells, mut raid) = setup();
    spells.add_spell(&db, FLURRY, 0, &mut raid);
    spells.add_spell(&db, FLURRY, 0, &mut raid);
}

#[test]
#[should_panic(expected = "not in the spell db")]
fn unknown_spells_panic() {
    let (db, mut spells, mut raid) = setup();
    spells.add_spell(&db, 1, 0, &mut raid);
}

#[test]
fn spells_of_one_category_share_one_control() {
    let (db, mut spells, mut raid) = setup();
    let battle = spells.add_spell(&db, BATTLE_STANCE, 0, &mut raid);
    let berserker = spells.add_spell(&db, BERSERKER_STANCE, 0, &mut raid);
    assert_eq!(
        spells.spell(battle.spell.unwrap()).category_cooldown_id(),
        spells
            .spell(berserker.spell.unwrap())
            .category_cooldown_id()
    );
    assert_eq!(spells.cooldowns().len(), 1);
    assert!(
        spells
            .cooldowns()
            .id_by_name(&category_cooldown_name(47))
            .is_some()
    );
}

#[test]
fn name_collisions_and_hidden_spells_stay_out_of_rank_groups() {
    let (db, mut spells, mut raid) = setup();
    let ability = spells.add_spell(&db, SWEEPING_STRIKES, 0, &mut raid);
    assert!(!ability.in_rank_group, "IGNORED by the overrides");
    let payload = spells.add_spell(&db, SWEEPING_STRIKES_PAYLOAD, 0, &mut raid);
    assert!(
        !payload.in_rank_group,
        "a triggered payload (AcquireMethod 3)"
    );
    let hs1 = spells.add_spell(&db, HEROIC_STRIKE_1, 0, &mut raid);
    assert!(hs1.in_rank_group);
    let db2 = {
        let mut copy = (**db.get(HEROIC_STRIKE_1).unwrap()).clone();
        copy.id = 999_078;
        let mut db2 = SpellDb::new();
        db2.add(None, copy).unwrap();
        db2
    };
    let twin = spells.add_spell(&db2, 999_078, 0, &mut raid);
    assert!(
        !twin.in_rank_group,
        "a second spell with the same name and rank"
    );
    let slam = spells.add_spell(&db, SLAM, 0, &mut raid);
    assert!(slam.in_rank_group);
    let replacement = spells.add_spell(&db, IMPROVED_SLAM_RANK_2, 0, &mut raid);
    assert!(!replacement.in_rank_group, "not in a skill line");
    let bleed = spells.add_spell(&db, DEEP_WOUNDS_BLEED, 0, &mut raid);
    assert!(!bleed.in_rank_group, "hidden payload");
    assert!(!bleed.enable_now);
    assert_eq!(spells.rank_groups().count(), 2);
    assert_eq!(spells.spell_ids().count(), 7);
}

#[test]
fn actionbar_overrides_swap_rank_group_members() {
    let (db, mut spells, mut raid) = setup();
    let slam = spells.add_spell(&db, SLAM, 0, &mut raid).spell.unwrap();
    let improved = spells
        .add_spell(&db, IMPROVED_SLAM_RANK_2, 0, &mut raid)
        .spell
        .unwrap();
    let rank2 = |spells: &CharacterSpells| {
        spells
            .rank_group("Slam")
            .unwrap()
            .get_spell_rank(2, |_| true)
    };
    assert_eq!(rank2(&spells), Some(slam));

    assert!(spells.apply_actionbar_override(SLAM, IMPROVED_SLAM_RANK_2, true));
    assert_eq!(rank2(&spells), Some(improved));
    assert_eq!(
        spells.actionbar_overrides(),
        &[(SLAM, IMPROVED_SLAM_RANK_2)]
    );
    assert!(
        !spells.apply_actionbar_override(SLAM, IMPROVED_SLAM_RANK_2, true),
        "already applied"
    );
    assert!(spells.apply_actionbar_override(SLAM, IMPROVED_SLAM_RANK_2, false));
    assert_eq!(rank2(&spells), Some(slam));
    assert!(spells.actionbar_overrides().is_empty());
    assert!(!spells.apply_actionbar_override(SLAM, IMPROVED_SLAM_RANK_2, false));
    assert!(
        !spells.apply_actionbar_override(SLAM, 12345, true),
        "unknown replacement"
    );
}

#[test]
fn spells_can_be_taken_out_and_put_back() {
    let (db, mut spells, mut raid) = setup();
    let id = spells
        .add_spell(&db, BATTLE_SHOUT_6, 0, &mut raid)
        .spell
        .unwrap();
    let spell = spells.take_spell(id);
    assert_eq!(spell.rank(), 6);
    assert_eq!(spells.spell_ids().count(), 0);
    spells.put_spell(id, spell);
    assert_eq!(spells.spell(id).rank(), 6);
}

#[test]
#[should_panic(expected = "already taken out")]
fn taking_a_spell_twice_panics() {
    let (db, mut spells, mut raid) = setup();
    let id = spells
        .add_spell(&db, BATTLE_SHOUT_6, 0, &mut raid)
        .spell
        .unwrap();
    let _first = spells.take_spell(id);
    let _second = spells.take_spell(id);
}

#[test]
fn start_of_combat_spells_can_be_managed() {
    let (db, mut spells, mut raid) = setup();
    let anger = spells
        .add_spell(&db, ANGER_MANAGEMENT, 0, &mut raid)
        .spell
        .unwrap();
    assert_eq!(spells.start_of_combat_spells(), &[anger]);
    spells.remove_start_of_combat_spell(anger);
    assert!(spells.start_of_combat_spells().is_empty());
    spells.add_start_of_combat_spell(anger);
    spells.add_start_of_combat_spell(anger);
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
        Some(InstanceId::for_character(CharId(2), 0))
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
    spells.add_spell(&db, BATTLE_SHOUT_6, 0, &mut raid);
    spells.add_spell(&db, BATTLE_SHOUT_7, 0, &mut raid);
    spells.add_spell(&db, SUNDER_ARMOR, 0, &mut raid);
    let learned_up_to_6 = |id: SpellId| spells.spell(id).rank() <= 6;
    let all = |_: SpellId| true;

    let shout6 = spells
        .buff_by_name("Battle Shout", 0, &raid, learned_up_to_6)
        .unwrap();
    assert_eq!(
        spells.buff_id_of_shared(raid.shared_party_buff("Battle Shout (11551)", 0).unwrap()),
        Some(shout6)
    );
    let shout7 = spells.buff_by_name("Battle Shout", 0, &raid, all).unwrap();
    assert_ne!(shout6, shout7);
    assert_eq!(
        spells.buff_by_name("Battle Shout (25289)", 0, &raid, all),
        Some(shout7)
    );
    assert!(spells.buff_by_name("Battle Shout", 1, &raid, all).is_none());

    let sunder = spells.buff_by_name("Sunder Armor", 0, &raid, all).unwrap();
    assert_eq!(
        spells.buff_id_of_shared(raid.shared_raid_buff("Sunder Armor (11597)").unwrap()),
        Some(sunder)
    );
    assert!(spells.buff_by_name("Nope", 0, &raid, all).is_none());
}

#[test]
fn charge_consumers_are_active_owned_buffs_listening_to_the_source() {
    let (db, mut spells, mut raid) = setup();
    let flurry = spells
        .add_spell(&db, FLURRY_BUFF, 0, &mut raid)
        .buff
        .unwrap();
    assert!(
        spells
            .charge_consumers(ProcSource::MainhandSwing)
            .is_empty()
    );
    spells.enable_buff(flurry);
    assert!(
        spells
            .charge_consumers(ProcSource::MainhandSwing)
            .is_empty(),
        "inactive buffs have no charges to lose"
    );
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
    spells.queue_next_swing(SpellId(3), Some(BuffId(7)));
    assert_eq!(spells.queued_next_swing(), Some(SpellId(3)));
    assert_eq!(spells.queued_next_swing_marker(), Some(BuffId(7)));
    assert_eq!(spells.cancel_next_swing(), Some(SpellId(3)));
    assert_eq!(spells.queued_next_swing_marker(), None);
    assert_eq!(spells.cancel_next_swing(), None);

    spells.queue_next_swing(SpellId(3), None);
    spells.reset_state();
    assert_eq!(spells.queued_next_swing(), None);
    assert_eq!(spells.auto_attack(Hand::Offhand).last_used(), 0.0);
}

#[test]
fn procs_can_be_taken_out_and_put_back() {
    let (db, mut spells, mut raid) = setup();
    let deep_wounds = spells.add_spell(&db, DEEP_WOUNDS, 0, &mut raid);
    let procs = spells.take_procs();
    assert_eq!(procs.procs().len(), 1);
    assert_eq!(spells.procs().procs().len(), 0);
    spells.put_procs(procs);
    assert_eq!(
        spells.procs().get(deep_wounds.proc.unwrap()).name(),
        "Deep Wounds"
    );
}

#[test]
fn rank_chains_join_one_group() {
    let (db, mut spells, mut raid) = setup();
    let r1 = spells.add_spell(&db, HEROIC_STRIKE_1, 0, &mut raid);
    let r2 = spells.add_spell(&db, HEROIC_STRIKE_2, 0, &mut raid);
    let group = spells.rank_group("Heroic Strike").unwrap();
    assert_eq!(
        group.spells().collect::<Vec<_>>(),
        vec![r1.spell.unwrap(), r2.spell.unwrap()]
    );
    assert_eq!(group.rank_numbers().collect::<Vec<_>>(), vec![1, 2]);
}

/// Every shipped record builds a spell or proc without panicking.
#[test]
fn every_shipped_spell_can_be_added() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/spells");
    let db = SpellDb::load(&dir).expect("shipped spell data loads");
    let mut spells = CharacterSpells::new(CharId(1), 1);
    let mut raid = SharedBuffRegistry::new();
    let mut ids: Vec<u32> = db.records().iter().map(|r| r.id).collect();
    ids.sort_unstable();
    let mut abilities = 0;
    let mut procs = 0;
    for id in ids {
        let added = spells.add_spell(&db, id, 0, &mut raid);
        abilities += usize::from(added.in_rank_group);
        procs += usize::from(added.proc.is_some());
    }
    assert!(abilities > 100, "{abilities} abilities");
    assert!(procs > 5, "{procs} procs");
    assert!(spells.rank_group("Mortal Strike").is_some());
    assert!(spells.rank_group("Heroic Strike").unwrap().len() >= 9);
}
