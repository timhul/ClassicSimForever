//! The results of a run as data, shared by `csim run` (which prints them as text tables, YAML
//! or HTML) and the web page's Sim view (which renders them from JSON): [`Results::collect`]
//! gathers them from a [`Report`], the statistics a run handed to its [`NumberCruncher`].
//!
//! Rates and shares are fractions (0.25 for 25 %); per fight values are averages per
//! iteration. The wall-clock time comes from the caller ([`Report::elapsed`]): the browser has
//! no clock the engine could read.

use std::time::Duration;

use serde::Serialize;

use super::report::{
    BuffRow, ProcRow, ResourceRow, ResourceTotal, SpellRow, buff_rows, proc_rows, resource_rows,
    resource_totals, spell_rows,
};
use super::{ClassStatistics, NumberCruncher};
use crate::character_loader::CharacterSetup;
use crate::sim_settings::{SimOption, SimSettings};

/// What a run's results are collected from.
pub struct Report<'a> {
    pub setup: &'a CharacterSetup,
    pub settings: &'a SimSettings,
    pub seed: u64,
    /// The wall-clock time the run took.
    pub elapsed: Duration,
    pub cruncher: &'a NumberCruncher,
    /// In a raid.
    pub raid: Option<&'a RaidRoster>,
}

/// The raid's name and, in `CharId` order, each member's party (1-based) and setup name.
pub struct RaidRoster {
    pub name: String,
    pub members: Vec<(u8, String)>,
}

/// The results of a run.
#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetupInfo {
    pub name: String,
    pub race: String,
    pub class: String,
    pub rotation: String,
    pub phase: String,
    pub ruleset: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunInfo {
    pub iterations: u64,
    pub combat_length: u32,
    /// Percent.
    pub length_variance: f64,
    /// The named settings that are not the default (`--setting`), as `name:value,...`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<String>,
    pub threads: usize,
    pub seed: u64,
    pub elapsed_seconds: f64,
    /// Engine events handled per second of wall-clock time.
    pub events_per_second: f64,
    pub events: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DpsSummary {
    pub mean: f64,
    pub confidence_interval: f64,
    pub standard_deviation: f64,
    pub min: f64,
    pub max: f64,
}

/// The raid's results; the player is the first member.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaidSummary {
    pub name: String,
    pub dps: f64,
    pub tps: f64,
    pub members: Vec<RaidMemberRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaidMemberRow {
    /// 1-based.
    pub party: u8,
    pub name: String,
    pub dps: f64,
    pub dps_share: f64,
    pub tps: f64,
}

/// A finisher's casts per fight by the combo points they spent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FinisherRow {
    pub name: String,
    /// Casts per fight with 1 to 5 combo points.
    pub per_fight: [f64; 5],
    /// The average combo points spent.
    pub average: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExecutorRow {
    pub name: String,
    pub outcomes: Vec<OutcomeRow>,
}

/// A `cast_if` line that was not linked to the character.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkippedRow {
    /// 1-based position among the rotation's `cast_if` lines.
    pub line: usize,
    pub spell: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutcomeRow {
    pub outcome: String,
    pub per_fight: f64,
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineRow {
    pub event: String,
    pub count: u64,
    pub per_fight: f64,
    /// Handled per second of wall-clock time.
    pub per_second: f64,
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatWeightRow {
    pub option: String,
    pub dps: f64,
    pub relative: f64,
    pub confidence_interval: f64,
    pub tps: f64,
}

impl Results {
    /// The results of the run `r`.
    ///
    /// # Panics
    /// Panics if the run collected no baseline.
    pub fn collect(r: &Report) -> Results {
        let stats = r
            .cruncher
            .merged(None)
            .expect("a run collects the baseline");
        let distribution = r.cruncher.dps_distribution();
        let spells = spell_rows(&stats, stats.iterations(), stats.time_in_combat());
        let resources = resource_rows(&stats);
        Results {
            setup: SetupInfo {
                name: r.setup.name.clone(),
                race: r.setup.race.name().to_string(),
                class: r.setup.class.name().to_string(),
                rotation: r.setup.rotation.clone(),
                phase: r.settings.phase.description().to_string(),
                ruleset: r.settings.ruleset.name().to_lowercase(),
            },
            run: RunInfo {
                iterations: stats.iterations(),
                combat_length: r.settings.combat_length,
                length_variance: r.settings.length_variance,
                settings: r.settings.named_settings_text(),
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
            resource_totals: resource_totals(
                &resources,
                |kind| stats.lost_at_cap(kind),
                stats.iterations(),
                stats.time_in_combat(),
            ),
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
}

/// `count` per iteration (or per attempt, per event): 0 without any.
pub fn per(count: u64, iterations: u64) -> f64 {
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
mod tests;
