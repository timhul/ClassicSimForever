//! Running the simulation: the iteration loop, the scaling runs and the threads. Port of
//! `GUI/SimControl.*` and `Thread/SimulationThreadPool.*` / `SimulationRunner.*`.
//!
//! [`SimControl::run_sim`] runs a set of iterations of a raid: each iteration starts early
//! enough for the slowest precombat actions, shuffles the raid (who acts first at a tie),
//! runs every character's precombat actions, starts the encounter for each of them (and the
//! incoming damage of the tanks), ends it after the combat length (drawn per iteration within
//! the length variance) and resets the raid.
//! [`SimControl::run_quick_sim`] runs the baseline, [`SimControl::run_full_sim`] also one run
//! per scaling option with the option's stat added to the first character; both hand the statistics of the raid's
//! first character, with every member's result added, to a [`NumberCruncher`].
//!
//! [`IterationStepper`] runs one iteration through the same pieces an event at a time, for
//! watching it unfold.
//!
//! [`run_threaded`] is the thread pool: each thread builds its own raid (the raid is not
//! shared between threads), seeds it, runs its share of the iterations and returns its
//! cruncher; the crunchers are merged in thread order. With the same seed and thread count
//! a run is reproducible.
//!
//! Differences from the C++: the encounter length varies per iteration
//! ([`SimSettings::length_variance`]), the iterations are split so that every requested iteration runs
//! (the C++ dropped the remainder of `iterations / threads`), a seed fixes every random roll
//! of the run including the raid shuffle, and the progress callback also reports the last
//! iterations that do not fill a group of ten.

use std::sync::Arc;

use crate::combat_log::CombatLog;
use crate::engine::{Event, EventKind};
use crate::ids::CharId;
use crate::raid::RaidControl;
use crate::rng::Xoroshiro128Plus;
use crate::sim_settings::{SimOption, SimSettings};
use crate::statistics::NumberCruncher;

/// Called with the number of iterations completed since the previous call.
pub type Progress = Arc<dyn Fn(u32) + Send + Sync>;

/// Iterations between two progress reports.
const PROGRESS_INTERVAL: u32 = 10;

/// Mixed into a sim control's seed for the encounter length generator, so that it is a stream
/// of its own (SplitMix64 seeding decorrelates the two).
const LENGTH_STREAM: u64 = 0x6C65_6E67_7468_5F31;

/// A baseline run, or also the scaling runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimMode {
    /// [`SimSettings::iterations_quick_sim`] iterations, no scaling.
    Quick,
    /// [`SimSettings::iterations_full_sim`] iterations for the baseline and for each option.
    Full,
}

impl SimMode {
    fn iterations(self, settings: &SimSettings) -> u32 {
        match self {
            SimMode::Quick => settings.iterations_quick_sim,
            SimMode::Full => settings.iterations_full_sim,
        }
    }

    fn set_iterations(self, settings: &mut SimSettings, iterations: u32) {
        match self {
            SimMode::Quick => settings.iterations_quick_sim = iterations,
            SimMode::Full => settings.iterations_full_sim = iterations,
        }
    }
}

/// Runs iterations of one raid. See the module documentation. Port of `SimControl`.
pub struct SimControl {
    settings: SimSettings,
    /// Seeds the raid (its combat rolls) when the first set of iterations begins; the later
    /// sets (scaling options, another run) continue its streams.
    raid_seed: Option<u64>,
    /// Shuffles the raid order each iteration (the C++ `std::mt19937` on a random device).
    shuffle: Xoroshiro128Plus,
    /// Seeds [`SimControl::lengths`] at the start of every set of iterations.
    length_seed: u64,
    /// Draws the encounter lengths and nothing else, so that they depend only on the seed:
    /// every set of iterations (baseline, scaling option, sweep variant) of the same seed
    /// and thread gets the same lengths, whatever the raid.
    lengths: Xoroshiro128Plus,
    progress: Option<Progress>,
}

impl SimControl {
    /// A sim control for `settings` whose combat rolls, raid shuffles and encounter lengths
    /// derive from `seed`: it seeds the raid when its first set of iterations begins. It runs
    /// what the only thread of a [`run_threaded`] with the same `seed` runs.
    pub fn new(settings: SimSettings, seed: u64) -> Self {
        let mut seeds = Xoroshiro128Plus::from_seed(seed);
        let (raid_seed, shuffle_seed) = (seeds.next(), seeds.next());
        Self::with_seeds(settings, raid_seed, shuffle_seed)
    }

    /// A sim control seeding the raid with `raid_seed`, its shuffles and encounter lengths
    /// deriving from `shuffle_seed`.
    fn with_seeds(settings: SimSettings, raid_seed: u64, shuffle_seed: u64) -> Self {
        let length_seed = shuffle_seed ^ LENGTH_STREAM;
        SimControl {
            settings,
            raid_seed: Some(raid_seed),
            shuffle: Xoroshiro128Plus::from_seed(shuffle_seed),
            length_seed,
            lengths: Xoroshiro128Plus::from_seed(length_seed),
            progress: None,
        }
    }

    /// Reports the iterations completed to `progress`. Port of the `update_progress` signal.
    pub fn with_progress(mut self, progress: Progress) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn settings(&self) -> &SimSettings {
        &self.settings
    }

    /// Runs the baseline and collects it. Port of `SimControl::run_quick_sim`.
    pub fn run_quick_sim(&mut self, raid: &mut RaidControl, cruncher: &mut NumberCruncher) {
        let (combat_length, iterations) = (
            self.settings.combat_length,
            self.settings.iterations_quick_sim,
        );
        self.run_sim(raid, combat_length, iterations);
        collect(raid, None, cruncher);
    }

    /// Runs the baseline, then each scaling option of the settings, and collects every run.
    /// Port of `SimControl::run_full_sim`.
    pub fn run_full_sim(&mut self, raid: &mut RaidControl, cruncher: &mut NumberCruncher) {
        let (combat_length, iterations) = (
            self.settings.combat_length,
            self.settings.iterations_full_sim,
        );
        self.run_sim(raid, combat_length, iterations);
        collect(raid, None, cruncher);

        let options: Vec<SimOption> = self.settings.options.iter().copied().collect();
        for option in options {
            self.run_sim_with_option(raid, option, combat_length, iterations);
            collect(raid, Some(option), cruncher);
        }
    }

    /// Runs [`SimControl::run_quick_sim`] or [`SimControl::run_full_sim`].
    pub fn run(&mut self, mode: SimMode, raid: &mut RaidControl, cruncher: &mut NumberCruncher) {
        match mode {
            SimMode::Quick => self.run_quick_sim(raid, cruncher),
            SimMode::Full => self.run_full_sim(raid, cruncher),
        }
    }

    /// Runs `iterations` encounters of `combat_length` seconds, each varied by the settings'
    /// length variance. The characters' statistics hold the result afterwards. Port of
    /// `SimControl::run_sim`.
    ///
    /// # Panics
    /// Panics if a character was set up for another combat length (its DPS would be wrong).
    pub fn run_sim(&mut self, raid: &mut RaidControl, combat_length: u32, iterations: u32) {
        let mut set = self.begin_set_of_iterations(raid, combat_length);
        let mut reported = 0;
        for _ in 0..iterations {
            self.begin_iteration(raid, &mut set);
            raid.run();
            end_iteration(raid);

            reported += 1;
            if reported == PROGRESS_INTERVAL {
                self.report(reported);
                reported = 0;
            }
        }
        if reported > 0 {
            self.report(reported);
        }
        end_set_of_iterations(raid, &set);
    }

    /// Prepares the raid for a set of iterations of `combat_length` seconds.
    ///
    /// # Panics
    /// Panics if a character was set up for another combat length.
    fn begin_set_of_iterations(
        &mut self,
        raid: &mut RaidControl,
        combat_length: u32,
    ) -> IterationSet {
        let combat_length = f64::from(combat_length);
        for character in raid.characters() {
            assert_eq!(
                character.sim().combat_length,
                combat_length,
                "{} was set up for another combat length",
                character.player_name()
            );
        }

        if let Some(seed) = self.raid_seed.take() {
            raid.set_seed(seed);
        }
        // Drops the attack tables and prepares the characters' statistics and rotations.
        raid.prepare_set_of_combat_iterations();
        self.lengths.set_state(self.length_seed);

        // The C++ started from the smallest positive double; the pull is at 0 either way.
        let start_at = raid
            .char_ids()
            .map(|id| raid.context(id).time_required_to_run_precombat())
            .fold(0.0, f64::max);

        IterationSet {
            combat_length,
            start_at,
            order: raid.char_ids().collect(),
        }
    }

    /// Starts an iteration: shuffles the raid, runs the precombat actions and schedules the
    /// encounter start and end. The raid's events then run the iteration.
    fn begin_iteration(&mut self, raid: &mut RaidControl, set: &mut IterationSet) {
        raid.engine_mut().prepare_iteration(-set.start_at);

        self.shuffle_order(&mut set.order);

        // Also casts the precast spell if it is enabled.
        for &id in &set.order {
            raid.with_character(id, |ctx| ctx.run_precombat_actions());
        }

        for &id in &set.order {
            if raid.character(id).is_tanking() {
                raid.engine_mut()
                    .add_event(Event::new(0.0, EventKind::IncomingDamage { character: id }));
            }
            raid.engine_mut()
                .add_event(Event::new(0.0, EventKind::EncounterStart { character: id }));
        }

        let iteration_length = self.draw_combat_length(set.combat_length);
        raid.set_combat_length(iteration_length);
        raid.engine_mut()
            .add_event(Event::new(iteration_length, EventKind::EncounterEnd));
    }

    /// `combat_length` scaled by a uniform draw from `[1 - v, 1 + v]`, `v` the length
    /// variance.
    fn draw_combat_length(&mut self, combat_length: f64) -> f64 {
        let variance = self.settings.length_variance / 100.0;
        if variance == 0.0 {
            return combat_length;
        }
        // 53 random bits: uniform in [0, 1).
        let unit = (self.lengths.next() >> 11) as f64 / (1u64 << 53) as f64;
        combat_length * (1.0 + variance * (2.0 * unit - 1.0))
    }

    /// Runs with `option`'s stat added to the raid's first character, the player whose stat
    /// weights are collected. Port of `SimControl::run_sim_with_option`, which added it to
    /// every character.
    fn run_sim_with_option(
        &mut self,
        raid: &mut RaidControl,
        option: SimOption,
        combat_length: u32,
        iterations: u32,
    ) {
        let player = CharId(0);
        option.add_to(raid.character_mut(player).stats_mut());
        self.run_sim(raid, combat_length, iterations);
        option.remove_from(raid.character_mut(player).stats_mut());
    }

    /// Fisher–Yates, the `std::shuffle` of `SimControl::run_sim`.
    fn shuffle_order(&mut self, order: &mut [CharId]) {
        for i in (1..order.len()).rev() {
            let j = (self.shuffle.next() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
    }

    fn report(&self, iterations: u32) {
        if let Some(progress) = &self.progress {
            progress(iterations);
        }
    }
}

/// What every iteration of a set needs, from [`SimControl::begin_set_of_iterations`].
#[derive(Debug, Clone)]
struct IterationSet {
    combat_length: f64,
    /// Seconds before the pull the iterations start at, for the slowest precombat actions.
    start_at: f64,
    /// Who acts first at a tie, shuffled every iteration.
    order: Vec<CharId>,
}

/// Ends an iteration whose events ran out: resets every character, checks that the target is
/// clean and closes the iteration for the statistics.
fn end_iteration(raid: &mut RaidControl) {
    raid.reset();
    raid.finish_combat_iteration();
}

/// Ends a set of iterations: the characters' combat length back to the set's.
fn end_set_of_iterations(raid: &mut RaidControl, set: &IterationSet) {
    raid.set_combat_length(set.combat_length);
}

/// Hands the first character's statistics, with every member's result added, to the
/// cruncher. The `add_player_result` / `add_class_statistic` lines of `SimControl`.
fn collect(raid: &mut RaidControl, option: Option<SimOption>, cruncher: &mut NumberCruncher) {
    let mut statistics = raid.take_statistics();
    assert!(
        !statistics.is_empty(),
        "Cannot collect the results of an empty raid"
    );
    let results: Vec<_> = statistics.iter().map(|s| s.personal_result()).collect();
    let mut first = statistics.swap_remove(0);
    for result in results {
        first.add_player_result(result);
    }
    cruncher.add_class_statistics(option, first);
}

/// Runs one iteration of `raid` with the combat log recorded and returns the log; the raid's
/// statistics hold the iteration's results. Seeded like the only thread of [`run_threaded`], so
/// it is the iteration a one-thread, one-iteration run with the same `seed` simulates.
pub fn run_logged_iteration(
    settings: &SimSettings,
    seed: u64,
    raid: &mut RaidControl,
) -> CombatLog {
    IterationStepper::new(settings, seed, raid).finish(raid)
}

/// One iteration run an event at a time, for watching it unfold: [`run_logged_iteration`]
/// stepped by the caller. Seeded and run like it (and so like the only thread of
/// [`run_threaded`]), through the same pieces as [`SimControl::run_sim`], with the combat log
/// recorded.
///
/// Every call takes the raid the stepper was created with. The combat log is final up to its
/// length after each [`IterationStepper::step`]: an event only inserts entries after the log
/// length at its own start.
#[derive(Debug)]
pub struct IterationStepper {
    set: IterationSet,
}

impl IterationStepper {
    /// Seeds `raid` from `seed` (as [`SimControl::new`]), enables its combat log and starts the iteration: the
    /// precombat actions have run, the engine's clock is at the start of the iteration
    /// (before the pull) and the events of the iteration are queued.
    ///
    /// # Panics
    /// Panics if a character was set up for another combat length than `settings`'.
    pub fn new(settings: &SimSettings, seed: u64, raid: &mut RaidControl) -> Self {
        Self::with_pre_pull(settings, seed, raid, 0.0)
    }

    /// As [`IterationStepper::new`], with the iteration starting at least `pre_pull` seconds
    /// before the pull (a player choosing when to pull).
    ///
    /// # Panics
    /// Panics if a character was set up for another combat length than `settings`'.
    pub fn with_pre_pull(
        settings: &SimSettings,
        seed: u64,
        raid: &mut RaidControl,
        pre_pull: f64,
    ) -> Self {
        raid.engine_mut().enable_combat_log();
        let mut control = SimControl::new(settings.clone(), seed);
        let mut set = control.begin_set_of_iterations(raid, settings.combat_length);
        set.start_at = set.start_at.max(pre_pull);
        control.begin_iteration(raid, &mut set);
        IterationStepper { set }
    }

    /// Seconds before the pull the iteration started at.
    pub fn start_at(&self) -> f64 {
        -self.set.start_at
    }

    /// Runs the next event and returns it; `None` once the iteration is over.
    pub fn step(&mut self, raid: &mut RaidControl) -> Option<Event> {
        raid.step()
    }

    /// The time of the next event; `None` once the iteration is over.
    pub fn next_event_time(&self, raid: &RaidControl) -> Option<f64> {
        raid.engine().peek().map(|event| event.time)
    }

    /// Runs every event at or before `time` and returns how many ran.
    pub fn step_until(&mut self, raid: &mut RaidControl, time: f64) -> usize {
        let mut steps = 0;
        while self.next_event_time(raid).is_some_and(|next| next <= time) {
            self.step(raid);
            steps += 1;
        }
        steps
    }

    /// Whether every event of the iteration ran.
    pub fn is_done(&self, raid: &RaidControl) -> bool {
        raid.engine().peek().is_none()
    }

    /// Runs what is left of the iteration, ends it (the characters' statistics hold its
    /// results) and returns the combat log.
    pub fn finish(mut self, raid: &mut RaidControl) -> CombatLog {
        while self.step(raid).is_some() {}
        end_iteration(raid);
        end_set_of_iterations(raid, &self.set);
        raid.engine_mut()
            .take_combat_log()
            .expect("the log was enabled")
    }
}

/// Runs `settings.threads` threads, each on a raid of its own from `build`, and merges their
/// results. `seed` fixes the run; `progress` hears from every thread. Port of
/// `SimulationThreadPool::run_sim` and `SimulationRunner::sim_runner_run`.
///
/// # Errors
/// The first error `build` returns, in thread order.
///
/// # Panics
/// Panics if a thread panicked.
pub fn run_threaded<E: Send>(
    settings: &SimSettings,
    mode: SimMode,
    seed: u64,
    progress: Option<Progress>,
    build: impl Fn() -> Result<RaidControl, E> + Sync,
) -> Result<NumberCruncher, E> {
    let threads = settings.threads.max(1);
    let iterations = mode.iterations(settings);
    let mut seeds = Xoroshiro128Plus::from_seed(seed);
    let jobs: Vec<(u32, u64, u64)> = split_iterations(iterations, threads)
        .into_iter()
        .map(|share| (share, seeds.next(), seeds.next()))
        .filter(|&(share, _, _)| share > 0)
        .collect();

    // One thread's share of the iterations, on a raid of its own.
    let run_share = |(share, raid_seed, shuffle_seed): (u32, u64, u64)| {
        let mut local = settings.clone();
        mode.set_iterations(&mut local, share);
        let mut raid = build()?;
        let mut control = SimControl::with_seeds(local, raid_seed, shuffle_seed);
        if let Some(progress) = progress.clone() {
            control = control.with_progress(progress);
        }
        let mut cruncher = NumberCruncher::new();
        control.run(mode, &mut raid, &mut cruncher);
        Ok(cruncher)
    };
    // A single share runs on the calling thread: the same result without spawning one, and
    // possible where there are no threads (the browser).
    let results: Vec<Result<NumberCruncher, E>> = if jobs.len() == 1 {
        jobs.into_iter().map(run_share).collect()
    } else {
        let run_share = &run_share;
        std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .into_iter()
                .map(|job| scope.spawn(move || run_share(job)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("simulation thread panicked"))
                .collect()
        })
    };

    let mut cruncher = NumberCruncher::new();
    for result in results {
        cruncher.absorb(result?);
    }
    Ok(cruncher)
}

/// `iterations` split over `threads`, the first threads taking one more of the remainder.
fn split_iterations(iterations: u32, threads: usize) -> Vec<u32> {
    let threads = u32::try_from(threads).expect("thread count fits u32");
    (0..threads)
        .map(|i| iterations / threads + u32::from(i < iterations % threads))
        .collect()
}

#[cfg(test)]
mod tests;
