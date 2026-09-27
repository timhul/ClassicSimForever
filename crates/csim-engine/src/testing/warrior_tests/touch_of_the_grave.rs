//! Touch of the Grave, the Undead racial: melee swings and abilities have a 5 % chance (1 s
//! internal cooldown) to drain 5 % of the warrior's maximum health from the target as shadow
//! damage (1260198, `HEALTH_LEECH`). The drain rolls the magic table: a binary spell (it also
//! heals), it misses a boss 17 % of the time less the spell hit, is fully resisted by the
//! target's shadow resistance, and crits with the spell crit chance.

use crate::combat_roll::{MagicAttackResult, MagicResistResult, SpellRoll};
use crate::magic_school::MagicSchool;
use crate::proc::ProcSource;
use crate::race::Race;
use crate::spell::{AttackOutcome, SpellHost};
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

/// The attack of one drain.
fn drain(test: &mut WarriorTest) -> AttackOutcome {
    test.with_ctx(|ctx| ctx.trigger_spell(DRAIN, None))
        .expect("the drain is learned")
        .attack
        .expect("an attack")
}

/// The damage of the first drain that lands without a crit.
fn landed_drain(test: &mut WarriorTest) -> u32 {
    (0..100)
        .map(|_| drain(test))
        .find(|attack| {
            attack
                .spell
                .is_some_and(|spell| spell.roll == SpellRoll::HIT)
        })
        .expect("a drain lands")
        .damage
}

#[test]
fn drains_5_percent_of_max_health() {
    let mut test = test();
    let max_health = test.stat(|s, c| s.get_max_health(c));
    let damage = landed_drain(&mut test);
    assert_eq!(damage, (f64::from(max_health) * 0.05).round() as u32);

    test.stats_mut().increase_stamina(100);
    assert_eq!(
        landed_drain(&mut test),
        damage + 50,
        "100 stamina: +1000 health"
    );
}

#[test]
fn drains_roll_the_magic_table() {
    let share = |test: &mut WarriorTest, f: &dyn Fn(SpellRoll) -> bool| {
        let n = 20_000;
        let count = (0..n)
            .filter(|_| f(drain(test).spell.expect("on the magic table").roll))
            .count();
        count as f64 / f64::from(n)
    };
    let mut test = test();
    let missed = share(&mut test, &|roll| roll.result == MagicAttackResult::Miss);
    assert!((missed - 0.17).abs() < 0.01, "missed {missed}");
    test.stats_mut().increase_spell_crit(1000);
    let crit_chance = test.stat(|s, c| s.get_spell_crit_chance(c, MagicSchool::Shadow));
    let crits = share(&mut test, &SpellRoll::is_critical);
    let expected = 0.83 * f64::from(crit_chance) / 10_000.0;
    assert!((crits - expected).abs() < 0.01, "crits {crits}");

    // 150 shadow resistance of the 300 cap: 83 % × (1 − 37.5 %) lands, never partially.
    test.target_mut().set_resistance(MagicSchool::Shadow, 150);
    let resisted = share(&mut test, &|roll| roll == SpellRoll::FULL_RESIST);
    assert!(
        (resisted - 0.83 * 0.375).abs() < 0.01,
        "resisted {resisted}"
    );
    let partial = share(&mut test, &|roll| {
        roll.landed() && roll.resist != MagicResistResult::NoResist
    });
    assert_eq!(partial, 0.0, "a binary spell is never partially resisted");
}
