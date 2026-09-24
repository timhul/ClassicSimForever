//! Tests of the harness itself.

use super::warrior::WarriorTest;
use super::{items, SpellTest, RUN_EVENT};
use crate::combat_roll::PhysicalAttackResult;
use crate::engine::EventType;
use crate::item::EquipmentSlot;
use crate::stance::Stance;

/// The unarmored, unrounded white damage of the 100 - 100 damage, 2.6 speed test weapon.
fn test_weapon_hit(test: &WarriorTest) -> f64 {
    let view = test.target().stat_view();
    let ap = f64::from(test.character().melee_ap(&view));
    100.0 + 2.6 * ap / 14.0
}

#[test]
fn warrior_setup() {
    let test = WarriorTest::new("setup");
    assert_eq!(test.character().clvl(), 60);
    assert_eq!(test.rage(), 100);
    assert_eq!(test.character().stance(), Stance::Battle);
    assert_eq!(test.now(), 0.0);
    test.given_no_previous_damage_dealt();
    test.then_threat_dealt_is(0);
    assert!(test.raid.engine().queue().is_empty());
}

#[test]
fn test_weapons() {
    let mut test = WarriorTest::new("weapons");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_3_speed();
    assert!(test.character().is_dual_wielding());
    test.given_an_offhand_weapon_with_2_speed();
    test.given_an_offhand_axe();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_a_mainhand_weapon_with_3_speed();
    test.given_a_mainhand_weapon_with_2_speed();
    test.given_a_mainhand_dagger_with_100_min_max_dmg();
    test.given_no_offhand();
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    assert!(!test.character().is_dual_wielding());
    test.given_no_mainhand();
    test.given_a_ranged_weapon_with_100_min_max_dmg();
    test.given_a_ranged_weapon_with_3_speed();
    test.given_a_ranged_weapon_with_2_speed();
}

#[test]
fn real_weapons() {
    let mut test = WarriorTest::new("real weapons");
    test.given_1h_axe_equipped_in_mainhand();
    test.given_1h_mace_equipped_in_mainhand();
    test.given_fist_weapon_equipped_in_mainhand();
    test.given_dagger_equipped_in_mainhand();
    test.given_1h_sword_equipped_in_mainhand();
    test.given_1h_axe_equipped_in_offhand();
    test.given_1h_mace_equipped_in_offhand();
    test.given_fist_weapon_equipped_in_offhand();
    test.given_dagger_equipped_in_offhand();
    test.given_1h_sword_equipped_in_offhand();
    test.given_2h_axe_equipped();
    test.given_2h_mace_equipped();
    test.given_2h_sword_equipped();
    test.given_polearm_equipped();
    test.given_staff_equipped();
}

#[test]
fn weapon_skill_rings() {
    let mut test = WarriorTest::new("weapon skill");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_300_weapon_skill_mh();
    test.given_300_weapon_skill_oh();
    test.given_305_weapon_skill_mh();
    test.given_305_weapon_skill_oh();
    test.given_310_weapon_skill_mh();
    test.given_310_weapon_skill_oh();
    test.given_315_weapon_skill_mh();
    test.given_315_weapon_skill_oh();
    assert_eq!(
        test.character().equipment().item_id(EquipmentSlot::Ring1),
        Some(items::TEST_15_SWORD_SKILL)
    );
}

/// A white hit of the test weapon against an unarmored target, from the swing started at 0.
fn white_swing_damage(force: impl FnOnce(&mut SpellTest)) -> u64 {
    let mut test = WarriorTest::new("white swing");
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    force(&mut test);
    test.given_event_is_ignored(EventType::PlayerAction);
    test.when_starting_attack();
    test.then_next_event_is(EventType::MainhandMeleeHit, "0.000", RUN_EVENT);
    test.then_next_event_is(EventType::MainhandMeleeHit, "2.600", false);
    test.damage_dealt()
}

#[test]
fn guaranteed_white_outcomes() {
    let hit = white_swing_damage(SpellTest::given_a_guaranteed_white_hit);
    let expected = test_weapon_hit(&WarriorTest::new("expected hit"));
    assert_eq!(hit, expected.round() as u64);
    assert_eq!(
        white_swing_damage(SpellTest::given_a_guaranteed_white_crit),
        (2.0 * expected).round() as u64
    );
    let glancing = white_swing_damage(SpellTest::given_a_guaranteed_white_glancing_blow);
    assert!(glancing > 0 && glancing < hit, "{glancing}");
    for force in [
        SpellTest::given_a_guaranteed_white_miss,
        SpellTest::given_a_guaranteed_white_dodge,
        SpellTest::given_a_guaranteed_white_parry,
    ] {
        assert_eq!(white_swing_damage(force), 0);
    }
    assert_eq!(
        white_swing_damage(SpellTest::given_a_guaranteed_white_block),
        hit,
        "the target blocks nothing"
    );
}

#[test]
fn guaranteed_special_outcomes() {
    let whirlwind = |force: fn(&mut SpellTest)| {
        let mut test = WarriorTest::new("whirlwind");
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
        force(&mut test);
        let report = test.cast("Whirlwind");
        let attack = report.attack.expect("Whirlwind rolls");
        assert_eq!(u64::from(attack.damage), test.damage_dealt());
        (attack.result, attack.damage)
    };
    let (result, hit) = whirlwind(SpellTest::given_a_guaranteed_melee_ability_hit);
    assert_eq!(result, PhysicalAttackResult::Hit);
    assert!(hit > 0);
    let (result, crit) = whirlwind(SpellTest::given_a_guaranteed_melee_ability_crit);
    assert_eq!(result, PhysicalAttackResult::Critical);
    assert!(crit > hit, "{crit} > {hit}");
    for (force, expected) in [
        (
            SpellTest::given_a_guaranteed_melee_ability_miss as fn(&mut SpellTest),
            PhysicalAttackResult::Miss,
        ),
        (
            SpellTest::given_a_guaranteed_melee_ability_dodge,
            PhysicalAttackResult::Dodge,
        ),
        (
            SpellTest::given_a_guaranteed_melee_ability_parry,
            PhysicalAttackResult::Parry,
        ),
    ] {
        assert_eq!(whirlwind(force), (expected, 0));
    }
    let (result, blocked) = whirlwind(SpellTest::given_a_guaranteed_melee_ability_block);
    assert_eq!(result, PhysicalAttackResult::Block);
    assert_eq!(blocked, hit, "the target blocks nothing");
}

#[test]
fn stances() {
    let mut test = WarriorTest::new("stances");
    test.given_warrior_in_battle_stance();
    assert_eq!(test.now(), 0.0, "already in Battle Stance");
    test.given_warrior_in_berserker_stance();
    assert_eq!(test.now(), 2.0);
    test.given_warrior_in_defensive_stance();
    test.given_warrior_in_battle_stance();
    test.when_switching_to_berserker_stance();
    assert_eq!(test.character().stance(), Stance::Berserker);
}

#[test]
fn overpower_activation() {
    let mut test = WarriorTest::new("overpower");
    test.then_overpower_is_inactive();
    test.given_overpower_is_active();
    assert_eq!(
        test.character().combo_points(),
        1,
        "one dodge, one combo point (the ranks react once between them)"
    );
    assert_eq!(test.rage(), 100, "the GCD is rage neutral");
    assert!(!test.character().on_global_cooldown(test.now()));
}

#[test]
fn gcd_and_rage() {
    let mut test = WarriorTest::new("gcd");
    test.given_warrior_has_rage(30);
    test.given_warrior_is_on_gcd();
    test.then_warrior_has_rage(30);
    assert!(test.character().on_global_cooldown(test.now()));
    test.given_warrior_has_rage(0);
    test.then_warrior_has_rage(0);
}

#[test]
fn talent_helpers() {
    let mut test = WarriorTest::new("talents");
    test.given_impale(2);
    test.given_tactical_mastery(5);
    // Deep in the tree without the tiers above it.
    test.given_fury_talent_with_rank("Enrage", 5);
    test.given_fury_talent_with_rank("Flurry", 3);
    test.given_talent_ranks("Arms", &[("Deflection", 5), ("Improved Rend", 2)]);
    let talents = test.character().talents().unwrap();
    let flurry = talents.node_of_name("Flurry", None).unwrap();
    assert_eq!(talents.rank(flurry), 3);
    let rend = talents.node_of_name("Improved Rend", None).unwrap();
    assert_eq!(talents.rank(rend), 2);
}

#[test]
#[should_panic(expected = "does it have parent talents")]
fn talent_helper_needs_the_prerequisite() {
    let mut test = WarriorTest::new("prerequisite");
    // Mortal Strike requires Sweeping Strikes.
    test.given_arms_talent_with_rank("Mortal Strike", 1);
}

#[test]
fn engine_helpers() {
    let mut test = WarriorTest::new("engine");
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_hit();
    test.given_event_is_ignored(EventType::PlayerAction);
    test.given_engine_priority_at(5.0);
    assert_eq!(test.now(), 5.0);
    test.when_starting_attack();
    test.when_running_queued_events_until(10.2);
    assert!(test.now() > 7.6 && test.now() < 10.2, "{}", test.now());
    let hit = test_weapon_hit(&test).round() as u64;
    test.then_damage_dealt_is(2 * hit);
    test.then_next_event_is(EventType::MainhandMeleeHit, "10.200", false);

    test.given_event_is_ignored(EventType::MainhandMeleeHit);
    test.given_engine_priority_pushed_forward(1.0);
    assert!((test.now() - 11.2).abs() < 1e-9);
    assert!(test.queued_events().is_empty());
    test.given_engine_priority_at(0.0);
    assert_eq!(test.now(), 0.0);

    let mut test = WarriorTest::new("ignored");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_event_is_ignored(EventType::MainhandMeleeHit);
    test.when_starting_attack();
    test.given_warrior_is_on_gcd();
    let damage = test.damage_dealt();
    let queued = test.queued_events();
    assert_eq!(queued[0].kind.event_type(), EventType::MainhandMeleeHit);
    assert_eq!(queued[0].time, 0.0);
    assert!(queued
        .iter()
        .any(|e| e.kind.event_type() == EventType::PlayerAction && e.time == 1.5));
    // The swing at 0 is dropped: no white damage.
    test.when_running_queued_events_until(1.5);
    test.then_damage_dealt_is(damage);
    test.then_next_event_is(EventType::PlayerAction, "1.500", RUN_EVENT);
}
