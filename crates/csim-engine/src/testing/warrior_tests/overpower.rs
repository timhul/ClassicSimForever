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

/// Restarts the Overpower window, which the stance swaps below would otherwise outlast.
fn given_overpower_window_refreshed(test: &mut WarriorTest) {
    let now = test.now();
    test.character_mut().spend_combo_points();
    test.character_mut().gain_combo_points(1, now);
}

#[test]
fn is_ready_conditions() {
    let mut test = test_with_overpower();
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    given_overpower_window_refreshed(&mut test);
    test.then_status_is(SPELL, SpellStatus::InBerserkerStance);

    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    given_overpower_window_refreshed(&mut test);
    test.then_status_is(SPELL, SpellStatus::InDefensiveStance);

    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    given_overpower_window_refreshed(&mut test);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test_with_overpower();
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    given_overpower_window_refreshed(&mut test);
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
    test.character().combo_points(test.now()) > 0
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

/// Battle Stance and full rage, then a dodged Whirlwind; returns the time of the dodge.
fn given_overpower_dodged(test: &mut WarriorTest) -> f64 {
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    let dodged_at = test.now();
    test.given_overpower_is_active();
    dodged_at
}

fn given_time_is(test: &mut WarriorTest, time: f64) {
    let delay = time - test.now();
    test.given_engine_priority_pushed_forward(delay);
}

/// The Overpower window: 6 seconds after the last dodge (or Bloodthrill proc).
const WINDOW: f64 = 6.0;

#[test]
fn overpower_window_lapses() {
    let mut test = test();
    let dodged_at = given_overpower_dodged(&mut test);

    given_time_is(&mut test, dodged_at + WINDOW - 0.01);
    test.then_overpower_is_active();
    test.then_status_is(SPELL, SpellStatus::Available);

    given_time_is(&mut test, dodged_at + WINDOW + 0.001);
    test.then_overpower_is_inactive();
    test.then_status_is(SPELL, SpellStatus::InsufficientComboPoints);
}

/// Dodges Bloodthirst at `time` (Whirlwind, the first dodge, is on cooldown).
fn given_bloodthirst_dodged_at(test: &mut WarriorTest, time: f64) {
    given_time_is(test, time);
    test.given_a_guaranteed_melee_ability_dodge();
    test.given_warrior_is_on_gcd_from("Bloodthirst");
}

#[test]
fn another_dodge_restarts_the_overpower_window() {
    let mut test = test();
    let dodged_at = given_overpower_dodged(&mut test);

    let dodged_again_at = dodged_at + 3.0;
    given_bloodthirst_dodged_at(&mut test, dodged_again_at);

    given_time_is(&mut test, dodged_at + WINDOW + 0.001);
    test.then_overpower_is_active();

    given_time_is(&mut test, dodged_again_at + WINDOW + 0.001);
    test.then_overpower_is_inactive();
}

#[test]
fn another_dodge_does_not_grant_a_second_combo_point() {
    let mut test = test();
    let dodged_at = given_overpower_dodged(&mut test);
    given_bloodthirst_dodged_at(&mut test, dodged_at + 3.0);
    assert_eq!(test.character().combo_points(test.now()), 1);

    // One Overpower spends the only one: there is no second Overpower banked.
    let gcd = test.character().global_cooldown();
    test.given_engine_priority_pushed_forward(gcd);
    test.given_a_guaranteed_melee_ability_hit();
    when_overpower_is_performed(&mut test);
    test.then_overpower_is_inactive();
}
