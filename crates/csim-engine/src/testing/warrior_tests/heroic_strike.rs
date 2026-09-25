//! Port of `Test/Warrior/Spells/TestHeroicStrike`.

use crate::buff::Buff;
use crate::spell::SpellStatus;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Heroic Strike";

/// Forever's Heroic Strike needs a melee weapon (the C++ spell did not): the fixture holds the
/// 100 - 100 test sword.
fn test() -> WarriorTest {
    let mut test = WarriorTest::new(SPELL);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test
}

/// The Heroic Strike swing lands (`calculate_damage`).
fn when_heroic_strike_is_performed(test: &mut WarriorTest) {
    if !test.character().has_mainhand() {
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
    }
    test.when_next_swing_spell_lands(SPELL);
}

fn then_heroic_strike_costs(test: &mut WarriorTest, rage: u32) {
    test.given_warrior_has_rage(rage);
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_has_rage(rage - 1);
    test.then_status_is(SPELL, SpellStatus::InsufficientResources);
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
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    assert!(test.action_ready());
    when_heroic_strike_is_performed(&mut test);
    assert!(test.action_ready());
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn resource_cost() {
    then_heroic_strike_costs(&mut test(), 15);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.when_switching_to_berserker_stance();
    test.given_warrior_has_rage(100);
    assert!(test.on_stance_cooldown());
    test.then_status_is(SPELL, SpellStatus::Available);
}

#[test]
fn improved_hs_reduces_rage_cost() {
    for (rank, cost) in [(1, 14), (2, 13), (3, 12)] {
        let mut test = test();
        test.given_arms_talent_with_rank("Improved Heroic Strike", rank);
        then_heroic_strike_costs(&mut test, cost);
    }
}

#[test]
fn removing_points_in_improved_hs_increases_rage_cost() {
    let mut test = test();
    test.given_arms_talent_with_rank("Improved Heroic Strike", 3);
    let node = test
        .character()
        .talents()
        .and_then(|t| t.node_of_name("Improved Heroic Strike", None))
        .expect("Improved Heroic Strike");
    for cost in [13, 14, 15] {
        assert!(test.with_ctx(|ctx| ctx.decrement_talent(node)));
        then_heroic_strike_costs(&mut test, cost);
    }
}

/// Heroic Strike with the 100 - 100, 2.6 speed test sword against an unarmored target.
fn damage(crit: bool, impale: u32) -> u64 {
    let mut test = test();
    test.given_target_has_0_armor();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_1000_melee_ap();
    if crit {
        test.given_a_guaranteed_melee_ability_crit();
    } else {
        test.given_a_guaranteed_melee_ability_hit();
    }
    test.given_no_previous_damage_dealt();
    test.given_impale(impale);
    when_heroic_strike_is_performed(&mut test);
    test.damage_dealt()
}

#[test]
fn hit_dmg() {
    // [Damage] = base_dmg + (wpn_speed * AP / 14) + hs_flat_dmg
    // [443] = 100 + (2.6 * 1000 / 14) + 157
    assert_eq!(damage(false, 2), 443);
}

#[test]
fn crit_dmg_0_of_2_impale() {
    // [885] = (100 + (2.6 * 1000 / 14) + 157) * 2
    assert_eq!(damage(true, 0), 885);
}

#[test]
fn crit_dmg_1_of_2_impale() {
    // [930] = (100 + (2.6 * 1000 / 14) + 157) * 2.1
    assert_eq!(damage(true, 1), 930);
}

#[test]
fn crit_dmg_2_of_2_impale() {
    // [974] = (100 + (2.6 * 1000 / 14) + 157) * 2.2
    assert_eq!(damage(true, 2), 974);
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    when_heroic_strike_is_performed(&mut test);
    test.then_overpower_is_active();
}

/// The white miss chance of the off hand, in percent of the roll range.
fn oh_white_miss(test: &mut WarriorTest) -> f64 {
    let view = test.target().stat_view();
    let skill = test.oh_weapon_skill();
    let character = test.character_mut();
    let ctx = character.refresh_roll_context(&view);
    character.roll_mut().get_white_miss_chance(&ctx, skill)
}

#[test]
fn miss_chance_while_dual_wielding() {
    let mut test = test();
    test.given_1h_axe_equipped_in_mainhand();
    test.given_1h_axe_equipped_in_offhand();
    test.given_warrior_has_rage(100);
    let dual_wield_miss = oh_white_miss(&mut test);
    assert!(test.character().is_dual_wielding());
    assert!(test.character().uses_dual_wield_hit_table());

    // Queued: the dual wield penalty is gone until the swing lands, but both hands still swing.
    test.cast(SPELL);
    assert!(test.character().is_dual_wielding());
    assert!(!test.character().uses_dual_wield_hit_table());
    let queued_miss = oh_white_miss(&mut test);
    assert!(
        queued_miss < dual_wield_miss,
        "{queued_miss} < {dual_wield_miss}"
    );

    when_heroic_strike_is_performed(&mut test);
    assert!(test.character().is_dual_wielding());
    assert!(test.character().uses_dual_wield_hit_table());
    assert_eq!(oh_white_miss(&mut test), dual_wield_miss);
}

#[test]
fn flurry_charges_not_consumed() {
    let mut test = test();
    test.given_1h_axe_equipped_in_mainhand();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_fury_talent_with_rank("Enrage", 5);
    test.given_fury_talent_with_rank("Flurry", 5);
    test.prepare_set_of_combat_iterations();
    test.given_warrior_has_rage(100);
    let flurry = test.flurry();
    test.apply_buff_id(flurry);
    assert_eq!(test.with_buff_id(flurry, Buff::charges), 3);

    when_heroic_strike_is_performed(&mut test);

    assert_eq!(test.with_buff_id(flurry, Buff::charges), 3);
}
