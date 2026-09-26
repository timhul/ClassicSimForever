//! The crit racials of Forever: Axe Specialization (Orc, 1 %) and Sword Specialization (Human,
//! 2 %). Their `MOD_CRIT_PCT` aura counts for abilities and spells, not for auto attacks, while
//! a weapon of the type is equipped.

use crate::race::Race;
use crate::testing::SpellTest;
use crate::testing::warrior::WarriorTest;

fn test(race: Race, given: fn(&mut SpellTest)) -> WarriorTest {
    let mut test = WarriorTest::unprepared_of_race(race, "Weapon specialization racial");
    test.prepare_set_of_combat_iterations();
    given(&mut test);
    // Enough aura crit that the level suppression does not eat into the racial.
    test.stats_mut().increase_melee_aura_crit(500);
    test
}

/// (auto attack, ability) crit of the main hand and the off hand.
fn crits(test: &WarriorTest) -> [(u32, u32); 2] {
    [
        (
            test.stat(|s, c| s.get_mh_crit_chance(c)),
            test.stat(|s, c| s.get_mh_ability_crit_chance(c)),
        ),
        (
            test.stat(|s, c| s.get_oh_crit_chance(c)),
            test.stat(|s, c| s.get_oh_ability_crit_chance(c)),
        ),
    ]
}

/// The ability crit the racial adds with the weapons `given` equips.
fn ability_bonus(race: Race, given: fn(&mut SpellTest)) -> u32 {
    let [(auto, ability), _] = crits(&test(race, given));
    ability - auto
}

#[test]
fn axe_specialization_adds_1_percent_ability_crit_with_axes() {
    for given in [
        SpellTest::given_1h_axe_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_2h_axe_equipped,
    ] {
        assert_eq!(ability_bonus(Race::Orc, given), 100);
    }
}

#[test]
fn sword_specialization_adds_2_percent_ability_crit_with_swords() {
    for given in [
        SpellTest::given_1h_sword_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_2h_sword_equipped,
    ] {
        assert_eq!(ability_bonus(Race::Human, given), 200);
    }
}

#[test]
fn no_ability_crit_with_other_weapons() {
    for (race, given) in [
        (
            Race::Orc,
            SpellTest::given_1h_sword_equipped_in_mainhand as fn(&mut SpellTest),
        ),
        (Race::Orc, SpellTest::given_dagger_equipped_in_mainhand),
        (Race::Human, SpellTest::given_1h_axe_equipped_in_mainhand),
        (Race::Human, SpellTest::given_2h_mace_equipped),
    ] {
        assert_eq!(ability_bonus(race, given), 0, "{race:?}");
    }
}

#[test]
fn auto_attack_crit_is_unchanged() {
    // Same character and test weapons without crit: only the weapon type differs.
    let axe = crits(&test(
        Race::Orc,
        SpellTest::given_1h_axe_equipped_in_mainhand,
    ));
    let sword = crits(&test(
        Race::Orc,
        SpellTest::given_1h_sword_equipped_in_mainhand,
    ));
    assert_eq!(axe[0].0, sword[0].0);
    assert_eq!(axe[0].1, sword[0].1 + 100);
}

#[test]
fn an_off_hand_axe_also_counts_for_off_hand_abilities() {
    let mut test = test(Race::Orc, SpellTest::given_dagger_equipped_in_mainhand);
    test.given_1h_axe_equipped_in_offhand();
    let [(mh_auto, mh_ability), (oh_auto, oh_ability)] = crits(&test);
    assert_eq!(mh_ability - mh_auto, 100);
    assert_eq!(oh_ability - oh_auto, 100);
}
