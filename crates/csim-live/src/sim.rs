//! A sim of many iterations of a setup, the page's Sim view: what `csim run` does, run in
//! chunks the page asks for ([`SimJob::step`]) so that it can show the progress and stop. A
//! job's setup is resolved as a load's ([`App::start_sim`](crate::app::App::start_sim)), and it
//! runs on one thread: seed S and N iterations give the results of
//! `csim run -n N -t 1 --seed S`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use csim_engine::character_loader::CharacterSetup;
use csim_engine::data_bundle::DataBundle;
use csim_engine::raid::RaidControl;
use csim_engine::sim_control::SimRun;
use csim_engine::sim_settings::SimSettings;
use csim_engine::statistics::NumberCruncher;
use csim_engine::statistics::results::{Report, Results};
use serde::{Deserialize, Serialize};

use crate::app::LoadRequest;
use crate::icons::IconLookup;
use crate::session::{Icon, as_string};

/// The most iterations a job runs.
pub const MAX_ITERATIONS: u32 = 1_000_000;

/// An `api/sim/start` request: the setup as `api/load` takes it (its keybinds are ignored: the
/// rotation plays) and the iterations.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimRequest {
    pub load: LoadRequest,
    pub iterations: u32,
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

/// A sim being run, or run: its raid, its run until it is done, then its statistics.
pub struct SimJob {
    data: Arc<DataBundle>,
    setup: CharacterSetup,
    settings: SimSettings,
    seed: u64,
    raid: RaidControl,
    /// `None` once done.
    run: Option<SimRun>,
    /// Once done.
    cruncher: Option<NumberCruncher>,
}

impl SimJob {
    /// A job of `iterations` iterations of `setup` under `settings` (on one thread), seeded
    /// with `seed`; none has run yet.
    ///
    /// # Errors
    /// The iterations are not in `1..=MAX_ITERATIONS`, the settings are invalid or the setup
    /// does not build.
    pub fn new(
        data: Arc<DataBundle>,
        setup: CharacterSetup,
        mut settings: SimSettings,
        seed: u64,
        iterations: u32,
    ) -> Result<SimJob, String> {
        if !(1..=MAX_ITERATIONS).contains(&iterations) {
            return Err(format!(
                "{iterations} iterations: from 1 to {MAX_ITERATIONS}"
            ));
        }
        settings.iterations_quick_sim = iterations;
        settings.threads = 1;
        settings.validate().map_err(|error| error.to_string())?;
        let mut raid = setup
            .build_raid(&data, &settings)
            .map_err(|error| error.to_string())?;
        let run = SimRun::new(settings.clone(), seed, &mut raid);
        Ok(SimJob {
            data,
            setup,
            settings,
            seed,
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

    pub fn progress(&self) -> SimProgress {
        let total = self.settings.iterations_quick_sim;
        SimProgress {
            name: self.setup.name.clone(),
            seed: self.seed,
            done: self.run.as_ref().map_or(total, SimRun::done),
            total,
        }
    }

    /// Whether every iteration ran.
    pub fn is_done(&self) -> bool {
        self.cruncher.is_some()
    }

    /// The results, once done; `elapsed` is the wall-clock time the run took (the page's
    /// measure: the browser has no clock here).
    pub fn results(&self, elapsed: Duration) -> Option<SimResults> {
        let cruncher = self.cruncher.as_ref()?;
        let results = Results::collect(&Report {
            setup: &self.setup,
            settings: &self.settings,
            seed: self.seed,
            elapsed,
            cruncher,
            raid: None,
        });
        let icons = SimIcons::of(
            IconLookup {
                data: &self.data,
                raid: &self.raid,
            },
            &results,
        );
        Some(SimResults { results, icons })
    }
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
