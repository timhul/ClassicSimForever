use crate::magic_school::MagicSchool;
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

/// Main-hand auto attack, main-hand ability, off-hand auto attack, off-hand ability and spell
/// crit.
fn crits(test: &WarriorTest) -> [u32; 5] {
    [
        test.stat(|s, c| s.get_mh_crit_chance(c)),
        test.stat(|s, c| s.get_mh_ability_crit_chance(c)),
        test.stat(|s, c| s.get_oh_crit_chance(c)),
        test.stat(|s, c| s.get_oh_ability_crit_chance(c)),
        test.stat(|s, c| s.get_spell_crit_chance(c, MagicSchool::Fire)),
    ]
}

/// The crit `race` gains by equipping `given` instead of `baseline`.
fn gain(race: Race, given: fn(&mut SpellTest), baseline: fn(&mut SpellTest)) -> [i64; 5] {
    let (with, without) = (crits(&test(race, given)), crits(&test(race, baseline)));
    std::array::from_fn(|i| i64::from(with[i]) - i64::from(without[i]))
}

/// The crit the racial of `race` adds with the weapons `given` equips over `baseline`: the
/// gain of `race` less a Troll's (no weapon racial), which cancels the weapon skill and the
/// race's attributes.
fn bonus_over(race: Race, given: fn(&mut SpellTest), baseline: fn(&mut SpellTest)) -> [i64; 5] {
    let (racial, control) = (
        gain(race, given, baseline),
        gain(Race::Troll, given, baseline),
    );
    std::array::from_fn(|i| racial[i] - control[i])
}

/// [`bonus_over`] a main-hand dagger.
fn bonus(race: Race, given: fn(&mut SpellTest)) -> [i64; 5] {
    bonus_over(race, given, SpellTest::given_dagger_equipped_in_mainhand)
}

/// `crit` for the main hand (auto attacks and abilities) and spells; nothing for the empty off
/// hand.
fn main_hand_and_spells(crit: i64) -> [i64; 5] {
    [crit, crit, 0, 0, crit]
}

#[test]
fn axe_specialization_adds_1_percent_crit_with_axes() {
    for given in [
        SpellTest::given_1h_axe_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_2h_axe_equipped,
    ] {
        assert_eq!(bonus(Race::Orc, given), main_hand_and_spells(100));
    }
}

#[test]
fn sword_specialization_adds_2_percent_crit_with_swords() {
    for given in [
        SpellTest::given_1h_sword_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_2h_sword_equipped,
    ] {
        assert_eq!(bonus(Race::Human, given), main_hand_and_spells(200));
    }
}

#[test]
fn mace_specialization_adds_1_percent_crit_with_maces() {
    for given in [
        SpellTest::given_1h_mace_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_2h_mace_equipped,
    ] {
        assert_eq!(bonus(Race::Dwarf, given), main_hand_and_spells(100));
    }
}

#[test]
fn no_crit_with_other_weapons() {
    for (race, given) in [
        (
            Race::Orc,
            SpellTest::given_1h_sword_equipped_in_mainhand as fn(&mut SpellTest),
        ),
        (Race::Orc, SpellTest::given_2h_mace_equipped),
        (Race::Human, SpellTest::given_1h_axe_equipped_in_mainhand),
        (Race::Human, SpellTest::given_2h_mace_equipped),
        (Race::Dwarf, SpellTest::given_1h_sword_equipped_in_mainhand),
        (Race::Dwarf, SpellTest::given_2h_axe_equipped),
    ] {
        assert_eq!(bonus(race, given), [0; 5], "{race:?}");
    }
}

#[test]
fn an_off_hand_weapon_of_the_type_counts_for_both_hands() {
    fn dagger_and_off_hand_axe(test: &mut SpellTest) {
        test.given_dagger_equipped_in_mainhand();
        test.given_1h_axe_equipped_in_offhand();
    }
    fn dagger_and_off_hand_mace(test: &mut SpellTest) {
        test.given_dagger_equipped_in_mainhand();
        test.given_1h_mace_equipped_in_offhand();
    }
    fn dual_daggers(test: &mut SpellTest) {
        test.given_dagger_equipped_in_mainhand();
        test.given_dagger_equipped_in_offhand();
    }
    for (race, given) in [
        (Race::Orc, dagger_and_off_hand_axe as fn(&mut SpellTest)),
        (Race::Dwarf, dagger_and_off_hand_mace),
    ] {
        assert_eq!(bonus_over(race, given, dual_daggers), [100; 5], "{race:?}");
    }
}
