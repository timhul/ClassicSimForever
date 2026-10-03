//! Thunder Clap (rank 6): 103 physical damage plus 3 % of attack power (server side, the
//! tables have no attack power coefficient), on the magic table (`DefenseType` magic): it
//! crits with the spell crit chance, for the spell crit multiplier, and a crit applies Deep
//! Wounds but not Flurry ("melee critical strike").

use crate::engine::EventType;
use crate::testing::warrior::WarriorTest;

const SPELL: &str = "Thunder Clap";
const DEEP_WOUNDS: &str = "Deep Wounds";

/// A warrior in Battle Stance with the 100 - 100 test sword and 1000 AP, against an unarmored
/// target, Deep Wounds at `deep_wounds` ranks. The spell hit leaves the 1 % minimum miss
/// chance; the seeded rolls land.
fn test(deep_wounds: u32) -> WarriorTest {
    test_with(|test| {
        if deep_wounds > 0 {
            test.given_arms_talent_with_rank("Improved Rend", 3);
            test.given_arms_talent_with_rank(DEEP_WOUNDS, deep_wounds);
        }
    })
}

/// [`test`] with the talents `given` picks (before the iterations are prepared).
fn test_with(given: impl FnOnce(&mut WarriorTest)) -> WarriorTest {
    let mut test = WarriorTest::unprepared(SPELL);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    given(&mut test);
    test.prepare_set_of_combat_iterations();
    test.given_target_has_0_armor();
    test.given_1000_melee_ap();
    test.stats_mut().increase_spell_hit(10_000);
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.given_no_previous_damage_dealt();
    test
}

fn given_a_guaranteed_spell_crit(test: &mut WarriorTest) {
    test.stats_mut().increase_spell_crit(1_000_000);
}

fn given_no_crit(test: &mut WarriorTest) {
    test.stats_mut().increase_crit_penalty(1_000_000);
}

/// Casts Thunder Clap and returns its damage and whether it crit.
fn when_thunder_clap_is_performed(test: &mut WarriorTest) -> (u64, bool) {
    let report = test.cast(SPELL);
    let attack = report.attack.expect("Thunder Clap rolled");
    let crit = attack.spell.expect("on the magic table").roll.is_critical();
    (test.damage_dealt_by(SPELL), crit)
}

/// Runs every queued bleed tick and returns the Deep Wounds damage.
fn deep_wounds_damage(test: &mut WarriorTest) -> u64 {
    test.when_running_only(EventType::DotTick);
    test.damage_dealt_by(DEEP_WOUNDS)
}

#[test]
fn damage_adds_3_percent_of_attack_power() {
    // [133] = 103 + 1000 * 0.03
    let mut test = test(0);
    given_no_crit(&mut test);
    assert_eq!(when_thunder_clap_is_performed(&mut test), (133, false));
}

#[test]
fn crits_with_the_spell_crit_chance_for_the_spell_crit_multiplier() {
    // [200] = (103 + 1000 * 0.03) * 1.5
    let mut test = test(0);
    given_a_guaranteed_spell_crit(&mut test);
    assert_eq!(when_thunder_clap_is_performed(&mut test), (200, true));
}

#[test]
fn melee_crit_does_not_make_it_crit() {
    let mut test = test(0);
    test.given_a_guaranteed_melee_ability_crit();
    test.given_a_guaranteed_white_crit();
    assert_eq!(when_thunder_clap_is_performed(&mut test), (133, false));
}

#[test]
fn critical_thunder_clap_applies_deep_wounds_when_talented() {
    // 60 % of the 100 average weapon damage plus 2 % of the 1000 attack power.
    let mut test = test(3);
    given_a_guaranteed_spell_crit(&mut test);
    assert!(when_thunder_clap_is_performed(&mut test).1);
    assert_eq!(deep_wounds_damage(&mut test), 80);
}

#[test]
fn critical_thunder_clap_does_not_apply_deep_wounds_without_the_talent() {
    let mut test = test(0);
    given_a_guaranteed_spell_crit(&mut test);
    assert!(when_thunder_clap_is_performed(&mut test).1);
    assert_eq!(deep_wounds_damage(&mut test), 0);
}

#[test]
fn regular_hit_thunder_clap_does_not_apply_deep_wounds() {
    let mut test = test(3);
    given_no_crit(&mut test);
    assert!(!when_thunder_clap_is_performed(&mut test).1);
    assert_eq!(deep_wounds_damage(&mut test), 0);
}

/// Flurry's tooltip reads "melee critical strike": Thunder Clap is a physical spell, not a
/// melee attack. The tables' `ProcTypeMask` (any damage) says otherwise; to be confirmed in
/// game.
#[test]
fn critical_thunder_clap_triggers_flurry() {
    let mut test = test_with(|test| {
        test.given_fury_talent_with_rank("Enrage", 5);
        test.given_fury_talent_with_rank("Flurry", 5);
    });
    given_a_guaranteed_spell_crit(&mut test);
    assert!(when_thunder_clap_is_performed(&mut test).1);
    let flurry = test.flurry();
    // TODO:  Unlikely that TC actually procs Flurry regardless of what ProcTypeMask says.
    assert!(test.with_buff_id(flurry, |buff| buff.is_active()));
}
