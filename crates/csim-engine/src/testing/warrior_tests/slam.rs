//! Port of `Test/Warrior/Spells/TestSlam`.

use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;
use crate::testing::RUN_EVENT;

const SPELL: &str = "Slam";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

/// Forever's Improved Slam is an Arms talent of 2 ranks (the C++ a Fury talent of 5).
fn given_improved_slam(test: &mut WarriorTest, rank: u32) {
    test.given_arms_talent_with_rank("Improved Slam", rank);
}

/// Starts the Slam cast, then drops the queued events up to its completion and completes it.
fn when_slam_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.cast(SPELL);
    test.when_running_until_event(EventType::CastComplete);
}

fn cast_time(test: &mut WarriorTest) -> f64 {
    let id = test.spell(SPELL);
    test.with_ctx(|ctx| ctx.with_spell(id, |spell, ctx| spell.cast_time(ctx)))
}

#[test]
fn name_correct() {
    let mut test = test();
    let id = test.spell(SPELL);
    assert_eq!(test.character().spells().spell(id).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    let mut test = test();
    test.given_a_guaranteed_melee_ability_hit();
    // Forever's Slam has an 18 s category cooldown (the C++ none).
    assert_eq!(test.base_cooldown(SPELL), "18.000");
}

#[test]
fn improved_slam_reduces_the_cooldown() {
    for (rank, cooldown) in [(1, "16.500"), (2, "15.000")] {
        let mut test = test();
        given_improved_slam(&mut test, rank);
        assert_eq!(test.base_cooldown(SPELL), cooldown);
    }
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_improved_slam(&mut test, 1);
    test.cast(SPELL);
    // Improved Slam shortens the GCD along with the cast (the C++ kept the 1.5 s GCD).
    test.then_next_event_is(EventType::PlayerAction, "1.250", false);
    test.then_next_event_is(EventType::CastComplete, "1.250", false);
    // The category cooldown, 1.5 s shorter with Improved Slam.
    test.then_next_event_is(EventType::PlayerAction, "16.500", false);
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd();

    test.then_status_is(SPELL, SpellStatus::OnGcd);
    assert_eq!(test.cooldown_remaining(SPELL), 0.0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(15);
    when_slam_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.when_switching_to_berserker_stance();
    test.given_warrior_has_rage(100);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnGcd);

    test.given_engine_priority_pushed_forward(0.99);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnStanceCooldown);

    test.given_engine_priority_pushed_forward(0.02);
    assert!(!test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);
}

/// Slam with the 100 - 100 two-hander against an unarmored target, 1000 AP.
fn damage(crit: bool, impale: u32) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    if crit {
        test.given_a_guaranteed_melee_ability_crit();
    } else {
        test.given_a_guaranteed_melee_ability_hit();
    }
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(impale);
    when_slam_is_performed(&mut test);
    test.damage_dealt()
}

// Forever's Slam uses the weapon's own speed plus 87 (the C++ comments mixed a normalized
// 3.3 speed and 87 or 160 flat damage).

/// [Damage] = (base_dmg + (wpn_speed * AP / 14) + 87) * crit_dmg_modifier
fn expected(crit_dmg_modifier: f64) -> u64 {
    ((100.0 + 3.5 * 1000.0 / 14.0 + 87.0) * crit_dmg_modifier).round() as u64
}

#[test]
fn hit_dmg() {
    assert_eq!(damage(false, 2), expected(1.0));
}

#[test]
fn crit_dmg_0_of_2_impale() {
    assert_eq!(damage(true, 0), expected(2.0));
}

#[test]
fn crit_dmg_1_of_2_impale() {
    assert_eq!(damage(true, 1), expected(2.1));
}

#[test]
fn crit_dmg_2_of_2_impale() {
    assert_eq!(damage(true, 2), expected(2.2));
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    when_slam_is_performed(&mut test);
    test.then_overpower_is_active();
}

#[test]
fn cast_time_with_improved_slam() {
    // Forever: 1.5 s, and 0.25 s less per rank of Improved Slam (the C++ 0.1 s of 5).
    assert_eq!(cast_time(&mut test()), 1.5);
    for (rank, time) in [(1, 1.25), (2, 1.0)] {
        let mut test = test();
        given_improved_slam(&mut test, rank);
        assert!((cast_time(&mut test) - time).abs() < 1e-9, "{rank} of 2");
    }
}

/// Dual wields the 2 and 3 speed test swords, attacks from 0 and starts a Slam at 1.
fn given_slam_cast_while_attacking(improved_slam: u32) -> WarriorTest {
    let mut test = test();
    if improved_slam > 0 {
        given_improved_slam(&mut test, improved_slam);
    }
    test.given_a_mainhand_weapon_with_2_speed();
    test.given_an_offhand_weapon_with_3_speed();
    test.given_a_guaranteed_white_hit();
    test.when_starting_attack();
    test.then_next_event_is(EventType::MainhandMeleeHit, "0.000", RUN_EVENT);
    test.then_next_event_is(EventType::OffhandMeleeHit, "0.000", RUN_EVENT);
    // One reaction after the swings (the C++ queued one per hand).
    test.then_next_event_is(EventType::PlayerAction, "0.100", false);

    test.given_engine_priority_at(1.0);
    assert!(test.character().spells().is_melee_attacking());
    test.cast(SPELL);
    assert!(!test.character().spells().is_melee_attacking());
    test
}

#[test]
fn auto_attacks_cancelled_during_slam_cast() {
    // Without Improved Slam (the C++ test had 2 of 5, which only shortened the cast): the
    // cast takes 1.5 s and restarts both swing timers when it completes.
    let mut test = given_slam_cast_while_attacking(0);
    test.then_next_event_is(EventType::MainhandMeleeHit, "2.000", RUN_EVENT);
    test.then_next_event_is(EventType::PlayerAction, "2.500", false);
    test.then_next_event_is(EventType::CastComplete, "2.500", RUN_EVENT);
    assert!(test.character().spells().is_melee_attacking());
    test.then_next_event_is(EventType::PlayerAction, "2.600", false);
    test.then_next_event_is(EventType::OffhandMeleeHit, "3.000", RUN_EVENT);
    test.then_next_event_is(EventType::MainhandMeleeHit, "4.500", false);
    test.then_next_event_is(EventType::OffhandMeleeHit, "5.500", false);
}

#[test]
fn improved_slam_keeps_the_swing_timers() {
    // Forever's Improved Slam replaces Slam with a rank that does not interrupt the swing
    // timer: the main-hand swing that came due during the cast lands when it completes.
    let mut test = given_slam_cast_while_attacking(1);
    test.given_event_is_ignored(EventType::PlayerAction);
    test.then_next_event_is(EventType::MainhandMeleeHit, "2.000", RUN_EVENT);
    test.then_next_event_is(EventType::CastComplete, "2.250", RUN_EVENT);
    assert!(test.character().spells().is_melee_attacking());
    test.then_next_event_is(EventType::MainhandMeleeHit, "2.250", RUN_EVENT);
    // The off hand keeps its swing at 3 (the one queued before the cast is stale).
    test.then_next_event_is(EventType::OffhandMeleeHit, "3.000", RUN_EVENT);
    test.then_next_event_is(EventType::OffhandMeleeHit, "3.000", RUN_EVENT);
    test.then_next_event_is(EventType::MainhandMeleeHit, "4.250", false);
}
