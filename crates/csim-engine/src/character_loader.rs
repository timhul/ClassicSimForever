//! Character setup files: `data/characters/*.yaml` describe one character (class, race,
//! talents, gear, buffs, rotation) and the target it fights; [`CharacterSetup::build_raid`]
//! turns one into a [`RaidControl`] ready for [`SimControl`](crate::sim_control::SimControl).
//! Replaces the C++ GUI's character setup and its saved-settings files.
//!
//! ```yaml
//! name: DW Fury Orc            # display name
//! class: WARRIOR
//! race: ORC
//! level: 60                    # default 60
//! phase: 3                     # optional; overrides the sim settings' content phase
//! ruleset: STANDARD            # optional; STANDARD | VAELASTRASZ | LOATHEB
//! rotation: DW Fury            # a rotation of data/rotations/<class>/, by name
//! tanking: false               # default false; a tank is attacked by the target
//! talents:                     # tab name → talent name → rank; default none
//!   Fury:
//!     Cruelty: 5
//! equipment:                   # slot → item id and enchants; default nothing
//!   MAINHAND:
//!     item: 18828
//!     enchant: Crusader
//!     # at most one of each group: stone / oil, Windfury, poison
//!     temp_enchants: [WindfuryTotem, ElementalSharpeningStone]
//!   HEAD: { item: 12640 }
//! buffs: [Battle Squawk]       # data/external_buffs.yaml `buffs`, by name
//! debuffs: [Sunder Armor]      # data/external_buffs.yaml `debuffs`, by name
//! consumables: [Thistle Tea]   # data/external_buffs.yaml `consumables`, by name; the
//!                              # rotation uses them
//! target:                      # default: a level 63 Dragonkin raid boss with 3750 armor
//!   level: 63
//!   armor: 3731
//!   creature_type: Dragonkin
//!   resistances: { fire: 93, shadow: 186 }  # default none
//! ```
//!
//! `include` (a path relative to the file, or a list of them) reads other setup files, which
//! may be partial and may include further files, in place: the keys before it are overwritten by
//! the included ones, and the keys after it overwrite them. A key replaces the earlier value,
//! except `equipment`, which replaces the slots it names (a slot set to `null` is emptied); a
//! top-level key set to `null` returns to its default. `include` may appear several times.
//!
//! ```yaml
//! include: common/dw_fury.yaml  # everything but the race
//! name: DW Fury Troll
//! race: TROLL
//! equipment:
//!   MAINHAND: { item: 17075, enchant: Crusader, temp_enchants: [WindfuryTotem] }
//! ```
//!
//! Loading only parses the file; everything that needs the data (does the talent exist, may
//! the class use the enchant, does the item fit the slot in that phase, ...) is checked while
//! building, and every problem found is reported together, each with the field it came from.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::{Mapping, Value};

use crate::character::{Character, ClassSpec};
use crate::data_bundle::DataBundle;
use crate::enchant::EnchantName;
use crate::equipment::Equipment;
use crate::faction::{Faction, PlayerClass};
use crate::files::{Files, FsFiles, yaml_files};
use crate::ids::CharId;
use crate::item::EquipmentSlot;
use crate::magic_school::MagicSchool;
use crate::mechanics::Mechanics;
use crate::phase::Phase;
use crate::race::Race;
use crate::raid::RaidControl;
use crate::rulesets::Ruleset;
use crate::sim_settings::SimSettings;
use crate::talent::{CharacterTalents, TalentFile};
use crate::target::{CreatureType, Target};

/// Highest character level.
pub const MAX_LEVEL: u32 = 60;
/// Highest target level (a raid boss).
pub const MAX_TARGET_LEVEL: u32 = 63;

fn default_level() -> u32 {
    MAX_LEVEL
}

/// One slot of the gear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquippedSetup {
    /// Item id (`data/items/*.yaml`).
    pub item: u32,
    /// Permanent enchant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enchant: Option<EnchantName>,
    /// Temporary enchants (stone or oil, Windfury Totem, poison), at most one of each group. A
    /// single name is accepted too, also as `temp_enchant`.
    #[serde(
        default,
        alias = "temp_enchant",
        deserialize_with = "one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub temp_enchants: Vec<EnchantName>,
}

/// A change of a setup's gear ([`CharacterSetup::change_equipment`]): the item to wear in
/// `slot`, or `None` to empty it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearChange {
    pub slot: EquipmentSlot,
    pub item: Option<u32>,
}

/// An enchant of the setup that a gear change took off: it does not apply to the slot's new
/// item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DroppedEnchant {
    pub slot: EquipmentSlot,
    pub enchant: EnchantName,
}

/// What [`CharacterSetup::change_equipment`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GearChanged {
    /// Every slot whose item now differs from the setup's, in slot order: the changes asked
    /// for and the slots they emptied (a two-hander empties the off hand). Applied to the
    /// original setup, in any order, they give the same gear.
    pub changes: Vec<GearChange>,
    pub dropped_enchants: Vec<DroppedEnchant>,
}

/// A list, or a single value as a list of one.
pub(crate) fn one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(value) => vec![value],
        OneOrMany::Many(values) => values,
    })
}

/// The target of the encounter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSetup {
    #[serde(default = "default_target_level")]
    pub level: u32,
    /// Base armor, before debuffs.
    #[serde(default = "default_target_armor")]
    pub armor: i32,
    #[serde(default = "default_creature_type")]
    pub creature_type: CreatureType,
    /// Damage a blocked attack loses.
    #[serde(default)]
    pub block_value: u32,
    /// Resistance to each magic school, before debuffs.
    #[serde(default, skip_serializing_if = "SchoolResistances::is_none")]
    pub resistances: SchoolResistances,
}

/// A target's resistance to each magic school (none by default, like most raid bosses).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SchoolResistances {
    pub arcane: i32,
    pub fire: i32,
    pub frost: i32,
    pub nature: i32,
    pub shadow: i32,
    pub holy: i32,
}

impl SchoolResistances {
    fn is_none(&self) -> bool {
        *self == SchoolResistances::default()
    }

    /// Each magic school with its resistance.
    pub fn by_school(&self) -> [(MagicSchool, i32); 6] {
        [
            (MagicSchool::Arcane, self.arcane),
            (MagicSchool::Fire, self.fire),
            (MagicSchool::Frost, self.frost),
            (MagicSchool::Nature, self.nature),
            (MagicSchool::Shadow, self.shadow),
            (MagicSchool::Holy, self.holy),
        ]
    }
}

fn default_target_level() -> u32 {
    MAX_TARGET_LEVEL
}

fn default_target_armor() -> i32 {
    Mechanics::BOSS_BASE_ARMOR
}

fn default_creature_type() -> CreatureType {
    CreatureType::Dragonkin
}

impl Default for TargetSetup {
    /// The [`Target::new`] raid boss.
    fn default() -> Self {
        TargetSetup {
            level: default_target_level(),
            armor: default_target_armor(),
            creature_type: default_creature_type(),
            block_value: 0,
            resistances: SchoolResistances::default(),
        }
    }
}

impl TargetSetup {
    pub fn target(&self) -> Target {
        let mut target = Target::new(self.level);
        target.set_base_armor(self.armor);
        target.set_creature_type(self.creature_type);
        target.set_block_value(self.block_value);
        for (school, resistance) in self.resistances.by_school() {
            target.set_resistance(school, resistance);
        }
        target
    }
}

/// A `data/characters/*.yaml` file. See the module documentation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterSetup {
    pub name: String,
    pub class: PlayerClass,
    pub race: Race,
    #[serde(default = "default_level")]
    pub level: u32,
    /// The content phase; absent = the sim settings'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    /// The encounter ruleset; absent = the sim settings'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruleset: Option<Ruleset>,
    /// Rotation name, within the class.
    pub rotation: String,
    #[serde(default)]
    pub tanking: bool,
    /// Tab name → talent name → rank.
    #[serde(default)]
    pub talents: BTreeMap<String, BTreeMap<String, u32>>,
    #[serde(default)]
    pub equipment: BTreeMap<EquipmentSlot, EquippedSetup>,
    #[serde(default)]
    pub buffs: Vec<String>,
    #[serde(default)]
    pub debuffs: Vec<String>,
    /// Items used in combat, by name (`consumables` of `data/external_buffs.yaml`).
    #[serde(default)]
    pub consumables: Vec<String>,
    #[serde(default)]
    pub target: TargetSetup,
    /// The file the setup was loaded from, for error messages.
    #[serde(skip)]
    pub path: Option<PathBuf>,
    /// Builds even when the character lacks a spell its rotation names as a prerequisite (the
    /// rotation then skips the lines casting it). Not in the file: for a character still being
    /// put together, such as one without talents yet.
    #[serde(skip)]
    pub allow_missing_prerequisites: bool,
}

/// One problem with a setup: the field it is about and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupIssue {
    /// Where in the file, e.g. `equipment.MAINHAND.enchant` or `talents.Fury.Cruelty`.
    pub context: String,
    pub message: String,
}

impl fmt::Display for SetupIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.message)
    }
}

/// Why a setup could not be loaded or built.
#[derive(Debug, thiserror::Error)]
pub enum CharacterSetupError {
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
    #[error("invalid include in {path}: {message}")]
    Include { path: PathBuf, message: String },
    #[error("character setup {setup} is invalid:{}", issues.iter().map(|i| format!("\n  {i}")).collect::<String>())]
    Invalid {
        /// The file, or the setup name when it was not loaded from a file.
        setup: String,
        issues: Vec<SetupIssue>,
    },
}

/// Collects the issues found while building.
#[derive(Default)]
struct Issues(Vec<SetupIssue>);

impl Issues {
    fn push(&mut self, context: impl Into<String>, message: impl Into<String>) {
        self.0.push(SetupIssue {
            context: context.into(),
            message: message.into(),
        });
    }
}

/// The key that includes other setup files.
const INCLUDE: &str = "include";

fn read(files: &dyn Files, path: &Path) -> Result<String, CharacterSetupError> {
    files.read(path).map_err(|source| CharacterSetupError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The top-level entries of a setup file in file order, repeated keys (`include`) kept.
struct Entries(Vec<(Value, Value)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;

        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = Entries;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a character setup mapping")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }

        deserializer.deserialize_map(EntriesVisitor)
    }
}

/// The setup file at `path` (with contents `text`) as a mapping, its includes merged in
/// place, and whether it had any. `stack` holds the files being included, to catch cycles.
fn resolve(
    files: &dyn Files,
    path: &Path,
    text: &str,
    stack: &mut Vec<PathBuf>,
) -> Result<(Mapping, bool), CharacterSetupError> {
    let include_error = |message: String| CharacterSetupError::Include {
        path: path.to_path_buf(),
        message,
    };
    let canonical = files.canonical(path);
    if stack.contains(&canonical) {
        return Err(include_error("the file includes itself".to_string()));
    }
    let Entries(entries) =
        serde_yaml::from_str(text).map_err(|source| CharacterSetupError::Yaml {
            path: path.to_path_buf(),
            source,
        })?;
    stack.push(canonical);
    let mut mapping = Mapping::new();
    let mut included = false;
    for (key, value) in entries {
        if key.as_str() != Some(INCLUDE) {
            merge_entry(&mut mapping, key, value);
            continue;
        }
        included = true;
        let includes = match value {
            Value::String(file) => vec![file],
            Value::Sequence(includes) => includes
                .into_iter()
                .map(|file| match file {
                    Value::String(file) => Ok(file),
                    _ => Err(include_error("expected a path".to_string())),
                })
                .collect::<Result<_, _>>()?,
            _ => {
                return Err(include_error(
                    "expected a path or a list of paths".to_string(),
                ));
            }
        };
        let dir = path.parent().unwrap_or(Path::new(""));
        for file in includes {
            let file = normalize(&dir.join(file));
            let text = files.read(&file).map_err(|error| {
                include_error(format!("cannot read {}: {error}", file.display()))
            })?;
            let (other, _) = resolve(files, &file, &text, stack)?;
            merge(&mut mapping, other);
        }
    }
    stack.pop();
    Ok((mapping, included))
}

/// `path` with its `.` and `..` components folded away where they follow a directory name,
/// e.g. `data/sweeps/../characters/a.yaml` → `data/characters/a.yaml`.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) =>
            {
                normalized.pop();
            }
            component => normalized.push(component),
        }
    }
    normalized
}

/// Merges the (partial) setup `changes` into the setup `target`: keys replace, `equipment`
/// replaces by slot, `null` removes.
pub(crate) fn merge(target: &mut Mapping, changes: Mapping) {
    for (key, value) in changes {
        merge_entry(target, key, value);
    }
}

fn merge_entry(target: &mut Mapping, key: Value, value: Value) {
    match value {
        Value::Mapping(changed) if key.as_str() == Some("equipment") => {
            let slots = target
                .entry(key)
                .or_insert_with(|| Value::Mapping(Mapping::new()));
            if !slots.is_mapping() {
                *slots = Value::Mapping(Mapping::new());
            }
            let slots = slots.as_mapping_mut().expect("a mapping");
            for (slot, item) in changed {
                if item.is_null() {
                    slots.remove(&slot);
                } else {
                    slots.insert(slot, item);
                }
            }
        }
        Value::Null => {
            target.remove(&key);
        }
        value => {
            target.insert(key, value);
        }
    }
}

fn slot_name(slot: EquipmentSlot) -> String {
    serde_yaml::to_string(&slot)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| format!("{slot:?}"))
}

impl CharacterSetup {
    /// Parses a setup file. Checking it against the data is left to
    /// [`build_raid`](Self::build_raid) / [`validate`](Self::validate).
    pub fn load(path: &Path) -> Result<Self, CharacterSetupError> {
        Self::load_from(&FsFiles, path)
    }

    /// [`Self::load`] from `files`: the includes are read from `files` too.
    pub fn load_from(files: &dyn Files, path: &Path) -> Result<Self, CharacterSetupError> {
        let text = read(files, path)?;
        let (mapping, included) = resolve(files, path, &text, &mut Vec::new())?;
        // Without includes the text is parsed directly, so errors keep their line numbers.
        let parsed = if included {
            serde_yaml::from_value(Value::Mapping(mapping))
        } else {
            serde_yaml::from_str(&text)
        };
        let mut setup: CharacterSetup = parsed.map_err(|source| CharacterSetupError::Yaml {
            path: path.to_path_buf(),
            source,
        })?;
        setup.path = Some(path.to_path_buf());
        Ok(setup)
    }

    /// The setup file at `path` as a YAML mapping, its includes resolved but not checked
    /// against the setup schema.
    pub fn load_mapping(path: &Path) -> Result<Mapping, CharacterSetupError> {
        Self::load_mapping_from(&FsFiles, path)
    }

    /// [`Self::load_mapping`] from `files`.
    pub fn load_mapping_from(
        files: &dyn Files,
        path: &Path,
    ) -> Result<Mapping, CharacterSetupError> {
        let text = read(files, path)?;
        Ok(resolve(files, path, &text, &mut Vec::new())?.0)
    }

    /// Parses every `*.yaml` setup of `dir`, sorted by file name.
    pub fn load_dir(dir: &Path) -> Result<Vec<Self>, CharacterSetupError> {
        Self::load_dir_from(&FsFiles, dir)
    }

    /// [`Self::load_dir`] from `files`.
    pub fn load_dir_from(files: &dyn Files, dir: &Path) -> Result<Vec<Self>, CharacterSetupError> {
        let paths = yaml_files(files, dir).map_err(|source| CharacterSetupError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        paths
            .iter()
            .map(|path| Self::load_from(files, path))
            .collect()
    }

    /// `settings` with the setup's phase and ruleset, when it names them.
    pub fn sim_settings(&self, settings: &SimSettings) -> SimSettings {
        let mut settings = settings.clone();
        if let Some(phase) = self.phase {
            settings.phase = phase;
        }
        if let Some(ruleset) = self.ruleset {
            settings.ruleset = ruleset;
        }
        settings
    }

    /// Changes the gear: the slots `changes` name are emptied, then each change is worn in
    /// order, as equipping in the game does it (a two-hander empties the off hand, an
    /// off-hand item takes a two-hander off, a unique item leaves the paired slot, ...), so a
    /// later change wins over an earlier one it conflicts with. A new item keeps the slot's
    /// enchant and temporary enchants that apply to it; the others are dropped. Items as of
    /// content phase `phase`.
    ///
    /// Only what equipping checks is checked here (the item exists in the phase and fits the
    /// slot, its unique-equipped group); the class, faction and proficiency are left to the
    /// build, as for the setup's own items.
    ///
    /// # Errors
    /// A change names an unknown item, or one that cannot be equipped in its slot.
    pub fn change_equipment(
        &mut self,
        data: &DataBundle,
        phase: Phase,
        changes: &[GearChange],
    ) -> Result<GearChanged, String> {
        let mut worn = Equipment::new(
            Arc::clone(&data.equipment),
            phase,
            self.race.faction(),
            self.class,
        );
        for (&slot, equipped) in &self.equipment {
            if worn.equip(slot, equipped.item).is_ok() {
                wear_enchants(&mut worn, slot, equipped);
            }
        }
        let named = |slot| changes.iter().any(|change| change.slot == slot);
        // The setup's items that do not equip (the build reports them) stay as written.
        let kept: Vec<EquipmentSlot> = self
            .equipment
            .iter()
            .filter(|&(&slot, equipped)| !named(slot) && worn.item_id(slot) != Some(equipped.item))
            .map(|(&slot, _)| slot)
            .collect();
        for change in changes {
            worn.unequip(change.slot);
        }
        for change in changes {
            let Some(item) = change.item else {
                worn.unequip(change.slot);
                continue;
            };
            worn.equip(change.slot, item)
                .map_err(|error| format!("equipment.{}: {error}", slot_name(change.slot)))?;
            if let Some(equipped) = self.equipment.get(&change.slot) {
                wear_enchants(&mut worn, change.slot, equipped);
            }
        }

        let mut equipment = BTreeMap::new();
        let mut dropped_enchants = Vec::new();
        for slot in EquipmentSlot::ALL {
            let setup = self.equipment.get(&slot);
            if kept.contains(&slot) {
                equipment.extend(setup.map(|equipped| (slot, equipped.clone())));
                continue;
            }
            let Some(item) = worn.item_id(slot) else {
                continue;
            };
            // A new item has the setup's enchants of the slot that apply to it, in the
            // setup's order.
            let equipped = match setup {
                Some(equipped) if equipped.item == item => equipped.clone(),
                _ => EquippedSetup {
                    item,
                    enchant: worn.enchant(slot),
                    temp_enchants: setup
                        .iter()
                        .flat_map(|equipped| &equipped.temp_enchants)
                        .copied()
                        .filter(|enchant| worn.temp_enchants(slot).contains(enchant))
                        .collect(),
                },
            };
            if let Some(setup) = setup {
                let had = setup.enchant.iter().chain(&setup.temp_enchants);
                let has = |enchant| {
                    equipped.enchant == Some(enchant) || equipped.temp_enchants.contains(&enchant)
                };
                dropped_enchants.extend(
                    had.filter(|&&enchant| !has(enchant))
                        .map(|&enchant| DroppedEnchant { slot, enchant }),
                );
            }
            equipment.insert(slot, equipped);
        }
        let item_in = |gear: &BTreeMap<EquipmentSlot, EquippedSetup>, slot| {
            gear.get(&slot).map(|equipped| equipped.item)
        };
        let changes = EquipmentSlot::ALL
            .into_iter()
            .filter(|&slot| item_in(&self.equipment, slot) != item_in(&equipment, slot))
            .map(|slot| GearChange {
                slot,
                item: item_in(&equipment, slot),
            })
            .collect();
        self.equipment = equipment;
        Ok(GearChanged {
            changes,
            dropped_enchants,
        })
    }

    /// Builds the setup against the data (with default sim settings) and reports every
    /// problem found.
    pub fn validate(&self, data: &DataBundle) -> Result<(), CharacterSetupError> {
        self.build_raid(data, &SimSettings::default()).map(|_| ())
    }

    /// A raid of this one character facing the setup's target, built from `data` under
    /// `settings` (whose phase and ruleset the setup overrides, see
    /// [`sim_settings`](Self::sim_settings)). Fails with every problem found.
    pub fn build_raid(
        &self,
        data: &DataBundle,
        settings: &SimSettings,
    ) -> Result<RaidControl, CharacterSetupError> {
        let mut raid = RaidControl::new(self.target.target());
        self.add_to_raid(&mut raid, data, settings)?;
        Ok(raid)
    }

    /// Adds the character to `raid` at its first free place (the raid's target is left as it
    /// is). Fails with every problem found; the raid may then hold a partly set up character.
    pub fn add_to_raid(
        &self,
        raid: &mut RaidControl,
        data: &DataBundle,
        settings: &SimSettings,
    ) -> Result<CharId, CharacterSetupError> {
        self.add_to_raid_place(raid, None, data, settings)
    }

    /// [`add_to_raid`](Self::add_to_raid) at `party` (0-based) `member`, which must be free.
    pub fn add_to_raid_at(
        &self,
        raid: &mut RaidControl,
        party: u8,
        member: u8,
        data: &DataBundle,
        settings: &SimSettings,
    ) -> Result<CharId, CharacterSetupError> {
        self.add_to_raid_place(raid, Some((party, member)), data, settings)
    }

    fn add_to_raid_place(
        &self,
        raid: &mut RaidControl,
        place: Option<(u8, u8)>,
        data: &DataBundle,
        settings: &SimSettings,
    ) -> Result<CharId, CharacterSetupError> {
        let settings = self.sim_settings(settings);
        let mut issues = Issues::default();

        // What `Character::new` needs; nothing can be built without it.
        let class = match data.classes.get(self.class) {
            Ok(class) => Some(Arc::clone(class)),
            Err(error) => {
                issues.push("class", error.to_string());
                None
            }
        };
        if let Some(class) = &class
            && !class.race_available(self.race)
        {
            issues.push(
                "race",
                format!(
                    "{} is not available to the {:?}",
                    self.race.name(),
                    self.class
                ),
            );
        }
        let talent_file = data.talents.get(self.class);
        if talent_file.is_none() && !self.talents.is_empty() {
            issues.push(
                "talents",
                format!("no talent tree for the {:?}", self.class),
            );
        }
        let rotation = data.rotations.get(self.class, &self.rotation);
        if rotation.is_none() {
            issues.push(
                "rotation",
                format!("no rotation {:?} for the {:?}", self.rotation, self.class),
            );
        }
        if !(1..=MAX_LEVEL).contains(&self.level) {
            issues.push("level", format!("{} is not in 1..={MAX_LEVEL}", self.level));
        }
        if !(1..=MAX_TARGET_LEVEL).contains(&self.target.level) {
            issues.push(
                "target.level",
                format!("{} is not in 1..={MAX_TARGET_LEVEL}", self.target.level),
            );
        }
        let talent_setup = talent_file.map(|file| self.talent_setup(file, &mut issues));
        let (Some(class), false) = (class, issues.0.iter().any(|i| i.context == "race")) else {
            return Err(self.invalid(issues));
        };

        let race = data.races.get(self.race);
        let target_level = raid.target().level();
        let build = |id, party, member| {
            Character::new(
                id,
                Arc::clone(&class),
                race,
                Arc::clone(&data.equipment),
                settings.phase,
                settings.sim_params(),
                target_level,
                party,
                member,
            )
        };
        let id = match place {
            Some((party, member)) => raid.add_character_at(party, member, build),
            None => raid.add_character(build),
        }
        .map_err(|error| {
            let mut issues = Issues::default();
            issues.push("raid", error.to_string());
            self.invalid(issues)
        })?;
        let character = raid.character_mut(id);
        character.set_clvl(self.level);
        character.set_tanking(self.tanking);

        let db = &data.spells;
        raid.with_character(id, |ctx| {
            if let (Some(file), Some(setup)) = (talent_file, &talent_setup) {
                ctx.set_talents(CharacterTalents::new(Arc::clone(file)));
                for (node, reached) in ctx.spend_talent_points(setup) {
                    let spec = file.talent(node).expect("the node is in the tree");
                    let tab = file.tab(spec.tab).map_or("?", |tab| tab.name.as_str());
                    let wanted = setup.iter().find(|(n, _)| *n == node).map_or(0, |s| s.1);
                    issues.push(
                        format!("talents.{tab}.{}", spec.name),
                        format!(
                            "only rank {reached} of {wanted} could be spent (points, tier or \
                             prerequisite)"
                        ),
                    );
                }
            }
            ctx.learn_all(db);

            for (&slot, equipped) in &self.equipment {
                let context = format!("equipment.{}", slot_name(slot));
                if let Err(message) = check_item(
                    &class,
                    self.race.faction(),
                    data,
                    &settings,
                    slot,
                    equipped.item,
                ) {
                    issues.push(&context, message);
                    continue;
                }
                if let Err(error) = ctx.equip(db, slot, equipped.item) {
                    issues.push(&context, error.to_string());
                    continue;
                }
                let enchants = equipped.enchant.iter().map(|&enchant| (enchant, false));
                let temp_enchants = equipped
                    .temp_enchants
                    .iter()
                    .map(|&enchant| (enchant, true));
                let mut valid_temp_enchants = Vec::new();
                for (enchant, temporary) in enchants.chain(temp_enchants) {
                    let context = format!(
                        "{context}.{}",
                        if temporary {
                            "temp_enchants"
                        } else {
                            "enchant"
                        }
                    );
                    if !class.enchants_for_slot(slot, temporary).contains(&enchant) {
                        issues.push(
                            context,
                            format!(
                                "{enchant:?} is not a{} enchant the {:?} uses on {}",
                                if temporary { " temporary" } else { "n" },
                                self.class,
                                slot_name(slot)
                            ),
                        );
                        continue;
                    }
                    let result = if temporary {
                        // Each on its own first, so every problem is reported; two of one
                        // group are caught below.
                        ctx.set_temp_enchants(db, slot, &[enchant])
                            .map(|()| valid_temp_enchants.push(enchant))
                    } else {
                        ctx.set_enchant(db, slot, Some(enchant))
                    };
                    if let Err(error) = result {
                        issues.push(context, error.to_string());
                    }
                }
                if let Err(error) = ctx.set_temp_enchants(db, slot, &valid_temp_enchants) {
                    issues.push(format!("{context}.temp_enchants"), error.to_string());
                }
            }
            // Equipping may push an earlier item out (a two-hander and an off-hand, a unique
            // item in both slots of a pair, mutually exclusive items).
            for (&slot, equipped) in &self.equipment {
                let worn = ctx.character.equipment().item_id(slot);
                let item_ok = !issues
                    .0
                    .iter()
                    .any(|i| i.context == format!("equipment.{}", slot_name(slot)));
                if item_ok && worn != Some(equipped.item) {
                    issues.push(
                        format!("equipment.{}", slot_name(slot)),
                        format!(
                            "item {} was taken off by another item of the setup",
                            equipped.item
                        ),
                    );
                }
            }

            ctx.add_external_buffs(&data.external_buffs, db);
            let mut mutexes: BTreeMap<&str, &str> = BTreeMap::new();
            for (list, debuffs) in [(&self.buffs, false), (&self.debuffs, true)] {
                let field = if debuffs { "debuffs" } else { "buffs" };
                for name in list {
                    let context = format!("{field}.{name}");
                    let Some((spec, is_debuff)) = data.external_buffs.get(name) else {
                        issues.push(context, "not in data/external_buffs.yaml");
                        continue;
                    };
                    if is_debuff != debuffs {
                        issues.push(
                            context,
                            format!(
                                "is a {}; list it under {}",
                                if is_debuff { "debuff" } else { "buff" },
                                if is_debuff { "debuffs" } else { "buffs" }
                            ),
                        );
                        continue;
                    }
                    if let Some(mutex) = spec.mutex.as_deref()
                        && let Some(other) = mutexes.insert(mutex, name)
                    {
                        issues.push(context, format!("excludes {other:?} (both are {mutex:?})"));
                        continue;
                    }
                    if let Err(error) = ctx.set_external_buff_selected(name, true) {
                        issues.push(context, error.to_string());
                    }
                }
            }

            let mut consumables = Vec::new();
            for name in &self.consumables {
                let context = format!("consumables.{name}");
                match data.external_buffs.consumable(name) {
                    None => issues.push(context, "not in data/external_buffs.yaml `consumables`"),
                    Some(spec) if !spec.valid_for_class(self.class) => issues.push(
                        context,
                        format!("is not a consumable the {:?} uses", self.class),
                    ),
                    Some(spec) if consumables.contains(spec) => {
                        issues.push(context, "is listed twice");
                    }
                    Some(spec) => consumables.push(spec.clone()),
                }
            }
            ctx.set_consumables(db, consumables);

            ctx.sync_ruleset_spells(db);
            if let Some(rotation) = rotation {
                ctx.set_rotation(Arc::clone(rotation));
                let linked = ctx.character.rotation().expect("the rotation was just set");
                let missing = if self.allow_missing_prerequisites {
                    &[][..]
                } else {
                    linked.missing_prerequisites()
                };
                for (spell, reason) in missing {
                    issues.push(
                        "rotation",
                        format!(
                            "{:?} is not valid for this character: prerequisite {spell:?}: {reason}",
                            self.rotation
                        ),
                    );
                }
            }
        });

        if issues.0.is_empty() {
            Ok(id)
        } else {
            Err(self.invalid(issues))
        }
    }

    /// The `(node, rank)` list to spend, tier by tier so tier and prerequisite rules are met
    /// in order. Unknown tabs and talents and ranks above the maximum are reported.
    fn talent_setup(&self, file: &TalentFile, issues: &mut Issues) -> Vec<(u32, u32)> {
        let mut setup = Vec::new();
        for (tab_name, talents) in &self.talents {
            let Some(tab) = file.tabs.iter().find(|tab| &tab.name == tab_name) else {
                let tabs: Vec<&str> = file.tabs.iter().map(|tab| tab.name.as_str()).collect();
                issues.push(
                    format!("talents.{tab_name}"),
                    format!("no such tab (the tabs are {})", tabs.join(", ")),
                );
                continue;
            };
            for (name, &rank) in talents {
                let context = format!("talents.{tab_name}.{name}");
                let Some(spec) = file.talent_by_name(name, Some(tab.skill_line)) else {
                    issues.push(context, format!("no such talent in {tab_name}"));
                    continue;
                };
                if rank > spec.max_ranks {
                    issues.push(
                        context,
                        format!("rank {rank} is above the maximum {}", spec.max_ranks),
                    );
                    continue;
                }
                if rank > 0 {
                    setup.push((spec, rank));
                }
            }
        }
        setup.sort_by_key(|(spec, _)| (spec.tier, spec.tab, spec.column));
        setup
            .into_iter()
            .map(|(spec, rank)| (spec.node, rank))
            .collect()
    }

    fn invalid(&self, issues: Issues) -> CharacterSetupError {
        CharacterSetupError::Invalid {
            setup: self
                .path
                .as_ref()
                .map_or_else(|| format!("{:?}", self.name), |p| p.display().to_string()),
            issues: issues.0,
        }
    }
}

/// Puts the enchants of `equipped` that apply on the item worn in `slot`.
fn wear_enchants(worn: &mut Equipment, slot: EquipmentSlot, equipped: &EquippedSetup) {
    if equipped.enchant.is_some() {
        let _ = worn.set_enchant(slot, equipped.enchant);
    }
    let applying: Vec<EnchantName> = equipped
        .temp_enchants
        .iter()
        .copied()
        .filter(|&enchant| worn.set_temp_enchants(slot, &[enchant]).is_ok())
        .collect();
    let _ = worn.set_temp_enchants(slot, &applying);
}

/// What `Equipment::equip` does not check: the item exists in the phase, its faction and
/// class restrictions, the armor type and weapon proficiency.
fn check_item(
    class: &ClassSpec,
    faction: Faction,
    data: &DataBundle,
    settings: &SimSettings,
    slot: EquipmentSlot,
    item_id: u32,
) -> Result<(), String> {
    let Some(any) = data.equipment.item(item_id) else {
        return Err(format!("no item {item_id}"));
    };
    let Some(item) = data.equipment.get_item(item_id, settings.phase) else {
        return Err(format!(
            "{} ({item_id}) is not available in phase {} (from phase {})",
            any.name(),
            settings.phase as u8,
            any.phase() as u8
        ));
    };
    if !item.available_for_faction(faction) {
        return Err(format!(
            "{} ({item_id}) is not available to the {}",
            item.name(),
            faction.name()
        ));
    }
    if !item.available_for_class(class.class) {
        return Err(format!(
            "{} ({item_id}) is not usable by the {:?}",
            item.name(),
            class.class
        ));
    }
    if let Some(armor_type) = item.item_type().armor_type()
        && !class.can_wear(armor_type)
    {
        return Err(format!(
            "the {:?} cannot wear {} ({item_id}, {armor_type:?})",
            class.class,
            item.name()
        ));
    }
    if let Some(weapon_type) = item.weapon_type() {
        let proficiency_slot = match slot {
            EquipmentSlot::Mainhand | EquipmentSlot::Offhand | EquipmentSlot::Ranged => Some(slot),
            _ => None,
        };
        if proficiency_slot.is_some_and(|slot| !class.can_wield(slot, weapon_type)) {
            return Err(format!(
                "the {:?} cannot wield {} ({item_id}, {weapon_type:?}) in {}",
                class.class,
                item.name(),
                slot_name(slot)
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
