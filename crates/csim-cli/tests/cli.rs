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

    let order = [
        "\nDamage and threat\n",
        "\nStat weights\n",
        "\nBuffs and debuffs\n",
        "\nResource gains\n",
        "\nRotation\n",
    ]
    .map(|title| report.find(title).unwrap_or_else(|| panic!("{title:?}")));
    assert!(order.is_sorted(), "sections out of order:\n{report}");
}

#[test]
fn weights_file_writes_the_weights_per_item_stat_point() {
    let path = std::env::temp_dir().join(format!("csim-weights-{}.yaml", std::process::id()));
    stdout(&csim(
        &[
            &RUN[..],
            &["--seed", "5", "--scale=strength,hit"],
            &["--weights-file", path.to_str().unwrap()],
        ]
        .concat(),
    ));
    let yaml = std::fs::read_to_string(&path).expect("the weights file is written");
    std::fs::remove_file(&path).ok();
    let weights: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("valid YAML");

    assert_eq!(weights["setup"].as_str(), Some("DW Fury Orc"));
    assert_eq!(weights["class"].as_str(), Some("WARRIOR"));
    assert_eq!(weights["iterations"].as_u64(), Some(40));
    assert!(weights["dps"].as_f64().unwrap() > 0.0, "{yaml}");
    let stats: Vec<_> = weights["weights"]
        .as_mapping()
        .unwrap()
        .keys()
        .map(|key| key.as_str().unwrap())
        .collect();
    assert_eq!(stats, ["STRENGTH", "HIT_CHANCE", "HIT_RATING"], "{yaml}");
    // 10 hit rating make the same 1 % as 0.01 hit chance.
    let per_point = |stat: &str| weights["weights"][stat]["dps"].as_f64().unwrap();
    assert!(
        (per_point("HIT_CHANCE") * 0.01 - per_point("HIT_RATING") * 10.0).abs() < 1e-9,
        "{yaml}"
    );
}

#[test]
fn rank_items_orders_by_the_weighted_stats() {
    let path = std::env::temp_dir().join(format!("csim-rank-{}.yaml", std::process::id()));
    std::fs::write(
        &path,
        "setup: Test\nclass: WARRIOR\nrotation: Fury\nphase: 3\niterations: 1\nseed: 1\n\
         dps: 500\ntps: 400\nweights:\n  STRENGTH: { dps: 1.0, tps: 0.5 }\n",
    )
    .unwrap();
    let output = csim(&[
        "rank-items",
        "--weights",
        path.to_str().unwrap(),
        "--slot",
        "gloves",
        "--limit",
        "5",
    ]);
    std::fs::remove_file(&path).ok();
    let report = stdout(&output);

    assert!(
        report.starts_with("Stat weights of Test (Warrior Fury, phase 3, 500.0 DPS)"),
        "{report}"
    );
    let header = report.lines().nth(1).unwrap();
    let score_column = header.find("Score").unwrap();
    let scores: Vec<f64> = report
        .lines()
        .skip(3)
        .map(|line| {
            let end = score_column + "Score".len();
            line[..end].rsplit(' ').next().unwrap().parse().unwrap()
        })
        .collect();
    assert_eq!(scores.len(), 5, "{report}");
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{report}");
    assert!(scores[0] > 0.0, "{report}");
    assert!(report.contains("GLOVES"), "{report}");
}

#[test]
fn weights_file_requires_scale() {
    let output = csim(&[&RUN[..], &["--weights-file", "weights.yaml"]].concat());
    assert!(!output.status.success());
}

#[test]
fn damage_and_resource_gains_end_in_totals() {
    let report = stdout(&csim(&[&RUN[..], &["--seed", "7"]].concat()));
    let section = |title: &str| {
        let start = report.find(&format!("\n{title}\n")).unwrap();
        let rest = &report[start + 1..];
        rest[..rest.find("\n\n").unwrap_or(rest.len())].to_string()
    };
    let damage = section("Damage and threat");
    let total = damage.lines().last().unwrap();
    assert!(total.starts_with("Total "), "{damage}");
    assert!(total.contains("100.0%"), "{damage}");
    let resources = section("Resource gains");
    let total = resources.lines().last().unwrap();
    assert!(
        total.starts_with("Total ") && total.contains("Rage"),
        "{resources}"
    );
}

#[test]
fn output_file_writes_the_chosen_format_instead_of_printing() {
    let path = std::env::temp_dir().join(format!("csim-results-{}.yaml", std::process::id()));
    let printed = stdout(&csim(
        &[
            &RUN[..],
            &["--seed", "5", "--scale=strength", "--output-format", "yaml"],
            &["--output-file", path.to_str().unwrap()],
        ]
        .concat(),
    ));
    let yaml = std::fs::read_to_string(&path).expect("the results file is written");
    std::fs::remove_file(&path).ok();
    let results: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("valid YAML");

    assert_eq!(printed, "", "nothing is printed");
    assert_eq!(results["setup"]["name"].as_str(), Some("DW Fury Orc"));
    assert_eq!(results["run"]["iterations"].as_u64(), Some(40));
    assert_eq!(results["run"]["seed"].as_u64(), Some(5));
    assert!(results["dps"]["mean"].as_f64().unwrap() > 0.0, "{yaml}");
    let spells = results["spells"].as_sequence().unwrap();
    assert!(
        spells.iter().any(|s| s["name"] == "Mainhand Attack"),
        "{yaml}"
    );
    for section in ["buffs", "procs", "resources", "resource_totals", "rotation"] {
        assert!(results[section].is_sequence(), "{section} missing:\n{yaml}");
    }

    let spell_dps: f64 = spells.iter().map(|s| s["dps"].as_f64().unwrap()).sum();
    let total = &results["spell_total"];
    assert!(
        (total["dps"].as_f64().unwrap() - spell_dps).abs() < 1e-6,
        "{yaml}"
    );
    assert!(
        (total["damage_share"].as_f64().unwrap() - 1.0).abs() < 1e-6,
        "{yaml}"
    );
    let rage_per_fight: f64 = results["resources"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|r| r["per_fight"].as_f64().unwrap())
        .sum();
    let rage = &results["resource_totals"][0];
    assert_eq!(rage["resource"].as_str(), Some("Rage"), "{yaml}");
    assert!(
        (rage["per_fight"].as_f64().unwrap() - rage_per_fight).abs() < 1e-6,
        "{yaml}"
    );
    assert_eq!(
        results["stat_weights"][0]["option"].as_str(),
        Some("+10 Strength")
    );
}

#[test]
fn output_format_prints_yaml_and_html() {
    let yaml = stdout(&csim(
        &[&RUN[..], &["--seed", "9", "--output-format", "yaml"]].concat(),
    ));
    let results: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("valid YAML");
    assert_eq!(results["run"]["seed"].as_u64(), Some(9));
    assert!(results.get("stat_weights").is_none(), "{yaml}");

    let html = stdout(&csim(
        &[&RUN[..], &["--seed", "9", "--output-format", "html"]].concat(),
    ));
    let dps = results["dps"]["mean"].as_f64().unwrap();
    for expected in [
        "<!DOCTYPE html>".to_string(),
        "<h1>DW Fury Orc</h1>".to_string(),
        format!("<div class=\"dps\">{dps:.2}</div>"),
        "<details open>\n<summary><h2>Damage and threat</h2></summary>".to_string(),
        "<details>\n<summary><h2>Rotation</h2></summary>".to_string(),
        "<tfoot>".to_string(),
        "<td class=\"left\">Mainhand Attack</td>".to_string(),
        "</html>".to_string(),
    ] {
        assert!(html.contains(&expected), "{expected:?} missing:\n{html}");
    }
}

#[test]
fn raid_runs_the_player_with_the_members() {
    let raid = [
        &RUN[..],
        &["--seed", "4", "--raid", "data/raids/horde_melee.yaml"],
    ]
    .concat();
    let report = stdout(&csim(&raid));
    for expected in [
        "Raid Horde melee: 5 players, DPS ",
        "Raid members",
        "2H Fury Orc",
        "Damage and threat",
    ] {
        assert!(
            report.contains(expected),
            "{expected:?} missing:
{report}"
        );
    }

    let yaml = stdout(&csim(&[&raid[..], &["--output-format", "yaml"]].concat()));
    let results: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("valid YAML");
    let members = results["raid"]["members"].as_sequence().unwrap();
    assert_eq!(members.len(), 5, "{yaml}");
    assert_eq!(members[0]["name"].as_str(), Some("DW Fury Orc"));
    assert_eq!(members[0]["party"].as_u64(), Some(1));
    assert_eq!(members[4]["party"].as_u64(), Some(2));
    let raid_dps = results["raid"]["dps"].as_f64().unwrap();
    let sum: f64 = members.iter().map(|m| m["dps"].as_f64().unwrap()).sum();
    assert!((raid_dps - sum).abs() < 1e-6, "{yaml}");
    let player = members[0]["dps"].as_f64().unwrap();
    assert!((player - results["dps"]["mean"].as_f64().unwrap()).abs() < 1e-6);

    let solo = stdout(&csim(&[&RUN[..], &["--output-format", "yaml"]].concat()));
    assert!(!solo.contains("raid:"), "{solo}");
}

#[test]
fn a_raid_of_the_other_faction_fails() {
    let output = csim(&[
        "run",
        "data/characters/arms_human.yaml",
        "--raid",
        "data/raids/horde_melee.yaml",
        "--iterations",
        "1",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("2H Fury Orc is Horde, the raid is Alliance"),
        "{stderr}"
    );
}

#[test]
fn an_unknown_output_format_fails() {
    let output = csim(&[&RUN[..], &["--output-format", "csv"]].concat());
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value 'csv'"));
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
    assert!(report.contains("raid setups are valid"), "{report}");
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
        "cleaver",
    ]));
    assert!(items.contains("18828"), "{items}");
    assert!(items.contains("High Warlord's Cleaver"), "{items}");
}
