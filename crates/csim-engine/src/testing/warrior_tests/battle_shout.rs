//! Port of `Test/Warrior/Spells/TestBattleShout`.

use crate::faction::PlayerClass;
use crate::ids::CharId;
use crate::race::Race;
use crate::resource::ResourceType;
use crate::rotation::RotationHost;
use crate::spell::{MAX_RANK, SpellStatus};
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Battle Shout";

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
    assert_eq!(test().base_cooldown(SPELL), "0.000");
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_warrior_has_rage(9);
    test.then_status_is(SPELL, SpellStatus::InsufficientResources);

    test.given_warrior_has_rage(11);
    test.cast(SPELL);
    test.then_warrior_has_rage(1);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_warrior_has_rage(10);
    assert!(!test.on_global_cooldown());
    assert!(!test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(!test.on_global_cooldown());
    test.given_engine_priority_at(0.0);

    test.cast(SPELL);

    assert!(test.on_global_cooldown());
    test.given_engine_priority_at(1.49);
    assert!(test.on_global_cooldown());
    test.given_engine_priority_at(1.51);
    assert!(!test.on_global_cooldown());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

// ---------------------------------------------------------------- party

/// The melee attack power of raid member `id`.
fn melee_ap(test: &WarriorTest, id: CharId) -> u32 {
    let view = test.target().stat_view();
    test.raid.character(id).melee_ap(&view)
}

/// Raid member `id` shouts with the highest rank it has learned.
fn shout(test: &mut WarriorTest, id: CharId) {
    test.raid
        .character_mut(id)
        .gain_resource(ResourceType::Rage, 10);
    test.raid.with_character(id, |ctx| {
        let spell = ctx.spell_by_name(SPELL, MAX_RANK).expect("Battle Shout");
        ctx.cast(spell);
    });
}

/// The attack power Battle Shout gives at the highest rank: Forever's rank 7 gives 139 (the
/// C++ assumed Classic's 232).
const SHOUT_AP: u32 = 139;

#[test]
fn battle_shout_in_party() {
    let mut test = test();
    let warr_1 = test.add_character(PlayerClass::Warrior, Race::Orc, None);
    let warr_2 = test.add_character(PlayerClass::Warrior, Race::Orc, None);
    test.prepare_set_of_combat_iterations();
    // Forever has no Improved Battle Shout (the C++ gave both 5 of 5, for 25 % more).
    let before_1 = melee_ap(&test, warr_1);
    let before_2 = melee_ap(&test, warr_2);

    shout(&mut test, warr_1);

    assert_eq!(melee_ap(&test, warr_1), before_1 + SHOUT_AP);
    assert_eq!(melee_ap(&test, warr_2), before_2 + SHOUT_AP);

    // Does not stack.
    shout(&mut test, warr_2);

    assert_eq!(melee_ap(&test, warr_1), before_1 + SHOUT_AP);
    assert_eq!(melee_ap(&test, warr_2), before_2 + SHOUT_AP);
}

#[test]
fn battle_shout_in_separate_parties() {
    let mut test = test();
    let warr_1 = test.add_character(PlayerClass::Warrior, Race::Orc, Some((0, 1)));
    let warr_2 = test.add_character(PlayerClass::Warrior, Race::Orc, Some((1, 0)));
    test.prepare_set_of_combat_iterations();
    let before_1 = melee_ap(&test, warr_1);
    let before_2 = melee_ap(&test, warr_2);

    shout(&mut test, warr_1);

    assert_eq!(melee_ap(&test, warr_1), before_1 + SHOUT_AP);
    assert_eq!(melee_ap(&test, warr_2), before_2);

    shout(&mut test, warr_2);

    assert_eq!(melee_ap(&test, warr_1), before_1 + SHOUT_AP);
    assert_eq!(melee_ap(&test, warr_2), before_2 + SHOUT_AP);
}
