//! Results tests: short seeded runs of the shipped setups.

use std::time::Duration;

use super::*;
use crate::data_bundle::DataBundle;
use crate::sim_control::SimRun;

/// `iterations` iterations of the shipped setup `name`, seed 1, taking `elapsed`.
fn results(name: &str, iterations: u32, elapsed: Duration) -> Results {
    let dir = DataBundle::repository_dir();
    let data = DataBundle::load(&dir).unwrap();
    let setup = CharacterSetup::load(&dir.join(format!("characters/{name}.yaml"))).unwrap();
    let mut settings = setup.sim_settings(&SimSettings::default());
    settings.iterations_quick_sim = iterations;
    let mut raid = setup.build_raid(&data, &settings).unwrap();
    let mut cruncher = NumberCruncher::new();
    SimRun::new(settings.clone(), 1, &mut raid).finish(&mut raid, &mut cruncher);
    Results::collect(&Report {
        setup: &setup,
        settings: &settings,
        seed: 1,
        elapsed,
        cruncher: &cruncher,
        raid: None,
    })
}

#[test]
fn the_results_of_a_run() {
    let results = results("warrior_fury_dw_orc", 20, Duration::from_secs(2));
    assert_eq!(results.setup.name, "DW Fury Orc");
    assert_eq!(results.setup.class, "Warrior");
    assert_eq!(results.setup.race, "Orc");
    assert_eq!(results.setup.ruleset, "standard");
    assert_eq!((results.run.iterations, results.run.seed), (20, 1));
    assert_eq!(results.run.elapsed_seconds, 2.0);
    assert_eq!(
        results.run.events_per_second,
        results.run.events as f64 / 2.0
    );

    let dps = &results.dps;
    assert!(dps.min <= dps.mean && dps.mean <= dps.max, "{dps:?}");
    assert!(dps.confidence_interval > 0.0 && dps.standard_deviation > 0.0);
    let total = results.spell_total.as_ref().unwrap();
    // Close, not equal: the iterations' lengths weigh the mean of their DPS differently.
    assert!(
        (total.dps - dps.mean).abs() < 0.01 * dps.mean,
        "{total:?} {dps:?}"
    );
    assert!((total.damage_share - 1.0).abs() < 1e-9);

    assert!(
        results
            .rotation
            .iter()
            .any(|row| row.name.ends_with(") Bloodthirst"))
    );
    assert!(!results.buffs.is_empty() && !results.procs.is_empty());
    assert!(
        results
            .resource_totals
            .iter()
            .any(|sum| sum.resource == "Rage")
    );
    // A warrior spends no combo points; no raid, no scaling options.
    assert!(results.finishers.is_empty());
    assert!(results.raid.is_none() && results.stat_weights.is_empty());

    // Most frequent first, the shares of every event.
    let counts: Vec<u64> = results.engine.iter().map(|row| row.count).collect();
    assert!(counts.is_sorted_by(|a, b| a >= b), "{counts:?}");
    assert_eq!(counts.iter().sum::<u64>(), results.run.events);
    let shares: f64 = results.engine.iter().map(|row| row.share).sum();
    assert!((shares - 1.0).abs() < 1e-9);
}

#[test]
fn a_rogue_reports_its_finishers() {
    let results = results("rogue_combat_swords_human", 10, Duration::ZERO);
    assert!(
        results
            .finishers
            .iter()
            .any(|row| row.name.starts_with("Slice and Dice")),
        "{:?}",
        results.finishers
    );
    for finisher in &results.finishers {
        assert!(finisher.per_fight.iter().sum::<f64>() > 0.0, "{finisher:?}");
        assert!((1.0..=5.0).contains(&finisher.average), "{finisher:?}");
    }
    // Without a wall-clock time there is no rate.
    assert_eq!(results.run.events_per_second, 0.0);
    assert!(results.engine.iter().all(|row| row.per_second == 0.0));
}

#[test]
fn the_empty_parts_are_left_out_of_the_serialized_results() {
    let yaml = serde_yaml::to_string(&results("warrior_fury_dw_orc", 2, Duration::ZERO)).unwrap();
    for absent in ["raid:", "finishers:", "stat_weights:"] {
        assert!(!yaml.contains(absent), "{absent} in\n{yaml}");
    }
    for present in ["spells:", "spell_total:", "rotation:", "engine:"] {
        assert!(yaml.contains(present), "{present} missing");
    }
}
