//! Port of `Test/Warrior/Buffs/TestFlurryWarrior`.
//!
//! Forever's Flurry gives 5 % attack speed per rank, 5 - 25 % (the C++ 10 - 30 %). The C++
//! followed the swing events; the swing timers here say the same without the re-timed swing
//! events the queue also holds.

use crate::engine::EventType;
use crate::ids::BuffId;
use crate::spell::Hand;
use crate::testing::RUN_EVENT;
use crate::testing::warrior::WarriorTest;

fn test() -> WarriorTest {
    WarriorTest::new("FlurryWarrior")
}

/// The attack speed `rank` of 5 Flurry gives.
fn haste(rank: u32) -> f64 {
    0.05 * f64::from(rank)
}

/// `rank` of 5 Flurry, behind its prerequisite Enrage. Preparing the iterations drops the
/// forced tables: force outcomes after.
fn given_flurry(test: &mut WarriorTest, rank: u32) {
    test.given_fury_talent_with_rank("Death Wish", 1);
    test.given_fury_talent_with_rank("Flurry", rank);
    test.prepare_set_of_combat_iterations();
}

fn given_flurry_enabled(test: &mut WarriorTest) {
    given_flurry(test, 1);
    test.proc("Flurry");
}

fn flurry_is_active(test: &mut WarriorTest) -> bool {
    let flurry = test.flurry();
    test.with_buff_id(flurry, |buff| buff.is_active())
}

fn when_flurry_is_applied(test: &mut WarriorTest) -> BuffId {
    assert!(!flurry_is_active(test));
    let flurry = test.flurry();
    test.apply_buff_id(flurry);
    assert!(flurry_is_active(test));
    flurry
}

fn when_flurry_is_removed(test: &mut WarriorTest) {
    let flurry = test.flurry();
    test.cancel_buff_id(flurry);
    assert!(!flurry_is_active(test));
}

fn when_performing_mh_attack(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.when_swing_is_performed(Hand::Mainhand);
}

fn when_performing_oh_attack(test: &mut WarriorTest) {
    test.when_swing_is_performed(Hand::Offhand);
}

fn when_performing_attack(test: &mut WarriorTest, name: &str) {
    test.cast(name);
}

fn given_a_mainhand_and_offhand_equipped(test: &mut WarriorTest) {
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
}

fn time(seconds: f64) -> String {
    format!("{seconds:.3}")
}

#[test]
fn name_correct() {
    let mut test = test();
    given_flurry_enabled(&mut test);
    let flurry = test.flurry();
    assert_eq!(
        test.with_buff_id(flurry, |buff| buff.name().to_string()),
        "Flurry"
    );
}

#[test]
fn has_15_second_duration() {
    let mut test = test();
    given_a_mainhand_and_offhand_equipped(&mut test);
    given_flurry_enabled(&mut test);
    when_flurry_is_applied(&mut test);
    test.then_next_event_is(EventType::BuffRemoval, "15.000", false);
}

#[test]
fn has_3_charges() {
    let mut test = test();
    given_flurry_enabled(&mut test);
    let flurry = when_flurry_is_applied(&mut test);
    assert_eq!(test.with_buff_id(flurry, |buff| buff.charges()), 3);
}

#[test]
fn swing_that_applies_flurry_does_not_use_a_charge() {
    for (hand, attack) in [
        ("main hand", mh_attack as fn(&mut WarriorTest)),
        ("off hand", oh_attack),
    ] {
        let mut test = test();
        given_a_mainhand_and_offhand_equipped(&mut test);
        given_flurry_enabled(&mut test);
        test.given_a_guaranteed_white_crit();
        attack(&mut test);
        assert!(flurry_is_active(&mut test), "{hand}");
        let flurry = test.flurry();
        assert_eq!(
            test.with_buff_id(flurry, |buff| buff.charges()),
            3,
            "{hand}"
        );
    }
}

/// A crit uses a charge of the Flurry already up, then refreshes it to three charges.
#[test]
fn crit_refreshes_flurry_to_3_charges() {
    let mut test = test();
    given_a_mainhand_and_offhand_equipped(&mut test);
    given_flurry_enabled(&mut test);
    let flurry = test.flurry();
    test.given_a_guaranteed_white_crit();
    when_performing_mh_attack(&mut test);
    test.given_a_guaranteed_white_hit();
    when_performing_oh_attack(&mut test);
    assert_eq!(test.with_buff_id(flurry, |buff| buff.charges()), 2);
    test.given_a_guaranteed_white_crit();
    when_performing_oh_attack(&mut test);
    assert_eq!(test.with_buff_id(flurry, |buff| buff.charges()), 3);
}

#[test]
fn attack_speed_increased_when_flurry_applied() {
    for rank in 1..=5 {
        let mut test = test();
        test.given_a_mainhand_weapon_with_3_speed();
        test.given_an_offhand_weapon_with_2_speed();
        given_flurry(&mut test, rank);
        test.given_a_guaranteed_white_crit();
        test.when_starting_attack();
        test.given_event_is_ignored(EventType::PlayerAction);

        // The main hand crits and applies Flurry, which re-times the off-hand swing due now.
        test.then_next_event_is(EventType::MainhandMeleeHit, "0.000", RUN_EVENT);
        test.then_next_event_is(EventType::OffhandMeleeHit, "0.000", RUN_EVENT);
        test.then_next_event_is(EventType::OffhandMeleeHit, "0.000", RUN_EVENT);

        let speed = 1.0 + haste(rank);
        assert_eq!(
            test.next_expected_use(Hand::Offhand),
            time(2.0 / speed),
            "{rank} of 5"
        );
        assert_eq!(
            test.next_expected_use(Hand::Mainhand),
            time(3.0 / speed),
            "{rank} of 5"
        );
    }
}

#[test]
fn attack_speed_decreased_when_flurry_removed() {
    for rank in 1..=5 {
        let mut test = test();
        test.given_event_is_ignored(EventType::PlayerAction);
        test.given_a_mainhand_weapon_with_3_speed();
        test.given_an_offhand_weapon_with_2_speed();
        given_flurry(&mut test, rank);
        // Flurry comes from a crit so that it has the haste of its rank (applying the buff
        // directly gives the record's 30 %): the off hand swings, then the main hand crits.
        test.given_a_guaranteed_white_hit();
        when_performing_oh_attack(&mut test);
        test.given_a_guaranteed_white_crit();
        when_performing_mh_attack(&mut test);
        assert!(flurry_is_active(&mut test));
        test.given_a_guaranteed_white_hit();

        let speed = 1.0 + haste(rank);
        let oh = 2.0 / speed;
        assert_eq!(
            test.next_expected_use(Hand::Offhand),
            time(oh),
            "{rank} of 5"
        );
        test.given_engine_priority_at(oh);
        when_performing_oh_attack(&mut test);

        let mh = 3.0 / speed;
        assert_eq!(
            test.next_expected_use(Hand::Mainhand),
            time(mh),
            "{rank} of 5"
        );
        test.given_engine_priority_at(mh);
        when_performing_mh_attack(&mut test);
        // The crit that applied Flurry kept its three charges: one is left.
        assert!(flurry_is_active(&mut test));

        let oh_2 = 2.0 * oh;
        assert_eq!(
            test.next_expected_use(Hand::Offhand),
            time(oh_2),
            "{rank} of 5"
        );
        test.given_engine_priority_at(oh_2);
        // The swing uses the last of the three charges: Flurry falls off.
        when_performing_oh_attack(&mut test);
        assert!(!flurry_is_active(&mut test));

        // updated swing = curr_time + (curr_expected_use - curr_time) * haste_change
        let mh_after = oh_2 + (2.0 * mh - oh_2) * speed;
        assert_eq!(
            test.next_expected_use(Hand::Mainhand),
            time(mh_after),
            "{rank} of 5"
        );
        assert_eq!(
            test.next_expected_use(Hand::Offhand),
            time(oh_2 + 2.0),
            "{rank} of 5"
        );
    }
}

#[test]
fn attack_speed_decreased_values_of_the_cpp_tests() {
    // The C++ expectation at 1 of 5 (10 % haste): 3.727 = 2.727 + (3.636 - 2.727) * 1.1.
    let (speed, oh, mh) = (1.1, 2.0 / 1.1, 3.0 / 1.1);
    assert_eq!(time(mh + (2.0 * oh - mh) * speed), "3.727");
}

/// Whether Flurry is up after `attack` with the outcome `force`.
fn applies(force: fn(&mut WarriorTest), attack: fn(&mut WarriorTest)) -> bool {
    let mut test = test();
    given_a_mainhand_and_offhand_equipped(&mut test);
    given_flurry_enabled(&mut test);
    assert!(!flurry_is_active(&mut test));
    force(&mut test);
    attack(&mut test);
    flurry_is_active(&mut test)
}

fn white_crit(test: &mut WarriorTest) {
    test.given_a_guaranteed_white_crit();
}

fn white_hit(test: &mut WarriorTest) {
    test.given_a_guaranteed_white_hit();
}

fn ability_crit(test: &mut WarriorTest) {
    test.given_a_guaranteed_melee_ability_crit();
}

fn ability_hit(test: &mut WarriorTest) {
    test.given_a_guaranteed_melee_ability_hit();
}

fn mh_attack(test: &mut WarriorTest) {
    when_performing_mh_attack(test);
}

fn oh_attack(test: &mut WarriorTest) {
    when_performing_oh_attack(test);
}

fn bloodthirst(test: &mut WarriorTest) {
    when_performing_attack(test, "Bloodthirst");
}

fn whirlwind(test: &mut WarriorTest) {
    when_performing_attack(test, "Whirlwind");
}

fn heroic_strike(test: &mut WarriorTest) {
    test.when_next_swing_spell_lands("Heroic Strike");
}

fn overpower(test: &mut WarriorTest) {
    let now = test.now();
    test.character_mut().gain_combo_points(1, now);
    when_performing_attack(test, "Overpower");
}

fn mortal_strike(test: &mut WarriorTest) {
    when_performing_attack(test, "Mortal Strike");
}

#[test]
fn critical_attacks_apply_flurry() {
    assert!(applies(white_crit, mh_attack), "main hand");
    assert!(applies(white_crit, oh_attack), "off hand");
    assert!(applies(ability_crit, bloodthirst), "Bloodthirst");
    assert!(applies(ability_crit, whirlwind), "Whirlwind");
    assert!(applies(ability_crit, heroic_strike), "Heroic Strike");
    assert!(applies(ability_crit, overpower), "Overpower");
    assert!(applies(ability_crit, mortal_strike), "Mortal Strike");
}

#[test]
fn regular_hits_do_not_apply_flurry() {
    assert!(!applies(white_hit, mh_attack), "main hand");
    assert!(!applies(white_hit, oh_attack), "off hand");
    assert!(!applies(ability_hit, bloodthirst), "Bloodthirst");
    assert!(!applies(ability_hit, whirlwind), "Whirlwind");
    assert!(!applies(ability_hit, heroic_strike), "Heroic Strike");
    assert!(!applies(ability_hit, overpower), "Overpower");
    assert!(!applies(ability_hit, mortal_strike), "Mortal Strike");
}
