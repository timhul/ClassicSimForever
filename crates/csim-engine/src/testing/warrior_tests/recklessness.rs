//! Port of `Test/Warrior/Spells/TestRecklessness`.

use crate::character_loader::MAX_TARGET_LEVEL;
use crate::engine::EventType;
use crate::mechanics::Mechanics;
use crate::spell::{Hand, SpellStatus};
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Recklessness";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

fn when_recklessness_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.cast(SPELL);
}

fn gcd(test: &WarriorTest) -> String {
    format!("{:.3}", test.character().global_cooldown())
}

fn when_reck_and_mh_attack_is_performed(test: &mut WarriorTest) {
    when_recklessness_is_performed(test);
    let gcd = gcd(test);
    test.then_next_event_is(EventType::PlayerAction, &gcd, false);
    test.when_swing_is_performed(Hand::Mainhand);
}

fn when_reck_and_whirlwind_is_performed(test: &mut WarriorTest) {
    when_recklessness_is_performed(test);
    let gcd = gcd(test);
    test.then_next_event_is(EventType::PlayerAction, &gcd, false);
    test.cast("Whirlwind");
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
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    assert_eq!(test.base_cooldown(SPELL), "1800.000");

    when_recklessness_is_performed(&mut test);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::BuffRemoval, "15.000", false);
    test.then_next_event_is(EventType::PlayerAction, "1800.000", false);
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.given_warrior_in_berserker_stance();
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_warrior_has_rage(0);
    test.given_warrior_in_battle_stance();
    assert!(test.action_ready());
    test.then_status_is(SPELL, SpellStatus::InBattleStance);

    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBattleStance);

    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.given_warrior_in_berserker_stance();
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_battle_stance();
    test.when_switching_to_berserker_stance();
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnGcd);

    test.given_engine_priority_pushed_forward(0.99);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::OnStanceCooldown);

    test.given_engine_priority_pushed_forward(0.02);
    assert!(!test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn crit_reduced_after_buff_expires() {
    let mut test = test();
    let crit = mh_crit(&test);

    // Forever's Recklessness is +100 % aura crit, suppressed against the level 63 target like
    // any aura crit (the C++ added 999 999 hundredths of a percent).
    when_recklessness_is_performed(&mut test);
    let bonus = Mechanics::new(MAX_TARGET_LEVEL).suppressed_aura_crit_chance(60, 10_000);
    assert_eq!(mh_crit(&test), crit + bonus);

    test.when_running_queued_events_until(15.01);
    test.given_warrior_in_battle_stance();
    assert_eq!(mh_crit(&test), crit);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(test.action_ready());
    when_recklessness_is_performed(&mut test);
    assert!(!test.action_ready());
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(0);
    when_recklessness_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

/// Recklessness, then a Whirlwind (`white` false) or a main-hand swing of the test sword
/// against an unarmored target with 1000 AP.
fn damage(force: fn(&mut WarriorTest), white: bool) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_1000_melee_ap();
    force(&mut test);
    if white {
        when_reck_and_mh_attack_is_performed(&mut test);
    } else {
        when_reck_and_whirlwind_is_performed(&mut test);
    }
    test.damage_dealt()
}

#[test]
fn ability_miss_still_misses() {
    assert_eq!(
        damage(|t| t.given_a_guaranteed_melee_ability_miss(), false),
        0
    );
}

#[test]
fn ability_dodge_still_dodges() {
    assert_eq!(
        damage(|t| t.given_a_guaranteed_melee_ability_dodge(), false),
        0
    );
}

#[test]
fn ability_parry_still_parries() {
    assert_eq!(
        damage(|t| t.given_a_guaranteed_melee_ability_parry(), false),
        0
    );
}

#[test]
fn ability_block_still_blocks() {
    // [Damage] = base_dmg + (normalized_wpn_speed * AP / 14)
    // [271] = 100 + (2.4 * 1000 / 14)
    assert_eq!(
        damage(|t| t.given_a_guaranteed_melee_ability_block(), false),
        271
    );
}

#[test]
fn white_miss_still_misses() {
    assert_eq!(damage(|t| t.given_a_guaranteed_white_miss(), true), 0);
}

#[test]
fn white_dodge_still_dodges() {
    assert_eq!(damage(|t| t.given_a_guaranteed_white_dodge(), true), 0);
}

#[test]
fn white_parry_still_parries() {
    assert_eq!(damage(|t| t.given_a_guaranteed_white_parry(), true), 0);
}

#[test]
fn white_block_still_blocks() {
    // The C++ expected 0 (its blocks absorbed the whole swing); here the target blocks
    // nothing, so the block lands as a hit: [286] = 100 + (2.6 * 1000 / 14).
    assert_eq!(damage(|t| t.given_a_guaranteed_white_block(), true), 286);
}

#[test]
fn glancing_hits_still_glances() {
    // [157 - 214] = (100 + (2.6 * 1000 / 14)) * [0.55 - 0.75]
    let damage = damage(|t| t.given_a_guaranteed_white_glancing_blow(), true);
    assert!((157..=214).contains(&damage), "{damage}");
}

#[test]
fn white_crit_still_crits() {
    // [571] = (100 + (2.6 * 1000 / 14)) * 2.0
    assert_eq!(damage(|t| t.given_a_guaranteed_white_crit(), true), 571);
}

#[test]
fn ability_hit_converted_to_crit() {
    // No avoidance and the character's own crit chance: Recklessness makes it a crit (the
    // C++ forced the crit, as in the test below).
    // [543] = (100 + (2.4 * 1000 / 14)) * 2.0
    assert_eq!(damage(|t| t.given_no_melee_ability_avoidance(), false), 543);
}

#[test]
fn ability_crit_still_crits() {
    // [543] = (100 + (2.4 * 1000 / 14)) * 2.0
    assert_eq!(
        damage(|t| t.given_a_guaranteed_melee_ability_crit(), false),
        543
    );
}
