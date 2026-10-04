//! Port of `Test/Warrior/Spells/TestExecute`.

use crate::character::SimParams;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Execute";

/// Forever's Execute needs a melee weapon (the C++ spell did not): the fixture holds the
/// 100 - 100 test sword.
fn test() -> WarriorTest {
    let mut test = WarriorTest::new(SPELL);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test
}

/// 20 s left of the 300 s fight.
fn given_target_in_execute_range(test: &mut WarriorTest) {
    test.given_engine_priority_at(280.0);
}

fn given_target_not_in_execute_range(test: &mut WarriorTest) {
    test.given_engine_priority_at(0.0);
}

fn when_execute_is_performed_with_rage(test: &mut WarriorTest, rage: u32) {
    test.given_warrior_has_rage(rage);
    test.cast(SPELL);
}

fn execute_available_with_rage(test: &mut WarriorTest, rage: u32) -> bool {
    test.given_warrior_has_rage(rage);
    test.status(SPELL) == SpellStatus::Available
}

/// Checks that Execute costs exactly `rage`.
fn then_execute_costs(test: &mut WarriorTest, rage: u32) {
    assert!(execute_available_with_rage(test, rage), "{rage} rage");
    assert!(
        !execute_available_with_rage(test, rage - 1),
        "{} rage",
        rage - 1
    );
}

/// The cost of Execute at 0, 1 and 2 of 2 Improved Execute: Forever's talent takes 3 and 5
/// rage off (the C++ 2 and 5).
const COST: [u32; 3] = [15, 12, 10];

fn given_improved_execute(test: &mut WarriorTest, rank: u32) {
    then_execute_costs(test, COST[0]);
    if rank > 0 {
        test.given_fury_talent_with_rank("Improved Execute", rank);
    }
    then_execute_costs(test, COST[rank as usize]);
}

#[test]
fn name_correct() {
    let mut test = test();
    let id = test.spell(SPELL);
    assert_eq!(test.character().spells().spell(id).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    assert_eq!(test().base_cooldown(SPELL), "0.000");
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_target_in_execute_range(&mut test);
    assert!(test.action_ready());
    when_execute_is_performed_with_rage(&mut test, 100);
    assert!(!test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    given_target_in_execute_range(&mut test);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_target_in_execute_range(&mut test);
    test.given_a_guaranteed_melee_ability_hit();
    then_execute_costs(&mut test, 15);
    when_execute_is_performed_with_rage(&mut test, 15);
    test.then_warrior_has_rage(0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    given_target_not_in_execute_range(&mut test);
    test.given_warrior_has_rage(0);
    assert!(test.action_ready());
    test.then_status_is(SPELL, SpellStatus::InsufficientResources);

    given_target_in_execute_range(&mut test);
    test.then_status_is(SPELL, SpellStatus::InsufficientResources);

    given_target_not_in_execute_range(&mut test);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::NotInExecuteRange);

    given_target_in_execute_range(&mut test);
    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InDefensiveStance);

    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    given_target_in_execute_range(&mut test);
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

#[test]
fn improved_execute_reduces_rage_cost() {
    for rank in 1..=2 {
        let mut test = test();
        given_target_in_execute_range(&mut test);
        given_improved_execute(&mut test, rank);
    }
}

#[test]
fn removing_points_in_improved_execute_increases_rage_cost() {
    let mut test = test();
    given_target_in_execute_range(&mut test);
    then_execute_costs(&mut test, COST[0]);
    let node = test
        .character()
        .talents()
        .and_then(|t| t.node_of_name("Improved Execute", None))
        .expect("Improved Execute");
    test.given_fury_talent_with_rank("Improved Execute", 2);
    then_execute_costs(&mut test, COST[2]);
    assert!(test.with_ctx(|ctx| ctx.decrement_talent(node)));
    then_execute_costs(&mut test, COST[1]);
    assert!(test.with_ctx(|ctx| ctx.decrement_talent(node)));
    then_execute_costs(&mut test, COST[0]);
}

/// A critical Execute against an unarmored target with `rage`.
fn crit_damage(improved_execute: u32, impale: u32, rage: u32) -> u64 {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_target_has_0_armor();
    given_target_in_execute_range(&mut test);
    test.given_a_guaranteed_melee_ability_crit();
    test.given_impale(impale);
    given_improved_execute(&mut test, improved_execute);
    test.given_no_previous_damage_dealt();
    when_execute_is_performed_with_rage(&mut test, rage);
    test.damage_dealt()
}

/// [Damage] = (600 + 15 * (rage - rage_cost)) * crit_dmg_modifier
fn expected(improved_execute: u32, impale: u32, rage: u32) -> u64 {
    let base = 600.0 + 15.0 * f64::from(rage - COST[improved_execute as usize]);
    (base * (2.0 + 0.1 * f64::from(impale))).round() as u64
}

#[test]
fn min_crit_dmg() {
    for improved_execute in 0..=2 {
        for impale in 0..=2 {
            let rage = COST[improved_execute as usize];
            assert_eq!(
                crit_damage(improved_execute, impale, rage),
                expected(improved_execute, impale, rage),
                "{improved_execute} of 2 Improved Execute, {impale} of 2 Impale"
            );
        }
    }
}

#[test]
fn max_crit_dmg() {
    for improved_execute in 0..=2 {
        for impale in 0..=2 {
            assert_eq!(
                crit_damage(improved_execute, impale, 100),
                expected(improved_execute, impale, 100),
                "{improved_execute} of 2 Improved Execute, {impale} of 2 Impale"
            );
        }
    }
}

#[test]
fn crit_dmg_values_of_the_cpp_tests() {
    // The C++ expectations at 0 of 2 Improved Execute, where the costs agree.
    assert_eq!(expected(0, 0, 15), 1200);
    assert_eq!(expected(0, 0, 100), 3750);
    assert_eq!(expected(0, 1, 100), 3938);
    assert_eq!(expected(0, 2, 100), 4125);
    assert_eq!(expected(2, 2, 100), 4290);
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_target_in_execute_range(&mut test);
    test.given_a_guaranteed_melee_ability_dodge();
    when_execute_is_performed_with_rage(&mut test, 100);
    test.then_overpower_is_active();
}

/// A target that starts the fight at 30 % health is below 20 % for the last two thirds of it;
/// one that starts at 15 % already is when it starts.
#[test]
fn the_target_start_health_places_the_execute_range() {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    let sim = *test.character_mut().sim();
    test.character_mut().set_sim(SimParams {
        target_start_health: 0.3,
        ..sim
    });
    for (time, status) in [
        (99.0, SpellStatus::NotInExecuteRange),
        (101.0, SpellStatus::Available),
    ] {
        test.given_engine_priority_at(time);
        test.given_warrior_has_rage(100);
        test.then_status_is(SPELL, status);
    }

    test.character_mut().set_sim(SimParams {
        target_start_health: 0.15,
        ..sim
    });
    test.given_engine_priority_at(0.0);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}
