//! Port of `Test/Warrior/Spells/TestMortalStrike`.

use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Mortal Strike";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

/// Forever's Mortal Strike needs a melee weapon (the C++ spell did not), so the status tests
/// equip one first.
fn test_with_weapon() -> WarriorTest {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test
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
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    assert_eq!(test.base_cooldown(SPELL), "6.000");

    test.cast(SPELL);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::PlayerAction, "6.000", false);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    assert!(test.action_ready());
    test.cast(SPELL);
    assert!(!test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test_with_weapon();
    test.enable_spell(SPELL);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd();

    test.then_status_is(SPELL, SpellStatus::OnGcd);
    assert_eq!(test.cooldown_remaining(SPELL), 0.0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test_with_weapon();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::NotEnabled);

    test.enable_spell(SPELL);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(30);
    test.cast(SPELL);
    test.then_warrior_has_rage(0);
}

#[test]
fn stance_cooldown() {
    let mut test = test_with_weapon();
    test.enable_spell(SPELL);
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

/// Mortal Strike with the 100 - 100 two-hander against an unarmored target, 1000 AP.
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
    test.cast(SPELL);
    test.damage_dealt()
}

#[test]
fn hit_dmg() {
    // [Damage] = base_dmg + (normalized_wpn_speed * AP / 14) + flat_damage_bonus
    // [496] = 100 + (3.3 * 1000 / 14) + 160
    assert_eq!(damage(false, 2), 496);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    // [991] = (100 + (3.3 * 1000 / 14) + 160) * 2.0
    assert_eq!(damage(true, 0), 991);
}

#[test]
fn crit_dmg_1_of_2_impale() {
    // [1041] = (100 + (3.3 * 1000 / 14) + 160) * 2.1
    assert_eq!(damage(true, 1), 1041);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    // [1091] = (100 + (3.3 * 1000 / 14) + 160) * 2.2
    assert_eq!(damage(true, 2), 1091);
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    test.cast(SPELL);
    test.then_overpower_is_active();
}
