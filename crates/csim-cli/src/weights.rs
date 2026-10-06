//! The stat weights file `csim run --scale --weights-file` writes: the DPS and TPS one point of
//! each item stat is worth, derived from the scaling options' results
//! ([`SimOption::item_stats`]). An approximation: the weights hold near the simulated setup
//! only.

use std::collections::BTreeMap;
use std::path::Path;

use csim_engine::faction::PlayerClass;
use csim_engine::item::ItemStat;
use csim_engine::phase::Phase;
use csim_engine::sim_settings::SimOption;
use csim_engine::statistics::NumberCruncher;
use csim_engine::statistics::results::Report;
use serde::{Deserialize, Serialize};

use crate::Result;

/// The stat weights of one setup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatWeights {
    pub setup: String,
    pub class: PlayerClass,
    pub rotation: String,
    pub phase: Phase,
    pub iterations: u64,
    pub seed: u64,
    /// The baseline DPS and TPS.
    pub dps: f64,
    pub tps: f64,
    /// Per point of each item stat, in data-file units (a `HIT_CHANCE` weight is per 1.0, so
    /// per 100 %). Stats without a weight were not scaled.
    pub weights: BTreeMap<ItemStat, StatWeight>,
}

/// What one point of an item stat adds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatWeight {
    pub dps: f64,
    pub tps: f64,
}

impl StatWeights {
    /// The weights of a run with scaling options.
    pub fn collect(r: &Report) -> StatWeights {
        let stats = r
            .cruncher
            .merged(None)
            .expect("a run collects the baseline");
        StatWeights {
            setup: r.setup.name.clone(),
            class: r.setup.class,
            rotation: r.setup.rotation.clone(),
            phase: r.settings.phase,
            iterations: stats.iterations(),
            seed: r.seed,
            dps: stats.personal_dps(),
            tps: stats.personal_tps(),
            weights: item_stat_weights(r.cruncher),
        }
    }

    pub fn read(path: &Path) -> Result<StatWeights> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        serde_yaml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()).into())
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(path, yaml)
            .map_err(|error| format!("cannot write {}: {error}", path.display()).into())
    }

    /// The weight of one point of `stat`, if it was scaled.
    pub fn get(&self, stat: ItemStat) -> Option<StatWeight> {
        self.weights.get(&stat).copied()
    }
}

/// Each scaled option's gain spread over the item stats equal to it.
fn item_stat_weights(cruncher: &NumberCruncher) -> BTreeMap<ItemStat, StatWeight> {
    let mut weights = BTreeMap::new();
    for (dps, tps) in cruncher
        .stat_weights_dps()
        .into_iter()
        .zip(cruncher.stat_weights_tps())
    {
        let Some(option) = dps.option else { continue };
        for (stat, amount) in SimOption::item_stats(option) {
            weights.insert(
                stat,
                StatWeight {
                    dps: dps.absolute_value / amount,
                    tps: tps.absolute_value / amount,
                },
            );
        }
    }
    weights
}
