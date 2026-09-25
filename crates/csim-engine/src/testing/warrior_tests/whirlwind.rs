//! Port of `Test/Warrior/Spells/TestWhirlwind`.

use crate::combat_roll::PhysicalAttackResult;
use crate::engine::EventType;
use crate::proc::ProcSource;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Whirlwind";

fn test() -> WarriorTest {
    WarriorTest::new(SPELL)
}

fn when_whirlwind_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.cast(SPELL);
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
    assert_eq!(test.base_cooldown(SPELL), "10.000");

    when_whirlwind_is_performed(&mut test);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::PlayerAction, "10.000", false);
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(test.action_ready());
    when_whirlwind_is_performed(&mut test);
    assert!(!test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_is_on_gcd_from("Execute");

    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBattleStance);

    test.when_switching_to_berserker_stance();
    test.given_warrior_has_rage(100);
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
fn resource_cost() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(25);
    when_whirlwind_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

/// Whirlwind with the 100 - 100 test sword against an unarmored target, 1000 AP.
fn damage(crit: bool, impale: u32) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    if crit {
        test.given_a_guaranteed_melee_ability_crit();
    } else {
        test.given_a_guaranteed_melee_ability_hit();
    }
    test.given_1000_melee_ap();
    test.given_no_previous_damage_dealt();
    test.given_impale(impale);
    when_whirlwind_is_performed(&mut test);
    test.damage_dealt()
}

#[test]
fn hit_dmg() {
    // [Damage] = base_dmg + (normalized_wpn_speed * AP / 14)
    // [271] = 100 + (2.4 * 1000 / 14)
    assert_eq!(damage(false, 2), 271);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    // [543] = (100 + (2.4 * 1000 / 14)) * 2.0
    assert_eq!(damage(true, 0), 543);
}

#[test]
fn crit_dmg_1_of_2_impale() {
    // [570] = (100 + (2.4 * 1000 / 14)) * 2.1
    assert_eq!(damage(true, 1), 570);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    // [597] = (100 + (2.4 * 1000 / 14)) * 2.2
    assert_eq!(damage(true, 2), 597);
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    when_whirlwind_is_performed(&mut test);
    test.then_overpower_is_active();
}

// ---------------------------------------------------------------- Raging Blows (off hand)

const OFFHAND: &str = "Whirlwind Off-Hand";

/// Dual wielding the 100 - 100 test swords against an unarmored target, 1000 AP.
fn dual_wield_test(raging_blows: bool) -> WarriorTest {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_1000_melee_ap();
    if raging_blows {
        test.given_fury_talent_with_rank("Raging Blows", 1);
    }
    test.given_no_previous_damage_dealt();
    test
}

fn offhand_attempts(test: &WarriorTest) -> u64 {
    test.character()
        .statistics()
        .spell_statistics(OFFHAND, 1)
        .map_or(0, |s| s.total_attempts())
}

#[test]
fn whirlwind_strikes_with_the_mainhand_only_without_raging_blows() {
    let mut test = dual_wield_test(false);
    test.given_a_guaranteed_melee_ability_hit();
    let report = test.cast(SPELL);
    assert!(report.offhand.is_none());
    assert_eq!(test.damage_dealt(), 271);
    assert_eq!(offhand_attempts(&test), 0);
}

#[test]
fn raging_blows_adds_an_offhand_strike() {
    let mut test = dual_wield_test(true);
    test.given_a_guaranteed_melee_ability_hit();
    let report = test.cast(SPELL);
    // Off hand: [136] = (100 + (2.4 * 1000 / 14)) * 0.5
    assert_eq!(report.offhand.unwrap().attack.damage, 136);
    assert_eq!(test.damage_dealt_by(SPELL), 271);
    assert_eq!(test.damage_dealt_by(OFFHAND), 136);
    assert_eq!(offhand_attempts(&test), 1);
}

#[test]
fn offhand_strike_crits_like_the_mainhand() {
    let mut test = dual_wield_test(true);
    test.given_a_guaranteed_melee_ability_crit();
    test.cast(SPELL);
    // [543] = (100 + (2.4 * 1000 / 14)) * 2.0, [271] = the same * 0.5 * 2.0
    assert_eq!(test.damage_dealt_by(SPELL), 543);
    assert_eq!(test.damage_dealt_by(OFFHAND), 271);
}

#[test]
fn offhand_strike_uses_the_dual_wield_specialization_penalty() {
    let mut test = dual_wield_test(true);
    test.given_fury_talent_with_rank("Dual Wield Specialization", 5);
    test.given_a_guaranteed_melee_ability_hit();
    test.cast(SPELL);
    // [170] = (100 + (2.4 * 1000 / 14)) * 0.5 * 1.25
    assert_eq!(test.damage_dealt_by(OFFHAND), 170);
}

#[test]
fn offhand_strike_rolls_on_its_own() {
    let mut test = dual_wield_test(true);
    test.given_a_guaranteed_melee_ability_dodge();
    let report = test.cast(SPELL);
    // The main hand was dodged; the off hand still swings (and is dodged too).
    assert_eq!(
        report.offhand.unwrap().attack.result,
        PhysicalAttackResult::Dodge
    );
    let offhand = test
        .character()
        .statistics()
        .spell_statistics(OFFHAND, 1)
        .unwrap();
    assert_eq!(offhand.dodges(), 1);
    assert_eq!(test.damage_dealt(), 0);
}

/// Raging Blows on a dual wielder with 1000 AP against an unarmored target.
fn raging_blows_test() -> WarriorTest {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_1000_melee_ap();
    test.given_fury_talent_with_rank("Raging Blows", 1);
    test.given_no_previous_damage_dealt();
    test
}

#[test]
fn offhand_strike_hits_when_the_mainhand_is_dodged() {
    let mut test = raging_blows_test();
    test.given_a_mainhand_ability_dodge_and_an_offhand_ability_hit();
    let report = test.cast(SPELL);
    assert_eq!(report.attack.unwrap().result, PhysicalAttackResult::Dodge);
    let offhand = report.offhand.unwrap().attack;
    assert_eq!(offhand.result, PhysicalAttackResult::Hit);
    assert!(offhand.damage > 0);
    assert_eq!(test.damage_dealt_by(SPELL), 0);
    assert_eq!(test.damage_dealt_by(OFFHAND), u64::from(offhand.damage));
}

#[test]
fn offhand_strike_is_dodged_when_the_mainhand_hits() {
    let mut test = raging_blows_test();
    test.given_a_mainhand_ability_hit_and_an_offhand_ability_dodge();
    let report = test.cast(SPELL);
    let mainhand = report.attack.unwrap();
    assert_eq!(mainhand.result, PhysicalAttackResult::Hit);
    assert!(mainhand.damage > 0);
    assert_eq!(
        report.offhand.unwrap().attack.result,
        PhysicalAttackResult::Dodge
    );
    assert_eq!(test.damage_dealt_by(SPELL), u64::from(mainhand.damage));
    assert_eq!(test.damage_dealt_by(OFFHAND), 0);
}

#[test]
fn no_offhand_strike_with_a_twohander() {
    let mut test = test();
    test.given_a_twohand_weapon_with_100_min_max_dmg();
    test.given_fury_talent_with_rank("Raging Blows", 1);
    test.given_a_guaranteed_melee_ability_hit();
    let report = test.cast(SPELL);
    assert!(report.offhand.is_none());
    assert_eq!(offhand_attempts(&test), 0);
}

#[test]
fn offhand_strike_costs_no_extra_rage() {
    let mut test = dual_wield_test(true);
    test.given_a_guaranteed_melee_ability_hit();
    test.given_warrior_has_rage(25);
    let report = test.cast(SPELL);
    assert!(report.offhand.is_some());
    test.then_warrior_has_rage(0);
}

#[test]
fn offhand_strike_is_an_offhand_proc_event() {
    let mut test = dual_wield_test(true);
    test.given_a_guaranteed_melee_ability_crit();
    let report = test.cast(SPELL);
    assert_eq!(
        report.offhand.unwrap().proc_sources,
        [ProcSource::OffhandSpell, ProcSource::MeleeCritical]
    );
    assert!(!report.proc_sources.contains(&ProcSource::OffhandSpell));
}
