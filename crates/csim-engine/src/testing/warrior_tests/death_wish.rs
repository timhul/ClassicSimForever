//! Port of `Test/Warrior/Spells/TestDeathWish` (set up without preparing the iterations).

use crate::engine::EventType;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Death Wish";

fn test() -> WarriorTest {
    WarriorTest::unprepared(SPELL)
}

fn given_death_wish_is_enabled(test: &mut WarriorTest) {
    test.enable_spell(SPELL);
    test.prepare_set_of_combat_iterations();
}

fn given_death_wish_is_not_enabled(test: &mut WarriorTest) {
    if test.is_enabled(SPELL) {
        test.disable_spell(SPELL);
    }
}

fn physical_damage_mod(test: &WarriorTest) -> String {
    let modifier = test.stat(|stats, ctx| stats.get_total_physical_damage_mod(ctx));
    format!("{modifier:.3}")
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
    given_death_wish_is_enabled(&mut test);
    assert_eq!(test.base_cooldown(SPELL), "180.000");

    test.cast(SPELL);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::BuffRemoval, "30.000", false);
    test.then_next_event_is(EventType::PlayerAction, "180.000", false);
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    given_death_wish_is_enabled(&mut test);
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd();

    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    given_death_wish_is_enabled(&mut test);
    test.cast(SPELL);
    let gcd = format!("{:.3}", test.character().global_cooldown());
    test.then_next_event_is(EventType::PlayerAction, &gcd, false);
}

#[test]
fn resource_cost() {
    let mut test = test();
    given_death_wish_is_enabled(&mut test);
    test.given_warrior_has_rage(100);
    test.cast(SPELL);
    test.then_warrior_has_rage(90);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    given_death_wish_is_not_enabled(&mut test);
    test.given_warrior_has_rage(0);
    assert!(test.action_ready());
    test.then_status_is(SPELL, SpellStatus::NotEnabled);

    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::NotEnabled);

    given_death_wish_is_enabled(&mut test);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    given_death_wish_is_enabled(&mut test);
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_battle_stance();
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
fn dmg_mod_reduced_after_buff_expires() {
    let mut test = test();
    given_death_wish_is_enabled(&mut test);
    test.given_warrior_has_rage(100);

    test.cast(SPELL);
    assert_eq!(physical_damage_mod(&test), "1.200");

    test.when_running_queued_events_until(30.01);
    assert_eq!(physical_damage_mod(&test), "1.000");
}
