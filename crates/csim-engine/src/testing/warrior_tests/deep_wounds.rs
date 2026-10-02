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

// total_deep_wounds_damage = avg_mh_wpn_dmg * deep_wounds_percent, attack power ignored
// [20 / 40 / 60] = 100 * [0.2 / 0.4 / 0.6], over four ticks

#[test]
fn damage_of_1_of_3_deep_wounds() {
    assert_eq!(damage(1, 1), (20, "12.000".to_string()));
}

#[test]
fn damage_of_2_of_3_deep_wounds() {
    assert_eq!(damage(2, 1), (40, "12.000".to_string()));
}

#[test]
fn damage_of_3_of_3_deep_wounds() {
    assert_eq!(damage(3, 1), (60, "12.000".to_string()));
}

/// Two crits at once pool two applications over the four ticks.
#[test]
fn damage_pools_when_multiple_crits_occur() {
    assert_eq!(damage(3, 2), (120, "12.000".to_string()));
}

/// A crit 6 s into the bleed adds its 60 to the 30 left and spreads the 90 over four fresh
/// ticks (22.5 each, the half carried to the next tick): the bleed runs to 18 s.
#[test]
fn a_later_crit_rolls_the_rest_of_the_bleed_into_fresh_ticks() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_deep_wounds(&mut test, 3);
    test.given_a_guaranteed_white_crit();
    when_mh_attack_is_performed(&mut test);
    let mut ticks = Vec::new();
    let mut tick = |test: &mut WarriorTest| {
        let before = test.damage_dealt_by(SPELL);
        let time = test.when_running_until_event(EventType::DotTick);
        ticks.push((format!("{time:.3}"), test.damage_dealt_by(SPELL) - before));
    };
    tick(&mut test);
    tick(&mut test);
    when_mh_attack_is_performed(&mut test);
    for _ in 0..4 {
        tick(&mut test);
    }
    let expected = [
        ("3.000", 15),
        ("6.000", 15),
        ("9.000", 23),
        ("12.000", 22),
        ("15.000", 23),
        ("18.000", 22),
    ];
    let expected: Vec<_> = expected.iter().map(|(t, d)| (t.to_string(), *d)).collect();
    assert_eq!(ticks, expected);
    assert_eq!(
        deep_wounds_damage(&mut test),
        120,
        "no tick after the last stack"
    );
}

/// Deep Wounds at rank 3 after critical swings of the 100 damage main hand and the 50 damage
/// off hand, in `hands` order, with `dual_wield_specialization` ranks.
fn damage_after_crits_of(hands: &[Hand], dual_wield_specialization: u32) -> u64 {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_50_min_max_dmg();
    if dual_wield_specialization > 0 {
        test.given_fury_talent_with_rank("Dual Wield Specialization", dual_wield_specialization);
    }
    given_deep_wounds(&mut test, 3);
    test.given_a_guaranteed_white_crit();
    for &hand in hands {
        test.when_swing_is_performed(hand);
    }
    deep_wounds_damage(&mut test)
}

/// An off-hand crit bleeds for 60 % of the off-hand weapon's 50 average damage, halved by the
/// off-hand penalty.
#[test]
fn offhand_crit_bleeds_for_the_offhand_weapon_damage() {
    assert_eq!(damage_after_crits_of(&[Hand::Offhand], 0), 15);
}

/// Dual Wield Specialization raises the off-hand penalty to 62.5 %: 50 * 0.625 * 0.6 = 18.75.
#[test]
fn dual_wield_specialization_raises_the_offhand_bleed() {
    assert_eq!(damage_after_crits_of(&[Hand::Offhand], 5), 19);
}

/// Each crit adds its own weapon's share to the pool: 60 + 15.
#[test]
fn mainhand_and_offhand_crits_pool_their_own_weapon_damage() {
    assert_eq!(
        damage_after_crits_of(&[Hand::Mainhand, Hand::Offhand], 0),
        75
    );
    assert_eq!(
        damage_after_crits_of(&[Hand::Offhand, Hand::Mainhand], 0),
        75
    );
}

/// A critical Whirlwind with Raging Blows' off-hand strike: the main-hand crit adds 60 % of the
/// main hand's 100, the off-hand strike's crit 60 % of half the off hand's 50.
#[test]
fn offhand_strike_crit_bleeds_for_the_offhand_weapon_damage() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_50_min_max_dmg();
    test.given_fury_talent_with_rank("Raging Blows", 1);
    given_deep_wounds(&mut test, 3);
    test.given_a_guaranteed_melee_ability_crit();
    let report = test.cast("Whirlwind");
    assert!(
        report.offhand.is_some(),
        "Whirlwind strikes with the off hand"
    );
    assert_eq!(deep_wounds_damage(&mut test), 75);
}

/// Death Wish's 20 % applies to the bleed once: 60 % of the 100 average weapon damage, times
/// 1.2, not also to the weapon damage it is based on.
#[test]
fn death_wish_increases_deep_wounds_damage_once() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.enable_spell("Death Wish");
    given_deep_wounds(&mut test, 3);
    test.given_warrior_has_rage(100);
    test.cast("Death Wish");
    test.given_a_guaranteed_white_crit();
    test.given_1000_melee_ap();
    when_mh_attack_is_performed(&mut test);
    assert_eq!(deep_wounds_damage(&mut test), 72);
}

/// The bleed ticks never crit, whatever the crit chance: 60 % of the 100 average weapon damage.
#[test]
fn deep_wounds_cannot_crit() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_deep_wounds(&mut test, 3);
    test.given_a_guaranteed_white_crit();
    test.given_a_guaranteed_melee_ability_crit();
    when_mh_attack_is_performed(&mut test);
    assert_eq!(deep_wounds_damage(&mut test), 60);
}
