//! Port of `Test/Warrior/Spells/TestMainhandAttackWarrior`.
//!
//! The auto attack is not a spell here and has no status: the mandatory status tests check
//! that the swing goes through (the C++ checked that its status stayed `Available`).

use crate::spell::Hand;
use crate::testing::warrior::WarriorTest;

fn test() -> WarriorTest {
    WarriorTest::new("MainhandAttackWarrior")
}

fn when_mh_attack_is_performed(test: &mut WarriorTest) -> u32 {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.when_swing_is_performed(Hand::Mainhand).attack.damage
}

fn then_next_expected_use_is(test: &WarriorTest, time: &str) {
    assert_eq!(test.next_expected_use(Hand::Mainhand), time);
}

/// A landed swing of the test sword.
fn given_a_landing_swing(test: &mut WarriorTest) {
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_hit();
}

#[test]
fn name_correct() {
    let test = test();
    assert_eq!(
        test.character().spells().mh_attack().name(),
        "Mainhand Attack"
    );
}

#[test]
fn spell_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_3_speed();
    when_mh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "3.000");
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    given_a_landing_swing(&mut test);
    test.given_warrior_is_on_gcd();
    assert!(when_mh_attack_is_performed(&mut test) > 0);
}

#[test]
fn resource_cost() {
    let mut test = test();
    given_a_landing_swing(&mut test);
    test.given_warrior_has_rage(0);
    assert!(when_mh_attack_is_performed(&mut test) > 0);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    given_a_landing_swing(&mut test);
    test.when_switching_to_berserker_stance();
    assert!(test.on_stance_cooldown());
    assert!(when_mh_attack_is_performed(&mut test) > 0);
}

#[test]
fn changing_weapons_changes_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_3_speed();
    when_mh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "3.000");

    test.given_a_mainhand_weapon_with_2_speed();
    when_mh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "2.000");
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(test.action_ready());
    when_mh_attack_is_performed(&mut test);
    assert!(test.action_ready());
}

/// A white swing of the 100 - 100, 2.6 speed test sword against an unarmored target, 1000 AP.
fn damage(force: fn(&mut WarriorTest), skill: fn(&mut WarriorTest)) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    skill(&mut test);
    force(&mut test);
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(2);
    when_mh_attack_is_performed(&mut test);
    test.damage_dealt()
}

fn skill_300(test: &mut WarriorTest) {
    test.given_300_weapon_skill_mh();
}

#[test]
fn hit_dmg() {
    // [Damage] = base_dmg + (wpn_speed * AP / 14)
    // [286] = 100 + (2.6 * 1000 / 14)
    assert_eq!(damage(|t| t.given_a_guaranteed_white_hit(), skill_300), 286);
}

#[test]
fn crit_dmg() {
    // Impale does not touch white crits.
    // [571] = (100 + (2.6 * 1000 / 14)) * 2.0
    assert_eq!(
        damage(|t| t.given_a_guaranteed_white_crit(), skill_300),
        571
    );
}

fn glancing(skill: fn(&mut WarriorTest)) -> u64 {
    damage(|t| t.given_a_guaranteed_white_glancing_blow(), skill)
}

fn assert_in_range(damage: u64, min: u64, max: u64) {
    assert!(
        (min..=max).contains(&damage),
        "expected damage in {min} - {max} but got {damage}"
    );
}

#[test]
fn glancing_damage_300_wpn_skill() {
    // [157 - 214] = (100 + (2.6 * 1000 / 14)) * [0.55 - 0.75]
    assert_in_range(glancing(skill_300), 157, 214);
}

#[test]
fn glancing_damage_305_wpn_skill() {
    // [229 - 257] = (100 + (2.6 * 1000 / 14)) * [0.8 - 0.9]
    assert_in_range(glancing(|t| t.given_305_weapon_skill_mh()), 229, 257);
}

#[test]
fn glancing_damage_310_wpn_skill() {
    // [260 - 283] = (100 + (2.6 * 1000 / 14)) * [0.91 - 0.99]
    assert_in_range(glancing(|t| t.given_310_weapon_skill_mh()), 260, 283);
}

#[test]
fn glancing_damage_315_wpn_skill() {
    // [260 - 283] = (100 + (2.6 * 1000 / 14)) * [0.91 - 0.99]
    assert_in_range(glancing(|t| t.given_315_weapon_skill_mh()), 260, 283);
}

#[test]
fn mid_swing_haste_increase_updates_attack_speed() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_2_speed();
    test.given_no_offhand();
    when_mh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "2.000");

    test.given_engine_priority_at(1.0);
    test.when_increasing_attack_speed(100);
    then_next_expected_use_is(&test, "1.500");
}

#[test]
fn mid_swing_haste_decrease_updates_attack_speed() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_3_speed();
    test.given_no_offhand();
    test.when_increasing_attack_speed(100);
    when_mh_attack_is_performed(&mut test);
    then_next_expected_use_is(&test, "1.500");

    test.given_engine_priority_at(1.0);
    test.when_decreasing_attack_speed(100);
    then_next_expected_use_is(&test, "2.000");
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_white_dodge();
    when_mh_attack_is_performed(&mut test);
    test.then_overpower_is_active();
}

/// Rage of one main-hand swing of the weapon `equip` puts on, with the outcome `force` picks.
fn swing_rage(equip: fn(&mut WarriorTest), force: fn(&mut WarriorTest)) -> Option<f64> {
    let mut test = test();
    equip(&mut test);
    force(&mut test);
    test.given_warrior_has_rage(0);
    test.when_swing_is_performed(Hand::Mainhand).rage_gained
}

fn sword(test: &mut WarriorTest) {
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
}

/// Forever swing rage (magey/forever-warrior#3): 3.46 per second of weapon speed for a
/// one-hander, whatever the damage; 3.46 × 2.6 = 8.996 rage lands as 89 tenths. A crit gives
/// double: 17.992 rage.
#[test]
fn landed_swings_give_rage_by_weapon_speed() {
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_hit()),
        Some(8.9)
    );
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_crit()),
        Some(17.9),
        "100 % crit bonus"
    );
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_glancing_blow()),
        Some(8.9),
        "glancing blows give full rage"
    );
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_block()),
        Some(8.9)
    );
    assert_eq!(
        swing_rage(sword, |t| {
            t.given_a_guaranteed_white_hit();
            t.given_1000_melee_ap();
        }),
        Some(8.9),
        "attack power does not change the rage"
    );
}

#[test]
fn avoided_swings_give_no_rage() {
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_miss()),
        None
    );
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_dodge()),
        None
    );
    assert_eq!(
        swing_rage(sword, |t| t.given_a_guaranteed_white_parry()),
        None
    );
}

/// Two-handers: 4.5 per second, 4.5 × 3.5 = 15.75 rage; a crit 15.75 × 2 = 31.5.
#[test]
fn two_handers_give_more_rage_per_second() {
    assert_eq!(
        swing_rage(
            |t| t.given_a_twohand_weapon_with_100_min_max_dmg(),
            |t| t.given_a_guaranteed_white_hit()
        ),
        Some(15.7)
    );
    assert_eq!(
        swing_rage(
            |t| t.given_a_twohand_weapon_with_100_min_max_dmg(),
            |t| t.given_a_guaranteed_white_crit()
        ),
        Some(31.5)
    );
}

/// Haste shortens the swing but not the rage of a swing (the base weapon speed counts).
#[test]
fn hasted_swings_give_base_speed_rage() {
    assert_eq!(
        swing_rage(sword, |t| {
            t.given_a_guaranteed_white_hit();
            t.when_increasing_attack_speed(30);
        }),
        Some(8.9)
    );
}
