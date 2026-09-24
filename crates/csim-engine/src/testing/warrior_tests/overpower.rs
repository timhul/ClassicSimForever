//! Port of `Test/Warrior/Spells/TestOverpower`.

use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Overpower";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

/// Battle Stance, full rage and a dodged Whirlwind behind: Overpower is usable.
fn test_with_overpower() -> WarriorTest {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.given_overpower_is_active();
    test
}

fn when_overpower_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.cast(SPELL);
}

#[test]
fn name_correct() {
    let mut test = test();
    let id = test.spell(SPELL);
    assert_eq!(test.character().spells().spell(id).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    let mut test = test_with_overpower();
    assert_eq!(test.base_cooldown(SPELL), "5.000");

    when_overpower_is_performed(&mut test);

    test.then_next_event_is(EventType::PlayerAction, "3.000", false);
    test.then_next_event_is(EventType::PlayerAction, "6.500", false);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test_with_overpower();
    when_overpower_is_performed(&mut test);
    assert!(!test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test_with_overpower();
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn resource_cost() {
    let mut test = test_with_overpower();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(5);
    when_overpower_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test_with_overpower();
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBerserkerStance);

    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InDefensiveStance);

    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test_with_overpower();
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBerserkerStance);

    test.when_switching_to_battle_stance();
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

/// Overpower with the 100 - 100 test sword against an unarmored target, 1000 AP.
fn damage(crit: bool, impale: u32) -> u64 {
    let mut test = test();
    test.given_overpower_is_active();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    if crit {
        test.given_a_guaranteed_melee_ability_crit();
    } else {
        test.given_a_guaranteed_melee_ability_hit();
    }
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(impale);
    when_overpower_is_performed(&mut test);
    test.damage_dealt()
}

#[test]
fn hit_dmg() {
    // [Damage] = base_dmg + (normalized_wpn_speed * AP / 14) + overpower_additional_dmg
    // [306] = 100 + (2.4 * 1000 / 14) + 35
    assert_eq!(damage(false, 2), 306);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    // [613] = (100 + (2.4 * 1000 / 14) + 35) * 2.0
    assert_eq!(damage(true, 0), 613);
}

#[test]
fn crit_dmg_1_of_2_impale() {
    // [644] = (100 + (2.4 * 1000 / 14) + 35) * 2.1
    assert_eq!(damage(true, 1), 644);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    // [674] = (100 + (2.4 * 1000 / 14) + 35) * 2.2
    assert_eq!(damage(true, 2), 674);
}

/// Performs Overpower with the outcome `force` and says whether it is still usable.
fn overpower_active_after(force: fn(&mut WarriorTest)) -> bool {
    let mut test = test();
    test.given_overpower_is_active();
    force(&mut test);
    when_overpower_is_performed(&mut test);
    test.character().combo_points() > 0
}

fn overpower_removes_buff(force: fn(&mut WarriorTest)) {
    assert!(!overpower_active_after(force), "Overpower is active");
}

#[test]
fn overpower_hit_removes_buff() {
    overpower_removes_buff(|t| t.given_a_guaranteed_melee_ability_hit());
}

#[test]
fn overpower_crit_removes_buff() {
    overpower_removes_buff(|t| t.given_a_guaranteed_melee_ability_crit());
}

#[test]
fn overpower_miss_keeps_buff() {
    // The dodge marker is spent by a successful cast only, so a missed Overpower can be
    // tried again (the C++ spent it on the miss too).
    assert!(overpower_active_after(
        |t| t.given_a_guaranteed_melee_ability_miss()
    ));
}
