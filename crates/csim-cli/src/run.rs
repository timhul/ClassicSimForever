//! `csim run`: simulates one character setup, alone or in a raid (`--raid`), and prints the
//! results as text tables, YAML or HTML (`--output-format`), to stdout or a file
//! (`--output-file`). With `--scale`, `--weights-file` also writes the stat weights per item
//! stat point (see [`crate::weights`]). `--combat-log` instead prints the combat log of one
//! iteration.

use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use csim_engine::character_loader::CharacterSetup;
use csim_engine::combat_log::UnitNames;
use csim_engine::raid::RaidControl;
use csim_engine::raid_loader::RaidSetup;
use csim_engine::resource::ResourceType;
use csim_engine::sim_control::{Progress, SimMode, run_logged_iteration, run_threaded};
use csim_engine::sim_settings::{SimOption, SimSettings};
use csim_engine::statistics::spell::{Outcome, SpellStatistics};
use csim_engine::statistics::{ClassStatistics, NumberCruncher};
use serde::Serialize;

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

/// What a run's results are collected from.
pub struct Report<'a> {
    pub setup: &'a CharacterSetup,
    pub settings: &'a SimSettings,
    pub seed: u64,
    pub elapsed: Duration,
    pub cruncher: &'a NumberCruncher,
    /// With `--raid`.
    pub raid: Option<&'a RaidRoster>,
}

/// The raid's name and, in `CharId` order, each member's party (1-based) and setup name.
pub struct RaidRoster {
    pub name: String,
    pub members: Vec<(u8, String)>,
}

/// The results of a run, as printed and as written by `--output-file`. Rates and shares are
/// fractions (0.25 for 25 %); per fight values are averages per iteration.
#[derive(Debug, Serialize)]
pub struct Results {
    pub setup: SetupInfo,
    pub run: RunInfo,
    pub dps: DpsSummary,
    pub tps: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raid: Option<RaidSummary>,
    pub spells: Vec<SpellRow>,
    /// The sums over `spells`; absent without spells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spell_total: Option<SpellTotal>,
    pub buffs: Vec<BuffRow>,
    pub procs: Vec<ProcRow>,
    pub resources: Vec<ResourceRow>,
    /// The sums over `resources`, one per resource.
    pub resource_totals: Vec<ResourceTotal>,
    /// The finishers cast, by combo points spent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub finishers: Vec<FinisherRow>,
    pub rotation: Vec<ExecutorRow>,
    /// The rotation lines that never run, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_rotation_lines: Vec<SkippedRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stat_weights: Vec<StatWeightRow>,
    /// Engine events by type, most frequent first.
    pub engine: Vec<EngineRow>,
}

#[derive(Debug, Serialize)]
pub struct SetupInfo {
    pub name: String,
    pub race: String,
    pub class: String,
    pub rotation: String,
    pub phase: String,
    pub ruleset: String,
}

#[derive(Debug, Serialize)]
pub struct RunInfo {
    pub iterations: u64,
    pub combat_length: u32,
    /// Percent.
    pub length_variance: f64,
    pub threads: usize,
    pub seed: u64,
    pub elapsed_seconds: f64,
    /// Engine events handled per second of wall-clock time.
    pub events_per_second: f64,
    pub events: u64,
}

#[derive(Debug, Serialize)]
pub struct DpsSummary {
    pub mean: f64,
    pub confidence_interval: f64,
    pub standard_deviation: f64,
    pub min: f64,
    pub max: f64,
}

/// The raid's results; the player is the first member.
#[derive(Debug, Serialize)]
pub struct RaidSummary {
    pub name: String,
    pub dps: f64,
    pub tps: f64,
    pub members: Vec<RaidMemberRow>,
}

#[derive(Debug, Serialize)]
pub struct RaidMemberRow {
    /// 1-based.
    pub party: u8,
    pub name: String,
    pub dps: f64,
    pub dps_share: f64,
    pub tps: f64,
}

#[derive(Debug, Serialize)]
pub struct SpellRow {
    pub name: String,
    pub dps: f64,
    pub damage_share: f64,
    pub tps: f64,
    pub casts: f64,
    /// The smallest and largest damage of one successful attempt; none without damage.
    pub min_hit: Option<u32>,
    pub max_hit: Option<u32>,
    /// Mean damage per resource point of the successful attempts; none for spells without a
    /// cost.
    pub damage_per_resource: Option<f64>,
    pub hit: f64,
    pub crit: f64,
    pub glance: f64,
    pub miss: f64,
    pub dodge: f64,
    pub parry: f64,
    pub block: f64,
    pub resist: f64,
    /// The attempts split by outcome, each part a row of its own (named after the outcome, its
    /// rates shares of all the spell's attempts, so the parts add up to the spell): a magic
    /// spell's by the share of damage resisted (see [`RESIST_BREAKDOWN`]), a white swing's by
    /// crit, hit and glancing blow, a melee ability's by crit and hit. Empty for other spells
    /// (physical damage over time, such as Deep Wounds).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breakdown: Vec<SpellRow>,
}

/// A magic spell's outcomes by the share of damage resisted: 100 % (a miss or a full resist)
/// down to 0 %.
const RESIST_BREAKDOWN: [(&str, &[Outcome]); 5] = [
    ("100% resisted", &[Outcome::Miss, Outcome::FullResist]),
    (
        "75% resisted",
        &[Outcome::PartialResist75, Outcome::PartialResistCrit75],
    ),
    (
        "50% resisted",
        &[Outcome::PartialResist50, Outcome::PartialResistCrit50],
    ),
    (
        "25% resisted",
        &[Outcome::PartialResist25, Outcome::PartialResistCrit25],
    ),
    ("0% resisted", &[Outcome::Hit, Outcome::Crit]),
];

/// A white swing's connecting outcomes.
const SWING_BREAKDOWN: [(&str, &[Outcome]); 3] = [
    ("Crit", &[Outcome::Crit]),
    ("Hit", &[Outcome::Hit]),
    ("Glancing", &[Outcome::Glancing]),
];

/// A melee ability's connecting outcomes (abilities cannot glance).
const ABILITY_BREAKDOWN: [(&str, &[Outcome]); 2] =
    [("Crit", &[Outcome::Crit]), ("Hit", &[Outcome::Hit])];

#[derive(Debug, Serialize)]
pub struct BuffRow {
    pub name: String,
    pub debuff: bool,
    pub uptime: f64,
    pub shortest_seconds: f64,
    pub longest_seconds: f64,
}

#[derive(Debug, Serialize)]
pub struct ProcRow {
    pub name: String,
    pub per_fight: f64,
    pub proc_rate: f64,
    pub ppm: f64,
}

#[derive(Debug, Serialize)]
pub struct ResourceRow {
    pub source: String,
    pub resource: String,
    pub per_fight: f64,
    pub per_second: f64,
}

/// A finisher's casts per fight by the combo points they spent.
#[derive(Debug, Serialize)]
pub struct FinisherRow {
    pub name: String,
    /// Casts per fight with 1 to 5 combo points.
    pub per_fight: [f64; 5],
    /// The average combo points spent.
    pub average: f64,
}

#[derive(Debug, Serialize)]
pub struct SpellTotal {
    pub dps: f64,
    pub damage_share: f64,
    pub tps: f64,
    pub casts: f64,
}

impl SpellTotal {
    fn of(spells: &[SpellRow]) -> Option<SpellTotal> {
        let sum = |value: fn(&SpellRow) -> f64| spells.iter().map(value).sum::<f64>();
        (!spells.is_empty()).then(|| SpellTotal {
            dps: sum(|s| s.dps),
            damage_share: sum(|s| s.damage_share),
            tps: sum(|s| s.tps),
            casts: sum(|s| s.casts),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ResourceTotal {
    pub resource: String,
    pub per_fight: f64,
    pub per_second: f64,
    /// Regeneration lost because the resource was full, per fight and per second.
    pub lost_at_cap_per_fight: f64,
    pub lost_at_cap_per_second: f64,
}

impl ResourceTotal {
    /// One total per resource, as rage and mana do not add up; `gains` are grouped by resource.
    fn of(gains: &[ResourceRow], stats: &ClassStatistics) -> Vec<ResourceTotal> {
        let iterations = stats.iterations().max(1) as f64;
        let time = stats.time_in_combat();
        gains
            .chunk_by(|a, b| a.resource == b.resource)
            .map(|rows| {
                let lost = ResourceType::ALL
                    .into_iter()
                    .find(|kind| kind.name() == rows[0].resource)
                    .map_or(0.0, |kind| stats.lost_at_cap(kind));
                ResourceTotal {
                    resource: rows[0].resource.clone(),
                    per_fight: rows.iter().map(|r| r.per_fight).sum(),
                    per_second: rows.iter().map(|r| r.per_second).sum(),
                    lost_at_cap_per_fight: lost / iterations,
                    lost_at_cap_per_second: if time > 0.0 { lost / time } else { 0.0 },
                }
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct ExecutorRow {
    pub name: String,
    pub outcomes: Vec<OutcomeRow>,
}

/// A `cast_if` line that was not linked to the character.
#[derive(Debug, Serialize)]
pub struct SkippedRow {
    /// 1-based position among the rotation's `cast_if` lines.
    pub line: usize,
    pub spell: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct OutcomeRow {
    pub outcome: String,
    pub per_fight: f64,
    pub share: f64,
}

#[derive(Debug, Serialize)]
pub struct EngineRow {
    pub event: String,
    pub count: u64,
    pub per_fight: f64,
    /// Handled per second of wall-clock time.
    pub per_second: f64,
    pub share: f64,
}

#[derive(Debug, Serialize)]
pub struct StatWeightRow {
    pub option: String,
    pub dps: f64,
    pub relative: f64,
    pub confidence_interval: f64,
    pub tps: f64,
}

impl Results {
    pub fn collect(r: &Report) -> Results {
        let stats = r
            .cruncher
            .merged(None)
            .expect("a run collects the baseline");
        let distribution = r.cruncher.dps_distribution();
        let spells = spell_rows(&stats);
        let resources = resource_rows(&stats);
        Results {
            setup: SetupInfo {
                name: r.setup.name.clone(),
                race: r.setup.race.name().to_string(),
                class: r.setup.class.name().to_string(),
                rotation: r.setup.rotation.clone(),
                phase: r.settings.phase.description().to_string(),
                ruleset: crate::serde_name(&r.settings.ruleset).to_lowercase(),
            },
            run: RunInfo {
                iterations: stats.iterations(),
                combat_length: r.settings.combat_length,
                length_variance: r.settings.length_variance,
                threads: r.settings.threads,
                seed: r.seed,
                elapsed_seconds: r.elapsed.as_secs_f64(),
                events_per_second: per_second(stats.engine().total_events(), r.elapsed),
                events: stats.engine().total_events(),
            },
            dps: DpsSummary {
                mean: stats.personal_dps(),
                confidence_interval: distribution.confidence_interval,
                standard_deviation: distribution.standard_deviation,
                min: distribution.min_dps,
                max: distribution.max_dps,
            },
            tps: stats.personal_tps(),
            raid: r.raid.map(|roster| raid_summary(roster, r.cruncher)),
            spell_total: SpellTotal::of(&spells),
            spells,
            buffs: buff_rows(&stats),
            procs: proc_rows(&stats),
            resource_totals: ResourceTotal::of(&resources, &stats),
            resources,
            finishers: finisher_rows(&stats),
            rotation: executor_rows(&stats),
            skipped_rotation_lines: skipped_rows(&stats),
            stat_weights: if r.settings.options.is_empty() {
                Vec::new()
            } else {
                stat_weight_rows(r.cruncher)
            },
            engine: engine_rows(&stats, r.elapsed),
        }
    }

    /// The results as the text `csim run` prints.
    pub fn text(&self) -> String {
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

    /// The breakdowns as titled tables, leaving out the empty ones.
    pub fn tables(&self) -> Vec<(&'static str, Table)> {
        [
            ("Raid members", self.raid_table()),
            ("Damage and threat", self.spell_table()),
            ("Stat weights", self.stat_weight_table()),
            ("Buffs and debuffs", self.buff_table()),
            ("Procs", self.proc_table()),
            ("Resource gains", self.resource_table()),
            ("Finishers", self.finisher_table()),
            ("Rotation", self.executor_table()),
            ("Skipped rotation lines", self.skipped_table()),
            ("Engine", self.engine_table()),
        ]
        .into_iter()
        .filter(|(_, table)| !table.is_empty())
        .collect()
    }

    /// The results as YAML.
    pub fn yaml(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    fn raid_table(&self) -> Table {
        let mut table = Table::new(["Party", "Member", "DPS", "Damage", "TPS"]).left(1);
        for member in self.raid.iter().flat_map(|raid| &raid.members) {
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

    fn spell_table(&self) -> Table {
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
        for spell in &self.spells {
            table.row_with_sub_rows(cells(spell), spell.breakdown.iter().map(cells).collect());
        }
        if let Some(sum) = &self.spell_total {
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

    fn buff_table(&self) -> Table {
        let mut table = Table::new(["Buff", "Kind", "Uptime", "Shortest", "Longest"]).left(1);
        for buff in &self.buffs {
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

    fn proc_table(&self) -> Table {
        let mut table = Table::new(["Proc", "Per fight", "Proc rate", "PPM"]);
        for proc in &self.procs {
            table.row(vec![
                proc.name.clone(),
                format!("{:.1}", proc.per_fight),
                format!("{:.1}%", proc.proc_rate * 100.0),
                format!("{:.2}", proc.ppm),
            ]);
        }
        table
    }

    fn resource_table(&self) -> Table {
        let mut table = Table::new(["Source", "Resource", "Per fight", "Per s"]).left(1);
        for gain in &self.resources {
            table.row(vec![
                gain.source.clone(),
                gain.resource.clone(),
                format!("{:.1}", gain.per_fight),
                format!("{:.2}", gain.per_second),
            ]);
        }
        for sum in &self.resource_totals {
            table.total(vec![
                "Total".to_string(),
                sum.resource.clone(),
                format!("{:.1}", sum.per_fight),
                format!("{:.2}", sum.per_second),
            ]);
        }
        for sum in &self.resource_totals {
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

    fn finisher_table(&self) -> Table {
        let mut table = Table::new([
            "Finisher", "1 CP", "2 CP", "3 CP", "4 CP", "5 CP", "Average",
        ])
        .left(1);
        for finisher in &self.finishers {
            let mut row = vec![finisher.name.clone()];
            row.extend(finisher.per_fight.iter().map(|casts| format!("{casts:.1}")));
            row.push(format!("{:.2}", finisher.average));
            table.row(row);
        }
        table
    }

    fn executor_table(&self) -> Table {
        let mut table = Table::new(["Executor", "Outcome", "Per fight", "Share"]).left(1);
        for executor in &self.rotation {
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

    fn skipped_table(&self) -> Table {
        let mut table = Table::new(["Line", "Spell", "Reason"]).left(1).left(2);
        for row in &self.skipped_rotation_lines {
            table.row(vec![
                row.line.to_string(),
                row.spell.clone(),
                row.reason.clone(),
            ]);
        }
        table
    }

    fn engine_table(&self) -> Table {
        let mut table = Table::new(["Event", "Count", "Per fight", "Handled k/s", "Share"]);
        for row in &self.engine {
            table.row(vec![
                row.event.clone(),
                row.count.to_string(),
                format!("{:.1}", row.per_fight),
                format!("{:.0}", row.per_second / 1000.0),
                percent(row.share),
            ]);
        }
        if !self.engine.is_empty() {
            table.total(vec![
                "Total".to_string(),
                self.run.events.to_string(),
                format!("{:.1}", per(self.run.events, self.run.iterations)),
                format!("{:.0}", self.run.events_per_second / 1000.0),
                percent(1.0),
            ]);
        }
        table
    }

    fn stat_weight_table(&self) -> Table {
        let mut table = Table::new(["Option", "DPS", "Relative", "± 95% CI", "TPS"]);
        for weight in &self.stat_weights {
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
}

fn per(count: u64, iterations: u64) -> f64 {
    if iterations == 0 {
        0.0
    } else {
        count as f64 / iterations as f64
    }
}

fn raid_summary(roster: &RaidRoster, cruncher: &NumberCruncher) -> RaidSummary {
    let raid_dps = cruncher.raid_dps();
    let results = cruncher.player_results();
    assert_eq!(results.len(), roster.members.len(), "a result per member");
    RaidSummary {
        name: roster.name.clone(),
        dps: raid_dps,
        tps: cruncher.raid_tps(),
        members: roster
            .members
            .iter()
            .zip(results)
            .map(|((party, name), result)| RaidMemberRow {
                party: *party,
                name: name.clone(),
                dps: result.dps,
                dps_share: if raid_dps > 0.0 {
                    result.dps / raid_dps
                } else {
                    0.0
                },
                tps: result.tps,
            })
            .collect(),
    }
}

fn spell_rows(stats: &ClassStatistics) -> Vec<SpellRow> {
    let iterations = stats.iterations();
    let time = stats.time_in_combat();
    let total_damage = stats.total_damage();
    let mut spells: Vec<_> = stats
        .spells()
        .filter(|(_, spell)| {
            spell.total_attempts() > 0 || spell.total_damage() > 0 || spell.total_threat() > 0
        })
        .collect();
    spells.sort_by(|(a_key, a), (b_key, b)| {
        (b.total_damage(), b.total_threat())
            .cmp(&(a.total_damage(), a.total_threat()))
            .then_with(|| a_key.cmp(b_key))
    });
    spells
        .into_iter()
        .map(|(key, spell)| {
            let row = |name: String, outcomes: &[Outcome]| {
                outcome_row(name, spell, outcomes, iterations, time, total_damage)
            };
            let breakdown: &[(&str, &[Outcome])] = if spell.is_magic() {
                &RESIST_BREAKDOWN
            } else if spell.is_auto_attack() {
                &SWING_BREAKDOWN
            } else if spell.is_melee() {
                &ABILITY_BREAKDOWN
            } else {
                &[]
            };
            SpellRow {
                damage_per_resource: spell.dpr().is_set().then(|| spell.dpr().avg()),
                breakdown: breakdown
                    .iter()
                    .map(|(name, outcomes)| row(name.to_string(), outcomes))
                    .collect(),
                ..row(key.display_name(), &Outcome::ALL)
            }
        })
        .collect()
}

/// The attempts of `spell` that ended with one of `outcomes`, as a row named `name`: their
/// damage and threat, and their rates as shares of all the spell's attempts.
fn outcome_row(
    name: String,
    spell: &SpellStatistics,
    outcomes: &[Outcome],
    iterations: u64,
    time: f64,
    total_damage: u64,
) -> SpellRow {
    let attempts = spell.total_attempts();
    let sum = |value: &dyn Fn(Outcome) -> u64| outcomes.iter().map(|&o| value(o)).sum::<u64>();
    // The share of the attempts that ended with one of `outcomes` in `column`.
    let rate = |column: &[Outcome]| {
        per(
            sum(&|o| {
                if column.contains(&o) {
                    spell.attempts(o)
                } else {
                    0
                }
            }),
            attempts,
        )
    };
    let damage = sum(&|o| spell.damage(o).total());
    let damaging = || {
        outcomes
            .iter()
            .map(|&outcome| spell.damage(outcome))
            .filter(|tally| tally.max() > 0)
    };
    SpellRow {
        name,
        dps: damage as f64 / time,
        damage_share: per(damage, total_damage),
        tps: sum(&|o| spell.threat(o).total()) as f64 / time,
        casts: per(sum(&|o| spell.attempts(o)), iterations),
        min_hit: damaging().map(|tally| tally.min()).min(),
        max_hit: damaging().map(|tally| tally.max()).max(),
        damage_per_resource: None,
        hit: rate(&[
            Outcome::Hit,
            Outcome::PartialResist25,
            Outcome::PartialResist50,
            Outcome::PartialResist75,
        ]),
        crit: rate(&[
            Outcome::Crit,
            Outcome::PartialResistCrit25,
            Outcome::PartialResistCrit50,
            Outcome::PartialResistCrit75,
        ]),
        glance: rate(&[Outcome::Glancing]),
        miss: rate(&[Outcome::Miss]),
        dodge: rate(&[Outcome::Dodge]),
        parry: rate(&[Outcome::Parry]),
        block: rate(&[
            Outcome::FullBlock,
            Outcome::PartialBlock,
            Outcome::PartialBlockCrit,
        ]),
        resist: rate(&[Outcome::FullResist]),
        breakdown: Vec::new(),
    }
}

fn buff_rows(stats: &ClassStatistics) -> Vec<BuffRow> {
    let mut buffs: Vec<_> = stats.buffs().filter(|b| b.avg_uptime() > 0.0).collect();
    buffs.sort_by(|a, b| {
        b.avg_uptime()
            .total_cmp(&a.avg_uptime())
            .then_with(|| a.name().cmp(b.name()))
    });
    buffs
        .into_iter()
        .map(|buff| BuffRow {
            name: buff.name().to_string(),
            debuff: buff.is_debuff(),
            uptime: buff.avg_uptime(),
            shortest_seconds: buff.min_uptime(),
            longest_seconds: buff.max_uptime(),
        })
        .collect()
}

fn proc_rows(stats: &ClassStatistics) -> Vec<ProcRow> {
    let iterations = stats.iterations();
    let mut procs: Vec<_> = stats.procs().filter(|p| p.attempts() > 0).collect();
    procs.sort_by(|a, b| {
        b.procs()
            .cmp(&a.procs())
            .then_with(|| a.name().cmp(b.name()))
    });
    procs
        .into_iter()
        .map(|proc| ProcRow {
            name: proc.name().to_string(),
            per_fight: per(proc.procs(), iterations),
            proc_rate: proc.avg_proc_rate(),
            ppm: proc.effective_ppm(stats.time_in_combat()),
        })
        .collect()
}

fn finisher_rows(stats: &ClassStatistics) -> Vec<FinisherRow> {
    let iterations = stats.iterations().max(1) as f64;
    stats
        .finishers()
        .map(|(key, counts)| {
            let casts: u64 = counts.iter().sum();
            let points: u64 = counts.iter().zip(1..).map(|(n, cp)| n * cp).sum();
            FinisherRow {
                name: key.display_name(),
                per_fight: counts.map(|n| n as f64 / iterations),
                average: points as f64 / casts.max(1) as f64,
            }
        })
        .collect()
}

fn resource_rows(stats: &ClassStatistics) -> Vec<ResourceRow> {
    let iterations = stats.iterations();
    let time = stats.time_in_combat();
    let mut gains: Vec<_> = stats
        .resources()
        .flat_map(|(key, resource)| {
            ResourceType::ALL
                .into_iter()
                .map(move |kind| (key, resource, kind, resource.gain(kind)))
        })
        .filter(|&(_, _, _, gain)| gain > 0.0)
        .collect();
    gains.sort_by(|a, b| {
        (a.2 as u8)
            .cmp(&(b.2 as u8))
            .then(b.3.total_cmp(&a.3))
            .then(a.0.cmp(b.0))
    });
    gains
        .into_iter()
        .map(|(key, resource, kind, gain)| ResourceRow {
            source: key.display_name(),
            resource: kind.name().to_string(),
            per_fight: if iterations == 0 {
                0.0
            } else {
                gain / iterations as f64
            },
            per_second: resource.gain_per_5(kind, time) / 5.0,
        })
        .collect()
}

fn executor_rows(stats: &ClassStatistics) -> Vec<ExecutorRow> {
    let iterations = stats.iterations();
    stats
        .executors()
        .iter()
        .filter(|executor| executor.attempts() > 0)
        .map(|executor| {
            let attempts = executor.attempts();
            ExecutorRow {
                name: executor.name().to_string(),
                outcomes: executor
                    .outcomes()
                    .into_iter()
                    .filter(|outcome| outcome.count > 0)
                    .map(|outcome| OutcomeRow {
                        outcome: outcome.description().to_string(),
                        per_fight: per(outcome.count, iterations),
                        share: per(outcome.count, attempts),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn skipped_rows(stats: &ClassStatistics) -> Vec<SkippedRow> {
    stats
        .skipped_executors()
        .iter()
        .map(|skipped| SkippedRow {
            line: skipped.line,
            spell: skipped.spell_name.clone(),
            reason: skipped.reason.clone(),
        })
        .collect()
}

fn per_second(count: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if seconds > 0.0 {
        count as f64 / seconds
    } else {
        0.0
    }
}

fn engine_rows(stats: &ClassStatistics, elapsed: Duration) -> Vec<EngineRow> {
    let engine = stats.engine();
    let mut events = engine.non_zero();
    events.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    events
        .into_iter()
        .map(|(event, count)| EngineRow {
            event: event.name().to_string(),
            count,
            per_fight: per(count, stats.iterations()),
            per_second: per_second(count, elapsed),
            share: per(count, engine.total_events()),
        })
        .collect()
}

fn stat_weight_rows(cruncher: &NumberCruncher) -> Vec<StatWeightRow> {
    cruncher
        .stat_weights_dps()
        .into_iter()
        .zip(cruncher.stat_weights_tps())
        .map(|(dps, tps)| StatWeightRow {
            option: dps.option.map_or("", SimOption::description).to_string(),
            dps: dps.absolute_value,
            relative: dps.relative_value,
            confidence_interval: dps.confidence_interval,
            tps: tps.absolute_value,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use csim_engine::combat_roll::PhysicalAttackResult;
    use csim_engine::spell::AttackOutcome;

    #[test]
    fn swings_break_down_by_crit_hit_and_glance() {
        let swing = |result, damage| AttackOutcome {
            result,
            spell: None,
            damage,
            threat: f64::from(damage),
            execution_time: 0.0,
        };
        let mut stats = ClassStatistics::new("Tester", 100.0);
        let mh = stats.spell("Mainhand Attack", 1);
        for (result, damage) in [
            (PhysicalAttackResult::Critical, 800),
            (PhysicalAttackResult::Hit, 400),
            (PhysicalAttackResult::Hit, 420),
            (PhysicalAttackResult::Glancing, 250),
            (PhysicalAttackResult::Dodge, 0),
        ] {
            mh.record_swing(&swing(result, damage));
        }
        stats
            .spell("Bloodthirst", 1)
            .record_attack(&swing(PhysicalAttackResult::Hit, 600), 30.0);
        stats
            .spell("Deep Wounds", 1)
            .record_tick(25, 25.0, 0.0, 0.0, None);

        let rows = spell_rows(&stats);
        let names = |name: &str| -> Vec<String> {
            let row = rows.iter().find(|r| r.name == name).unwrap();
            row.breakdown.iter().map(|r| r.name.clone()).collect()
        };
        assert_eq!(names("Bloodthirst"), ["Crit", "Hit"]);
        assert!(names("Deep Wounds").is_empty());
        let mh = rows.iter().find(|r| r.name == "Mainhand Attack").unwrap();
        let parts: Vec<_> = mh
            .breakdown
            .iter()
            .map(|r| {
                (
                    r.name.as_str(),
                    r.min_hit,
                    r.max_hit,
                    r.hit,
                    r.crit,
                    r.glance,
                )
            })
            .collect();
        assert_eq!(
            parts,
            [
                ("Crit", Some(800), Some(800), 0.0, 0.2, 0.0),
                ("Hit", Some(400), Some(420), 0.4, 0.0, 0.0),
                ("Glancing", Some(250), Some(250), 0.0, 0.0, 0.2),
            ]
        );
        let share: f64 = mh.breakdown.iter().map(|r| r.damage_share).sum();
        assert!((share - mh.damage_share).abs() < 1e-12);
        assert_eq!(mh.dodge, 0.2);
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
