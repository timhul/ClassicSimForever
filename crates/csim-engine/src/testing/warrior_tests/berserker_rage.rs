//! Berserker Rage with Improved Berserker Rage (no C++ test class: ClassicSim did not model
//! the talent's rage).

use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Berserker Rage";

/// The rage a Berserker Rage cast from 0 rage leaves with `rank` of 2 Improved Berserker
/// Rage.
fn rage_after_cast(rank: u32) -> u32 {
    let mut test = WarriorTest::new(SPELL);
    if rank > 0 {
        test.given_fury_talent_with_rank("Improved Berserker Rage", rank);
    }
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(0);
    test.cast(SPELL);
    test.rage()
}

#[test]
fn gives_no_rage_without_improved_berserker_rage() {
    assert_eq!(rage_after_cast(0), 0);
}

// Forever: Improved Berserker Rage's first effect is 50 stored rage per rank.

#[test]
fn gives_5_rage_per_rank_of_improved_berserker_rage() {
    assert_eq!(rage_after_cast(1), 5);
    assert_eq!(rage_after_cast(2), 10);
}

#[test]
fn gives_rage_on_every_cast() {
    let mut test = WarriorTest::new(SPELL);
    test.given_fury_talent_with_rank("Improved Berserker Rage", 2);
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(0);
    test.cast(SPELL);
    test.then_warrior_has_rage(10);

    let cooldown: f64 = test.base_cooldown(SPELL).parse().unwrap();
    test.given_engine_priority_pushed_forward(cooldown + 1.0);
    test.cast(SPELL);
    test.then_warrior_has_rage(20);
}
