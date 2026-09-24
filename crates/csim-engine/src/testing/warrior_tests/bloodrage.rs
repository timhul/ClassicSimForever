//! Port of `Test/Warrior/Spells/TestBloodrage`.

use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Bloodrage";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

#[test]
fn name_correct() {
    let mut test = test();
    let id = test.spell(SPELL);
    assert_eq!(test.character().spells().spell(id).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    assert_eq!(test().base_cooldown(SPELL), "60.000");
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.cast(SPELL);
    assert!(test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);

    // Forever's Bloodrage has no stance requirement (the C++ refused Defensive Stance).
    test.given_warrior_in_defensive_stance();
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_battle_stance();
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_berserker_stance();
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);

    test.when_switching_to_berserker_stance();
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_engine_priority_pushed_forward(0.99);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_engine_priority_pushed_forward(0.02);
    assert!(!test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_warrior_has_rage(0);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn gain_10_rage_immediately() {
    let mut test = test();
    test.given_warrior_has_rage(0);
    test.cast(SPELL);
    test.then_warrior_has_rage(10);
}

#[test]
fn gain_10_rage_over_10_seconds() {
    let mut test = test();
    test.given_warrior_has_rage(0);
    test.cast(SPELL);

    let before = test.rage();
    test.when_running_queued_events_until(10.01);
    assert_eq!(test.rage() - before, 10);
}
