//! Port of `Test/Warrior/Spells/TestRevenge`.
//!
//! Forever's Revenge needs the `DEFENSIVE` aura state (an incoming attack was dodged, parried
//! or blocked) and a melee weapon, which the C++ spell did not check: the fixture has both.

use crate::character::DEFENSIVE_STATE_DURATION;
use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Revenge";

fn test() -> WarriorTest {
    let mut test = WarriorTest::new(SPELL);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given_revenge_is_active(&mut test);
    test
}

fn given_revenge_is_active(test: &mut WarriorTest) {
    let now = test.now();
    test.character_mut().note_avoided_incoming_attack(now);
}

/// Performs Revenge, then drops the queued events up to the first player action and runs it.
fn when_revenge_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.cast(SPELL);
    test.when_running_until_event(EventType::PlayerAction);
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
    assert_eq!(test.base_cooldown(SPELL), "5.000");
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    let now = test.now();
    test.cast(SPELL);
    let gcd = format!("{:.3}", now + 1.5);
    test.then_next_event_is(EventType::PlayerAction, &gcd, false);
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    given_revenge_is_active(&mut test);
    test.given_warrior_has_rage(100);
    test.given_a_mainhand_weapon_with_2_speed();
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd();

    test.then_status_is(SPELL, SpellStatus::OnGcd);
    assert_eq!(test.cooldown_remaining(SPELL), 0.0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBattleStance);

    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBerserkerStance);

    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.given_engine_priority_pushed_forward(DEFENSIVE_STATE_DURATION);
    test.then_status_is(SPELL, SpellStatus::BuffInactive);
    given_revenge_is_active(&mut test);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    given_revenge_is_active(&mut test);
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(5);
    when_revenge_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.when_switching_to_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::OnGcd);

    test.given_engine_priority_pushed_forward(1.01);
    test.when_switching_to_berserker_stance();
    test.given_warrior_has_rage(100);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnGcd);

    // The stance cooldown is checked before the stance (the C++ said InBerserkerStance).
    test.given_engine_priority_pushed_forward(0.99);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnStanceCooldown);

    test.given_engine_priority_pushed_forward(0.02);
    test.when_switching_to_defensive_stance();
    test.given_warrior_has_rage(100);
    given_revenge_is_active(&mut test);
    test.given_engine_priority_pushed_forward(0.99);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnStanceCooldown);

    test.given_engine_priority_pushed_forward(1.1);
    test.given_a_guaranteed_melee_ability_hit();
    let now = test.now();
    test.cast(SPELL);
    test.given_engine_priority_at(now + 1.4);
    test.then_status_is(SPELL, SpellStatus::OnGcd);
    test.given_engine_priority_at(now + 2.8);
    test.then_status_is(SPELL, SpellStatus::OnCooldown);
}

/// Revenge in Defensive Stance against an unarmored target with 1000 AP.
fn damage(crit: bool, impale: u32) -> u64 {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    given_revenge_is_active(&mut test);
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_2_speed();
    if crit {
        test.given_a_guaranteed_melee_ability_crit();
    } else {
        test.given_a_guaranteed_melee_ability_hit();
    }
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(impale);
    when_revenge_is_performed(&mut test);
    test.damage_dealt()
}

fn assert_in_range(value: u64, min: u64, max: u64) {
    assert!((min..=max).contains(&value), "{value} not in {min} - {max}");
}

// Forever's rank 6 deals 153 ± 10 % (the C++ 64 - 78), and Revenge is only usable in
// Defensive Stance, which takes 10 % off the damage dealt: 124 - 151.

#[test]
fn hit_dmg() {
    assert_in_range(damage(false, 2), 124, 151);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    assert_in_range(damage(true, 0), 248, 302);
}

#[test]
fn crit_dmg_1_of_2_impale() {
    assert_in_range(damage(true, 1), 260, 317);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    assert_in_range(damage(true, 2), 273, 333);
}

#[test]
fn hit_threat() {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_in_melee_attack_mode();
    test.given_warrior_in_defensive_stance();
    given_revenge_is_active(&mut test);
    test.given_1000_melee_ap();
    test.given_warrior_has_rage(100);
    test.given_a_guaranteed_melee_ability_hit();

    when_revenge_is_performed(&mut test);

    assert_in_range(test.damage_dealt(), 124, 151);
    // [Threat] = (damage + innate_threat) * defensive_stance_threat
    // [(124 + 355) * 1.3 - (151 + 355) * 1.3] = [622 - 658]
    assert_in_range(test.threat_dealt(), 622, 658);
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    given_revenge_is_active(&mut test);
    test.given_a_guaranteed_melee_ability_dodge();
    when_revenge_is_performed(&mut test);
    test.then_overpower_is_active();
}
