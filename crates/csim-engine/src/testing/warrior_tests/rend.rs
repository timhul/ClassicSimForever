//! Port of `Test/Warrior/Spells/TestRend`.

use crate::engine::EventType;
use crate::rotation::condition::ConditionContext;
use crate::rotation::executor::RotationHost;
use crate::spell::SpellStatus;
use crate::testing::RUN_EVENT;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Rend";

/// Forever's Rend needs a melee weapon (the C++ spell did not): the fixture holds the test
/// sword.
fn test() -> WarriorTest {
    let mut test = WarriorTest::new(SPELL);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test
}

fn when_rend_is_performed(test: &mut WarriorTest) {
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
    assert_eq!(test.base_cooldown(SPELL), "0.000");

    when_rend_is_performed(&mut test);

    test.then_next_event_is(EventType::PlayerAction, "1.500", false);
    test.then_next_event_is(EventType::DotTick, "3.000", RUN_EVENT);
    test.then_status_is(SPELL, SpellStatus::Available);
    for time in ["6.000", "9.000", "12.000", "15.000", "18.000"] {
        test.then_next_event_is(EventType::DotTick, time, RUN_EVENT);
    }
}

#[test]
fn whether_spell_causes_global_cooldown() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    assert!(test.action_ready());

    when_rend_is_performed(&mut test);

    assert!(test.on_global_cooldown());
    let gcd = format!("{:.3}", test.character().global_cooldown());
    test.then_next_event_is(EventType::PlayerAction, &gcd, false);
}

#[test]
fn how_spell_observes_global_cooldown() {
    let mut test = test();
    test.then_status_is(SPELL, SpellStatus::Available);
    test.given_warrior_is_on_gcd();
    test.then_status_is(SPELL, SpellStatus::OnGcd);
}

#[test]
fn resource_cost() {
    let mut test = test();
    test.given_warrior_has_rage(9);
    test.then_status_is(SPELL, SpellStatus::InsufficientResources);

    test.given_warrior_has_rage(10);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    when_rend_is_performed(&mut test);
    test.then_warrior_has_rage(0);
}

#[test]
fn is_ready_conditions() {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.given_warrior_in_berserker_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::InBerserkerStance);
}

#[test]
fn stance_cooldown() {
    let mut test = test();
    test.given_warrior_in_defensive_stance();
    test.given_warrior_has_rage(100);
    test.then_status_is(SPELL, SpellStatus::Available);

    test.when_switching_to_battle_stance();
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

/// Rend with `improved_rend` of 3 Improved Rend: all its ticks, and when the last one came.
fn rend_damage(improved_rend: u32) -> (u64, f64) {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_hit();
    test.given_1000_melee_ap();
    if improved_rend > 0 {
        test.given_arms_talent_with_rank("Improved Rend", improved_rend);
    }
    test.given_no_previous_damage_dealt();
    when_rend_is_performed(&mut test);
    test.when_running_only(EventType::DotTick);
    (test.damage_dealt(), test.now())
}

// Forever's rank 7 deals a flat 21 per tick over 21 s (the C++ added a share of the weapon
// damage, for 162), and Improved Rend adds 12, 23 and 35 % to each tick (the C++ 15, 25 and
// 35 % to the total).

#[test]
fn damage_of_improved_rend() {
    for (rank, damage) in [(0, 147), (1, 168), (2, 182), (3, 196)] {
        // [Damage] = 7 * round(21 * improved_rend_percent)
        assert_eq!(
            rend_damage(rank),
            (damage, 21.0),
            "{rank} of 3 Improved Rend"
        );
    }
}

/// Rend's ticks crit (its `PERIODIC_CAN_CRIT`) at the melee ability crit chance, for double
/// damage on top of Improved Rend: 7 * round(21 * 2) and 7 * round(21 * 1.35 * 2).
#[test]
fn rend_ticks_crit() {
    for (improved_rend, damage) in [(0, 294), (3, 399)] {
        let mut test = test();
        test.given_a_guaranteed_melee_ability_crit();
        if improved_rend > 0 {
            test.given_arms_talent_with_rank("Improved Rend", improved_rend);
        }
        test.given_no_previous_damage_dealt();
        when_rend_is_performed(&mut test);
        test.when_running_only(EventType::DotTick);
        assert_eq!(
            test.damage_dealt(),
            damage,
            "{improved_rend} of 3 Improved Rend"
        );
        let statistics = test.character().statistics();
        let (_, rend) = statistics
            .spells()
            .find(|(key, _)| key.name == SPELL)
            .expect("Rend statistics");
        assert_eq!(rend.total_damage(), damage);
        assert!(rend.crits() >= 7, "every tick is a crit");
    }
}

/// Death Wish's 20 % applies to every tick, on top of Improved Rend: 7 * round(21 * 1.2) and
/// 7 * round(21 * 1.35 * 1.2).
#[test]
fn death_wish_increases_rend_damage() {
    for (improved_rend, damage) in [(0, 175), (3, 238)] {
        let mut test = WarriorTest::unprepared(SPELL);
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
        test.enable_spell("Death Wish");
        if improved_rend > 0 {
            test.given_arms_talent_with_rank("Improved Rend", improved_rend);
        }
        test.prepare_set_of_combat_iterations();
        test.given_a_guaranteed_melee_ability_hit();
        test.given_warrior_has_rage(100);
        test.cast("Death Wish");
        test.given_engine_priority_pushed_forward(1.5);
        test.given_warrior_has_rage(100);
        test.given_no_previous_damage_dealt();
        when_rend_is_performed(&mut test);
        test.when_running_only(EventType::DotTick);
        assert_eq!(
            test.damage_dealt(),
            damage,
            "{improved_rend} of 3 Improved Rend"
        );
    }
}

#[test]
fn dodge_applies_overpower_buff() {
    let mut test = test();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_a_guaranteed_melee_ability_dodge();
    when_rend_is_performed(&mut test);
    test.then_overpower_is_active();
}

#[test]
fn rotations_read_the_debuff_of_the_rank_cast() {
    // Every rank owns a debuff called "Rend"; the rotation casts the highest learned rank, so
    // `buff_duration "Rend"` must read that rank's debuff, not the first rank's.
    let mut test = test();
    test.given_a_guaranteed_melee_ability_hit();
    when_rend_is_performed(&mut test);
    let time_left = test.with_ctx(|ctx| {
        let rend = RotationHost::buff_by_name(ctx, "Rend").expect("Rend's debuff");
        ctx.buff_time_left(&rend)
    });
    assert_eq!(time_left, 21.0);
}
