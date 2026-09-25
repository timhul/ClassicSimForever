//! `csim run`: simulates one character setup and prints the results.

use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Args;
use csim_engine::character_loader::CharacterSetup;
use csim_engine::raid::RaidControl;
use csim_engine::resource::ResourceType;
use csim_engine::sim_control::{run_threaded, Progress, SimMode};
use csim_engine::sim_settings::{SimOption, SimSettings};
use csim_engine::statistics::{ClassStatistics, NumberCruncher};

use crate::table::{percent, Table};
use crate::Result;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// The character setup (see data/characters/).
    setup: PathBuf,
    /// Iterations (default: 1000, or 10000 per run with --scale).
    #[arg(long, short = 'n')]
    iterations: Option<u32>,
    /// Worker threads (default: every available thread).
    #[arg(long, short = 't')]
    threads: Option<usize>,
    /// Encounter length in seconds.
    #[arg(long, short = 'l', default_value_t = SimSettings::default().combat_length)]
    length: u32,
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
        ..SimSettings::default()
    });
    if let Some(threads) = args.threads {
        settings.set_threads(threads)?;
    }
    // Builds the raid once up front so that an invalid setup fails before the threads start.
    let raid = setup.build_raid(&data, &settings)?;
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
    let cruncher = run_threaded(&settings, mode, seed, progress.clone(), || {
        setup.build_raid(&data, &settings)
    })?;
    let elapsed = start.elapsed();
    if progress.is_some() {
        eprint!("\r{:<20}\r", "");
    }

    print!(
        "{}",
        report(&Report {
            setup: &setup,
            settings: &settings,
            seed,
            elapsed,
            cruncher: &cruncher,
        })
    );
    Ok(())
}

fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64)
}

/// Prints the share of `total` iterations done to stderr whenever the percentage changes.
fn progress_bar(total: u32) -> Progress {
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

/// What a run printed from.
pub struct Report<'a> {
    pub setup: &'a CharacterSetup,
    pub settings: &'a SimSettings,
    pub seed: u64,
    pub elapsed: Duration,
    pub cruncher: &'a NumberCruncher,
}

/// The results of a run as text.
pub fn report(r: &Report) -> String {
    let mut out = String::new();
    let stats = r
        .cruncher
        .merged(None)
        .expect("a run collects the baseline");
    let iterations = stats.iterations();

    let _ = writeln!(
        out,
        "{}: {} {}, rotation {:?}, {}, {} ruleset",
        r.setup.name,
        r.setup.race.name(),
        r.setup.class.name(),
        r.setup.rotation,
        r.settings.phase.description(),
        crate::serde_name(&r.settings.ruleset).to_lowercase(),
    );
    let _ = writeln!(
        out,
        "{iterations} iterations of {} s, {} threads, seed {}, {:.2} s ({} events)",
        r.settings.combat_length,
        r.settings.threads,
        r.seed,
        r.elapsed.as_secs_f64(),
        stats.engine().total_events(),
    );
    out.push('\n');

    let distribution = r.cruncher.dps_distribution();
    let _ = writeln!(
        out,
        "DPS  {:.2} ± {:.2} (95% CI)  std dev {:.2}  min {:.2}  max {:.2}",
        stats.personal_dps(),
        distribution.confidence_interval,
        distribution.standard_deviation,
        distribution.min_dps,
        distribution.max_dps,
    );
    let _ = writeln!(out, "TPS  {:.2}", stats.personal_tps());

    section(&mut out, "Damage and threat", &spell_table(&stats));
    section(&mut out, "Buffs and debuffs", &buff_table(&stats));
    section(&mut out, "Procs", &proc_table(&stats));
    section(&mut out, "Resource gains", &resource_table(&stats));
    section(&mut out, "Rotation", &executor_table(&stats));
    if !r.settings.options.is_empty() {
        section(&mut out, "Stat weights", &stat_weight_table(r.cruncher));
    }
    out
}

fn section(out: &mut String, title: &str, table: &Table) {
    if table.is_empty() {
        return;
    }
    let _ = write!(out, "\n{title}\n{}", table.render());
}

fn per(count: u64, iterations: u64) -> f64 {
    if iterations == 0 {
        0.0
    } else {
        count as f64 / iterations as f64
    }
}

fn spell_table(stats: &ClassStatistics) -> Table {
    let iterations = stats.iterations();
    let time = stats.time_in_combat();
    let total_damage = stats.total_damage();
    let mut table = Table::new([
        "Spell",
        "DPS",
        "Damage",
        "TPS",
        "Per fight",
        "Hit",
        "Crit",
        "Glance",
        "Miss",
        "Dodge",
        "Parry",
        "Block",
        "Resist",
    ]);
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
    for (key, spell) in spells {
        let attempts = spell.total_attempts();
        let rate = |count: u64| per(count, attempts);
        table.row(vec![
            key.display_name(),
            format!("{:.1}", spell.total_damage() as f64 / time),
            percent(spell.damage_share(total_damage)),
            format!("{:.1}", spell.total_threat() as f64 / time),
            format!("{:.1}", per(attempts, iterations)),
            percent(rate(spell.hits_including_partial_resists())),
            percent(rate(spell.crits_including_partial_resists())),
            percent(rate(spell.glances())),
            percent(rate(spell.misses())),
            percent(rate(spell.dodges())),
            percent(rate(spell.parries())),
            percent(rate(
                spell.full_blocks() + spell.partial_blocks() + spell.partial_block_crits(),
            )),
            percent(rate(spell.full_resists())),
        ]);
    }
    table
}

fn buff_table(stats: &ClassStatistics) -> Table {
    let mut table = Table::new(["Buff", "Kind", "Uptime", "Shortest", "Longest"]).left(1);
    let mut buffs: Vec<_> = stats.buffs().filter(|b| b.avg_uptime() > 0.0).collect();
    buffs.sort_by(|a, b| {
        b.avg_uptime()
            .total_cmp(&a.avg_uptime())
            .then_with(|| a.name().cmp(b.name()))
    });
    for buff in buffs {
        table.row(vec![
            buff.name().to_string(),
            if buff.is_debuff() { "debuff" } else { "buff" }.to_string(),
            percent(buff.avg_uptime()),
            format!("{:.1} s", buff.min_uptime()),
            format!("{:.1} s", buff.max_uptime()),
        ]);
    }
    table
}

fn proc_table(stats: &ClassStatistics) -> Table {
    let iterations = stats.iterations();
    let mut table = Table::new(["Proc", "Per fight", "Proc rate", "PPM"]);
    let mut procs: Vec<_> = stats.procs().filter(|p| p.attempts() > 0).collect();
    procs.sort_by(|a, b| {
        b.procs()
            .cmp(&a.procs())
            .then_with(|| a.name().cmp(b.name()))
    });
    for proc in procs {
        table.row(vec![
            proc.name().to_string(),
            format!("{:.1}", per(proc.procs(), iterations)),
            format!("{:.1}%", proc.avg_proc_rate() * 100.0),
            format!("{:.2}", proc.effective_ppm(stats.time_in_combat())),
        ]);
    }
    table
}

fn resource_table(stats: &ClassStatistics) -> Table {
    let iterations = stats.iterations();
    let time = stats.time_in_combat();
    let mut table = Table::new(["Source", "Resource", "Per fight", "Per 5 s"]).left(1);
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
    for (key, resource, kind, gain) in gains {
        table.row(vec![
            key.display_name(),
            kind.name().to_string(),
            format!(
                "{:.1}",
                if iterations == 0 {
                    0.0
                } else {
                    gain / iterations as f64
                }
            ),
            format!("{:.2}", resource.gain_per_5(kind, time)),
        ]);
    }
    table
}

fn executor_table(stats: &ClassStatistics) -> Table {
    let iterations = stats.iterations();
    let mut table = Table::new(["Executor", "Outcome", "Per fight", "Share"]).left(1);
    for executor in stats.executors() {
        let attempts = executor.attempts();
        if attempts == 0 {
            continue;
        }
        let mut name = executor.name().to_string();
        for outcome in executor.outcomes() {
            if outcome.count == 0 {
                continue;
            }
            table.row(vec![
                std::mem::take(&mut name),
                outcome.description().to_string(),
                format!("{:.1}", per(outcome.count, iterations)),
                format!("{:.1}%", per(outcome.count, attempts) * 100.0),
            ]);
        }
    }
    table
}

fn stat_weight_table(cruncher: &NumberCruncher) -> Table {
    let mut table = Table::new(["Option", "DPS", "Relative", "± 95% CI", "TPS"]);
    for (dps, tps) in cruncher
        .stat_weights_dps()
        .into_iter()
        .zip(cruncher.stat_weights_tps())
    {
        table.row(vec![
            dps.option.map_or("", SimOption::description).to_string(),
            format!("{:+.2}", dps.absolute_value),
            format!("{:+.2}%", dps.relative_value * 100.0),
            format!("{:.2}", dps.confidence_interval),
            format!("{:+.2}", tps.absolute_value),
        ]);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

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
