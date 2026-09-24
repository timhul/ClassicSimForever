//! Port of `Test/Warrior/Spells/TestWhirlwind`.

use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Whirlwind";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

fn when_whirlwind_is_performed(test: &mut WarriorTest) {
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
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    assert_eq!(test.base_cooldown(SPELL), "10.000");

    when_whirlwind_is_performed(&mut test);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::PlayerAction, "10.000", false);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(test.action_ready());
    when_whirlwind_is_performed(&mut test);
    assert!(!test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd_from("Execute");

    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBattleStance);

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
fn resource_cost() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(25);
    when_whirlwind_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

/// Whirlwind with the 100 - 100 test sword against an unarmored target, 1000 AP.
fn damage(crit: bool, impale: u32) -> u64 {
    let mut test = test();
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
    when_whirlwind_is_performed(&mut test);
    test.damage_dealt()
}

#[test]
fn hit_dmg() {
    // [Damage] = base_dmg + (normalized_wpn_speed * AP / 14)
    // [271] = 100 + (2.4 * 1000 / 14)
    assert_eq!(damage(false, 2), 271);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    // [543] = (100 + (2.4 * 1000 / 14)) * 2.0
    assert_eq!(damage(true, 0), 543);
}

#[test]
fn crit_dmg_1_of_2_impale() {
    // [570] = (100 + (2.4 * 1000 / 14)) * 2.1
    assert_eq!(damage(true, 1), 570);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    // [597] = (100 + (2.4 * 1000 / 14)) * 2.2
    assert_eq!(damage(true, 2), 597);
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    when_whirlwind_is_performed(&mut test);
    test.then_overpower_is_active();
}
