//! `csim sweep`: simulates every variant of a sweep file (a base setup or whole character
//! setups, and variation points, see [`csim_engine::sweep_loader`]) and ranks the variants by
//! DPS.
//!
//! The number of variants, the product of each variation point's, and the iterations it
//! takes are printed before anything runs; `--dry-run` stops there and lists the variants.
//! Every variant runs with the same seed, so the variants' differences are not drowned in the
//! noise of different rolls.

use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, ValueEnum};
use csim_engine::sim_control::{SimMode, run_threaded};
use csim_engine::sim_settings::SimSettings;
use csim_engine::sweep_loader::{Expansion, SweepSetup};
use serde::Serialize;

use crate::Result;
use crate::run::{clock_seed, progress_bar};
use crate::table::Table;

/// Iterations per variant without `--iterations` or the sweep's `iterations`.
const DEFAULT_ITERATIONS: u32 = 1000;

#[derive(Debug, Args)]
pub struct SweepArgs {
    /// The sweep file (see data/sweeps/).
    sweep: PathBuf,
    /// Iterations per variant (default: the sweep's, else 1000).
    #[arg(long, short = 'n')]
    iterations: Option<u32>,
    /// Worker threads (default: every available thread).
    #[arg(long, short = 't')]
    threads: Option<usize>,
    /// Encounter length in seconds (default: the sweep's, else the sim settings').
    #[arg(long, short = 'l')]
    length: Option<u32>,
    /// Seed every variant runs with (default: from the clock; printed).
    #[arg(long)]
    seed: Option<u64>,
    /// Only lists the variants and the iterations they would take.
    #[arg(long)]
    dry_run: bool,
    /// Shows the best this many variants; 0 shows all.
    #[arg(long, default_value_t = 0)]
    top: usize,
    /// The format of the results.
    #[arg(long, value_enum, default_value_t = SweepFormat::Terminal)]
    output_format: SweepFormat,
    /// Writes the results to this file instead of stdout.
    #[arg(long, value_name = "PATH")]
    output_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SweepFormat {
    /// The ranking as a text table.
    Terminal,
    /// The raw numbers as YAML.
    Yaml,
}

/// The results of a sweep, as `--output-format yaml` writes them.
#[derive(Debug, Serialize)]
pub struct SweepResults {
    pub name: String,
    /// The base setup; absent when a `characters` variation point provides the setups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    pub iterations: u32,
    pub combat_length: u32,
    pub seed: u64,
    /// Per variation point, its description and number of alternatives.
    pub variation_points: Vec<VariationPointRow>,
    /// Ranked by DPS, best first.
    pub variants: Vec<VariantRow>,
    /// The variants that are not valid setups, with why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub invalid: Vec<InvalidRow>,
}

#[derive(Debug, Serialize)]
pub struct VariationPointRow {
    pub description: String,
    pub alternatives: usize,
}

#[derive(Debug, Serialize)]
pub struct VariantRow {
    pub label: String,
    pub dps: f64,
    pub confidence_interval: f64,
    pub tps: f64,
}

#[derive(Debug, Serialize)]
pub struct InvalidRow {
    pub label: String,
    pub reason: String,
}

pub fn sweep(data_dir: &Path, args: &SweepArgs) -> Result<()> {
    let data = crate::load(data_dir)?;
    let sweep = SweepSetup::load(&args.sweep)?;
    let expansion = sweep.expand(&data)?;

    let mut settings = SimSettings::default();
    if let Some(length) = args.length.or(sweep.length) {
        settings.combat_length = length;
    }
    if let Some(threads) = args.threads {
        settings.set_threads(threads)?;
    }
    let iterations = args
        .iterations
        .or(sweep.iterations)
        .unwrap_or(DEFAULT_ITERATIONS);
    settings.iterations_quick_sim = iterations;
    settings.validate()?;
    let seed = args.seed.unwrap_or_else(clock_seed);

    let mut results = SweepResults {
        name: sweep.name.clone(),
        base: sweep.base.as_ref().map(|base| base.display().to_string()),
        iterations,
        combat_length: settings.combat_length,
        seed,
        variation_points: expansion
            .points
            .iter()
            .map(|(description, alternatives)| VariationPointRow {
                description: description.clone(),
                alternatives: *alternatives,
            })
            .collect(),
        variants: Vec::new(),
        invalid: expansion
            .invalid
            .iter()
            .map(|(label, reason)| InvalidRow {
                label: label.clone(),
                reason: reason.clone(),
            })
            .collect(),
    };
    // The count goes to stderr so that it shows while the variants run, whatever the output.
    eprint!("{}", header(&results, &expansion));
    if args.dry_run {
        for variant in &expansion.variants {
            println!("{}", variant.label);
        }
        return Ok(());
    }

    let total = iterations.saturating_mul(expansion.variants.len() as u32);
    let progress = std::io::stderr().is_terminal().then(|| progress_bar(total));
    let start = Instant::now();
    for variant in &expansion.variants {
        let settings = variant.setup.sim_settings(&settings);
        let build = || variant.setup.build_raid(&data, &settings);
        let cruncher = run_threaded(&settings, SimMode::Quick, seed, progress.clone(), build)?;
        let stats = cruncher.merged(None).expect("a run collects the baseline");
        results.variants.push(VariantRow {
            label: variant.label.clone(),
            dps: stats.personal_dps(),
            confidence_interval: cruncher.dps_distribution().confidence_interval,
            tps: stats.personal_tps(),
        });
    }
    if progress.is_some() {
        eprint!("\r{:<20}\r", "");
    }
    eprintln!("Simulated in {:.1} s", start.elapsed().as_secs_f64());

    results
        .variants
        .sort_by(|a, b| b.dps.total_cmp(&a.dps).then_with(|| a.label.cmp(&b.label)));
    if args.top > 0 {
        results.variants.truncate(args.top);
    }
    let output = match args.output_format {
        SweepFormat::Terminal => ranking(&results),
        SweepFormat::Yaml => serde_yaml::to_string(&results)?,
    };
    match &args.output_file {
        None => print!("{output}"),
        Some(path) => std::fs::write(path, output)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?,
    }
    Ok(())
}

/// The sweep, its variation points and what it takes to simulate them.
fn header(results: &SweepResults, expansion: &Expansion) -> String {
    let mut out = String::new();
    let base = (results.base.as_ref()).map_or_else(String::new, |base| format!(": base {base}"));
    let _ = writeln!(out, "Sweep {}{base}", results.name);
    for (index, point) in results.variation_points.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {}. {}: {}",
            index + 1,
            point.description,
            point.alternatives
        );
    }
    let variants = expansion.variants.len();
    let _ = write!(
        out,
        "{variants} variants × {} iterations = {} iterations",
        results.iterations,
        variants as u64 * u64::from(results.iterations)
    );
    let _ = writeln!(out, " ({} s, seed {})", results.combat_length, results.seed);
    if !expansion.invalid.is_empty() {
        let _ = writeln!(
            out,
            "{} of {} combinations are not valid setups and are skipped:",
            expansion.invalid.len(),
            expansion.combinations()
        );
        for (label, reason) in &expansion.invalid {
            let reason = reason.lines().skip(1).collect::<Vec<_>>().join(";");
            let _ = writeln!(out, "  {label}:{reason}");
        }
    }
    out
}

/// The variants ranked by DPS.
fn ranking(results: &SweepResults) -> String {
    let mut table = Table::new(["#", "Variant", "DPS", "±", "vs best", "TPS"]).left(1);
    let best = results.variants.first().map_or(0.0, |v| v.dps);
    for (index, variant) in results.variants.iter().enumerate() {
        table.row(vec![
            (index + 1).to_string(),
            variant.label.clone(),
            format!("{:.1}", variant.dps),
            format!("{:.1}", variant.confidence_interval),
            if index == 0 {
                String::new()
            } else {
                format!("{:.1}", variant.dps - best)
            },
            format!("{:.1}", variant.tps),
        ]);
    }
    table.render()
}
