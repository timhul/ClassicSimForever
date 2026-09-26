//! Weaponmaster's axe/polearm crit and mace/staff armor penetration (the sword extra attack is
//! in `sword_specialization`).
//!
//! The talent's `DUMMY` rank values go to the hidden auras the server applies (`ENABLE_AURA`):
//! E0 1-5 % crit → 12700 (axes, polearms), E1 3-15 % armor ignored → 12284 (maces, staves).
//! Both only count for attacks with a weapon of those types.

use crate::spell::{Hand, SpellHost};
use crate::testing::SpellTest;
use crate::testing::warrior::WarriorTest;

const TALENT: &str = "Weaponmaster";

fn test() -> WarriorTest {
    WarriorTest::new(TALENT)
}

fn mh_crit(test: &WarriorTest) -> u32 {
    test.stat(|stats, ctx| stats.get_mh_crit_chance(ctx))
}

fn oh_crit(test: &WarriorTest) -> u32 {
    test.stat(|stats, ctx| stats.get_oh_crit_chance(ctx))
}

/// The crit (read by `crit`) that `rank` should add with the weapons `setup` equips: what 1 %
/// of aura crit per rank adds there, after the level suppression (the weapons' own crit counts
/// towards it).
fn rank_crit(setup: impl Fn(&mut WarriorTest), rank: u32, crit: fn(&WarriorTest) -> u32) -> u32 {
    let mut test = test();
    setup(&mut test);
    let before = crit(&test);
    test.stats_mut().increase_melee_aura_crit(100 * rank);
    crit(&test) - before
}

fn armor_against(test: &mut WarriorTest, hand: Hand) -> i32 {
    test.with_ctx(|ctx| ctx.target_armor_against(hand))
}

fn node(test: &WarriorTest) -> u32 {
    test.character()
        .talents()
        .and_then(|t| t.node_of_name(TALENT, None))
        .expect("the talent")
}

fn when_decrementing(test: &mut WarriorTest) {
    let node = node(test);
    assert!(test.with_ctx(|ctx| ctx.decrement_talent(node)));
}

#[test]
fn crit_with_axes_and_polearms_per_rank() {
    for rank in 1..=5 {
        for given in [
            SpellTest::given_1h_axe_equipped_in_mainhand as fn(&mut SpellTest),
            SpellTest::given_2h_axe_equipped,
            SpellTest::given_polearm_equipped,
        ] {
            let mut test = test();
            given(&mut test);
            let crit = mh_crit(&test);
            test.given_arms_talent_with_rank(TALENT, rank);
            let expected = crit + rank_crit(|t| given(t), rank, mh_crit);
            assert_eq!(mh_crit(&test), expected, "rank {rank}");
        }
    }
}

#[test]
fn no_crit_with_other_weapons() {
    for given in [
        SpellTest::given_1h_sword_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_1h_mace_equipped_in_mainhand,
        SpellTest::given_dagger_equipped_in_mainhand,
        SpellTest::given_fist_weapon_equipped_in_mainhand,
        SpellTest::given_2h_sword_equipped,
        SpellTest::given_2h_mace_equipped,
        SpellTest::given_staff_equipped,
    ] {
        let mut test = test();
        given(&mut test);
        let crit = mh_crit(&test);
        test.given_arms_talent_with_rank(TALENT, 5);
        assert_eq!(mh_crit(&test), crit);
    }
}

#[test]
fn crit_counts_for_the_hand_holding_the_axe() {
    let setup = |test: &mut WarriorTest| {
        test.given_1h_sword_equipped_in_mainhand();
        test.given_1h_axe_equipped_in_offhand();
    };
    let mut test = test();
    setup(&mut test);
    let (mh, oh) = (mh_crit(&test), oh_crit(&test));
    test.given_arms_talent_with_rank(TALENT, 5);
    assert_eq!(mh_crit(&test), mh);
    assert_eq!(oh_crit(&test), oh + rank_crit(setup, 5, oh_crit));
}

#[test]
fn crit_follows_rank_changes_and_removal() {
    let mut test = test();
    test.given_1h_axe_equipped_in_mainhand();
    let crit = mh_crit(&test);
    test.given_arms_talent_with_rank(TALENT, 5);
    let axe = |test: &mut WarriorTest| test.given_1h_axe_equipped_in_mainhand();
    assert_eq!(mh_crit(&test), crit + rank_crit(axe, 5, mh_crit));
    when_decrementing(&mut test);
    assert_eq!(mh_crit(&test), crit + rank_crit(axe, 4, mh_crit));
    test.with_ctx(|ctx| ctx.clear_talents());
    assert_eq!(mh_crit(&test), crit);
}

#[test]
fn crit_leaves_when_the_axe_is_unequipped() {
    let mut test = test();
    test.given_arms_talent_with_rank(TALENT, 5);
    test.given_1h_axe_equipped_in_mainhand();
    let with_axe = mh_crit(&test);
    test.given_1h_sword_equipped_in_mainhand();
    let with_sword = mh_crit(&test);
    assert!(with_axe > with_sword);
    test.given_1h_axe_equipped_in_mainhand();
    assert_eq!(mh_crit(&test), with_axe);
}

#[test]
fn maces_and_staves_ignore_armor_per_rank() {
    for rank in 1..=5 {
        for given in [
            SpellTest::given_1h_mace_equipped_in_mainhand as fn(&mut SpellTest),
            SpellTest::given_2h_mace_equipped,
            SpellTest::given_staff_equipped,
        ] {
            let mut test = test();
            given(&mut test);
            let armor = test.target().armor();
            assert_eq!(armor_against(&mut test, Hand::Mainhand), armor);
            test.given_arms_talent_with_rank(TALENT, rank);
            let expected = (f64::from(armor) * (1.0 - 0.03 * f64::from(rank))).round() as i32;
            assert_eq!(
                armor_against(&mut test, Hand::Mainhand),
                expected,
                "rank {rank}"
            );
        }
    }
}

#[test]
fn other_weapons_do_not_ignore_armor() {
    for given in [
        SpellTest::given_1h_sword_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_1h_axe_equipped_in_mainhand,
        SpellTest::given_dagger_equipped_in_mainhand,
        SpellTest::given_2h_sword_equipped,
        SpellTest::given_polearm_equipped,
    ] {
        let mut test = test();
        given(&mut test);
        test.given_arms_talent_with_rank(TALENT, 5);
        let armor = test.target().armor();
        assert_eq!(armor_against(&mut test, Hand::Mainhand), armor);
    }
}

#[test]
fn armor_penetration_counts_for_the_hand_holding_the_mace() {
    let mut test = test();
    test.given_1h_sword_equipped_in_mainhand();
    test.given_1h_mace_equipped_in_offhand();
    test.given_arms_talent_with_rank(TALENT, 5);
    let armor = test.target().armor();
    assert_eq!(armor_against(&mut test, Hand::Mainhand), armor);
    let expected = (f64::from(armor) * 0.85).round() as i32;
    assert_eq!(armor_against(&mut test, Hand::Offhand), expected);
}

#[test]
fn armor_penetration_follows_rank_changes_and_removal() {
    let mut test = test();
    test.given_2h_mace_equipped();
    test.given_arms_talent_with_rank(TALENT, 5);
    let armor = test.target().armor();
    let at = |rank: f64| (f64::from(armor) * (1.0 - 0.03 * rank)).round() as i32;
    assert_eq!(armor_against(&mut test, Hand::Mainhand), at(5.0));
    when_decrementing(&mut test);
    assert_eq!(armor_against(&mut test, Hand::Mainhand), at(4.0));
    test.with_ctx(|ctx| ctx.clear_talents());
    assert_eq!(armor_against(&mut test, Hand::Mainhand), armor);
}

#[test]
fn mace_swings_hit_harder_with_the_talent() {
    let damage = |rank: u32| {
        let mut test = test();
        test.given_2h_mace_equipped();
        test.given_a_guaranteed_white_hit();
        if rank > 0 {
            test.given_arms_talent_with_rank(TALENT, rank);
        }
        test.when_swing_is_performed(Hand::Mainhand);
        test.damage_dealt()
    };
    assert!(damage(5) > damage(0));
}
