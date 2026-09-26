//! Touch of the Grave, the Undead racial: melee swings and abilities have a 5 % chance (1 s
//! internal cooldown) to drain 5 % of the warrior's maximum health from the target as shadow
//! damage (1260198, `HEALTH_LEECH`). The magic hit table is not ported: the drain always lands.

use crate::proc::ProcSource;
use crate::race::Race;
use crate::spell::SpellHost;
use crate::testing::warrior::WarriorTest;

const PROC: &str = "Touch of the Grave";
const DRAIN: u32 = 1260198;

fn test() -> WarriorTest {
    let mut test = WarriorTest::unprepared_of_race(Race::Undead, PROC);
    test.prepare_set_of_combat_iterations();
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test
}

/// The game ids of the enabled procs named Touch of the Grave.
fn enabled(test: &WarriorTest) -> Vec<u32> {
    let procs = test.character().spells().procs();
    procs
        .enabled()
        .iter()
        .map(|&id| procs.get(id))
        .filter(|proc| proc.name() == PROC)
        .map(|proc| proc.spell().game_id())
        .collect()
}

#[test]
fn undead_have_it_and_orcs_do_not() {
    // The priest / mage / warlock variant (1260201) is ignored and never rolls.
    assert_eq!(enabled(&test()), [1260189]);
    assert!(enabled(&WarriorTest::new(PROC)).is_empty());
}

#[test]
fn procs_5_percent_of_swings_and_abilities() {
    let mut test = test();
    for source in [
        ProcSource::MainhandSwing,
        ProcSource::OffhandSwing,
        ProcSource::MainhandSpell,
        ProcSource::OffhandSpell,
    ] {
        assert!(test.proc_conditions_fulfilled(PROC, source), "{source:?}");
        assert_eq!(test.proc_range(PROC, source), 500, "{source:?}");
    }
    assert!(!test.proc_conditions_fulfilled(PROC, ProcSource::MeleeMiss));
}

#[test]
fn drains_5_percent_of_max_health() {
    let mut test = test();
    let max_health = test.stat(|s, c| s.get_max_health(c));
    let report = test
        .with_ctx(|ctx| ctx.trigger_spell(DRAIN, None))
        .expect("the drain is learned");
    let attack = report.attack.expect("an attack");
    assert_eq!(attack.damage, (f64::from(max_health) * 0.05).round() as u32);

    test.stats_mut().increase_stamina(100);
    let report = test.with_ctx(|ctx| ctx.trigger_spell(DRAIN, None)).unwrap();
    assert_eq!(
        report.attack.unwrap().damage,
        attack.damage + 50,
        "100 stamina: +1000 health"
    );
}
