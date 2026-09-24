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
//! rotation: DW Fury High Rage  # a rotation of data/rotations/<class>/, by name
//! tanking: false               # default false; a tank is attacked by the target
//! talents:                     # tab name → talent name → rank; default none
//!   Fury:
//!     Cruelty: 5
//! equipment:                   # slot → item id and enchants; default nothing
//!   MAINHAND: { item: 18832, enchant: Crusader, temp_enchant: WindfuryTotem }
//!   HEAD: { item: 12640 }
//! buffs: [Battle Squawk]       # data/external_buffs.yaml `buffs`, by name
//! debuffs: [Sunder Armor]      # data/external_buffs.yaml `debuffs`, by name
//! target:                      # default: a level 63 Dragonkin raid boss with 3750 armor
//!   level: 63
//!   armor: 3731
//!   creature_type: Dragonkin
//! ```
//!
//! Loading only parses the file; everything that needs the data (does the talent exist, may
//! the class use the enchant, does the item fit the slot in that phase, ...) is checked while
//! building, and every problem found is reported together, each with the field it came from.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::character::{Character, ClassSpec};
use crate::data_bundle::DataBundle;
use crate::enchant::EnchantName;
use crate::faction::{Faction, PlayerClass};
use crate::ids::CharId;
use crate::item::EquipmentSlot;
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
    /// Temporary enchant (stone, oil, Windfury Totem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_enchant: Option<EnchantName>,
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
        }
    }
}

impl TargetSetup {
    pub fn target(&self) -> Target {
        let mut target = Target::new(self.level);
        target.set_base_armor(self.armor);
        target.set_creature_type(self.creature_type);
        target.set_block_value(self.block_value);
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
    #[serde(default)]
    pub target: TargetSetup,
    /// The file the setup was loaded from, for error messages.
    #[serde(skip)]
    pub path: Option<PathBuf>,
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

fn slot_name(slot: EquipmentSlot) -> String {
    serde_yaml::to_string(&slot)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| format!("{slot:?}"))
}

impl CharacterSetup {
    /// Parses a setup file. Checking it against the data is left to
    /// [`build_raid`](Self::build_raid) / [`validate`](Self::validate).
    pub fn load(path: &Path) -> Result<Self, CharacterSetupError> {
        let text = fs::read_to_string(path).map_err(|source| CharacterSetupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut setup: CharacterSetup =
            serde_yaml::from_str(&text).map_err(|source| CharacterSetupError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        setup.path = Some(path.to_path_buf());
        Ok(setup)
    }

    /// Parses every `*.yaml` setup of `dir`, sorted by file name.
    pub fn load_dir(dir: &Path) -> Result<Vec<Self>, CharacterSetupError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|source| CharacterSetupError::Io {
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
        if let Some(class) = &class {
            if !class.race_available(self.race) {
                issues.push(
                    "race",
                    format!(
                        "{} is not available to the {:?}",
                        self.race.name(),
                        self.class
                    ),
                );
            }
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
        let id = raid
            .add_character(|id, party, member| {
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
            })
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
                for (enchant, temporary, field) in [
                    (equipped.enchant, false, "enchant"),
                    (equipped.temp_enchant, true, "temp_enchant"),
                ] {
                    let Some(enchant) = enchant else { continue };
                    let context = format!("{context}.{field}");
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
                        ctx.set_temp_enchant(db, slot, Some(enchant))
                    } else {
                        ctx.set_enchant(db, slot, Some(enchant))
                    };
                    if let Err(error) = result {
                        issues.push(context, error.to_string());
                    }
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
                    if let Some(mutex) = spec.mutex.as_deref() {
                        if let Some(other) = mutexes.insert(mutex, name) {
                            issues
                                .push(context, format!("excludes {other:?} (both are {mutex:?})"));
                            continue;
                        }
                    }
                    if let Err(error) = ctx.set_external_buff_selected(name, true) {
                        issues.push(context, error.to_string());
                    }
                }
            }

            ctx.sync_ruleset_spells(db);
            if let Some(rotation) = rotation {
                ctx.set_rotation(Arc::clone(rotation));
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

/// What `Equipment::equip` does not check: the item exists in the phase, its faction and
/// class restrictions, and weapon proficiency.
fn check_item(
    class: &ClassSpec,
    faction: Faction,
    data: &DataBundle,
    settings: &SimSettings,
    slot: EquipmentSlot,
    item_id: u32,
) -> Result<(), String> {
    let Some(any) = data.equipment.get_item_any_phase(item_id) else {
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
