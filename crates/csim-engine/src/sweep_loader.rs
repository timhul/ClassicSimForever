//! Sweep files: `data/sweeps/*.yaml` take a base character setup and variation points and
//! describe every combination of them; [`SweepSetup::expand`] turns one into the character
//! setups to simulate, so the best one can be found under the base's constraints.
//!
//! ```yaml
//! name: DW Fury last 3 points
//! base: ../characters/dw_fury_orc.yaml  # a character setup, relative to this file
//! overrides:                            # optional; changes the base (see below)
//!   talents: { Arms: { Deep Wounds: 3 }, Fury: { Cruelty: 5 } }
//! iterations: 10000                     # optional; iterations per variant
//! length: 300                           # optional; encounter length in seconds
//! variations:
//!   - talent_points:                    # every way to spend exactly `points` more points
//!       points: 3                       # over these talents, up to their maximum ranks
//!       talents:
//!         Arms: [Impale, Improved Overpower]
//!         Fury: [Precision, Dual Wield Specialization]
//!   - options:                          # these variants, each changing the setup
//!       - { label: Orc, race: ORC }     # `label` names it; default: the changes
//!       - { race: TROLL }
//! ```
//!
//! An override (`overrides` and each option) is a partial character setup: its keys replace
//! the setup's, except `equipment`, which replaces the slots it names (a slot set to `null` is
//! emptied). A top-level key set to `null` returns to its default.
//!
//! The variants are the cartesian product of the variation points, so their count is the
//! product of each point's; without variation points the base is the only variant. Each
//! variant applies its points' changes in the order listed. Variants that are not valid
//! setups (a tier not reached, points beyond the level, ...) are reported apart and not
//! simulated.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use crate::character_loader::{CharacterSetup, SetupIssue};
use crate::data_bundle::DataBundle;

/// A `data/sweeps/*.yaml` file. See the module documentation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepSetup {
    pub name: String,
    /// The base character setup, relative to the sweep file.
    pub base: PathBuf,
    /// Changes to the base, applied before the variation points.
    #[serde(default, skip_serializing_if = "Mapping::is_empty")]
    pub overrides: Mapping,
    /// Iterations per variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterations: Option<u32>,
    /// Encounter length in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
    #[serde(default)]
    pub variations: Vec<VariationPoint>,
    /// The file the sweep was loaded from; `base` is relative to its directory.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// One variation point: a set of alternative changes to the setup. Written as a map with the
/// kind as its one key (`talent_points: {...}`, `options: [...]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "VariationPointFile", into = "VariationPointFile")]
pub enum VariationPoint {
    /// Every way to spend exactly `points` more talent points over the talents.
    TalentPoints(TalentPoints),
    /// Explicit alternatives, each an override with an optional `label`.
    Options(Vec<Mapping>),
}

/// The file form of [`VariationPoint`]: serde_yaml writes enums as `!tags`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariationPointFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    talent_points: Option<TalentPoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    options: Option<Vec<Mapping>>,
}

impl TryFrom<VariationPointFile> for VariationPoint {
    type Error = String;

    fn try_from(file: VariationPointFile) -> Result<Self, String> {
        match (file.talent_points, file.options) {
            (Some(points), None) => Ok(VariationPoint::TalentPoints(points)),
            (None, Some(options)) => Ok(VariationPoint::Options(options)),
            _ => Err("a variation point has exactly one of talent_points, options".to_string()),
        }
    }
}

impl From<VariationPoint> for VariationPointFile {
    fn from(point: VariationPoint) -> Self {
        match point {
            VariationPoint::TalentPoints(points) => VariationPointFile {
                talent_points: Some(points),
                ..Self::default()
            },
            VariationPoint::Options(options) => VariationPointFile {
                options: Some(options),
                ..Self::default()
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TalentPoints {
    pub points: u32,
    /// Tab name → the talents that may take points.
    pub talents: BTreeMap<String, Vec<String>>,
}

/// One alternative of a variation point.
#[derive(Debug, Clone, PartialEq)]
struct Alternative {
    label: String,
    change: Change,
}

#[derive(Debug, Clone, PartialEq)]
enum Change {
    /// Merged into the setup as an override.
    Override(Mapping),
    /// `(tab, talent, points)` added to the setup's ranks.
    AddTalents(Vec<(String, String, u32)>),
}

/// A variant to simulate.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// The variation points' labels, joined by ` | `; `base` without variation points.
    pub label: String,
    pub setup: CharacterSetup,
}

/// What a sweep expands to.
#[derive(Debug, Clone, PartialEq)]
pub struct Expansion {
    /// The base with `overrides` applied.
    pub base: CharacterSetup,
    /// Per variation point, a description and its number of alternatives.
    pub points: Vec<(String, usize)>,
    /// The valid variants, in expansion order.
    pub variants: Vec<Variant>,
    /// The variants that are not valid setups, with why.
    pub invalid: Vec<(String, String)>,
}

impl Expansion {
    /// Every combination of the variation points, valid or not.
    pub fn combinations(&self) -> usize {
        self.points.iter().map(|(_, count)| count).product()
    }
}

/// Why a sweep could not be loaded or expanded.
#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("sweep {sweep} is invalid:{}", issues.iter().map(|i| format!("\n  {i}")).collect::<String>())]
    Invalid {
        /// The file, or the sweep name when it was not loaded from a file.
        sweep: String,
        issues: Vec<SetupIssue>,
    },
}

impl SweepSetup {
    /// Parses a sweep file; the base is read by [`expand`](Self::expand).
    pub fn load(path: &Path) -> Result<Self, SweepError> {
        let text = read(path)?;
        let mut sweep: SweepSetup =
            serde_yaml::from_str(&text).map_err(|source| SweepError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        sweep.path = Some(path.to_path_buf());
        Ok(sweep)
    }

    /// Parses every `*.yaml` sweep of `dir`, sorted by file name.
    pub fn load_dir(dir: &Path) -> Result<Vec<Self>, SweepError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|source| SweepError::Io {
                path: dir.to_path_buf(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        paths.sort();
        paths.iter().map(|path| Self::load(path)).collect()
    }

    /// The base setup's file: `base` relative to the sweep file's directory.
    pub fn base_path(&self) -> PathBuf {
        match self.path.as_ref().and_then(|path| path.parent()) {
            Some(dir) => dir.join(&self.base),
            None => self.base.clone(),
        }
    }

    /// Reads the base, applies the overrides and expands the variation points against
    /// `data`. Fails when the sweep itself is wrong (an unknown talent, a variation point
    /// without alternatives, an override that is no setup, ...); variants that are merely
    /// invalid setups end up in [`Expansion::invalid`].
    pub fn expand(&self, data: &DataBundle) -> Result<Expansion, SweepError> {
        let base_path = self.base_path();
        let text = read(&base_path)?;
        let mut base: Value = serde_yaml::from_str(&text).map_err(|source| SweepError::Yaml {
            path: base_path.clone(),
            source,
        })?;
        let mut issues = Vec::new();
        if !base.is_mapping() {
            issues.push(issue("base", "is not a character setup"));
            return Err(self.invalid(issues));
        }
        merge(&mut base, &self.overrides);
        let base = match serde_yaml::from_value::<CharacterSetup>(base) {
            Ok(mut setup) => {
                setup.path = Some(base_path);
                setup
            }
            Err(error) => {
                issues.push(issue("overrides", error.to_string()));
                return Err(self.invalid(issues));
            }
        };

        let mut points = Vec::new();
        let mut alternatives = Vec::new();
        for (index, point) in self.variations.iter().enumerate() {
            let context = format!("variations[{index}]");
            let known = issues.len();
            let found = match point {
                VariationPoint::TalentPoints(spec) => {
                    talent_alternatives(spec, &base, data, &context, &mut issues)
                }
                VariationPoint::Options(options) => option_alternatives(options),
            };
            if found.is_empty() && issues.len() == known {
                issues.push(issue(&context, "has no alternatives"));
            }
            points.push((point.describe(), found.len()));
            alternatives.push(found);
        }
        if !issues.is_empty() {
            return Err(self.invalid(issues));
        }

        let mut variants = Vec::new();
        let mut invalid = Vec::new();
        for combination in cartesian(&alternatives) {
            let label = if combination.is_empty() {
                "base".to_string()
            } else {
                combination
                    .iter()
                    .map(|alt| alt.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            match apply(&base, &combination).and_then(|setup| {
                setup
                    .validate(data)
                    .map(|()| setup)
                    .map_err(|error| error.to_string())
            }) {
                Ok(setup) => variants.push(Variant { label, setup }),
                Err(reason) => invalid.push((label, reason)),
            }
        }
        Ok(Expansion {
            base,
            points,
            variants,
            invalid,
        })
    }

    fn invalid(&self, issues: Vec<SetupIssue>) -> SweepError {
        SweepError::Invalid {
            sweep: self
                .path
                .as_ref()
                .map_or_else(|| format!("{:?}", self.name), |p| p.display().to_string()),
            issues,
        }
    }
}

impl VariationPoint {
    /// A one-line description, e.g. `3 talent points over Precision, Impale`.
    pub fn describe(&self) -> String {
        match self {
            VariationPoint::TalentPoints(spec) => format!(
                "{} talent points over {}",
                spec.points,
                spec.talents
                    .values()
                    .flatten()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            VariationPoint::Options(options) => format!("{} options", options.len()),
        }
    }
}

fn read(path: &Path) -> Result<String, SweepError> {
    fs::read_to_string(path).map_err(|source| SweepError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn issue(context: impl Into<String>, message: impl Into<String>) -> SetupIssue {
    SetupIssue {
        context: context.into(),
        message: message.into(),
    }
}

/// Merges the override `changes` into the setup `target`: keys replace, `equipment` replaces
/// by slot, `null` removes.
fn merge(target: &mut Value, changes: &Mapping) {
    let Some(target) = target.as_mapping_mut() else {
        return;
    };
    for (key, value) in changes {
        if key.as_str() == Some("equipment") {
            if let (Some(Value::Mapping(slots)), Value::Mapping(changed)) =
                (target.get_mut(key), value)
            {
                for (slot, item) in changed {
                    if item.is_null() {
                        slots.remove(slot);
                    } else {
                        slots.insert(slot.clone(), item.clone());
                    }
                }
                continue;
            }
        }
        if value.is_null() {
            target.remove(key);
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}

/// `base` with the changes of `combination` applied in order.
fn apply(base: &CharacterSetup, combination: &[&Alternative]) -> Result<CharacterSetup, String> {
    let mut setup = base.clone();
    for alternative in combination {
        match &alternative.change {
            Change::Override(changes) => {
                let mut value = serde_yaml::to_value(&setup).map_err(|e| e.to_string())?;
                merge(&mut value, changes);
                let path = setup.path.take();
                setup = serde_yaml::from_value(value).map_err(|e| e.to_string())?;
                setup.path = path;
            }
            Change::AddTalents(added) => {
                for (tab, name, points) in added {
                    *setup
                        .talents
                        .entry(tab.clone())
                        .or_default()
                        .entry(name.clone())
                        .or_insert(0) += points;
                }
            }
        }
    }
    Ok(setup)
}

fn option_alternatives(options: &[Mapping]) -> Vec<Alternative> {
    options
        .iter()
        .map(|option| {
            let mut changes = option.clone();
            let label = match changes.remove("label") {
                Some(Value::String(label)) => label,
                Some(other) => yaml_inline(&other),
                None => yaml_inline(&Value::Mapping(changes.clone())),
            };
            Alternative {
                label,
                change: Change::Override(changes),
            }
        })
        .collect()
}

/// `value` as one line of flow YAML-ish text, for labels.
fn yaml_inline(value: &Value) -> String {
    match value {
        Value::Mapping(map) => map
            .iter()
            .map(|(k, v)| format!("{}: {}", yaml_inline(k), yaml_inline(v)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Sequence(items) => format!(
            "[{}]",
            items.iter().map(yaml_inline).collect::<Vec<_>>().join(", ")
        ),
        Value::String(s) => s.clone(),
        Value::Tagged(tagged) => yaml_inline(&tagged.value),
        other => serde_yaml::to_string(other)
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    }
}

/// Every distribution of exactly `spec.points` over the talents, each within the ranks the
/// base leaves open.
fn talent_alternatives(
    spec: &TalentPoints,
    base: &CharacterSetup,
    data: &DataBundle,
    context: &str,
    issues: &mut Vec<SetupIssue>,
) -> Vec<Alternative> {
    let Some(file) = data.talents.get(base.class) else {
        issues.push(issue(
            context,
            format!("no talent tree for the {:?}", base.class),
        ));
        return Vec::new();
    };
    // (tab, talent, open ranks)
    let mut talents: Vec<(String, String, u32)> = Vec::new();
    for (tab_name, names) in &spec.talents {
        let Some(tab) = file.tabs.iter().find(|tab| &tab.name == tab_name) else {
            issues.push(issue(
                format!("{context}.talent_points.talents.{tab_name}"),
                "no such tab",
            ));
            continue;
        };
        for name in names {
            let at = format!("{context}.talent_points.talents.{tab_name}.{name}");
            let Some(talent) = file.talent_by_name(name, Some(tab.skill_line)) else {
                issues.push(issue(at, format!("no such talent in {tab_name}")));
                continue;
            };
            if talents.iter().any(|(t, n, _)| t == tab_name && n == name) {
                issues.push(issue(at, "listed twice"));
                continue;
            }
            let rank = base
                .talents
                .get(tab_name)
                .and_then(|ranks| ranks.get(name))
                .copied()
                .unwrap_or(0);
            talents.push((
                tab_name.clone(),
                name.clone(),
                talent.max_ranks.saturating_sub(rank),
            ));
        }
    }
    if spec.points == 0 {
        issues.push(issue(
            format!("{context}.talent_points.points"),
            "must be at least 1",
        ));
        return Vec::new();
    }
    let caps: Vec<u32> = talents.iter().map(|(_, _, open)| *open).collect();
    distributions(spec.points, &caps)
        .into_iter()
        .map(|counts| {
            let added: Vec<(String, String, u32)> = talents
                .iter()
                .zip(&counts)
                .filter(|(_, &count)| count > 0)
                .map(|((tab, name, _), &count)| (tab.clone(), name.clone(), count))
                .collect();
            let label = added
                .iter()
                .map(|(_, name, count)| format!("{name} +{count}"))
                .collect::<Vec<_>>()
                .join(", ");
            Alternative {
                label,
                change: Change::AddTalents(added),
            }
        })
        .collect()
}

/// Every way to split `total` into `caps.len()` parts, part `i` at most `caps[i]`, in
/// lexicographic order from the first part's largest share.
fn distributions(total: u32, caps: &[u32]) -> Vec<Vec<u32>> {
    fn go(total: u32, caps: &[u32], prefix: &mut Vec<u32>, out: &mut Vec<Vec<u32>>) {
        let Some((&cap, rest)) = caps.split_first() else {
            if total == 0 {
                out.push(prefix.clone());
            }
            return;
        };
        let room: u32 = rest.iter().sum();
        for count in (0..=cap.min(total)).rev() {
            if total - count > room {
                break;
            }
            prefix.push(count);
            go(total - count, rest, prefix, out);
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    go(total, caps, &mut Vec::new(), &mut out);
    out
}

/// Every combination picking one alternative per point; one empty combination without
/// points.
fn cartesian(points: &[Vec<Alternative>]) -> Vec<Vec<&Alternative>> {
    let mut combinations: Vec<Vec<&Alternative>> = vec![Vec::new()];
    for alternatives in points {
        combinations = combinations
            .into_iter()
            .flat_map(|prefix| {
                alternatives.iter().map(move |alternative| {
                    let mut combination = prefix.clone();
                    combination.push(alternative);
                    combination
                })
            })
            .collect();
    }
    combinations
}

#[cfg(test)]
mod tests;
