//! Port of `Test/Warrior/Talents/TestDefiance`.
//!
//! Forever's Defiance has 3 ranks of 5 % more threat in Defensive Stance each (the C++ 5 ranks
//! of 3 %), a tier 3 talent behind Anticipation and Toughness, and only with a shield.

use crate::testing::warrior::WarriorTest;

fn test() -> WarriorTest {
    let mut test = WarriorTest::new("Defiance");
    test.given_a_shield_equipped();
    test.given_protection_talent_with_rank("Anticipation", 5);
    test.given_protection_talent_with_rank("Toughness", 5);
    test.prepare_set_of_combat_iterations();
    test
}

fn threat_mod(test: &WarriorTest) -> f64 {
    test.character().stats().get_total_threat_mod()
}

fn assert_threat(test: &WarriorTest, expected: f64) {
    let threat = threat_mod(test);
    assert!((threat - expected).abs() < 1e-9, "{threat} != {expected}");
}

#[test]
fn defensive_stance_threat_modifier() {
    let mut test = test();
    test.when_switching_to_defensive_stance();
    assert_threat(&test, 1.3);
    for rank in 1..=3 {
        test.given_protection_talent_with_rank("Defiance", 1);
        assert_threat(&test, 1.3 * (1.0 + 0.05 * f64::from(rank)));
    }
}

#[test]
fn battle_stance_threat_modifier() {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    assert_threat(&test, 0.8);
    for _ in 1..=3 {
        test.given_protection_talent_with_rank("Defiance", 1);
        assert_threat(&test, 0.8);
    }
}
