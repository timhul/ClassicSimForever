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
/// `report` without the wall-clock dependent parts: the run line and the last section, the
/// engine statistics with their events handled per second.
fn without_timing(report: &str) -> String {
    let engine = report.find("\nEngine\n").expect("engine statistics");
    report[..engine]
        .lines()
        .filter(|line| !line.contains(" events)"))
        .collect::<Vec<_>>()
        .join("\n")
}

const RUN: [&str; 8] = [
    "run",
    "data/characters/warrior_fury_dw_orc.yaml",
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
        "40 iterations of 120 s ± 10%, 2 threads, seed 7",
        "DPS  ",
        "TPS  ",
        "Damage and threat",
        "Mainhand Attack",
        "Buffs and debuffs",
        "Procs",
        "Resource gains",
        "Rotation",
        "Engine",
        "Player action",
    ] {
        assert!(report.contains(expected), "{expected:?} missing:\n{report}");
    }
    assert!(!report.contains("Stat weights"));
}

#[test]
fn run_lists_the_rotation_lines_that_never_run() {
    let report = stdout(&csim(&[&RUN[..], &["--seed", "7"]].concat()));
    let section = report
        .split("\nSkipped rotation lines\n")
        .nth(1)
        .unwrap_or_else(|| panic!("no skipped lines section:\n{report}"));
    let section = section.split("\n\n").next().unwrap();
    // DW Fury has no Spearing Strike talent and no Kiss of the Spider.
    let line = |spell: &str| {
        section
            .lines()
            .find(|line| line.contains(&format!(" {spell} ")))
            .unwrap_or_else(|| panic!("{spell} missing:\n{section}"))
            .to_string()
    };
    assert!(
        line("Spearing Strike").contains("talent Spearing Strike not taken"),
        "{section}"
    );
    assert!(
        line("Kiss of the Spider").ends_with("no spell of this name"),
        "{section}"
    );
    assert!(
        !section.contains("Bloodthirst"),
        "an active line:\n{section}"
    );
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
        "\nEngine\n",
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
fn rank_items_keeps_the_given_types() {
    let path = std::env::temp_dir().join(format!("csim-rank-type-{}.yaml", std::process::id()));
    std::fs::write(
        &path,
        "setup: Test\nclass: WARRIOR\nrotation: Fury\nphase: 6\niterations: 1\nseed: 1\n\
         dps: 500\ntps: 400\nweights:\n  STRENGTH: { dps: 1.0, tps: 0.5 }\n",
    )
    .unwrap();
    let rank = |types: &str| {
        stdout(&csim(&[
            "rank-items",
            "--weights",
            path.to_str().unwrap(),
            "--slot",
            "mainhand",
            "--type",
            types,
            "--limit",
            "0",
        ]))
    };
    let one_hand = rank("axe,sword,mace,dagger,fist");
    let two_hand = rank("TWOHAND_AXE,twohand_sword");
    std::fs::remove_file(&path).ok();

    let types = |report: &str| -> Vec<String> {
        report
            .lines()
            .skip(3)
            .map(|line| {
                let start = report.lines().nth(1).unwrap().find("Type").unwrap();
                line[start..].split_whitespace().next().unwrap().to_string()
            })
            .collect()
    };
    let one_hand = types(&one_hand);
    assert!(!one_hand.is_empty());
    assert!(
        one_hand
            .iter()
            .all(|t| ["AXE", "SWORD", "MACE", "DAGGER", "FIST"].contains(&t.as_str())),
        "{one_hand:?}"
    );
    let two_hand = types(&two_hand);
    assert!(
        two_hand.contains(&"TWOHAND_AXE".to_string()),
        "{two_hand:?}"
    );
    assert!(
        two_hand
            .iter()
            .all(|t| t == "TWOHAND_AXE" || t == "TWOHAND_SWORD"),
        "{two_hand:?}"
    );
}

/// The Rogue's weights rank what a Rogue can use: leather, no plate.
#[test]
fn rank_items_keeps_what_the_class_can_use() {
    let path = std::env::temp_dir().join(format!("csim-rank-rogue-{}.yaml", std::process::id()));
    std::fs::write(
        &path,
        "setup: Test\nclass: ROGUE\nrotation: Combat\nphase: 3\niterations: 1\nseed: 1\n\
         dps: 500\ntps: 400\nweights:\n  STRENGTH: { dps: 1.0, tps: 0.5 }\n\
         \x20 AGILITY: { dps: 2.0, tps: 1.0 }\n",
    )
    .unwrap();
    let report = stdout(&csim(&[
        "rank-items",
        "--weights",
        path.to_str().unwrap(),
        "--slot",
        "chest",
        "--limit",
        "0",
    ]));
    std::fs::remove_file(&path).ok();
    assert!(
        report.starts_with("Stat weights of Test (Rogue Combat"),
        "{report}"
    );
    assert!(report.contains("LEATHER"), "{report}");
    assert!(
        !report.contains("PLATE") && !report.contains("MAIL"),
        "{report}"
    );
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
    let header = damage.lines().nth(1).unwrap();
    assert!(header.contains(" Casts  Min "), "{damage}");
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
    let mainhand = spells
        .iter()
        .find(|s| s["name"] == "Mainhand Attack")
        .unwrap();
    let (min, max) = (
        mainhand["min_hit"].as_u64().unwrap(),
        mainhand["max_hit"].as_u64().unwrap(),
    );
    assert!(0 < min && min < max, "{yaml}");
    assert!(mainhand["casts"].as_f64().unwrap() > 0.0, "{yaml}");
    assert!(mainhand["damage_per_resource"].is_null(), "{yaml}");
    let bloodthirst = spells
        .iter()
        .find(|s| s["name"] == "Bloodthirst (rank 4)")
        .unwrap();
    assert!(
        bloodthirst["damage_per_resource"].as_f64().unwrap() > 0.0,
        "{yaml}"
    );
    for section in [
        "buffs",
        "procs",
        "resources",
        "resource_totals",
        "rotation",
        "engine",
    ] {
        assert!(results[section].is_sequence(), "{section} missing:\n{yaml}");
    }
    assert!(
        results["run"]["events_per_second"].as_f64().unwrap() > 0.0,
        "{yaml}"
    );
    assert!(
        results["engine"][0]["per_second"].as_f64().unwrap() > 0.0,
        "{yaml}"
    );

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
        "<details>\n<summary><h2>Engine</h2></summary>".to_string(),
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
        "DW Fury Orc",
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
        "data/characters/warrior_arms_human.yaml",
        "--raid",
        "data/raids/horde_melee.yaml",
        "--iterations",
        "1",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("DW Fury Orc is Horde, the raid is Alliance"),
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
fn lists_filter_by_class_slot_and_name() {
    let rotations = stdout(&csim(&["list-rotations", "--class", "warrior"]));
    assert!(rotations.contains("DW Fury"), "{rotations}");

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

    let rotations = stdout(&csim(&["list-rotations", "--class", "rogue"]));
    for name in [
        "Combat",
        "Combat Dagger",
        "Hemorrhage",
        "Seal Fate Mutilate",
        "Seal Fate Mutilate Expose Armor",
    ] {
        assert!(rotations.contains(name), "{rotations}");
    }
    assert!(!rotations.contains("DW Fury"), "{rotations}");

    // The Rogue wears leather and wields no shield.
    let chests = |class: &str| {
        stdout(&csim(&[
            "list-items",
            "--class",
            class,
            "--slot",
            "chest",
            "--search",
            "field marshal's",
        ]))
    };
    let rogue = chests("rogue");
    assert!(
        rogue.contains("Field Marshal's Leather Chestpiece"),
        "{rogue}"
    );
    assert!(
        !rogue.contains("PLATE") && !rogue.contains("MAIL"),
        "{rogue}"
    );
    assert!(chests("warrior").contains("PLATE"));
    let shields = stdout(&csim(&[
        "list-items",
        "--class",
        "rogue",
        "--search",
        "force reactive disk",
    ]));
    assert!(shields.contains("No items"), "{shields}");
}

const SWEEP: &str = "data/sweeps/dw_fury_last_3_points.yaml";

#[test]
fn sweep_dry_run_counts_and_lists_the_variants() {
    let output = csim(&["sweep", SWEEP, "--dry-run", "--seed", "1"]);
    let variants = stdout(&output);
    let header = String::from_utf8(output.stderr).unwrap();
    assert!(
        header.contains("46 variants × 10000 iterations = 460000 iterations (300 s ± 10%, seed 1)"),
        "{header}"
    );
    assert!(header.contains("3 talent points over Impale"), "{header}");
    assert_eq!(variants.lines().count(), 46, "{variants}");
    assert!(variants.contains("Precision +3"), "{variants}");
}

#[test]
fn sweep_ranks_every_variant_by_dps() {
    let results = stdout(&csim(&[
        "sweep",
        SWEEP,
        "--iterations",
        "4",
        "--length",
        "60",
        "--threads",
        "2",
        "--seed",
        "7",
        "--top",
        "5",
        "--output-format",
        "yaml",
    ]));
    let results: serde_yaml::Value = serde_yaml::from_str(&results).unwrap();
    assert_eq!(results["iterations"].as_u64(), Some(4));
    assert_eq!(
        results["variation_points"][0]["alternatives"].as_u64(),
        Some(46)
    );
    let dps: Vec<f64> = results["variants"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v["dps"].as_f64().unwrap())
        .collect();
    assert_eq!(dps.len(), 5, "--top 5");
    assert!(dps.windows(2).all(|w| w[0] >= w[1]), "best first: {dps:?}");
    assert!(dps[0] > 0.0);
}

const COMBAT_LOG: [&str; 5] = [
    "run",
    "data/characters/warrior_fury_dw_orc.yaml",
    "--length",
    "60",
    "--combat-log",
];

/// The fields of a combat log line after the timestamp, the event name first.
fn log_fields(line: &str) -> Vec<&str> {
    let (timestamp, event) = line.split_once("  ").expect("a timestamp");
    assert!(timestamp.starts_with("1/1 1"), "{line}");
    event.split(',').collect()
}

#[test]
fn combat_log_prints_one_iteration_as_combat_log_lines() {
    let log = stdout(&csim(&[&COMBAT_LOG[..], &["--seed", "3"]].concat()));
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(
        log_fields(lines[0]),
        ["COMBAT_LOG_VERSION", "9", "ADVANCED_LOG_ENABLED", "1"]
    );
    for line in &lines[1..] {
        let fields = log_fields(line);
        let expected = match fields[0] {
            "SWING_DAMAGE" => 35,
            "SPELL_DAMAGE" | "SPELL_PERIODIC_DAMAGE" => 38,
            "SWING_MISSED" => 11,
            "SPELL_MISSED" => 14,
            "SPELL_CAST_SUCCESS" => 28,
            "SPELL_ENERGIZE" | "SPELL_PERIODIC_ENERGIZE" => 32,
            "SPELL_AURA_APPLIED" | "SPELL_AURA_REFRESH" | "SPELL_AURA_REMOVED" => 13,
            "SPELL_AURA_APPLIED_DOSE" => 14,
            other => panic!("unexpected event {other}: {line}"),
        };
        assert_eq!(fields.len(), expected, "{line}");
    }
    for event in [
        "SWING_DAMAGE",
        "SPELL_CAST_SUCCESS",
        "SPELL_DAMAGE",
        "SPELL_AURA_APPLIED",
    ] {
        assert!(
            lines
                .iter()
                .any(|line| line.contains(&format!("  {event},"))),
            "no {event}"
        );
    }
    // Precombat actions before the pull, nothing after the encounter.
    assert!(lines[1].starts_with("1/1 11:59:"), "{}", lines[1]);
    assert!(lines.iter().all(|line| line[4..9] <= *"12:01"));
}

#[test]
fn combat_log_damage_is_the_damage_of_the_same_seeds_iteration() {
    // A fixed length, so that the DPS times 60 s is the damage.
    let log = stdout(&csim(
        &[&COMBAT_LOG[..], &["--seed", "8", "--length-variance", "0"]].concat(),
    ));
    let logged: u64 = log
        .lines()
        .skip(1)
        .map(log_fields)
        .map(|fields| match fields[0] {
            "SWING_DAMAGE" => fields[25].parse::<u64>().unwrap(),
            "SPELL_DAMAGE" | "SPELL_PERIODIC_DAMAGE" => fields[28].parse().unwrap(),
            _ => 0,
        })
        .sum();

    let yaml = stdout(&csim(&[
        "run",
        "data/characters/warrior_fury_dw_orc.yaml",
        "--length",
        "60",
        "--length-variance",
        "0",
        "-n",
        "1",
        "-t",
        "1",
        "--seed",
        "8",
        "--output-format",
        "yaml",
    ]));
    let results: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let dps = results["spell_total"]["dps"].as_f64().unwrap();
    assert!(logged > 0);
    assert!(
        (dps * 60.0 - logged as f64).abs() < 0.5,
        "{dps} DPS over 60 s vs {logged} logged"
    );
}

#[test]
fn a_seed_reproduces_the_combat_log() {
    let args = [&COMBAT_LOG[..], &["--seed", "12"]].concat();
    assert_eq!(stdout(&csim(&args)), stdout(&csim(&args)));
}

#[test]
fn combat_log_refuses_the_options_of_a_results_run() {
    for option in [
        &["--iterations", "5"][..],
        &["--threads", "2"],
        &["--scale"],
        &["--output-format", "yaml"],
        &["--output-file", "out.yaml"],
    ] {
        let output = csim(&[&COMBAT_LOG[..], option].concat());
        assert!(!output.status.success(), "{option:?} accepted");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("cannot be used with"),
            "{option:?}"
        );
    }
}

#[test]
fn a_negative_length_variance_is_rejected() {
    let output = csim(&[&RUN[..], &["--length-variance=-1"]].concat());
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("must be at least 0"), "{error}");
}
