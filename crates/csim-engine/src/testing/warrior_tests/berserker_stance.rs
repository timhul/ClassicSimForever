//! Port of `Test/Warrior/Spells/TestBerserkerStance`.

use crate::character_loader::MAX_TARGET_LEVEL;
use crate::mechanics::Mechanics;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Berserker Stance";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

fn when_berserker_stance_is_performed(test: &mut WarriorTest) {
    test.then_status_is(SPELL, SpellStatus::Available);
    test.cast(SPELL);
}

fn mh_crit(test: &WarriorTest) -> u32 {
    test.stat(|stats, ctx| stats.get_mh_crit_chance(ctx))
}

#[test]
fn name_correct() {
    let mut test = test();
    let id = test.spell(SPELL);
    assert_eq!(test.character().spells().spell(id).name(), SPELL);
}

#[test]
fn spell_cooldown() {
    // The stance cooldown is the stance spells' shared category cooldown here (the C++ kept
    // it on the character and gave the spell no cooldown).
    assert_eq!(test().base_cooldown(SPELL), "1.000");
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    assert!(!test.on_stance_cooldown());
    when_berserker_stance_is_performed(&mut test);
    assert!(test.on_stance_cooldown());
    test.given_engine_priority_at(0.99);
    assert!(test.on_stance_cooldown());
    test.given_engine_priority_at(1.01);
    assert!(!test.on_stance_cooldown());
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_warrior_has_rage(0);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    assert!(!test.on_global_cooldown());
    assert!(!test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(!test.on_global_cooldown());
    when_berserker_stance_is_performed(&mut test);
    assert!(test.on_global_cooldown());
    test.given_engine_priority_at(0.49);
    assert!(test.on_global_cooldown());
    test.given_engine_priority_at(0.51);
    assert!(!test.on_global_cooldown());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    // The data-driven stance spells do not wait for the GCD.
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn does_not_incur_extra_global_cooldown_if_gcd_longer_than_half_second() {
    let mut test = test();
    test.given_warrior_is_on_gcd();
    assert!(!test.on_stance_cooldown());

    when_berserker_stance_is_performed(&mut test);

    assert!(test.on_global_cooldown());
    assert!(test.on_stance_cooldown());
    test.given_engine_priority_at(0.99);
    assert!(test.on_global_cooldown());
    assert!(test.on_stance_cooldown());
    test.given_engine_priority_at(1.01);
    assert!(test.on_global_cooldown());
    assert!(!test.on_stance_cooldown());
    test.given_engine_priority_at(1.49);
    assert!(test.on_global_cooldown());
    test.given_engine_priority_at(1.51);
    assert!(!test.on_global_cooldown());
}

/// The crit Berserker Stance's passive gives, after the level 63 aura crit suppression.
fn stance_crit(test: &mut WarriorTest) -> u32 {
    Mechanics::new(MAX_TARGET_LEVEL).suppressed_aura_crit_chance(test.character().clvl(), 300)
}

#[test]
fn gives_crit_when_stance_entered() {
    let mut test = test();
    let crit = mh_crit(&test);
    when_berserker_stance_is_performed(&mut test);
    let bonus = stance_crit(&mut test);
    assert_eq!(mh_crit(&test), crit + bonus);
}

#[test]
fn removes_crit_when_stance_exited() {
    let mut test = test();
    let crit = mh_crit(&test);
    when_berserker_stance_is_performed(&mut test);
    let bonus = stance_crit(&mut test);
    assert_eq!(mh_crit(&test), crit + bonus);

    test.given_engine_priority_pushed_forward(crate::character::STANCE_COOLDOWN);
    test.cast("Battle Stance");
    assert_eq!(mh_crit(&test), crit);
}

/// The rage kept through a switch into Berserker Stance from 100 rage.
fn rage_after_switch(tactical_mastery: u32, rage: u32) -> u32 {
    let mut test = test();
    test.given_tactical_mastery(tactical_mastery);
    test.given_warrior_has_rage(rage);
    when_berserker_stance_is_performed(&mut test);
    test.rage()
}

// Forever: Tactical Mastery keeps 3 rage per rank on top of the rage every Warrior keeps
// through a stance switch (the C++ kept 5 per rank and nothing untalented).

#[test]
fn rage_remains_after_stance_switch_with_tactical_mastery() {
    let untalented = WarriorTest::new(SPELL).character().stance_rage_retained();
    for rank in 0..=5 {
        assert_eq!(
            rage_after_switch(rank, 100),
            untalented + 3 * rank,
            "{rank} of 5 Tactical Mastery"
        );
    }
}

#[test]
fn rage_is_not_increased_by_switching_stances_with_5_of_5_tactical_mastery() {
    assert_eq!(rage_after_switch(5, 0), 0);
}
