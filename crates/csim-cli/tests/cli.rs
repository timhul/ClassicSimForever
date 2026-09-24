//! Runs the `csim` binary on the repository's data.

use std::path::PathBuf;
use std::process::{Command, Output};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn csim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_csim"))
        .current_dir(repository())
        .args(args)
        .output()
        .expect("csim runs")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "csim failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// The report without the line holding the elapsed time.
fn without_timing(report: &str) -> String {
    report
        .lines()
        .filter(|line| !line.contains(" events)"))
        .collect::<Vec<_>>()
        .join("\n")
}

const RUN: [&str; 8] = [
    "run",
    "data/characters/dw_fury_orc.yaml",
    "--iterations",
    "40",
    "--threads",
    "2",
    "--length",
    "120",
];

#[test]
fn run_prints_the_breakdowns() {
    let report = stdout(&csim(&[&RUN[..], &["--seed", "7"]].concat()));
    for expected in [
        "DW Fury Orc: Orc Warrior",
        "40 iterations of 120 s, 2 threads, seed 7",
        "DPS  ",
        "TPS  ",
        "Damage and threat",
        "Mainhand Attack",
        "Buffs and debuffs",
        "Procs",
        "Resource gains",
        "Rotation",
    ] {
        assert!(report.contains(expected), "{expected:?} missing:\n{report}");
    }
    assert!(!report.contains("Stat weights"));
}

#[test]
fn a_seed_reproduces_the_run() {
    let args = [&RUN[..], &["--seed", "11"]].concat();
    let first = stdout(&csim(&args));
    let second = stdout(&csim(&args));
    assert_eq!(without_timing(&first), without_timing(&second));
}

#[test]
fn scale_prints_the_stat_weights() {
    let report = stdout(&csim(
        &[&RUN[..], &["--seed", "3", "--scale=strength,hit"]].concat(),
    ));
    assert!(report.contains("Stat weights"), "{report}");
    assert!(report.contains("+10 Strength"), "{report}");
    assert!(report.contains("+1% Hit"), "{report}");
}

#[test]
fn a_missing_setup_fails() {
    let output = csim(&["run", "data/characters/missing.yaml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("error: cannot read"));
}

#[test]
fn validate_checks_every_shipped_setup() {
    let report = stdout(&csim(&["validate"]));
    assert!(report.contains("ok       "), "{report}");
    assert!(report.contains("character setups are valid"), "{report}");
}

#[test]
fn lists_filter_by_class_slot_and_name() {
    let rotations = stdout(&csim(&["list-rotations", "--class", "warrior"]));
    assert!(rotations.contains("DW Fury High Rage"), "{rotations}");

    let spells = stdout(&csim(&[
        "list-spells",
        "--class",
        "warrior",
        "--search",
        "bloodthirst",
    ]));
    assert!(spells.contains("Bloodthirst"), "{spells}");
    assert!(!spells.contains("Heroic Strike"), "{spells}");

    let items = stdout(&csim(&[
        "list-items",
        "--slot",
        "mainhand",
        "--search",
        "brutality",
    ]));
    assert!(items.contains("18832"), "{items}");
    assert!(items.contains("Brutality Blade"), "{items}");
}
