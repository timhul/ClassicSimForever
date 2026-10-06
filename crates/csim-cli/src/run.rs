//! `csim run`: simulates one character setup, alone or in a raid (`--raid`), and prints the
//! results as text tables, YAML or HTML (`--output-format`), to stdout or a file
//! (`--output-file`). With `--scale`, `--weights-file` also writes the stat weights per item
//! stat point (see [`crate::weights`]). `--combat-log` instead prints the combat log of one
//! iteration.
//!
//! The results are the engine's [`Results`], which the web page's Sim view also shows; this
//! module prints them ([`ResultsText`]).

use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use csim_engine::character_loader::CharacterSetup;
use csim_engine::combat_log::UnitNames;
use csim_engine::named_settings::SettingPairs;
use csim_engine::raid::RaidControl;
use csim_engine::raid_loader::RaidSetup;
use csim_engine::sim_control::{Progress, SimMode, run_logged_iteration, run_threaded};
use csim_engine::sim_settings::{SimOption, SimSettings};
use csim_engine::statistics::report::SpellRow;
use csim_engine::statistics::results::{RaidRoster, Report, Results, per};

use crate::Result;
use crate::table::{Table, percent};
use crate::weights::StatWeights;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// The character setup (see data/characters/).
    setup: PathBuf,
    /// Simulates the character in this raid (see data/raids/); its members are setups of
    /// <data>/characters/.
    #[arg(long, value_name = "PATH")]
    raid: Option<PathBuf>,
    /// Iterations (default: 1000, or 10000 per run with --scale).
    #[arg(long, short = 'n')]
    iterations: Option<u32>,
    /// Worker threads (default: every available thread).
    #[arg(long, short = 't')]
    threads: Option<usize>,
    /// Encounter length in seconds.
    #[arg(long, short = 'l', default_value_t = SimSettings::default().combat_length)]
    length: u32,
    /// Encounter length variance in percent: each iteration lasts a uniformly distributed
    /// `length × [1 - v/100, 1 + v/100]` seconds. 0 fixes the length.
    #[arg(
        long,
        default_value_t = SimSettings::default().length_variance,
        value_parser = parse_length_variance,
        value_name = "PERCENT"
    )]
    length_variance: f64,
    /// Named settings, alternatives to the default behavior: `name:value` pairs separated by
    /// commas; may be repeated. Known: rage_formula (forever, marrow_sigmoid), sigmoid_floor,
    /// sigmoid_ceiling, sigmoid_midpoint, sigmoid_width, initial_rage,
    /// target_start_health_percent (1-100). E.g.
    /// `--setting=rage_formula:marrow_sigmoid,sigmoid_ceiling:46`.
    #[arg(long = "setting", value_name = "NAME:VALUE,...")]
    settings: Vec<SettingPairs>,
    /// Seed fixing every random roll of the run (default: from the clock; printed).
    #[arg(long)]
    seed: Option<u64>,
    /// Also runs the stat-weight scaling options and prints the stat weights. Without a list:
    /// agility, strength, hit, crit, attack power and the skills of the equipped weapons.
    /// Otherwise a comma separated list, e.g. `--scale=strength,hit,sword`.
    #[arg(
        long,
        num_args = 0..=1,
        require_equals = true,
        value_delimiter = ',',
        value_parser = parse_scale_option,
        value_name = "OPTIONS"
    )]
    scale: Option<Vec<SimOption>>,
    /// The format of the results.
    #[arg(long, value_enum, default_value_t = OutputFormat::Terminal)]
    output_format: OutputFormat,
    /// Writes the results to this file instead of stdout.
    #[arg(long, value_name = "PATH")]
    output_file: Option<PathBuf>,
    /// Writes the stat weights per item stat point to this YAML file, for
    /// `csim rank-items --weights`. Requires --scale.
    #[arg(long, value_name = "PATH", requires = "scale")]
    weights_file: Option<PathBuf>,
    /// Simulates one iteration and prints its combat log (the lines of a `WoWCombatLog.txt`)
    /// instead of the results. With the same seed it is the iteration `-n 1 -t 1` simulates.
    #[arg(
        long,
        conflicts_with_all = ["iterations", "threads", "scale", "output_format", "output_file"]
    )]
    combat_log: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Plain text tables.
    Terminal,
    /// The raw numbers as YAML.
    Yaml,
    /// A self-contained HTML page.
    Html,
}

/// A length variance: a percentage of at least 0 and below 100.
pub fn parse_length_variance(text: &str) -> std::result::Result<f64, String> {
    let variance: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{text:?} is not a number"))?;
    if (0.0..100.0).contains(&variance) {
        Ok(variance)
    } else {
        Err(format!("must be at least 0 and below 100, got {variance}"))
    }
}

/// A scaling option by its serde name with or without the `SCALE_` prefix (`hit_chance`,
/// `SCALE_SWORD_SKILL`, ...) or a short alias (`hit`, `crit`, `ap`, `sword`, ...).
fn parse_scale_option(text: &str) -> std::result::Result<SimOption, String> {
    let name = text.trim().to_uppercase().replace('-', "_");
    let name = name.strip_prefix("SCALE_").unwrap_or(&name);
    let name = match name {
        "AGI" => "AGILITY",
        "STR" => "STRENGTH",
        "HIT" => "HIT_CHANCE",
        "CRIT" => "CRIT_CHANCE",
        "AP" => "ATTACK_POWER",
        "AXE" | "DAGGER" | "MACE" | "SWORD" => return parse_scale_option(&format!("{name}_SKILL")),
        "INT" => "INTELLECT",
        "SPELL_CRIT" => "SPELL_CRIT_CHANCE",
        "SPELL_HIT" => "SPELL_HIT_CHANCE",
        other => other,
    };
    serde_yaml::from_str(&format!("SCALE_{name}")).map_err(|_| {
        format!("unknown scaling option {text:?} (e.g. agility, strength, hit, crit, ap, sword)")
    })
}

/// The melee options, with the weapon skill options of the weapons `raid`'s character wields.
fn default_scale_options(raid: &RaidControl) -> Vec<SimOption> {
    let mut options = vec![
        SimOption::ScaleAgility,
        SimOption::ScaleStrength,
        SimOption::ScaleHitChance,
        SimOption::ScaleCritChance,
        SimOption::ScaleAttackPower,
    ];
    if let Some(id) = raid.char_ids().next() {
        let equipment = raid.character(id).equipment();
        let wielded: Vec<_> = [equipment.mainhand(), equipment.offhand()]
            .into_iter()
            .flatten()
            .map(|weapon| weapon.weapon_type())
            .collect();
        options.extend(
            SimOption::ALL
                .into_iter()
                .filter(|option| option.weapon_type().is_some_and(|t| wielded.contains(&t))),
        );
    }
    options
}

pub fn run(data_dir: &Path, args: &RunArgs) -> Result<()> {
    let data = crate::load(data_dir)?;
    let setup = CharacterSetup::load(&args.setup)?;

    let mut settings = setup.sim_settings(&SimSettings {
        combat_length: args.length,
        length_variance: args.length_variance,
        ..SimSettings::default()
    });
    settings.apply_setting_flags(&args.settings)?;
    if let Some(threads) = args.threads {
        settings.set_threads(threads)?;
    }
    let raid_setup = match &args.raid {
        None => None,
        Some(path) => {
            let raid = RaidSetup::load(path)?;
            let members = raid.resolve(&data_dir.join("characters"))?;
            Some((raid, members))
        }
    };
    let build = |settings: &SimSettings| -> std::result::Result<RaidControl, String> {
        match &raid_setup {
            None => setup.build_raid(&data, settings).map_err(|e| e.to_string()),
            Some((raid, members)) => raid
                .build_raid(Some(&setup), members, &data, settings)
                .map_err(|e| e.to_string()),
        }
    };
    if args.combat_log {
        settings.validate()?;
        let mut raid = build(&settings)?;
        let seed = args.seed.unwrap_or_else(clock_seed);
        eprintln!("Combat log of one iteration, seed {seed}");
        let log = run_logged_iteration(&settings, seed, &mut raid);
        print!("{}", log.render(&UnitNames::of(&raid)));
        return Ok(());
    }
    // Builds the raid once up front so that an invalid setup fails before the threads start.
    let raid = build(&settings)?;
    let roster = raid_setup.as_ref().map(|(raid_setup, members)| RaidRoster {
        name: raid_setup.name.clone(),
        members: raid
            .characters()
            .iter()
            .zip(std::iter::once(&setup).chain(members.iter().map(|m| &m.setup)))
            .map(|(character, setup)| (character.party() + 1, setup.name.clone()))
            .collect(),
    });
    let mode = match &args.scale {
        None => SimMode::Quick,
        Some(options) => {
            let options = if options.is_empty() {
                default_scale_options(&raid)
            } else {
                options.clone()
            };
            settings.options = options.into_iter().collect();
            SimMode::Full
        }
    };
    drop(raid);
    if let Some(iterations) = args.iterations {
        settings.iterations_quick_sim = iterations;
        settings.iterations_full_sim = iterations;
    }
    settings.validate()?;

    let seed = args.seed.unwrap_or_else(clock_seed);
    let iterations = match mode {
        SimMode::Quick => settings.iterations_quick_sim,
        SimMode::Full => settings.iterations_full_sim,
    };
    let runs = 1 + if mode == SimMode::Full {
        settings.options.len() as u32
    } else {
        0
    };
    let progress = std::io::stderr()
        .is_terminal()
        .then(|| progress_bar(iterations.saturating_mul(runs)));

    let start = Instant::now();
    let cruncher = run_threaded(&settings, mode, seed, progress.clone(), || build(&settings))?;
    let elapsed = start.elapsed();
    if progress.is_some() {
        eprint!("\r{:<20}\r", "");
    }

    let report = Report {
        setup: &setup,
        settings: &settings,
        seed,
        elapsed,
        cruncher: &cruncher,
        raid: roster.as_ref(),
    };
    if let Some(path) = &args.weights_file {
        StatWeights::collect(&report).write(path)?;
    }
    let results = Results::collect(&report);
    let output = match args.output_format {
        OutputFormat::Terminal => results.text(),
        OutputFormat::Yaml => results.yaml()?,
        OutputFormat::Html => crate::html::render(&results),
    };
    match &args.output_file {
        None => print!("{output}"),
        Some(path) => std::fs::write(path, output)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?,
    }
    Ok(())
}

pub(crate) fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64)
}

/// Prints the share of `total` iterations done to stderr whenever the percentage changes.
pub(crate) fn progress_bar(total: u32) -> Progress {
    let done = AtomicU32::new(0);
    let shown = AtomicU32::new(0);
    Arc::new(move |iterations| {
        let done = done.fetch_add(iterations, Ordering::Relaxed) + iterations;
        let pct = (u64::from(done) * 100 / u64::from(total.max(1))) as u32;
        if shown.fetch_max(pct, Ordering::Relaxed) < pct {
            eprint!("\rSimulating... {pct:>3}%");
        }
    })
}

/// How `csim run` prints [`Results`]: as text tables or YAML (HTML: [`crate::html`]).
pub trait ResultsText {
    /// The results as the text `csim run` prints.
    fn text(&self) -> String;
    /// The breakdowns as titled tables, leaving out the empty ones.
    fn tables(&self) -> Vec<(&'static str, Table)>;
    /// The results as YAML.
    fn yaml(&self) -> Result<String>;
}

impl ResultsText for Results {
    fn text(&self) -> String {
        let mut out = String::new();
        let (setup, run) = (&self.setup, &self.run);
        let _ = writeln!(
            out,
            "{}: {} {}, rotation {:?}, {}, {} ruleset",
            setup.name, setup.race, setup.class, setup.rotation, setup.phase, setup.ruleset,
        );
        let _ = writeln!(
            out,
            "{} iterations of {} s ± {}%, {} threads, seed {}, {:.2} s ({} events)",
            run.iterations,
            run.combat_length,
            run.length_variance,
            run.threads,
            run.seed,
            run.elapsed_seconds,
            run.events,
        );
        if let Some(settings) = &run.settings {
            let _ = writeln!(out, "Settings: {settings}");
        }
        out.push('\n');

        let dps = &self.dps;
        let _ = writeln!(
            out,
            "DPS  {:.2} ± {:.2} (95% CI)  std dev {:.2}  min {:.2}  max {:.2}",
            dps.mean, dps.confidence_interval, dps.standard_deviation, dps.min, dps.max,
        );
        let _ = writeln!(out, "TPS  {:.2}", self.tps);
        if let Some(raid) = &self.raid {
            let _ = writeln!(
                out,
                "
Raid {}: {} players, DPS {:.2}, TPS {:.2}",
                raid.name,
                raid.members.len(),
                raid.dps,
                raid.tps,
            );
        }

        for (title, table) in self.tables() {
            let _ = write!(out, "\n{title}\n{}", table.render());
        }
        out
    }

    fn tables(&self) -> Vec<(&'static str, Table)> {
        [
            ("Raid members", raid_table(self)),
            ("Damage and threat", spell_table(self)),
            ("Stat weights", stat_weight_table(self)),
            ("Buffs and debuffs", buff_table(self)),
            ("Procs", proc_table(self)),
            ("Resource gains", resource_table(self)),
            ("Finishers", finisher_table(self)),
            ("Rotation", executor_table(self)),
            ("Skipped rotation lines", skipped_table(self)),
            ("Engine", engine_table(self)),
        ]
        .into_iter()
        .filter(|(_, table)| !table.is_empty())
        .collect()
    }

    fn yaml(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }
}

fn raid_table(results: &Results) -> Table {
    let mut table = Table::new(["Party", "Member", "DPS", "Damage", "TPS"]).left(1);
    for member in results.raid.iter().flat_map(|raid| &raid.members) {
        table.row(vec![
            member.party.to_string(),
            member.name.clone(),
            format!("{:.2}", member.dps),
            percent(member.dps_share),
            format!("{:.2}", member.tps),
        ]);
    }
    table
}

fn spell_table(results: &Results) -> Table {
    let mut table = Table::new([
        "Spell", "DPS", "Damage", "TPS", "Casts", "Min", "Max", "DPR", "Hit", "Crit", "Glance",
        "Miss", "Dodge", "Parry", "Block", "Resist",
    ]);
    let cells = |spell: &SpellRow| {
        vec![
            spell.name.clone(),
            format!("{:.1}", spell.dps),
            percent(spell.damage_share),
            format!("{:.1}", spell.tps),
            format!("{:.1}", spell.casts),
            spell.min_hit.map_or(String::new(), |min| min.to_string()),
            spell.max_hit.map_or(String::new(), |max| max.to_string()),
            spell
                .damage_per_resource
                .map_or(String::new(), |dpr| format!("{dpr:.1}")),
            percent(spell.hit),
            percent(spell.crit),
            percent(spell.glance),
            percent(spell.miss),
            percent(spell.dodge),
            percent(spell.parry),
            percent(spell.block),
            percent(spell.resist),
        ]
    };
    for spell in &results.spells {
        table.row_with_sub_rows(cells(spell), spell.breakdown.iter().map(cells).collect());
    }
    if let Some(sum) = &results.spell_total {
        let mut total = vec![
            "Total".to_string(),
            format!("{:.1}", sum.dps),
            percent(sum.damage_share),
            format!("{:.1}", sum.tps),
            format!("{:.1}", sum.casts),
        ];
        total.resize(table.headers().len(), String::new());
        table.total(total);
    }
    table
}

fn buff_table(results: &Results) -> Table {
    let mut table = Table::new(["Buff", "Kind", "Uptime", "Shortest", "Longest"]).left(1);
    for buff in &results.buffs {
        table.row(vec![
            buff.name.clone(),
            if buff.debuff { "debuff" } else { "buff" }.to_string(),
            percent(buff.uptime),
            format!("{:.1} s", buff.shortest_seconds),
            format!("{:.1} s", buff.longest_seconds),
        ]);
    }
    table
}

fn proc_table(results: &Results) -> Table {
    let mut table = Table::new(["Proc", "Per fight", "Proc rate", "PPM"]);
    for proc in &results.procs {
        table.row(vec![
            proc.name.clone(),
            format!("{:.1}", proc.per_fight),
            format!("{:.1}%", proc.proc_rate * 100.0),
            format!("{:.2}", proc.ppm),
        ]);
    }
    table
}

fn resource_table(results: &Results) -> Table {
    let mut table = Table::new(["Source", "Resource", "Per fight", "Per s"]).left(1);
    for gain in &results.resources {
        table.row(vec![
            gain.source.clone(),
            gain.resource.clone(),
            format!("{:.1}", gain.per_fight),
            format!("{:.2}", gain.per_second),
        ]);
    }
    for sum in &results.resource_totals {
        table.total(vec![
            "Total".to_string(),
            sum.resource.clone(),
            format!("{:.1}", sum.per_fight),
            format!("{:.2}", sum.per_second),
        ]);
    }
    for sum in &results.resource_totals {
        if sum.lost_at_cap_per_fight > 0.0 {
            table.total(vec![
                "Lost at the cap".to_string(),
                sum.resource.clone(),
                format!("{:.1}", sum.lost_at_cap_per_fight),
                format!("{:.2}", sum.lost_at_cap_per_second),
            ]);
        }
    }
    table
}

fn finisher_table(results: &Results) -> Table {
    let mut table = Table::new([
        "Finisher", "1 CP", "2 CP", "3 CP", "4 CP", "5 CP", "Average",
    ])
    .left(1);
    for finisher in &results.finishers {
        let mut row = vec![finisher.name.clone()];
        row.extend(finisher.per_fight.iter().map(|casts| format!("{casts:.1}")));
        row.push(format!("{:.2}", finisher.average));
        table.row(row);
    }
    table
}

fn executor_table(results: &Results) -> Table {
    let mut table = Table::new(["Executor", "Outcome", "Per fight", "Share"]).left(1);
    for executor in &results.rotation {
        let mut name = executor.name.clone();
        for outcome in &executor.outcomes {
            table.row(vec![
                std::mem::take(&mut name),
                outcome.outcome.clone(),
                format!("{:.1}", outcome.per_fight),
                format!("{:.1}%", outcome.share * 100.0),
            ]);
        }
    }
    table
}

fn skipped_table(results: &Results) -> Table {
    let mut table = Table::new(["Line", "Spell", "Reason"]).left(1).left(2);
    for row in &results.skipped_rotation_lines {
        table.row(vec![
            row.line.to_string(),
            row.spell.clone(),
            row.reason.clone(),
        ]);
    }
    table
}

fn engine_table(results: &Results) -> Table {
    let mut table = Table::new(["Event", "Count", "Per fight", "Handled k/s", "Share"]);
    for row in &results.engine {
        table.row(vec![
            row.event.clone(),
            row.count.to_string(),
            format!("{:.1}", row.per_fight),
            format!("{:.0}", row.per_second / 1000.0),
            percent(row.share),
        ]);
    }
    if !results.engine.is_empty() {
        table.total(vec![
            "Total".to_string(),
            results.run.events.to_string(),
            format!("{:.1}", per(results.run.events, results.run.iterations)),
            format!("{:.0}", results.run.events_per_second / 1000.0),
            percent(1.0),
        ]);
    }
    table
}

fn stat_weight_table(results: &Results) -> Table {
    let mut table = Table::new(["Option", "DPS", "Relative", "± 95% CI", "TPS"]);
    for weight in &results.stat_weights {
        table.row(vec![
            weight.option.clone(),
            format!("{:+.2}", weight.dps),
            format!("{:+.2}%", weight.relative * 100.0),
            format!("{:.2}", weight.confidence_interval),
            format!("{:+.2}", weight.tps),
        ]);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, clap::Parser)]
    struct Wrapper {
        #[command(flatten)]
        run: RunArgs,
    }

    /// `--setting` takes comma-separated `name:value` pairs and may be repeated; the pairs of
    /// every flag apply together.
    #[test]
    fn setting_flags_merge() {
        use clap::Parser;
        use csim_engine::rage_formula::{RageFormula, SigmoidParams};

        let args = Wrapper::try_parse_from([
            "csim",
            "setup.yaml",
            "--setting=rage_formula:marrow_sigmoid,sigmoid_floor:2",
            "--setting",
            "sigmoid_width:5",
        ])
        .unwrap()
        .run;
        assert_eq!(args.settings.len(), 2);
        let mut settings = SimSettings::default();
        settings.apply_setting_flags(&args.settings).unwrap();
        assert_eq!(
            settings.rage_formula,
            RageFormula::MarrowSigmoid(SigmoidParams {
                floor: 2.0,
                width: 5.0,
                ..SigmoidParams::default()
            })
        );
        assert!(Wrapper::try_parse_from(["csim", "setup.yaml", "--setting=floor"]).is_err());
    }

    #[test]
    fn scale_options_parse_by_name_and_alias() {
        assert_eq!(parse_scale_option("hit"), Ok(SimOption::ScaleHitChance));
        assert_eq!(parse_scale_option("AP"), Ok(SimOption::ScaleAttackPower));
        assert_eq!(parse_scale_option("sword"), Ok(SimOption::ScaleSwordSkill));
        assert_eq!(
            parse_scale_option("SCALE_SPELL_PENETRATION"),
            Ok(SimOption::ScaleSpellPenetration)
        );
        assert_eq!(
            parse_scale_option("crit-chance"),
            Ok(SimOption::ScaleCritChance)
        );
        assert!(parse_scale_option("haste").is_err());
    }

    #[test]
    fn every_option_parses_by_its_serde_name() {
        for option in SimOption::ALL {
            assert_eq!(parse_scale_option(&crate::serde_name(&option)), Ok(option));
        }
    }
}
