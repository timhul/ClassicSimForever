//! A sim of many iterations of a setup, the page's Sim view: what `csim run` does, run in
//! chunks the page asks for ([`SimJob::step`]) so that it can show the progress and stop. A
//! job's setup is resolved as a load's ([`App::start_sim`](crate::app::App::start_sim)).
//!
//! A job runs one share of a run over `threads` threads ([`shares`]): the browser runs the
//! shares in workers of their own, side by side, and merges their statistics
//! ([`App::merge_sim`](crate::app::App::merge_sim)). Seed S, N iterations and T threads give the
//! results of `csim run -n N -t T --seed S`, whoever ran the shares.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use csim_engine::character_loader::CharacterSetup;
use csim_engine::data_bundle::DataBundle;
use csim_engine::raid::RaidControl;
use csim_engine::sim_control::{Share, SimRun, shares};
use csim_engine::sim_settings::SimSettings;
use csim_engine::statistics::results::{Report, Results};
use csim_engine::statistics::{ClassStatistics, NumberCruncher};
use serde::{Deserialize, Serialize};

use crate::app::LoadRequest;
use crate::icons::IconLookup;
use crate::session::{Icon, as_string};

/// The most iterations a run has.
pub const MAX_ITERATIONS: u32 = 1_000_000;
/// The most threads a run is split over.
pub const MAX_THREADS: u32 = 64;

/// An `api/sim/start` request: the setup as `api/load` takes it (its keybinds are ignored: the
/// rotation plays), the iterations of the whole run, and which of its shares the job runs: the
/// run's `threads` (1 without) and the share's index (0 without).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimRequest {
    pub load: LoadRequest,
    pub iterations: u32,
    #[serde(default = "one")]
    pub threads: u32,
    #[serde(default)]
    pub share: u32,
}

/// An `api/sim/merge` request: the run (as `api/sim/start` has it, with the seed it ran with),
/// the statistics of each of its shares in order (`api/sim/statistics`) and the wall-clock time
/// it took.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRequest {
    pub load: LoadRequest,
    pub iterations: u32,
    #[serde(default = "one")]
    pub threads: u32,
    pub shares: Vec<ClassStatistics>,
    pub elapsed_seconds: f64,
}

fn one() -> u32 {
    1
}

/// The shares of a run of `iterations` over `threads` threads seeded with `seed`.
///
/// # Errors
/// The iterations or the threads are out of range, or there are more threads than iterations.
pub fn run_shares(iterations: u32, threads: u32, seed: u64) -> Result<Vec<Share>, String> {
    if !(1..=MAX_ITERATIONS).contains(&iterations) {
        return Err(format!(
            "{iterations} iterations: from 1 to {MAX_ITERATIONS}"
        ));
    }
    if !(1..=MAX_THREADS).contains(&threads) {
        return Err(format!("{threads} threads: from 1 to {MAX_THREADS}"));
    }
    if threads > iterations {
        return Err(format!(
            "{threads} threads for {iterations} iterations: at most one each"
        ));
    }
    Ok(shares(iterations, threads as usize, seed))
}

/// How far a job is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimProgress {
    /// The setup's name.
    pub name: String,
    /// A string in JSON: seeds do not fit a JavaScript number.
    #[serde(serialize_with = "as_string")]
    pub seed: u64,
    pub done: u32,
    pub total: u32,
}

/// The results of a finished job: `csim run`'s, with the icons of what its rows name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimResults {
    #[serde(flatten)]
    pub results: Results,
    pub icons: SimIcons,
}

/// The icons of the results' rows, by the name each row has (those without one are left out).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SimIcons {
    pub spells: BTreeMap<String, Icon>,
    pub buffs: BTreeMap<String, Icon>,
    pub procs: BTreeMap<String, Icon>,
    pub resources: BTreeMap<String, Icon>,
    /// The rotation's lines (`(12) Bloodthirst`) and the finishers.
    pub rotation: BTreeMap<String, Icon>,
}

/// A share of a sim being run, or run: its raid, its run until it is done, then its
/// statistics.
pub struct SimJob {
    data: Arc<DataBundle>,
    setup: CharacterSetup,
    /// The run's settings: its iterations, over its threads.
    settings: SimSettings,
    seed: u64,
    /// The share's iterations.
    total: u32,
    raid: RaidControl,
    /// `None` once done.
    run: Option<SimRun>,
    /// Once done.
    cruncher: Option<NumberCruncher>,
}

impl SimJob {
    /// A job of share `share` of a run of `iterations` iterations of `setup` under `settings`
    /// over `threads` threads, seeded with `seed`; none has run yet.
    ///
    /// # Errors
    /// The iterations, threads or share are out of range, the settings are invalid or the
    /// setup does not build.
    pub fn new(
        data: Arc<DataBundle>,
        setup: CharacterSetup,
        settings: SimSettings,
        seed: u64,
        (iterations, threads, share): (u32, u32, u32),
    ) -> Result<SimJob, String> {
        let shares = run_shares(iterations, threads, seed)?;
        let Some(share) = shares.get(share as usize) else {
            return Err(format!(
                "share {share} of {threads} threads: from 0 to {}",
                threads - 1
            ));
        };
        let settings = run_settings(settings, iterations, threads)?;
        let mut raid = setup
            .build_raid(&data, &settings)
            .map_err(|error| error.to_string())?;
        let run = SimRun::for_share(settings.clone(), share, &mut raid);
        Ok(SimJob {
            data,
            setup,
            settings,
            seed,
            total: share.iterations,
            raid,
            run: Some(run),
            cruncher: None,
        })
    }

    /// Runs up to `iterations` more iterations (fewer when fewer are left) and, after the
    /// last, collects them.
    pub fn step(&mut self, iterations: u32) -> SimProgress {
        if let Some(run) = &mut self.run {
            run.run(&mut self.raid, iterations);
            if run.is_done() {
                let run = self.run.take().expect("not done yet");
                let mut cruncher = NumberCruncher::new();
                run.finish(&mut self.raid, &mut cruncher);
                self.cruncher = Some(cruncher);
            }
        }
        self.progress()
    }

    /// How far the share is: its iterations done of its own.
    pub fn progress(&self) -> SimProgress {
        SimProgress {
            name: self.setup.name.clone(),
            seed: self.seed,
            done: self.run.as_ref().map_or(self.total, SimRun::done),
            total: self.total,
        }
    }

    /// Whether every iteration ran.
    pub fn is_done(&self) -> bool {
        self.cruncher.is_some()
    }

    /// Whether the job is the whole run (one thread), whose results it has once done.
    pub fn is_whole(&self) -> bool {
        self.settings.threads == 1
    }

    /// The share's statistics, once done: what [`App::merge_sim`](crate::app::App::merge_sim)
    /// merges.
    pub fn statistics(&self) -> Option<&ClassStatistics> {
        self.cruncher.as_ref()?.class_statistics(None).first()
    }

    /// The results of a whole run (one thread) once done; `elapsed` is the wall-clock time the
    /// run took (the page's measure: the browser has no clock here).
    pub fn results(&self, elapsed: Duration) -> Option<SimResults> {
        let cruncher = self.cruncher.as_ref().filter(|_| self.is_whole())?;
        Some(sim_results(
            &self.data,
            &self.setup,
            &self.settings,
            self.seed,
            elapsed,
            cruncher,
            &self.raid,
        ))
    }
}

/// `settings` for a run of `iterations` iterations over `threads` threads.
///
/// # Errors
/// The settings are invalid.
pub fn run_settings(
    mut settings: SimSettings,
    iterations: u32,
    threads: u32,
) -> Result<SimSettings, String> {
    settings.iterations_quick_sim = iterations;
    // Checked on one: the threads are the page's workers, not this machine's.
    settings.threads = 1;
    settings.validate().map_err(|error| error.to_string())?;
    settings.threads = threads as usize;
    Ok(settings)
}

/// The results of a run of `setup` under `settings` and `seed` that `cruncher` collected, with
/// the icons of their rows from `raid`, a raid of the setup.
pub fn sim_results(
    data: &DataBundle,
    setup: &CharacterSetup,
    settings: &SimSettings,
    seed: u64,
    elapsed: Duration,
    cruncher: &NumberCruncher,
    raid: &RaidControl,
) -> SimResults {
    let results = Results::collect(&Report {
        setup,
        settings,
        seed,
        elapsed,
        cruncher,
        raid: None,
    });
    let icons = SimIcons::of(IconLookup { data, raid }, &results);
    SimResults { results, icons }
}

impl SimIcons {
    fn of(icons: IconLookup, results: &Results) -> SimIcons {
        fn by_name<'a>(
            names: impl IntoIterator<Item = &'a String>,
            icon: impl Fn(&str) -> Option<Icon>,
        ) -> BTreeMap<String, Icon> {
            names
                .into_iter()
                .filter_map(|name| Some((name.clone(), icon(name)?)))
                .collect()
        }
        // A spell's off-hand strike is its own row (`Whirlwind Off-Hand`).
        let spell = |name: &str| {
            icons.source(name).or_else(|| {
                name.strip_suffix(" Off-Hand")
                    .and_then(|name| icons.source(name))
            })
        };
        let buffs = icons.buffs();
        let rotation_lines = results.rotation.iter().map(|row| &row.name);
        let finishers = results.finishers.iter().map(|row| &row.name);
        SimIcons {
            spells: by_name(results.spells.iter().map(|row| &row.name), spell),
            buffs: by_name(results.buffs.iter().map(|row| &row.name), |name| {
                icons.buff(&buffs, name)
            }),
            procs: by_name(results.procs.iter().map(|row| &row.name), |name| {
                icons.proc(name)
            }),
            resources: by_name(results.resources.iter().map(|row| &row.source), |name| {
                icons.source(name)
            }),
            // A line is named by its position and spell: `(12) Bloodthirst`.
            rotation: by_name(rotation_lines.chain(finishers), |name| {
                let spell_name = name
                    .strip_prefix('(')
                    .and_then(|rest| rest.split_once(") "))
                    .map_or(name, |(_, spell_name)| spell_name);
                spell(spell_name)
            }),
        }
    }
}

#[cfg(test)]
mod tests;
