//! The results of a run: the statistics of every thread and scaling option, merged. Port of
//! `Statistics/NumberCruncher.*` minus the GUI models.
//!
//! Each thread hands over, per scaling option, the [`ClassStatistics`] of the raid's first
//! character with the raid members' [`PlayerResult`]s added (`SimControl` collects them). The
//! cruncher keeps them by option (`None` is the C++ `NoScale` baseline) and answers from them:
//! the merged statistics ([`NumberCruncher::merged`], one call where the C++ merged spells,
//! buffs, procs, resources, engine and executors one by one), the DPS / TPS per option, the
//! raid's results, the baseline DPS distribution and the stat weights.
//!
//! Differences from the C++: the DPS of an option is the merged damage over the merged time
//! (the C++ averaged the threads' DPS unweighted, which is the same when every thread ran as
//! many iterations), and only the baseline runs make up the raid results (the C++ also blended
//! in the scaled runs) — with the TPS blended like the DPS (the C++ kept the first thread's).

use std::collections::BTreeMap;

use super::{ClassStatistics, PlayerResult};
use crate::sim_settings::SimOption;

/// z for a 95 % confidence interval.
const Z_95: f64 = 1.960;

/// How much a scaling option changed the DPS (or TPS), or for the baseline its distribution.
/// Port of `ScaleResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleResult {
    /// `None` for the baseline.
    pub option: Option<SimOption>,
    pub for_dps: bool,
    /// Lowest / highest iteration DPS (the baseline distribution only, else 0).
    pub min_dps: f64,
    pub max_dps: f64,
    /// The option's value minus the baseline's.
    pub absolute_value: f64,
    /// [`ScaleResult::absolute_value`] relative to the baseline.
    pub relative_value: f64,
    /// Of the iteration DPS values.
    pub standard_deviation: f64,
    /// Half-width of the 95 % confidence interval of the mean DPS.
    pub confidence_interval: f64,
}

/// See the module documentation.
#[derive(Debug, Clone, Default)]
pub struct NumberCruncher {
    class_stats: BTreeMap<Option<SimOption>, Vec<ClassStatistics>>,
    player_results: Vec<PlayerResult>,
}

impl NumberCruncher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops everything collected.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Adds one thread's statistics for `option`. Port of `add_class_statistic`.
    ///
    /// # Panics
    /// Panics if a baseline's player results do not line up with the ones collected (another
    /// raid), the C++ `check`s.
    pub fn add_class_statistics(&mut self, option: Option<SimOption>, statistics: ClassStatistics) {
        if option.is_none() {
            self.merge_player_results(statistics.player_results());
        }
        self.class_stats.entry(option).or_default().push(statistics);
    }

    /// Adds everything `other` collected (another thread's cruncher), in its order.
    pub fn absorb(&mut self, other: NumberCruncher) {
        for (option, statistics) in other.class_stats {
            for statistics in statistics {
                self.add_class_statistics(option, statistics);
            }
        }
    }

    /// The options with statistics, the baseline (`None`) first.
    pub fn options(&self) -> impl Iterator<Item = Option<SimOption>> + '_ {
        self.class_stats.keys().copied()
    }

    /// The statistics collected for `option`, one per thread.
    pub fn class_statistics(&self, option: Option<SimOption>) -> &[ClassStatistics] {
        self.class_stats.get(&option).map_or(&[], Vec::as_slice)
    }

    /// The statistics of `option` merged over the threads; `None` without any.
    pub fn merged(&self, option: Option<SimOption>) -> Option<ClassStatistics> {
        let (first, rest) = self.class_stats.get(&option)?.split_first()?;
        let mut merged = first.clone();
        for statistics in rest {
            merged.add(statistics);
        }
        Some(merged)
    }

    fn merged_or_panic(&self, option: Option<SimOption>) -> ClassStatistics {
        self.merged(option)
            .unwrap_or_else(|| panic!("Missing option {option:?} for requested calculation"))
    }

    /// The first character's DPS with `option`. Port of `get_personal_dps`.
    ///
    /// # Panics
    /// Panics if nothing was collected for `option`.
    pub fn personal_dps(&self, option: Option<SimOption>) -> f64 {
        self.merged_or_panic(option).personal_dps()
    }

    /// The first character's TPS with `option`. Port of `get_personal_tps`.
    ///
    /// # Panics
    /// Panics if nothing was collected for `option`.
    pub fn personal_tps(&self, option: Option<SimOption>) -> f64 {
        self.merged_or_panic(option).personal_tps()
    }

    /// Every raid member's baseline result, blended over the threads by iterations.
    pub fn player_results(&self) -> &[PlayerResult] {
        &self.player_results
    }

    /// Port of `get_raid_dps`.
    pub fn raid_dps(&self) -> f64 {
        self.player_results.iter().map(|result| result.dps).sum()
    }

    /// Port of `get_raid_tps`.
    pub fn raid_tps(&self) -> f64 {
        self.player_results.iter().map(|result| result.tps).sum()
    }

    /// The baseline's iteration DPS: lowest, highest, standard deviation and confidence
    /// interval. Port of `get_dps_distribution`.
    ///
    /// # Panics
    /// Panics without baseline statistics.
    pub fn dps_distribution(&self) -> ScaleResult {
        let baseline = self.merged_or_panic(None);
        let dps = baseline.dps_per_iteration();
        let min_dps = dps.iter().copied().fold(f64::INFINITY, f64::min);
        let max_dps = dps.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let (standard_deviation, confidence_interval) = spread(&baseline);
        ScaleResult {
            option: None,
            for_dps: true,
            min_dps,
            max_dps,
            absolute_value: 0.0,
            relative_value: 0.0,
            standard_deviation,
            confidence_interval,
        }
    }

    /// The DPS gained by each scaling option over the baseline. Port of
    /// `calculate_stat_weights_for_dps`.
    ///
    /// # Panics
    /// Panics without baseline statistics.
    pub fn stat_weights_dps(&self) -> Vec<ScaleResult> {
        self.stat_weights(true)
    }

    /// The TPS gained by each scaling option over the baseline; the spread is that of the DPS
    /// (there is no TPS per iteration), as in the C++. Port of
    /// `calculate_stat_weights_for_tps`.
    ///
    /// # Panics
    /// Panics without baseline statistics.
    pub fn stat_weights_tps(&self) -> Vec<ScaleResult> {
        self.stat_weights(false)
    }

    fn stat_weights(&self, for_dps: bool) -> Vec<ScaleResult> {
        let value = |statistics: &ClassStatistics| {
            if for_dps {
                statistics.personal_dps()
            } else {
                statistics.personal_tps()
            }
        };
        let base = value(&self.merged_or_panic(None));
        self.options()
            .filter_map(|option| option.map(|o| (o, self.merged_or_panic(option))))
            .map(|(option, statistics)| {
                let absolute_value = value(&statistics) - base;
                let (standard_deviation, confidence_interval) = spread(&statistics);
                ScaleResult {
                    option: Some(option),
                    for_dps,
                    min_dps: 0.0,
                    max_dps: 0.0,
                    absolute_value,
                    relative_value: absolute_value / base,
                    standard_deviation,
                    confidence_interval,
                }
            })
            .collect()
    }

    /// Blends a thread's baseline player results into the collected ones, weighted by
    /// iterations. Port of `merge_player_results`.
    fn merge_player_results(&mut self, results: &[PlayerResult]) {
        assert!(
            !results.is_empty(),
            "NumberCruncher expected non-empty ClassStatistics::player_results"
        );
        if self.player_results.is_empty() {
            self.player_results = results.to_vec();
            return;
        }
        assert_eq!(
            results.len(),
            self.player_results.len(),
            "NumberCruncher::merge_player_results - Mismatch in expected result sizes"
        );
        for (mine, theirs) in self.player_results.iter_mut().zip(results) {
            assert_eq!(
                mine.player_name, theirs.player_name,
                "Mismatch in player names"
            );
            let iterations = mine.iterations + theirs.iterations;
            if iterations == 0 {
                continue;
            }
            let ratio_new = theirs.iterations as f64 / iterations as f64;
            let ratio_previous = 1.0 - ratio_new;
            mine.dps = mine.dps * ratio_previous + theirs.dps * ratio_new;
            mine.tps = mine.tps * ratio_previous + theirs.tps * ratio_new;
            mine.iterations = iterations;
        }
    }
}

/// Population standard deviation of the iteration DPS around the mean DPS, and the half-width
/// of its 95 % confidence interval. Port of `get_standard_deviation_for_option` /
/// `get_confidence_interval_for_option`.
fn spread(statistics: &ClassStatistics) -> (f64, f64) {
    let dps = statistics.dps_per_iteration();
    if dps.is_empty() {
        return (0.0, 0.0);
    }
    let mean = statistics.personal_dps();
    let variance = dps.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / dps.len() as f64;
    let standard_deviation = variance.sqrt();
    let confidence_interval = Z_95 * standard_deviation / (dps.len() as f64).sqrt();
    (standard_deviation, confidence_interval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_roll::PhysicalAttackResult;
    use crate::spell::AttackOutcome;

    /// Statistics of `damages.len()` iterations of 10 s, each dealing its damage with one hit.
    fn statistics(damages: &[u32]) -> ClassStatistics {
        let mut statistics = ClassStatistics::new("You", 10.0);
        for &damage in damages {
            statistics.spell("Hit", 1).record_attack(
                &AttackOutcome {
                    result: PhysicalAttackResult::Hit,
                    spell: None,
                    damage,
                    threat: f64::from(damage) * 2.0,
                    execution_time: 0.0,
                },
                0.0,
            );
            statistics.finish_combat_iteration(10.0);
        }
        let result = statistics.personal_result();
        statistics.add_player_result(result);
        statistics
    }

    #[test]
    fn dps_is_merged_over_the_threads_and_weighted_by_iterations() {
        let mut cruncher = NumberCruncher::new();
        cruncher.add_class_statistics(None, statistics(&[100, 200, 300]));
        cruncher.add_class_statistics(None, statistics(&[600]));

        // (100 + 200 + 300 + 600) / 40 s.
        assert_eq!(cruncher.personal_dps(None), 30.0);
        assert_eq!(cruncher.personal_tps(None), 60.0);
        assert_eq!(cruncher.merged(None).unwrap().iterations(), 4);
        let results = cruncher.player_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].iterations, 4);
        assert!((results[0].dps - 30.0).abs() < 1e-9);
        assert!((results[0].tps - 60.0).abs() < 1e-9);
        assert!((cruncher.raid_dps() - 30.0).abs() < 1e-9);
    }

    #[test]
    fn dps_distribution_of_the_baseline() {
        let mut cruncher = NumberCruncher::new();
        cruncher.add_class_statistics(None, statistics(&[100, 300]));

        let distribution = cruncher.dps_distribution();
        assert_eq!(distribution.option, None);
        assert_eq!(distribution.min_dps, 10.0);
        assert_eq!(distribution.max_dps, 30.0);
        // Mean 20, deviations ±10.
        assert_eq!(distribution.standard_deviation, 10.0);
        assert!((distribution.confidence_interval - 1.96 * 10.0 / 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn stat_weights_compare_each_option_with_the_baseline() {
        let mut cruncher = NumberCruncher::new();
        cruncher.add_class_statistics(None, statistics(&[200]));
        cruncher.add_class_statistics(Some(SimOption::ScaleStrength), statistics(&[250]));
        cruncher.add_class_statistics(Some(SimOption::ScaleAgility), statistics(&[210]));

        let weights = cruncher.stat_weights_dps();
        assert_eq!(weights.len(), 2);
        assert_eq!(weights[0].option, Some(SimOption::ScaleAgility));
        assert!((weights[0].absolute_value - 1.0).abs() < 1e-9);
        assert!((weights[0].relative_value - 0.05).abs() < 1e-9);
        assert_eq!(weights[1].option, Some(SimOption::ScaleStrength));
        assert!((weights[1].absolute_value - 5.0).abs() < 1e-9);
        assert!(weights.iter().all(|w| w.for_dps));

        let tps = cruncher.stat_weights_tps();
        assert!((tps[1].absolute_value - 10.0).abs() < 1e-9);
        assert!(!tps[1].for_dps);

        // Scaled runs do not count towards the raid results.
        assert_eq!(cruncher.player_results()[0].iterations, 1);
        assert!((cruncher.raid_dps() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn absorb_keeps_the_other_crunchers_statistics() {
        let mut a = NumberCruncher::new();
        a.add_class_statistics(None, statistics(&[100]));
        let mut b = NumberCruncher::new();
        b.add_class_statistics(None, statistics(&[300]));
        b.add_class_statistics(Some(SimOption::ScaleHitChance), statistics(&[400]));

        a.absorb(b);
        assert_eq!(a.class_statistics(None).len(), 2);
        assert_eq!(a.personal_dps(None), 20.0);
        assert_eq!(a.personal_dps(Some(SimOption::ScaleHitChance)), 40.0);
        assert_eq!(
            a.options().collect::<Vec<_>>(),
            [None, Some(SimOption::ScaleHitChance)]
        );
    }

    #[test]
    #[should_panic(expected = "Missing option")]
    fn a_missing_option_panics() {
        NumberCruncher::new().personal_dps(None);
    }
}
