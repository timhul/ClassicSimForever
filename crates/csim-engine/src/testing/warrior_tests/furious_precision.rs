//! Furious Precision's hit chance.
//!
//! The Warrior talent (1323963, new in the 70205 hotfixes) increases "chance to hit with off-hand
//! attacks" by 4/7/10 % (E0 `MOD_HIT_CHANCE`, an `OFFHAND_HIT_CHANCE` script): the off-hand auto
//! attack and the off-hand strikes get it, the main hand's attacks do not. Dual Wield
//! Specialization, which held this hit before the hotfixes, no longer gives any; neither does
//! the Rogue's (13715).

use crate::character::context::CharacterContext;
use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult, ROLL_RANGE};
use crate::effect::EffectHost;
use crate::faction::PlayerClass;
use crate::race::Race;
use crate::raid::SharedBuffRegistry;
use crate::spell::{AutoAttackHost, Hand, SpellHost};
use crate::testing::SpellTest;
use crate::testing::warrior::WarriorTest;

const TALENT: &str = "Furious Precision";
const DUAL_WIELD_SPECIALIZATION: &str = "Dual Wield Specialization";

/// A dual-wielding Warrior (two test swords: 300 skill against a level 63 target).
fn test() -> WarriorTest {
    let mut test = WarriorTest::new(TALENT);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test
}

fn melee_hit(test: &SpellTest) -> u32 {
    test.stat(|stats, ctx| stats.get_melee_hit_chance(ctx))
}

fn offhand_hit(test: &SpellTest) -> u32 {
    test.stat(|stats, _| stats.get_offhand_melee_hit_chance())
}

/// The miss ranges (out of 10 000) of the attack tables the character rolls on:
/// main-hand white, off-hand white, main-hand yellow, off-hand yellow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MissRanges {
    mh_white: u32,
    oh_white: u32,
    mh_yellow: u32,
    oh_yellow: u32,
}

fn miss_ranges(test: &mut SpellTest) -> MissRanges {
    let view = test.target().stat_view();
    let mh = test.mh_weapon_skill();
    let oh = test.oh_weapon_skill();
    let character = test.character_mut();
    let ctx = character.refresh_roll_context(&view);
    let roll = character.roll_mut();
    MissRanges {
        mh_white: roll
            .get_melee_white_table(&ctx, Hand::Mainhand, mh)
            .miss_range(),
        oh_white: roll
            .get_melee_white_table(&ctx, Hand::Offhand, oh)
            .miss_range(),
        mh_yellow: roll
            .get_melee_special_table(&ctx, Hand::Mainhand, mh)
            .miss_range(),
        oh_yellow: roll
            .get_melee_special_table(&ctx, Hand::Offhand, oh)
            .miss_range(),
    }
}

/// The share of `ROLL_RANGE` rolls of `roll` that miss, in rolls out of 10 000.
fn misses_of(
    test: &mut WarriorTest,
    roll: fn(&mut CharacterContext<'_, SharedBuffRegistry>) -> PhysicalAttackResult,
) -> u32 {
    test.with_ctx(|ctx| {
        (0..ROLL_RANGE)
            .filter(|_| roll(ctx) == PhysicalAttackResult::Miss)
            .count() as u32
    })
}

#[test]
fn each_rank_gives_off_hand_hit_and_no_melee_hit() {
    for (rank, hit) in [(1, 400), (2, 700), (3, 1000)] {
        let mut test = test();
        assert_eq!((melee_hit(&test), offhand_hit(&test)), (0, 0));
        test.given_fury_talent_with_rank(TALENT, rank);
        assert_eq!(melee_hit(&test), 0, "{rank} of 3: no hit for every attack");
        assert_eq!(offhand_hit(&test), hit, "{rank} of 3: off-hand hit");
    }
}

#[test]
fn only_the_off_hand_tables_lose_miss_chance() {
    let mut test = test();
    let before = miss_ranges(&mut test);
    // Dual wielding with 300 skill against 315 defense: 27 % white, 8 % yellow.
    assert_eq!(
        before,
        MissRanges {
            mh_white: 2700,
            oh_white: 2700,
            mh_yellow: 800,
            oh_yellow: 800,
        }
    );

    test.given_fury_talent_with_rank(TALENT, 3);
    assert_eq!(
        miss_ranges(&mut test),
        MissRanges {
            mh_white: 2700,
            oh_white: 1700,
            mh_yellow: 800,
            oh_yellow: 0,
        }
    );
}

/// Dual Wield Specialization 5 of 5 gives no hit since the 70205 hotfixes (its E1 is the
/// off-hand rage now); Furious Precision's adds to nothing else.
#[test]
fn dual_wield_specialization_gives_no_hit() {
    let mut test = test();
    test.given_fury_talent_with_rank(DUAL_WIELD_SPECIALIZATION, 5);
    assert_eq!((melee_hit(&test), offhand_hit(&test)), (0, 0));
    test.given_fury_talent_with_rank(TALENT, 3);
    assert_eq!((melee_hit(&test), offhand_hit(&test)), (0, 1000));
}

/// The swings and strikes as the character rolls them: the off hand's misses drop by 10 %,
/// the main hand's do not move.
#[test]
fn the_character_rolls_off_hand_attacks_with_the_hit() {
    let mut test = test();
    test.given_fury_talent_with_rank(TALENT, 3);
    let mh_white = misses_of(&mut test, |ctx| ctx.roll_melee_hit(Hand::Mainhand));
    let oh_white = misses_of(&mut test, |ctx| ctx.roll_melee_hit(Hand::Offhand));
    let mh_yellow = misses_of(&mut test, |ctx| {
        ctx.roll_melee_ability(IncludedOutcomes::ALL, 0, true)
    });
    let oh_yellow = misses_of(&mut test, |ctx| {
        ctx.roll_offhand_melee_ability(IncludedOutcomes::ALL, 0, true)
    });
    let around = |misses: u32, expected: u32| misses.abs_diff(expected) <= 150;
    assert!(around(mh_white, 2700), "main-hand white: {mh_white}");
    assert!(around(oh_white, 1700), "off-hand white: {oh_white}");
    assert!(around(mh_yellow, 800), "main-hand yellow: {mh_yellow}");
    assert_eq!(oh_yellow, 0, "off-hand yellow");
}

/// The talent requires a one-handed weapon in the off hand (`SpellEquippedItems` mask 41105,
/// inventory type off-hand weapon): a two-hander gets nothing.
#[test]
fn no_hit_with_a_two_hander() {
    let mut test = WarriorTest::new(TALENT);
    test.given_fury_talent_with_rank(TALENT, 3);
    test.given_2h_axe_equipped();
    assert_eq!((melee_hit(&test), offhand_hit(&test)), (0, 0));
    test.given_1h_axe_equipped_in_mainhand();
    test.given_1h_axe_equipped_in_offhand();
    assert_eq!((melee_hit(&test), offhand_hit(&test)), (0, 1000));
}

/// The Rogue's Dual Wield Specialization has no hit effect: 5 of 5 changes no hit chance.
#[test]
fn the_rogue_talent_gives_no_hit() {
    let mut test = SpellTest::new(PlayerClass::Rogue, Race::Human, TALENT);
    test.prepare_set_of_combat_iterations();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    let before = miss_ranges(&mut test);
    test.given_talent_rank("Combat", "Precision", 3);
    test.given_talent_rank("Combat", DUAL_WIELD_SPECIALIZATION, 5);
    assert_eq!((melee_hit(&test), offhand_hit(&test)), (300, 0));
    assert_eq!(
        miss_ranges(&mut test),
        MissRanges {
            mh_white: before.mh_white - 300,
            oh_white: before.oh_white - 300,
            mh_yellow: before.mh_yellow - 300,
            oh_yellow: before.oh_yellow - 300,
        }
    );
}
