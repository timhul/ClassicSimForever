//! Eureka!, the Gnome racial: the next 3 damaging abilities cost 10 % less rage and deal 10 %
//! more damage (Rend's periodic damage too). A charge is only used by an ability the aura
//! modifies.

use crate::race::Race;
use crate::rotation::executor::RotationHost;
use crate::spell::{CastReport, Hand, SpellStatus};
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Eureka!";

fn test() -> WarriorTest {
    let mut test = WarriorTest::unprepared_of_race(Race::Gnome, SPELL);
    test.prepare_set_of_combat_iterations();
    test.enable_spell("Bloodthirst");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test
}

/// Casts spell `name` once the GCD and its cooldown are over.
fn when_casting(test: &mut WarriorTest, name: &str) -> CastReport {
    let now = test.now();
    let wait = test
        .character()
        .time_until_action_ready(now)
        .max(test.cooldown_remaining(name));
    if wait > 0.0 {
        test.given_engine_priority_pushed_forward(wait);
    }
    test.cast(name)
}

fn bloodthirst_damage(test: &mut WarriorTest) -> u32 {
    let report = when_casting(test, "Bloodthirst");
    report.attack.expect("an attack").damage
}

#[test]
fn gnomes_have_it_and_orcs_do_not() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);

    let mut orc = WarriorTest::new(SPELL);
    assert!(orc.with_ctx(|ctx| ctx.spell_by_name(SPELL, 1)).is_none());
}

#[test]
fn spell_cooldown() {
    let mut test = test();
    assert_eq!(test.base_cooldown(SPELL), "120.000");
}

#[test]
fn reduces_rage_cost_by_10_percent() {
    let mut test = test();
    test.given_warrior_has_rage(100);
    when_casting(&mut test, SPELL);
    test.then_warrior_has_rage(100);

    when_casting(&mut test, "Bloodthirst");

    test.then_warrior_has_rage(73);
}

#[test]
fn increases_damage_by_10_percent() {
    let mut test = test();
    test.given_1000_melee_ap();
    test.given_target_has_0_armor();
    let base = bloodthirst_damage(&mut test);

    when_casting(&mut test, SPELL);
    let boosted = bloodthirst_damage(&mut test);

    assert!(
        boosted.abs_diff(base * 11 / 10) <= 1,
        "{boosted} is not 110 % of {base}"
    );
}

#[test]
fn only_modified_abilities_use_charges() {
    let mut test = test();
    when_casting(&mut test, SPELL);
    assert_eq!(test.buff_charges(SPELL), 3);

    test.given_a_guaranteed_white_hit();
    test.when_swing_is_performed(Hand::Mainhand);
    when_casting(&mut test, "Battle Shout");
    assert_eq!(test.buff_charges(SPELL), 3);

    when_casting(&mut test, "Bloodthirst");
    assert_eq!(test.buff_charges(SPELL), 2);

    test.given_a_guaranteed_melee_ability_miss();
    when_casting(&mut test, "Hamstring");
    assert_eq!(test.buff_charges(SPELL), 2);

    test.given_a_guaranteed_melee_ability_hit();
    when_casting(&mut test, "Hamstring");
    assert_eq!(test.buff_charges(SPELL), 1);

    when_casting(&mut test, "Bloodthirst");
    assert!(!test.buff_is_active(SPELL));
    test.given_warrior_has_rage(100);
    when_casting(&mut test, "Bloodthirst");
    test.then_warrior_has_rage(70);
}
