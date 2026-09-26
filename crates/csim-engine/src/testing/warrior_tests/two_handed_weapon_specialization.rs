//! Port of `Test/Warrior/Talents/TestTwoHandedWeaponSpecialization`.
//!
//! Forever's talent has 3 ranks of 1 % physical damage with a two-hander (the C++ 5).

use crate::testing::SpellTest;
use crate::testing::warrior::WarriorTest;

const TALENT: &str = "Two-Handed Weapon Specialization";

fn test() -> WarriorTest {
    WarriorTest::new(TALENT)
}

fn assert_damage_mod(test: &WarriorTest, expected: f64) {
    let modifier = test.stat(|stats, ctx| stats.get_total_physical_damage_mod(ctx));
    assert!(
        (modifier - expected).abs() < 1e-9,
        "{modifier} != {expected}"
    );
}

fn node(test: &WarriorTest) -> u32 {
    test.character()
        .talents()
        .and_then(|t| t.node_of_name(TALENT, None))
        .expect("the talent")
}

fn given_rank(test: &mut WarriorTest) {
    test.given_arms_talent_with_rank(TALENT, 1);
}

fn when_decrementing(test: &mut WarriorTest) {
    let node = node(test);
    assert!(test.with_ctx(|ctx| ctx.decrement_talent(node)));
}

#[test]
fn basic_properties() {
    let test = test();
    let talents = test.character().talents().expect("talents");
    assert_eq!(talents.spec(node(&test)).expect("spec").name, TALENT);
}

#[test]
fn damage_modified_when_using_2handers() {
    let mut test = test();
    assert_damage_mod(&test, 1.0);
    given_rank(&mut test);
    assert_damage_mod(&test, 1.0);
    for given in [
        SpellTest::given_2h_axe_equipped as fn(&mut SpellTest),
        SpellTest::given_2h_sword_equipped,
        SpellTest::given_2h_mace_equipped,
        SpellTest::given_polearm_equipped,
        SpellTest::given_staff_equipped,
    ] {
        given(&mut test);
        assert_damage_mod(&test, 1.01);
        test.given_no_mainhand();
        assert_damage_mod(&test, 1.0);
    }
}

#[test]
fn damage_not_modified_when_not_using_2handers() {
    let mut test = test();
    given_rank(&mut test);
    test.given_2h_axe_equipped();
    assert_damage_mod(&test, 1.01);
    for given in [
        SpellTest::given_1h_axe_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_1h_mace_equipped_in_mainhand,
        SpellTest::given_1h_sword_equipped_in_mainhand,
        SpellTest::given_dagger_equipped_in_mainhand,
        SpellTest::given_fist_weapon_equipped_in_mainhand,
    ] {
        given(&mut test);
        assert_damage_mod(&test, 1.0);
    }
}

#[test]
fn damage_added_per_rank() {
    let mut test = test();
    test.given_2h_axe_equipped();
    assert_damage_mod(&test, 1.0);
    for rank in 1..=3 {
        given_rank(&mut test);
        assert_damage_mod(&test, 1.0 + 0.01 * f64::from(rank));
    }
    for rank in (0..3).rev() {
        when_decrementing(&mut test);
        assert_damage_mod(&test, 1.0 + 0.01 * f64::from(rank));
    }
}
