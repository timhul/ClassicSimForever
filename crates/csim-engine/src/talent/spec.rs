//! The talent data schema: `data/talents/<class>.yaml`, exported by `csim-tables export-talents`
//! from the Trait tables (`data/TALENT_INSTRUCTIONS.md` §1.2–1.5).
//!
//! A [`TalentFile`] is one class's tree: its tabs (the class skill lines: Arms, Fury,
//! Protection) and one [`TalentSpec`] per Trait node. A talent is one spell whatever its rank
//! count; the rank-dependent numbers are the `CurvePoint` values in `rank_values`, keyed by
//! effect index, and the runtime ([`crate::talent::CharacterTalents`]) applies rank *r* by
//! substituting `rank_values[index][r − 1]` for the effect's base points and enabling the
//! spell as a passive. Tier gating (`points_per_tier × tier` points spent in the tab) comes from
//! `TraitCond`, prerequisites from `TraitEdge`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::faction::PlayerClass;

/// Talent points a character has (`TraitCurrency.SourcedMax` for the classes' currency).
pub const DEFAULT_POINTS: u32 = 51;
/// Points a tab needs per tier (`TraitCond.SpentAmountRequired` / tier).
pub const DEFAULT_POINTS_PER_TIER: u32 = 5;

/// A tab of the tree (`TraitNodeGroupDisplayInfo`): one class skill line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TalentTab {
    /// `SkillLine.ID` (Warrior: 26 Arms, 256 Fury, 257 Protection).
    pub skill_line: u32,
    /// `SkillLine.DisplayName_lang`.
    pub name: String,
}

/// One talent (`TraitNode` → `TraitNodeEntry` → `TraitDefinition`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TalentSpec {
    /// `TraitNode.ID`: what the runtime and setups address a talent by.
    pub node: u32,
    /// `TraitDefinition.SpellID`: the talent spell (a passive, a proc aura or the ability the
    /// talent grants).
    pub spell: u32,
    /// The spell's name, for readability and name lookups (the spell db has it too).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The tab's skill line.
    pub tab: u32,
    /// Row in the tab, from 0.
    pub tier: u32,
    /// Column in the tab, from 0.
    pub column: u32,
    /// `TraitNodeEntry.MaxRanks`.
    pub max_ranks: u32,
    /// The node that must be maxed first (`TraitEdge`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<u32>,
    /// `CurvePoint` values per effect index: `rank_values[index][r − 1]` replaces the effect's
    /// base points at rank `r`. Effects without an entry keep their table value at every rank.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rank_values: BTreeMap<u32, Vec<f64>>,
}

impl TalentSpec {
    /// Whether `rank` is a rank the talent can have points in.
    pub fn is_rank(&self, rank: u32) -> bool {
        rank >= 1 && rank <= self.max_ranks
    }

    /// The `(effect index, value)` substitutions of `rank` (1-based); empty for rank 0.
    pub fn values_at(&self, rank: u32) -> Vec<(u32, f64)> {
        if rank == 0 {
            return Vec::new();
        }
        self.rank_values
            .iter()
            .filter_map(|(index, values)| {
                values
                    .get(usize::try_from(rank - 1).ok()?)
                    .map(|value| (*index, *value))
            })
            .collect()
    }
}

/// One `data/talents/<class>.yaml` file: the tree of one class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TalentFile {
    /// The client build the tree was exported from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build: String,
    pub class: PlayerClass,
    /// `TraitTree.ID`.
    #[serde(default)]
    pub tree: u32,
    /// Talent points available (`TraitCurrency.SourcedMax`).
    #[serde(default = "default_points")]
    pub points: u32,
    /// Points spent in a tab per tier a tier requires (`TraitCond`).
    #[serde(default = "default_points_per_tier")]
    pub points_per_tier: u32,
    pub tabs: Vec<TalentTab>,
    #[serde(default)]
    pub talents: Vec<TalentSpec>,
}

fn default_points() -> u32 {
    DEFAULT_POINTS
}

fn default_points_per_tier() -> u32 {
    DEFAULT_POINTS_PER_TIER
}

#[derive(Debug, thiserror::Error)]
pub enum TalentSpecError {
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
    #[error("talent data for {0:?}: {1}")]
    Invalid(PlayerClass, String),
    #[error("talent data for {0:?} appears twice")]
    DuplicateClass(PlayerClass),
}

impl TalentFile {
    /// Loads and validates one file.
    pub fn load(path: &Path) -> Result<Self, TalentSpecError> {
        let text = fs::read_to_string(path).map_err(|source| TalentSpecError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: TalentFile =
            serde_yaml::from_str(&text).map_err(|source| TalentSpecError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        file.validate()?;
        Ok(file)
    }

    /// Checks the tree is consistent: known tabs, unique nodes / spells / positions, ranks
    /// with a value per rank, prerequisites in the same tab, in an earlier or equal tier and
    /// without cycles.
    pub fn validate(&self) -> Result<(), TalentSpecError> {
        let invalid = |message: String| TalentSpecError::Invalid(self.class, message);
        if self.tabs.is_empty() {
            return Err(invalid("no tabs".into()));
        }
        if self.points == 0 || self.points_per_tier == 0 {
            return Err(invalid(
                "points and points_per_tier must be positive".into(),
            ));
        }
        let mut lines = HashSet::new();
        for tab in &self.tabs {
            if !lines.insert(tab.skill_line) {
                return Err(invalid(format!("tab {} appears twice", tab.skill_line)));
            }
        }
        let mut nodes = HashSet::new();
        let mut spells = HashSet::new();
        let mut positions = HashSet::new();
        let mut names: HashSet<(u32, &str)> = HashSet::new();
        for talent in &self.talents {
            let what = format!("talent {} ({})", talent.node, talent.name);
            if !nodes.insert(talent.node) {
                return Err(invalid(format!("{what}: node appears twice")));
            }
            if !spells.insert(talent.spell) {
                return Err(invalid(format!(
                    "{what}: spell {} belongs to another talent too",
                    talent.spell
                )));
            }
            if !lines.contains(&talent.tab) {
                return Err(invalid(format!("{what}: unknown tab {}", talent.tab)));
            }
            if !positions.insert((talent.tab, talent.tier, talent.column)) {
                return Err(invalid(format!(
                    "{what}: tier {} column {} of tab {} is taken",
                    talent.tier, talent.column, talent.tab
                )));
            }
            if !talent.name.is_empty() && !names.insert((talent.tab, talent.name.as_str())) {
                return Err(invalid(format!("{what}: name appears twice in its tab")));
            }
            if talent.max_ranks == 0 {
                return Err(invalid(format!("{what}: max_ranks must be positive")));
            }
            for (index, values) in &talent.rank_values {
                if values.len() != talent.max_ranks as usize {
                    return Err(invalid(format!(
                        "{what}: effect {index} has {} rank values for {} ranks",
                        values.len(),
                        talent.max_ranks
                    )));
                }
            }
            if talent.requires == Some(talent.node) {
                return Err(invalid(format!("{what}: requires itself")));
            }
        }
        let by_node: HashMap<u32, &TalentSpec> = self.talents.iter().map(|t| (t.node, t)).collect();
        for talent in &self.talents {
            let Some(parent) = talent.requires else {
                continue;
            };
            let what = format!("talent {} ({})", talent.node, talent.name);
            let Some(parent) = by_node.get(&parent) else {
                return Err(invalid(format!("{what}: requires unknown node {parent}")));
            };
            if parent.tab != talent.tab {
                return Err(invalid(format!(
                    "{what}: requires {} of another tab",
                    parent.node
                )));
            }
            if parent.tier > talent.tier {
                return Err(invalid(format!(
                    "{what}: requires {} of a later tier",
                    parent.node
                )));
            }
            // Same-tier chains cannot loop back: walk the parents.
            let mut seen = vec![talent.node];
            let mut current = parent.node;
            loop {
                if seen.contains(&current) {
                    return Err(invalid(format!("{what}: prerequisite cycle")));
                }
                seen.push(current);
                match by_node.get(&current).and_then(|t| t.requires) {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }
        Ok(())
    }

    /// The tab with `skill_line`.
    pub fn tab(&self, skill_line: u32) -> Option<&TalentTab> {
        self.tabs.iter().find(|t| t.skill_line == skill_line)
    }

    /// The talent at `node`.
    pub fn talent(&self, node: u32) -> Option<&TalentSpec> {
        self.talents.iter().find(|t| t.node == node)
    }

    /// The talent whose spell is `spell`.
    pub fn talent_by_spell(&self, spell: u32) -> Option<&TalentSpec> {
        self.talents.iter().find(|t| t.spell == spell)
    }

    /// The talent named `name` (in `tab` when given, else anywhere in the tree).
    pub fn talent_by_name(&self, name: &str, tab: Option<u32>) -> Option<&TalentSpec> {
        self.talents
            .iter()
            .find(|t| t.name == name && tab.is_none_or(|tab| t.tab == tab))
    }

    /// The talents of `tab`, by tier then column.
    pub fn talents_of_tab(&self, skill_line: u32) -> Vec<&TalentSpec> {
        let mut talents: Vec<&TalentSpec> = self
            .talents
            .iter()
            .filter(|t| t.tab == skill_line)
            .collect();
        talents.sort_by_key(|t| (t.tier, t.column));
        talents
    }

    /// The number of tiers the tree has (highest tier + 1).
    pub fn tier_count(&self) -> u32 {
        self.talents.iter().map(|t| t.tier + 1).max().unwrap_or(0)
    }
}

/// Every loaded talent file, by class.
#[derive(Debug, Clone, Default)]
pub struct TalentDb {
    files: BTreeMap<PlayerClass, Arc<TalentFile>>,
}

impl TalentDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads every `*.yaml` / `*.yml` file directly in `dir`.
    pub fn load(dir: &Path) -> Result<Self, TalentSpecError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|source| TalentSpecError::Io {
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
        let mut db = TalentDb::new();
        for path in paths {
            db.add(TalentFile::load(&path)?)?;
        }
        Ok(db)
    }

    /// Adds a file after validating it.
    pub fn add(&mut self, file: TalentFile) -> Result<(), TalentSpecError> {
        file.validate()?;
        if self.files.contains_key(&file.class) {
            return Err(TalentSpecError::DuplicateClass(file.class));
        }
        self.files.insert(file.class, Arc::new(file));
        Ok(())
    }

    /// The tree of `class`.
    pub fn get(&self, class: PlayerClass) -> Option<&Arc<TalentFile>> {
        self.files.get(&class)
    }

    pub fn classes(&self) -> impl Iterator<Item = PlayerClass> + '_ {
        self.files.keys().copied()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Four Arms talents (the `csim-tables` fixture nodes) plus Cruelty in Fury.
    pub(crate) const ARMS_YAML: &str = r#"
build: 1.60.1.70009
class: WARRIOR
tree: 1117
points: 51
points_per_tier: 5
tabs:
- { skill_line: 26, name: Arms }
- { skill_line: 256, name: Fury }
talents:
- { node: 105956, spell: 12286, name: Improved Rend, tab: 26, tier: 0, column: 2, max_ranks: 3,
    rank_values: { 0: [12, 23, 35] } }
- { node: 105950, spell: 12834, name: Deep Wounds, tab: 26, tier: 2, column: 2, max_ranks: 3,
    requires: 105956, rank_values: { 0: [20, 40, 60] } }
- { node: 105945, spell: 12292, name: Sweeping Strikes, tab: 26, tier: 4, column: 1, max_ranks: 1 }
- { node: 105941, spell: 12294, name: Mortal Strike, tab: 26, tier: 6, column: 1, max_ranks: 1,
    requires: 105945 }
- { node: 105939, spell: 12320, name: Cruelty, tab: 256, tier: 0, column: 2, max_ranks: 5,
    rank_values: { 0: [1, 2, 3, 4, 5] } }
"#;

    pub(crate) fn arms() -> TalentFile {
        let file: TalentFile = serde_yaml::from_str(ARMS_YAML).unwrap();
        file.validate().unwrap();
        file
    }

    #[test]
    fn parses_and_looks_up() {
        let file = arms();
        assert_eq!(file.class, PlayerClass::Warrior);
        assert_eq!(file.tab(26).unwrap().name, "Arms");
        assert_eq!(file.talent(105950).unwrap().spell, 12834);
        assert_eq!(file.talent_by_spell(12294).unwrap().node, 105941);
        assert_eq!(file.talent_by_name("Cruelty", None).unwrap().node, 105939);
        assert!(file.talent_by_name("Cruelty", Some(26)).is_none());
        assert_eq!(
            file.talents_of_tab(26)
                .iter()
                .map(|t| t.node)
                .collect::<Vec<_>>(),
            [105956, 105950, 105945, 105941]
        );
        assert_eq!(file.tier_count(), 7);
        let deep_wounds = file.talent(105950).unwrap();
        assert!(deep_wounds.values_at(0).is_empty());
        assert_eq!(deep_wounds.values_at(2), [(0, 40.0)]);
        assert!(deep_wounds.is_rank(3));
        assert!(!deep_wounds.is_rank(4));
        assert!(!deep_wounds.is_rank(0));
        assert!(file.talent(105945).unwrap().values_at(1).is_empty());
    }

    fn invalid(yaml: &str) -> String {
        let file: TalentFile = serde_yaml::from_str(yaml).unwrap();
        match file.validate() {
            Err(TalentSpecError::Invalid(_, message)) => message,
            other => panic!("expected an invalid file, got {other:?}"),
        }
    }

    #[test]
    fn validation_rejects_inconsistent_trees() {
        let base = |talents: &str| {
            format!("class: WARRIOR\ntabs: [{{ skill_line: 26, name: Arms }}]\ntalents:\n{talents}")
        };
        let one = "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 2 }\n";
        assert!(invalid(&base(&format!("{one}{one}"))).contains("node appears twice"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 1 }\n\
             - { node: 2, spell: 10, tab: 26, tier: 0, column: 1, max_ranks: 1 }\n"
        ))
        .contains("belongs to another talent"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 27, tier: 0, column: 0, max_ranks: 1 }\n"
        ))
        .contains("unknown tab"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 1 }\n\
             - { node: 2, spell: 11, tab: 26, tier: 0, column: 0, max_ranks: 1 }\n"
        ))
        .contains("is taken"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, name: A, tab: 26, tier: 0, column: 0, max_ranks: 1 }\n\
             - { node: 2, spell: 11, name: A, tab: 26, tier: 0, column: 1, max_ranks: 1 }\n"
        ))
        .contains("name appears twice"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 0 }\n"
        ))
        .contains("max_ranks"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 2, rank_values: { 0: [1] } }\n"
        ))
        .contains("rank values"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 1, requires: 1 }\n"
        ))
        .contains("requires itself"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 1, requires: 9 }\n"
        ))
        .contains("unknown node"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 1, requires: 2 }\n\
             - { node: 2, spell: 11, tab: 26, tier: 1, column: 0, max_ranks: 1 }\n"
        ))
        .contains("later tier"));
        assert!(invalid(&base(
            "- { node: 1, spell: 10, tab: 26, tier: 0, column: 0, max_ranks: 1, requires: 2 }\n\
             - { node: 2, spell: 11, tab: 26, tier: 0, column: 1, max_ranks: 1, requires: 1 }\n"
        ))
        .contains("cycle"));
        assert!(invalid("class: WARRIOR\ntabs: []\n").contains("no tabs"));
        assert!(invalid(
            "class: WARRIOR\ntabs: [{ skill_line: 26, name: Arms }, { skill_line: 26, name: Arms }]\n"
        )
        .contains("appears twice"));
        assert!(
            invalid("class: WARRIOR\npoints: 0\ntabs: [{ skill_line: 26, name: Arms }]\n")
                .contains("positive")
        );
    }

    #[test]
    fn the_db_loads_a_directory_and_rejects_duplicate_classes() {
        let dir = std::env::temp_dir().join(format!("csim-talents-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("warrior.yaml"), ARMS_YAML).unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let db = TalentDb::load(&dir).unwrap();
        assert_eq!(db.len(), 1);
        assert!(!db.is_empty());
        assert_eq!(db.get(PlayerClass::Warrior).unwrap().tree, 1117);
        assert_eq!(db.classes().collect::<Vec<_>>(), [PlayerClass::Warrior]);
        fs::write(dir.join("warrior2.yaml"), ARMS_YAML).unwrap();
        assert!(matches!(
            TalentDb::load(&dir),
            Err(TalentSpecError::DuplicateClass(PlayerClass::Warrior))
        ));
        fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(
            TalentDb::load(&dir),
            Err(TalentSpecError::Io { .. })
        ));
    }

    #[test]
    fn the_shipped_warrior_tree_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/talents/warrior.yaml");
        let file = TalentFile::load(&path).expect("shipped talent data loads");
        assert_eq!(file.class, PlayerClass::Warrior);
        assert_eq!(file.tree, 1117);
        assert_eq!(file.points, 51);
        assert_eq!(file.points_per_tier, 5);
        assert_eq!(
            file.tabs
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["Arms", "Fury", "Protection"]
        );
        assert_eq!(file.talents.len(), 53);
        let mortal_strike = file.talent_by_name("Mortal Strike", None).unwrap();
        assert_eq!(mortal_strike.spell, 12294);
        assert_eq!((mortal_strike.tier, mortal_strike.column), (6, 1));
        assert_eq!(mortal_strike.requires, Some(105945));
        let deep_wounds = file.talent(105950).unwrap();
        assert_eq!(deep_wounds.values_at(3), [(0, 60.0)]);
        let dual_wield = file
            .talent_by_name("Dual Wield Specialization", None)
            .unwrap();
        assert_eq!(dual_wield.values_at(5), [(0, 25.0), (1, 100.0), (2, 10.0)]);
    }
}
