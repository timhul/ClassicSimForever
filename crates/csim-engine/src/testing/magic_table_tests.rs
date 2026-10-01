//! The magic table on the shipped data: Ebon Hand's Shadow Bolt (18211), a shadow spell with
//! `DefenseType` magic that only deals damage (non-binary), cast by a level 60 warrior at a
//! level 63 boss: 17 % misses less the spell hit, partial resists from the boss's 24
//! level-based resistance plus its own, crits from the spell crit chance.

use crate::combat_log::{CombatLogEvent, MissType};
use crate::item::EquipmentSlot;
use crate::magic_school::MagicSchool;
use crate::mechanics::Mechanics;
use crate::statistics::SpellStatistics;
use crate::testing::warrior::WarriorTest;

/// Ebon Hand: chance on hit 18211 (Shadow Bolt).
const EBON_HAND: u32 = 19170;
const SHADOW_BOLT: u32 = 18211;
const CASTS: u32 = 40_000;

fn ebon_hand() -> WarriorTest {
    let mut test = WarriorTest::new("Magic table");
    test.equip(EquipmentSlot::Mainhand, EBON_HAND);
    test
}

/// Casts the Shadow Bolt `casts` times and returns its statistics.
fn cast(test: &mut WarriorTest, casts: u32) -> SpellStatistics {
    let proc = test.proc("Shadow Bolt");
    for _ in 0..casts {
        test.with_ctx(|ctx| ctx.perform_proc(proc));
    }
    let statistics = test.character().statistics();
    let mut spells = statistics
        .spells()
        .filter(|(key, _)| key.name == "Shadow Bolt");
    let (_, spell) = spells.next().expect("Shadow Bolt statistics");
    spell.clone()
}

fn share(count: u64, total: u64) -> f64 {
    count as f64 / total as f64
}

fn assert_near(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{what}: {actual}, expected {expected}"
    );
}

#[test]
fn a_boss_makes_spells_miss_17_percent_of_the_time() {
    let mut test = ebon_hand();
    let spell = cast(&mut test, CASTS);
    assert_eq!(spell.total_attempts(), u64::from(CASTS));
    assert_near(
        share(spell.misses(), spell.total_attempts()),
        0.17,
        0.01,
        "miss rate",
    );
    assert_eq!(
        spell.full_resists(),
        0,
        "a non-binary spell is never fully resisted"
    );
}

/// Spell hit lowers the miss chance to at least 1 %; melee hit does not.
#[test]
fn spell_hit_reduces_the_miss_chance() {
    let mut test = ebon_hand();
    test.stats_mut().increase_melee_hit(500);
    test.stats_mut().increase_spell_hit(500);
    let spell = cast(&mut test, CASTS);
    assert_near(
        share(spell.misses(), spell.total_attempts()),
        0.12,
        0.01,
        "miss rate",
    );

    let mut capped = ebon_hand();
    capped.stats_mut().increase_spell_hit(2500);
    let spell = cast(&mut capped, CASTS);
    assert_near(
        share(spell.misses(), spell.total_attempts()),
        0.01,
        0.003,
        "miss rate",
    );
}

/// The spells that land crit with the spell crit chance (a warrior's from intellect plus the
/// auras), not the melee one.
#[test]
fn spells_crit_with_the_spell_crit_chance() {
    let mut test = ebon_hand();
    test.stats_mut().increase_spell_crit(1000);
    let crit_chance = test.stat(|stats, ctx| stats.get_spell_crit_chance(ctx, MagicSchool::Shadow));
    let spell = cast(&mut test, CASTS);
    let landed = spell.total_attempts() - spell.misses();
    let crits = spell.crits()
        + spell.attempts(crate::statistics::Outcome::PartialResistCrit25)
        + spell.attempts(crate::statistics::Outcome::PartialResistCrit50)
        + spell.attempts(crate::statistics::Outcome::PartialResistCrit75);
    assert_near(
        share(crits, landed),
        f64::from(crit_chance) / 10_000.0,
        0.01,
        "crit rate",
    );
}

/// The partial resists of the spells that land follow royalgiraffe's table for the
/// resistance plus the boss's 24 level-based resistance, as a share of the 300 cap.
#[test]
fn partial_resists_follow_the_resistance() {
    for (resistance, label) in [(0, "level-based only"), (76, "76 shadow resistance")] {
        let mut test = ebon_hand();
        test.target_mut()
            .set_resistance(MagicSchool::Shadow, resistance);
        let spell = cast(&mut test, CASTS);
        let landed = spell.total_attempts() - spell.misses();
        let ratio = f64::from(resistance as u32 + 24) / 300.0;
        let [none, p25, p50, p75] = Mechanics::partial_resist_chances(ratio);
        let tolerance = 0.01;
        assert_near(
            share(spell.partial_resists_25(), landed),
            p25,
            tolerance,
            label,
        );
        assert_near(
            share(spell.partial_resists_50(), landed),
            p50,
            tolerance,
            label,
        );
        assert_near(
            share(spell.partial_resists_75(), landed),
            p75,
            tolerance,
            label,
        );
        assert_near(
            share(spell.hits() + spell.crits(), landed),
            none,
            tolerance,
            label,
        );
    }
}

/// A missed spell is logged as resisted, a partial resist with the damage it took away.
#[test]
fn the_combat_log_writes_resists() {
    let mut test = ebon_hand();
    test.target_mut().set_resistance(MagicSchool::Shadow, 76);
    test.raid.engine_mut().enable_combat_log();
    let proc = test.proc("Shadow Bolt");
    for _ in 0..500 {
        test.with_ctx(|ctx| ctx.perform_proc(proc));
    }
    let log = test.raid.engine_mut().take_combat_log().unwrap();
    let mut missed = 0;
    let mut resisted = 0;
    for entry in log.entries() {
        match &entry.event {
            CombatLogEvent::SpellMissed { spell, miss, .. } if spell.id == SHADOW_BOLT => {
                assert_eq!(*miss, MissType::Resist);
                missed += 1;
            }
            CombatLogEvent::SpellDamage { spell, damage, .. }
                if spell.id == SHADOW_BOLT && damage.resisted > 0 =>
            {
                let full = f64::from(damage.amount + damage.resisted);
                let share = f64::from(damage.resisted) / full;
                assert!(
                    [0.25, 0.5, 0.75]
                        .iter()
                        .any(|expected| (share - expected).abs() < 1.0 / full),
                    "{damage:?}"
                );
                resisted += 1;
            }
            _ => {}
        }
    }
    assert!(
        missed > 0 && resisted > 0,
        "{missed} missed, {resisted} resisted"
    );

    let names = crate::combat_log::UnitNames::of(&test.raid);
    let rendered = log.render(&names);
    assert!(rendered.contains(",RESIST,"), "{rendered}");
}
