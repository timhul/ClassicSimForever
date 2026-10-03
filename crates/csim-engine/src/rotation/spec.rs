//! The rotation file schema: `data/rotations/<class>/<file>.yaml`. Port of the data half of
//! `Rotation/RotationFileReader.*` and the fields of `Rotation/Rotation.h`.
//!
//! A [`RotationSpec`] is one rotation: its class, name, attack mode, the spells cast before the
//! pull, an optional precast (a cast that completes at t = 0) and the ordered `cast_if`
//! executors ([`CastIfSpec`]). The condition of an executor is kept as the text of the file
//! here; the condition mini-language is parsed by `rotation::condition` (Phase 5.2) and the
//! executors are linked to spells by `rotation::executor` (Phase 5.3).
//!
//! Rotation files are found by scanning the class subdirectories of `data/rotations/`
//! ([`RotationDb::load`]), which replaces the C++ `rotation_paths.xml`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attack_mode::AttackMode;
use crate::faction::PlayerClass;
use crate::rotation::condition::{Condition, ConditionParseError};
use crate::spell::MAX_RANK;

/// One `cast_if` executor: cast `name` (at `rank`) when `condition` holds.
///
/// Port of the `<cast_if name= [rank=]>` element. Without a condition the spell is cast
/// whenever it is available (the spell's own status checks still apply). The same spell may
/// appear in several executors with different conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastIfSpec {
    /// The spell name (a rank group name, e.g. `Heroic Strike`).
    pub name: String,
    /// The rank to cast; absent means the highest learned rank (`MAX_RANK`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    /// The condition text, one sentence per line, joined by `and` / `or` (Phase 5.2 parses
    /// it). Absent: cast whenever available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

impl CastIfSpec {
    /// An executor without a condition.
    pub fn always(name: &str) -> Self {
        CastIfSpec {
            name: name.to_string(),
            rank: None,
            condition: None,
        }
    }

    /// An executor with a condition.
    pub fn when(name: &str, condition: &str) -> Self {
        CastIfSpec {
            name: name.to_string(),
            rank: None,
            condition: Some(condition.to_string()),
        }
    }

    /// The rank to cast, `MAX_RANK` when the file does not name one. Port of
    /// `hasAttribute("rank") ? rank : Spell::MAX_RANK`.
    pub fn rank(&self) -> u32 {
        self.rank.unwrap_or(MAX_RANK)
    }

    /// The parsed condition, `None` without one (cast whenever available).
    pub fn parse_condition(&self) -> Result<Option<Condition>, ConditionParseError> {
        self.condition.as_deref().map(Condition::parse).transpose()
    }

    /// The non-blank, trimmed lines of the condition (empty without one): the sentences the
    /// condition parser consumes.
    pub fn condition_lines(&self) -> Vec<&str> {
        self.condition
            .as_deref()
            .map(|condition| {
                condition
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A rotation file. Port of the `<rotation>` element and `Rotation`'s data fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationSpec {
    /// The class the rotation is for.
    pub class: PlayerClass,
    /// Display name, unique within the class.
    pub name: String,
    /// What the auto attacks are; `melee` when absent.
    #[serde(default = "default_attack_mode")]
    pub attack_mode: AttackMode,
    /// Free text, whitespace-simplified on load.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Spells cast before the pull, in order (`<precombat_actions><spell name=/>`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub precombat_actions: Vec<String>,
    /// A cast started before the pull so that it completes at t = 0 (`<precast_spell>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precast: Option<String>,
    /// The executors, in priority order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cast_if: Vec<CastIfSpec>,
    /// Spells the rotation cannot do without (`prerequisite:`, one name or a list): the
    /// rotation is not valid for a character that lacks one of them, e.g. an arms rotation
    /// (`Mortal Strike`) on a character without the talent.
    #[serde(
        default,
        rename = "prerequisite",
        deserialize_with = "crate::character_loader::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub prerequisites: Vec<String>,
}

fn default_attack_mode() -> AttackMode {
    AttackMode::MeleeAttack
}

#[derive(Debug, thiserror::Error)]
pub enum RotationSpecError {
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
    #[error("rotation {name:?} ({class:?}): {message}")]
    Invalid {
        class: PlayerClass,
        name: String,
        message: String,
    },
    #[error("{path}: rotation is for {found:?} but lies in the {expected:?} directory")]
    ClassMismatch {
        path: PathBuf,
        found: PlayerClass,
        expected: PlayerClass,
    },
    #[error("{path}: rotation {name:?} for {class:?} is defined twice")]
    Duplicate {
        path: PathBuf,
        class: PlayerClass,
        name: String,
    },
    #[error("rotation {name:?} for {class:?} is not in the rotation directory")]
    Missing { class: PlayerClass, name: String },
    #[error("rotation {name:?} ({class:?}): cast_if {index} ({executor}): {source}")]
    Condition {
        class: PlayerClass,
        name: String,
        index: usize,
        executor: String,
        #[source]
        source: ConditionParseError,
    },
}

impl RotationSpec {
    /// Loads and validates one rotation file.
    pub fn load(path: &Path) -> Result<Self, RotationSpecError> {
        let text = fs::read_to_string(path).map_err(|source| RotationSpecError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut spec: RotationSpec =
            serde_yaml::from_str(&text).map_err(|source| RotationSpecError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        spec.description = simplified(&spec.description);
        spec.validate()?;
        Ok(spec)
    }

    /// Checks: non-empty names, no explicit rank 0, no blank conditions and every condition
    /// parses.
    pub fn validate(&self) -> Result<(), RotationSpecError> {
        let invalid = |message: String| RotationSpecError::Invalid {
            class: self.class,
            name: self.name.clone(),
            message,
        };
        if self.name.trim().is_empty() {
            return Err(invalid("name is empty".to_string()));
        }
        if let Some(index) = self
            .precombat_actions
            .iter()
            .position(|name| name.trim().is_empty())
        {
            return Err(invalid(format!("precombat action {index} has no name")));
        }
        if self.precast.as_deref().is_some_and(|s| s.trim().is_empty()) {
            return Err(invalid("precast has no name".to_string()));
        }
        if let Some(index) = self
            .prerequisites
            .iter()
            .position(|name| name.trim().is_empty())
        {
            return Err(invalid(format!("prerequisite {index} has no name")));
        }
        for (index, executor) in self.cast_if.iter().enumerate() {
            if executor.name.trim().is_empty() {
                return Err(invalid(format!("cast_if {index} has no name")));
            }
            if executor.rank == Some(MAX_RANK) {
                return Err(invalid(format!(
                    "cast_if {index} ({}): rank {MAX_RANK} is reserved for the highest rank; \
                     leave rank out instead",
                    executor.name
                )));
            }
            if executor
                .condition
                .as_deref()
                .is_some_and(|c| c.trim().is_empty())
            {
                return Err(invalid(format!(
                    "cast_if {index} ({}): condition is blank; leave it out to cast whenever \
                     available",
                    executor.name
                )));
            }
            if let Err(source) = executor.parse_condition() {
                return Err(RotationSpecError::Condition {
                    class: self.class,
                    name: self.name.clone(),
                    index,
                    executor: executor.name.clone(),
                    source,
                });
            }
        }
        Ok(())
    }
}

/// `QString::simplified()`: trims and collapses runs of whitespace to one space.
fn simplified(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The rotations of `data/rotations/`, grouped by class and in file order.
#[derive(Debug, Clone, Default)]
pub struct RotationDb {
    by_class: BTreeMap<PlayerClass, Vec<Arc<RotationSpec>>>,
}

impl RotationDb {
    /// Loads `dir/<class>/*.yaml`. Every subdirectory named after a class is scanned (sorted,
    /// for determinism); a file whose `class` does not match its directory or that repeats a
    /// `(class, name)` is an error. Other directories and files are ignored.
    pub fn load(dir: &Path) -> Result<Self, RotationSpecError> {
        let io = |path: &Path, source| RotationSpecError::Io {
            path: path.to_path_buf(),
            source,
        };
        let mut class_dirs: Vec<(PlayerClass, PathBuf)> = fs::read_dir(dir)
            .map_err(|source| io(dir, source))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter_map(|path| {
                let class = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| {
                        serde_yaml::from_str::<PlayerClass>(&name.to_uppercase()).ok()
                    })?;
                Some((class, path))
            })
            .collect();
        class_dirs.sort();
        let mut db = Self::default();
        for (expected, class_dir) in class_dirs {
            let mut paths: Vec<PathBuf> = fs::read_dir(&class_dir)
                .map_err(|source| io(&class_dir, source))?
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
            for path in paths {
                let spec = RotationSpec::load(&path)?;
                if spec.class != expected {
                    return Err(RotationSpecError::ClassMismatch {
                        path,
                        found: spec.class,
                        expected,
                    });
                }
                if db.get(spec.class, &spec.name).is_some() {
                    return Err(RotationSpecError::Duplicate {
                        path,
                        class: spec.class,
                        name: spec.name,
                    });
                }
                db.by_class
                    .entry(spec.class)
                    .or_default()
                    .push(Arc::new(spec));
            }
        }
        Ok(db)
    }

    /// The rotation `name` of `class`, if loaded.
    pub fn get(&self, class: PlayerClass, name: &str) -> Option<&Arc<RotationSpec>> {
        self.rotations_for(class)
            .iter()
            .find(|spec| spec.name == name)
    }

    /// Like [`get`](Self::get) but a [`RotationSpecError::Missing`] when absent.
    pub fn require(
        &self,
        class: PlayerClass,
        name: &str,
    ) -> Result<&Arc<RotationSpec>, RotationSpecError> {
        self.get(class, name)
            .ok_or_else(|| RotationSpecError::Missing {
                class,
                name: name.to_string(),
            })
    }

    /// The rotations of `class` in file order (empty for a class without any).
    pub fn rotations_for(&self, class: PlayerClass) -> &[Arc<RotationSpec>] {
        self.by_class.get(&class).map_or(&[], Vec::as_slice)
    }

    /// Every rotation, ordered by class then file.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<RotationSpec>> {
        self.by_class.values().flatten()
    }

    pub fn len(&self) -> usize {
        self.by_class.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DW_FURY: &str = r#"
class: WARRIOR
name: DW Fury
attack_mode: melee
description: >
  A rotation for dual-wield fury that does not
  attempt to dump rage   before switching to Battle Stance for Overpower.
precombat_actions:
  - Bloodrage
  - Battle Shout
  - Berserker Stance
cast_if:
  - name: Bloodrage
    condition: resource "Rage" less 70
  - name: Battle Shout
    condition: |
      buff_duration "Battle Shout" less 3
      or variable "time_remaining_execute" less 10
      and variable "time_remaining_execute" greater 0
      and buff_duration "Battle Shout" less 45
  - name: Overpower
  - name: Heroic Strike
    rank: 8
    condition: resource "Rage" greater 65
  - name: Heroic Strike
    condition: resource "Rage" greater 30
"#;

    fn parse(text: &str) -> Result<RotationSpec, RotationSpecError> {
        let dir = temp_dir("parse");
        let path = dir.join("rotation.yaml");
        fs::write(&path, text).unwrap();
        let result = RotationSpec::load(&path);
        fs::remove_dir_all(&dir).unwrap();
        result
    }

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "csim-rotation-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rotations")
    }

    #[test]
    fn parses_the_dw_fury_example() {
        let spec = parse(DW_FURY).unwrap();
        assert_eq!(spec.class, PlayerClass::Warrior);
        assert_eq!(spec.name, "DW Fury");
        assert_eq!(spec.attack_mode, AttackMode::MeleeAttack);
        assert_eq!(
            spec.description,
            "A rotation for dual-wield fury that does not attempt to dump rage before \
             switching to Battle Stance for Overpower."
        );
        assert_eq!(
            spec.precombat_actions,
            ["Bloodrage", "Battle Shout", "Berserker Stance"]
        );
        assert_eq!(spec.precast, None);

        let names: Vec<&str> = spec.cast_if.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Bloodrage",
                "Battle Shout",
                "Overpower",
                "Heroic Strike",
                "Heroic Strike"
            ]
        );
        assert_eq!(spec.cast_if[0].rank(), MAX_RANK);
        assert_eq!(
            spec.cast_if[0].condition_lines(),
            ["resource \"Rage\" less 70"]
        );
        assert_eq!(
            spec.cast_if[1].condition_lines(),
            [
                "buff_duration \"Battle Shout\" less 3",
                "or variable \"time_remaining_execute\" less 10",
                "and variable \"time_remaining_execute\" greater 0",
                "and buff_duration \"Battle Shout\" less 45",
            ]
        );
        assert_eq!(spec.cast_if[2].condition, None);
        assert!(spec.cast_if[2].condition_lines().is_empty());
        assert_eq!(spec.cast_if[3].rank(), 8);
        assert_eq!(spec.cast_if[4].rank(), MAX_RANK);
    }

    #[test]
    fn defaults_apply_to_a_minimal_file() {
        let spec = parse("class: WARRIOR\nname: Minimal\n").unwrap();
        assert_eq!(spec.attack_mode, AttackMode::MeleeAttack);
        assert!(spec.description.is_empty());
        assert!(spec.precombat_actions.is_empty());
        assert_eq!(spec.precast, None);
        assert!(spec.cast_if.is_empty());
        assert!(spec.prerequisites.is_empty());
    }

    #[test]
    fn prerequisite_is_one_spell_or_a_list() {
        let spec = parse(
            "class: WARRIOR
name: X
prerequisite: Mortal Strike
",
        )
        .unwrap();
        assert_eq!(spec.prerequisites, ["Mortal Strike"]);
        let spec = parse(
            "class: WARRIOR
name: X
prerequisite: [Bloodthirst, Death Wish]
",
        )
        .unwrap();
        assert_eq!(spec.prerequisites, ["Bloodthirst", "Death Wish"]);
        assert!(matches!(
            parse("class: WARRIOR
name: X
prerequisite: ' '
"),
            Err(RotationSpecError::Invalid { message, .. }) if message.contains("prerequisite 0")
        ));
    }

    #[test]
    fn precast_and_ranged_attack_mode() {
        let spec =
            parse("class: HUNTER\nname: Marksmanship\nattack_mode: ranged\nprecast: Aimed Shot\n")
                .unwrap();
        assert_eq!(spec.attack_mode, AttackMode::RangedAttack);
        assert_eq!(spec.precast.as_deref(), Some("Aimed Shot"));
    }

    #[test]
    fn serializes_and_deserializes_unchanged() {
        let spec = RotationSpec {
            class: PlayerClass::Warrior,
            name: "Round trip".to_string(),
            attack_mode: AttackMode::MeleeAttack,
            description: "Some text.".to_string(),
            precombat_actions: vec!["Bloodrage".to_string()],
            precast: None,
            cast_if: vec![
                CastIfSpec::when("Bloodrage", "resource \"Rage\" less 70"),
                CastIfSpec {
                    rank: Some(3),
                    ..CastIfSpec::always("Heroic Strike")
                },
                CastIfSpec::always("Overpower"),
            ],
            prerequisites: vec!["Overpower".to_string()],
        };
        let text = serde_yaml::to_string(&spec).unwrap();
        let back: RotationSpec = serde_yaml::from_str(&text).unwrap();
        assert_eq!(back, spec);
        assert!(!text.contains("precast"), "{text}");
        assert!(!text.contains("rank: null"), "{text}");
    }

    #[test]
    fn rejects_unknown_fields_and_missing_class() {
        assert!(matches!(
            parse("class: WARRIOR\nname: X\nbogus: 1\n"),
            Err(RotationSpecError::Yaml { .. })
        ));
        assert!(matches!(
            parse("name: X\n"),
            Err(RotationSpecError::Yaml { .. })
        ));
        assert!(matches!(
            parse("class: WARRIOR\nname: X\nattack_mode: bogus\n"),
            Err(RotationSpecError::Yaml { .. })
        ));
        assert!(matches!(
            parse("class: WARRIOR\nname: X\ncast_if:\n  - name: Y\n    extra: 1\n"),
            Err(RotationSpecError::Yaml { .. })
        ));
    }

    #[test]
    fn rejects_structural_errors() {
        let invalid = |text: &str, needle: &str| match parse(text) {
            Err(RotationSpecError::Invalid { message, .. }) => {
                assert!(message.contains(needle), "{message}")
            }
            other => panic!("expected Invalid containing {needle:?}, got {other:?}"),
        };
        invalid("class: WARRIOR\nname: ''\n", "name is empty");
        invalid(
            "class: WARRIOR\nname: X\nprecombat_actions: ['']\n",
            "precombat action 0",
        );
        invalid("class: WARRIOR\nname: X\nprecast: '  '\n", "precast");
        invalid(
            "class: WARRIOR\nname: X\ncast_if:\n  - name: ''\n",
            "cast_if 0 has no name",
        );
        invalid(
            "class: WARRIOR\nname: X\ncast_if:\n  - name: Y\n    rank: 0\n",
            "rank 0 is reserved",
        );
        invalid(
            "class: WARRIOR\nname: X\ncast_if:\n  - name: Y\n    condition: \"  \\n \"\n",
            "condition is blank",
        );
    }

    #[test]
    fn rejects_a_condition_that_does_not_parse() {
        match parse(
            "class: WARRIOR
name: X
cast_if:
  - name: Overpower
  - name: Bloodthirst
    condition: spell \"Whirlwind\" gt 1
",
        ) {
            Err(RotationSpecError::Condition {
                index,
                executor,
                source,
                ..
            }) => {
                assert_eq!(index, 1);
                assert_eq!(executor, "Bloodthirst");
                assert!(
                    source.message.contains("unknown comparator `gt`"),
                    "{source}"
                );
            }
            other => panic!("expected Condition, got {other:?}"),
        }
    }

    #[test]
    fn parse_condition_returns_none_without_one() {
        let spec = parse(DW_FURY).unwrap();
        assert!(spec.cast_if[2].parse_condition().unwrap().is_none());
        let battle_shout = spec.cast_if[1].parse_condition().unwrap().unwrap();
        assert_eq!(battle_shout.groups().len(), 2);
        assert_eq!(battle_shout.groups()[1].len(), 3);
    }

    #[test]
    fn loads_the_fixture_directory() {
        let db = RotationDb::load(&fixtures()).unwrap();
        assert_eq!(db.len(), 2);
        let names: Vec<&str> = db
            .rotations_for(PlayerClass::Warrior)
            .iter()
            .map(|spec| spec.name.as_str())
            .collect();
        assert_eq!(names, ["Fixture Fury", "Fixture Prot"]);
        assert!(db.rotations_for(PlayerClass::Rogue).is_empty());
        let fury = db.get(PlayerClass::Warrior, "Fixture Fury").unwrap();
        assert_eq!(fury.precombat_actions, ["Bloodrage", "Battle Shout"]);
        assert_eq!(fury.cast_if.len(), 3);
        assert!(db.get(PlayerClass::Rogue, "Fixture Fury").is_none());
        assert!(matches!(
            db.require(PlayerClass::Warrior, "Nope"),
            Err(RotationSpecError::Missing { .. })
        ));
        assert_eq!(db.iter().count(), 2);
    }

    #[test]
    fn rejects_a_duplicate_name_within_a_class() {
        let dir = temp_dir("dup");
        let warrior = dir.join("warrior");
        fs::create_dir_all(&warrior).unwrap();
        fs::write(warrior.join("a.yaml"), "class: WARRIOR\nname: Same\n").unwrap();
        fs::write(warrior.join("b.yaml"), "class: WARRIOR\nname: Same\n").unwrap();
        let result = RotationDb::load(&dir);
        fs::remove_dir_all(&dir).unwrap();
        match result {
            Err(RotationSpecError::Duplicate { path, name, .. }) => {
                assert_eq!(name, "Same");
                assert!(path.ends_with("b.yaml"), "{}", path.display());
            }
            other => panic!("expected Duplicate, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_class_in_the_wrong_directory() {
        let dir = temp_dir("mismatch");
        let warrior = dir.join("warrior");
        fs::create_dir_all(&warrior).unwrap();
        fs::write(warrior.join("a.yaml"), "class: ROGUE\nname: Combat\n").unwrap();
        let result = RotationDb::load(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(
            result,
            Err(RotationSpecError::ClassMismatch {
                found: PlayerClass::Rogue,
                expected: PlayerClass::Warrior,
                ..
            })
        ));
    }

    #[test]
    fn ignores_non_class_directories_and_non_yaml_files() {
        let dir = temp_dir("ignore");
        fs::create_dir_all(dir.join("warrior")).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join("README.md"), "# rotations\n").unwrap();
        fs::write(dir.join("warrior/README.md"), "# warrior\n").unwrap();
        fs::write(dir.join("warrior/a.yml"), "class: WARRIOR\nname: A\n").unwrap();
        fs::write(dir.join("notes/b.yaml"), "not: a rotation\n").unwrap();
        let result = RotationDb::load(&dir);
        fs::remove_dir_all(&dir).unwrap();
        let db = result.unwrap();
        assert_eq!(db.len(), 1);
        assert!(db.get(PlayerClass::Warrior, "A").is_some());
    }
}
