//! Port of `Test/Warrior/Spells/TestOffhandAttackWarrior`.
//!
//! As for the main hand, the mandatory status tests check that the swing goes through.

use crate::spell::Hand;
use crate::testing::warrior::WarriorTest;

/// The fixture holds the test sword in the main hand: an off hand needs one to swing here.
fn test() -> WarriorTest {
    let mut test = WarriorTest::new("OffhandAttackWarrior");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test
}

fn when_mh_attack_is_performed(test: &mut WarriorTest) {
    test.when_swing_is_performed(Hand::Mainhand);
}

fn when_oh_attack_is_performed(test: &mut WarriorTest) -> u32 {
    if !test.character().has_offhand() {
        test.given_an_offhand_weapon_with_100_min_max_dmg();
    }
    test.when_swing_is_performed(Hand::Offhand).attack.damage
}

fn then_next_expected_use_is(test: &WarriorTest, time: &str) {
    assert_eq!(test.next_expected_use(Hand::Offhand), time);
}

/// A landed off-hand swing of the test sword (a main hand is needed to dual wield).
fn given_a_landing_swing(test: &mut WarriorTest) {
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_hit();
}

#[test]
fn name_correct() {
    let test = test();
    assert_eq!(
        test.character().spells().oh_attack().name(),
        "Offhand Attack"
    );
}

#[test]
fn spell_cooldown() {
    let mut test = test();
    test.given_an_offhand_weapon_with_3_speed();
    when_oh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "3.000");
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    given_a_landing_swing(&mut test);
    test.given_warrior_is_on_gcd();
    assert!(when_oh_attack_is_performed(&mut test) > 0);
}

#[test]
fn resource_cost() {
    let mut test = test();
    given_a_landing_swing(&mut test);
    test.given_warrior_has_rage(0);
    assert!(when_oh_attack_is_performed(&mut test) > 0);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    given_a_landing_swing(&mut test);
    test.when_switching_to_berserker_stance();
    assert!(test.on_stance_cooldown());
    assert!(when_oh_attack_is_performed(&mut test) > 0);
}

#[test]
fn changing_weapons_changes_cooldown() {
    let mut test = test();
    test.given_an_offhand_weapon_with_3_speed();
    when_oh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "3.000");

    test.given_an_offhand_weapon_with_2_speed();
    when_oh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "2.000");
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(test.action_ready());
    when_oh_attack_is_performed(&mut test);
    assert!(test.action_ready());
}

/// An off-hand swing of the 100 - 100, 2.6 speed test sword against an unarmored target,
/// 1000 AP, with `dws` of 5 Dual Wield Specialization.
fn damage(force: fn(&mut WarriorTest), skill: fn(&mut WarriorTest), dws: u32) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    skill(&mut test);
    force(&mut test);
    if dws > 0 {
        test.given_fury_talent_with_rank("Dual Wield Specialization", dws);
    }
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(2);
    when_oh_attack_is_performed(&mut test);
    test.damage_dealt()
}

fn skill_300(test: &mut WarriorTest) {
    test.given_300_weapon_skill_oh();
}

/// The off-hand damage multiplier at `dws` of 5 Dual Wield Specialization: Forever's talent
/// adds 5 % off-hand damage per rank (the C++ 2.5 %).
fn dual_wield_penalty(dws: u32) -> f64 {
    0.5 * (1.0 + 0.05 * f64::from(dws))
}

/// [Damage] = (base_dmg + (wpn_speed * AP / 14)) * dual_wield_penalty
fn expected(dws: u32, crit: bool) -> u64 {
    let hit = 100.0 + 2.6 * 1000.0 / 14.0;
    let crit = if crit { 2.0 } else { 1.0 };
    (hit * crit * dual_wield_penalty(dws)).round() as u64
}

#[test]
fn hit_dmg_dual_wield_specialization() {
    // The C++ values at 0 of 5: [143] = (100 + (2.6 * 1000 / 14)) * 0.5
    assert_eq!(expected(0, false), 143);
    for dws in 0..=5 {
        assert_eq!(
            damage(|t| t.given_a_guaranteed_white_hit(), skill_300, dws),
            expected(dws, false),
            "{dws} of 5 Dual Wield Specialization"
        );
    }
}

#[test]
fn crit_dmg_dual_wield_specialization() {
    // [286] = (100 + (2.6 * 1000 / 14)) * 2 * 0.5
    assert_eq!(expected(0, true), 286);
    for dws in 0..=5 {
        assert_eq!(
            damage(|t| t.given_a_guaranteed_white_crit(), skill_300, dws),
            expected(dws, true),
            "{dws} of 5 Dual Wield Specialization"
        );
    }
}

fn glancing(skill: fn(&mut WarriorTest)) -> u64 {
    damage(|t| t.given_a_guaranteed_white_glancing_blow(), skill, 0)
}

fn assert_in_range(damage: u64, min: u64, max: u64) {
    assert!(
        (min..=max).contains(&damage),
        "expected damage in {min} - {max} but got {damage}"
    );
}

#[test]
fn glancing_damage_300_wpn_skill() {
    // [79 - 107] = (100 + (2.6 * 1000 / 14)) * [0.55 - 0.75] * 0.5
    assert_in_range(glancing(skill_300), 79, 107);
}

#[test]
fn glancing_damage_305_wpn_skill() {
    // [114 - 129] = (100 + (2.6 * 1000 / 14)) * [0.8 - 0.9] * 0.5
    assert_in_range(glancing(|t| t.given_305_weapon_skill_oh()), 114, 129);
}

#[test]
fn glancing_damage_310_wpn_skill() {
    // [130 - 141] = (100 + (2.6 * 1000 / 14)) * [0.91 - 0.99] * 0.5
    assert_in_range(glancing(|t| t.given_310_weapon_skill_oh()), 130, 141);
}

#[test]
fn glancing_damage_315_wpn_skill() {
    // [130 - 141] = (100 + (2.6 * 1000 / 14)) * [0.91 - 0.99] * 0.5
    assert_in_range(glancing(|t| t.given_315_weapon_skill_oh()), 130, 141);
}

#[test]
fn mid_swing_haste_increase_updates_attack_speed() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_3_speed();
    test.given_an_offhand_weapon_with_2_speed();
    when_mh_attack_is_performed(&mut test);
    when_oh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "2.000");

    test.given_engine_priority_at(1.0);
    test.when_increasing_attack_speed(100);
    then_next_expected_use_is(&test, "1.500");
}

#[test]
fn mid_swing_haste_decrease_updates_attack_speed() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_3_speed();
    test.given_an_offhand_weapon_with_3_speed();
    test.when_increasing_attack_speed(100);
    when_mh_attack_is_performed(&mut test);
    when_oh_attack_is_performed(&mut test);

    test.given_engine_priority_at(1.0);
    test.when_decreasing_attack_speed(100);
    then_next_expected_use_is(&test, "2.000");
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_dodge();
    when_oh_attack_is_performed(&mut test);
    test.then_overpower_is_active();
}

/// Rage of one landed off-hand swing of the 2.6 speed test sword with `dws` of 5 Dual Wield
/// Specialization.
fn swing_rage(dws: u32) -> Option<f64> {
    let mut test = test();
    given_a_landing_swing(&mut test);
    if dws > 0 {
        test.given_fury_talent_with_rank("Dual Wield Specialization", dws);
    }
    test.given_warrior_has_rage(0);
    test.when_swing_is_performed(Hand::Offhand).rage_gained
}

/// The off hand generates half the one-hand rate, 1.73 × 2.6 = 4.498 rage; each rank of
/// Dual Wield Specialization adds 10 % of it (patched from the client's 20 %), so 5 of 5 makes
/// 6.747, 75 % of the main hand's 8.996.
#[test]
fn rage_dual_wield_specialization() {
    let rage: Vec<Option<f64>> = (0..=5).map(swing_rage).collect();
    assert_eq!(
        rage,
        [44.0, 49.0, 53.0, 58.0, 62.0, 67.0].map(|t| Some(t / 10.0))
    );
}

/// Rage of one off-hand crit of the 2.6 speed test sword with `dws` of 5 Dual Wield
/// Specialization.
fn crit_rage(dws: u32) -> Option<f64> {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_crit();
    if dws > 0 {
        test.given_fury_talent_with_rank("Dual Wield Specialization", dws);
    }
    test.given_warrior_has_rage(0);
    test.when_swing_is_performed(Hand::Offhand).rage_gained
}

/// An off-hand crit doubles the off-hand rage, Dual Wield Specialization included:
/// 4.498 x (1 + 0.1 x rank) x 2 rage, from 100 % of a main-hand hit's 8.996 at 0/5 to 150 %
/// (13.494) at 5/5.
#[test]
fn offhand_crits_double_the_dual_wield_specialization_rage() {
    let rage: Vec<Option<f64>> = (0..=5).map(crit_rage).collect();
    assert_eq!(
        rage,
        [89.0, 98.0, 107.0, 116.0, 125.0, 134.0].map(|t| Some(t / 10.0))
    );
}

#[test]
fn avoided_offhand_swings_give_no_rage() {
    let mut test = test();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_dodge();
    assert_eq!(
        test.when_swing_is_performed(Hand::Offhand).rage_gained,
        None
    );
}
