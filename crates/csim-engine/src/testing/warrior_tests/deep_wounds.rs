//! Port of `Test/Warrior/Spells/TestDeepWounds` (set up without preparing the iterations).
//!
//! Deep Wounds is a talent proc here, not a spell with a status: the mandatory status tests
//! check that the proc runs whatever the GCD, the rage or the stance cooldown.

use crate::engine::EventType;
use crate::spell::Hand;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Deep Wounds";

fn test() -> WarriorTest {
    WarriorTest::unprepared(SPELL)
}

/// `rank` of 3 Deep Wounds, behind its prerequisite Improved Rend.
fn given_deep_wounds(test: &mut WarriorTest, rank: u32) {
    test.given_arms_talent_with_rank("Improved Rend", 3);
    test.given_arms_talent_with_rank(SPELL, rank);
    test.prepare_set_of_combat_iterations();
}

fn given_deep_wounds_enabled(test: &mut WarriorTest) {
    given_deep_wounds(test, 1);
}

fn when_mh_attack_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.when_swing_is_performed(Hand::Mainhand);
}

fn when_attack_is_performed(test: &mut WarriorTest, name: &str) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.cast(name);
}

/// Runs every queued bleed tick (dropping the other events) and returns the Deep Wounds damage.
fn deep_wounds_damage(test: &mut WarriorTest) -> u64 {
    test.when_running_only(EventType::DotTick);
    test.damage_dealt_by(SPELL)
}

fn then_deep_wounds_is_applied(test: &mut WarriorTest) {
    assert!(deep_wounds_damage(test) > 0, "Deep Wounds is not applied");
}

fn then_deep_wounds_is_not_applied(test: &mut WarriorTest) {
    assert_eq!(deep_wounds_damage(test), 0, "Deep Wounds is applied");
}

// ---------------------------------------------------------------- mandatory

#[test]
fn name_correct() {
    let mut test = test();
    given_deep_wounds_enabled(&mut test);
    let proc = test.proc(SPELL);
    assert_eq!(test.character().spells().procs().get(proc).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_deep_wounds_enabled(&mut test);
    test.given_a_guaranteed_white_crit();
    test.given_1000_melee_ap();
    when_mh_attack_is_performed(&mut test);
    assert!(test.action_ready());
}

/// A critical swing of the test sword, with the state `given` set up first, applies Deep
/// Wounds.
fn applied_after(given: fn(&mut WarriorTest)) {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_deep_wounds_enabled(&mut test);
    given(&mut test);
    test.given_a_guaranteed_white_crit();
    when_mh_attack_is_performed(&mut test);
    then_deep_wounds_is_applied(&mut test);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_deep_wounds_enabled(&mut test);
    test.given_a_guaranteed_white_crit();
    when_mh_attack_is_performed(&mut test);
    assert!(!test.on_global_cooldown());
}

#[test]
fn how_spell_observes_global_cooldown() {
    applied_after(|t| t.given_warrior_is_on_gcd());
}

#[test]
fn resource_cost() {
    applied_after(|t| t.given_warrior_has_rage(0));
}

#[test]
fn stance_cooldown() {
    applied_after(|t| {
        t.when_switching_to_berserker_stance();
        assert!(t.on_stance_cooldown());
    });
}

// ---------------------------------------------------------------- what applies it

/// Deep Wounds after `attack` with the outcome `force`.
fn applies(force: fn(&mut WarriorTest), attack: fn(&mut WarriorTest)) -> u64 {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_deep_wounds_enabled(&mut test);
    force(&mut test);
    test.given_no_previous_damage_dealt();
    attack(&mut test);
    deep_wounds_damage(&mut test)
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
    when_mh_attack_is_performed(test);
}

fn bloodthirst(test: &mut WarriorTest) {
    test.enable_spell("Bloodthirst");
    when_attack_is_performed(test, "Bloodthirst");
}

fn whirlwind(test: &mut WarriorTest) {
    when_attack_is_performed(test, "Whirlwind");
}

fn heroic_strike(test: &mut WarriorTest) {
    test.when_next_swing_spell_lands("Heroic Strike");
}

fn overpower(test: &mut WarriorTest) {
    when_attack_is_performed(test, "Overpower");
}

#[test]
fn critical_mh_attack_applies_deep_wounds() {
    assert!(applies(white_crit, mh_attack) > 0);
}

#[test]
fn critical_oh_attack_applies_deep_wounds() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    given_deep_wounds_enabled(&mut test);
    test.given_a_guaranteed_white_crit();
    test.when_swing_is_performed(Hand::Offhand);
    then_deep_wounds_is_applied(&mut test);
}

#[test]
fn critical_bloodthirst_applies_deep_wounds() {
    assert!(applies(ability_crit, bloodthirst) > 0);
}

#[test]
fn critical_whirlwind_applies_deep_wounds() {
    assert!(applies(ability_crit, whirlwind) > 0);
}

#[test]
fn critical_heroic_strike_applies_deep_wounds() {
    assert!(applies(ability_crit, heroic_strike) > 0);
}

#[test]
fn critical_overpower_applies_deep_wounds() {
    assert!(applies(ability_crit, overpower) > 0);
}

#[test]
fn regular_hit_mh_attack_does_not_apply_deep_wounds() {
    assert_eq!(applies(white_hit, mh_attack), 0);
}

#[test]
fn regular_hit_oh_attack_does_not_apply_deep_wounds() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    given_deep_wounds_enabled(&mut test);
    test.given_a_guaranteed_white_hit();
    test.when_swing_is_performed(Hand::Offhand);
    then_deep_wounds_is_not_applied(&mut test);
}

#[test]
fn regular_hit_bloodthirst_does_not_apply_deep_wounds() {
    assert_eq!(applies(ability_hit, bloodthirst), 0);
}

#[test]
fn regular_hit_whirlwind_does_not_apply_deep_wounds() {
    assert_eq!(applies(ability_hit, whirlwind), 0);
}

#[test]
fn regular_hit_heroic_strike_does_not_apply_deep_wounds() {
    assert_eq!(applies(ability_hit, heroic_strike), 0);
}

#[test]
fn regular_hit_overpower_does_not_apply_deep_wounds() {
    assert_eq!(applies(ability_hit, overpower), 0);
}

// ---------------------------------------------------------------- damage

/// Deep Wounds at `rank` after `crits` critical swings of the test sword with 1000 AP: its
/// damage and when the last tick came.
fn damage(rank: u32, crits: u32) -> (u64, String) {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    // Preparing the iterations drops the forced tables: talents first.
    given_deep_wounds(&mut test, rank);
    test.given_a_guaranteed_white_crit();
    test.given_1000_melee_ap();
    for _ in 0..crits {
        when_mh_attack_is_performed(&mut test);
    }
    let damage = deep_wounds_damage(&mut test);
    (damage, format!("{:.3}", test.now()))
}

// total_deep_wounds_damage = (avg_mh_wpn_dmg + (mh_wpn_speed * melee_ap / 14)) * deep_wounds_percent
// [57 / 114 / 171] = (100 + (2.6 * 1000 / 14)) * [0.2 / 0.4 / 0.6], over four ticks

#[test]
fn damage_of_1_of_3_deep_wounds() {
    assert_eq!(damage(1, 1), (57, "12.000".to_string()));
}

#[test]
fn damage_of_2_of_3_deep_wounds() {
    assert_eq!(damage(2, 1), (114, "12.000".to_string()));
}

#[test]
fn damage_of_3_of_3_deep_wounds() {
    // Each of the four ticks rounds 42.86 up: 172, as in the C++.
    assert_eq!(damage(3, 1), (172, "12.000".to_string()));
}

#[test]
fn damage_does_not_stack_when_multiple_crits_occur() {
    assert_eq!(damage(3, 2), (172, "12.000".to_string()));
}
