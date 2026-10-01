//! External buffs and debuffs: what other players and consumables put on the character or the
//! target. Port of `Spells/ExternalBuff.*` and `Class/Common/GeneralBuffs.*`.
//!
//! The C++ code carried the numbers in a `switch` per buff name. Here an external buff *is* a
//! spell from the tables: the hand-written registry `data/external_buffs.yaml`
//! ([`ExternalBuffDb`]) names the aura spell each entry applies (Greater Blessing of Kings
//! 25898, Sunder Armor 11597, …), and its numbers come from the spell record in
//! `data/spells/externals.yaml`, exported from those ids. The registry adds what the tables do
//! not say: the faction the buff exists for, the classes it is offered to, the mutually
//! exclusive groups (one food, one strength elixir, …) and how many stacks a stacking debuff
//! is kept at.
//!
//! [`GeneralBuffs`] is one character's view of the registry: the entries it is offered, each
//! with the [`Buff`](crate::buff::Buff) the character's spell registry holds for it and whether
//! the user selected it. Applying and cancelling needs the world and is done by the character
//! context (`CharacterContext::toggle_external_buff` and friends), which keeps the selected
//! ones applied across iterations as the C++ did.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::faction::{Faction, PlayerClass};
use crate::ids::BuffId;
use crate::spell::record::SpellDb;

/// One entry of `data/external_buffs.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalBuffSpec {
    /// Display name, unique across buffs and debuffs ("Greater Blessing of Kings").
    pub name: String,
    /// The aura spell the entry applies (`SpellName.ID`), in the spell db.
    pub spell: u32,
    /// The faction the buff exists for; absent = both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<Faction>,
    /// The classes the buff is offered to; empty = every class.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<PlayerClass>,
    /// Entries sharing a mutex key exclude each other (selecting one cancels the others).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutex: Option<String>,
    /// How many stacks are applied; absent = the spell's `max_stacks` (at least 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacks: Option<u32>,
    /// A buff another raid member's class provides (Blessing of Kings, Trueshot Aura, ...),
    /// left out when the character is simulated in a raid, which provides it or not. Debuffs
    /// are always provided by the raid ([`ExternalBuffSpec::provided_by_raid`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raid: bool,
}

impl ExternalBuffSpec {
    pub fn new(name: &str, spell: u32) -> Self {
        Self {
            name: name.to_string(),
            spell,
            faction: None,
            classes: Vec::new(),
            mutex: None,
            stacks: None,
            raid: false,
        }
    }

    /// Whether the raid, not the character, provides the entry: a buff marked `raid` or any
    /// target debuff. A raid simulation leaves these out.
    pub fn provided_by_raid(&self, debuff: bool) -> bool {
        debuff || self.raid
    }

    /// Port of `ExternalBuff::valid_for_faction`.
    pub fn valid_for_faction(&self, faction: Faction) -> bool {
        self.faction.is_none_or(|f| f == faction)
    }

    /// Whether the entry is offered to `class`.
    pub fn valid_for_class(&self, class: PlayerClass) -> bool {
        self.classes.is_empty() || self.classes.contains(&class)
    }

    /// The stack count to apply, given the spell's `max_stacks`.
    pub fn applied_stacks(&self, max_stacks: u32) -> u32 {
        self.stacks.unwrap_or(max_stacks).max(1)
    }
}

/// One `consumables` entry of `data/external_buffs.yaml`: an item the character uses in
/// combat from its bags (Thistle Tea). Its use effects (spell, cooldown, shared category
/// cooldown) are the item's `ItemEffect` rows, which `export-spells --externals` writes into
/// `data/spells/externals.yaml` with the spells they cast; the rotation casts it by `name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumableSpec {
    /// Display name, unique across the registry; the name the rotation casts it by.
    pub name: String,
    /// The item (`Item.ID`).
    pub item: u32,
    /// The classes the consumable is offered to; empty = every class.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<PlayerClass>,
}

impl ConsumableSpec {
    /// Whether the entry is offered to `class`.
    pub fn valid_for_class(&self, class: PlayerClass) -> bool {
        self.classes.is_empty() || self.classes.contains(&class)
    }
}

/// The file layout of `data/external_buffs.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalBuffFile {
    /// Buffs on the character (raid buffs, marked `raid`, and consumables).
    #[serde(default)]
    pub buffs: Vec<ExternalBuffSpec>,
    /// Debuffs on the target kept up by other players.
    #[serde(default)]
    pub debuffs: Vec<ExternalBuffSpec>,
    /// Items the character uses in combat (Thistle Tea).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumables: Vec<ConsumableSpec>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExternalBuffError {
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
    #[error("external buff {0:?} is listed twice")]
    DuplicateName(String),
    #[error("external buff {name:?}: spell {spell} is not in the spell db")]
    UnknownSpell { name: String, spell: u32 },
    #[error("external buff {name:?}: spell {spell} applies no auras")]
    NoAuras { name: String, spell: u32 },
    #[error("external buff {name:?}: stacks must be at least 1")]
    ZeroStacks { name: String },
    #[error("consumable {name:?}: item {item} has no use effect in the spell db")]
    UnknownConsumable { name: String, item: u32 },
}

/// The registry of external buffs and debuffs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalBuffDb {
    buffs: Vec<ExternalBuffSpec>,
    debuffs: Vec<ExternalBuffSpec>,
    consumables: Vec<ConsumableSpec>,
}

impl ExternalBuffDb {
    /// Loads `data/external_buffs.yaml` (structure only; [`ExternalBuffDb::validate`] checks
    /// it against a spell db).
    pub fn load(path: &Path) -> Result<Self, ExternalBuffError> {
        let text = fs::read_to_string(path).map_err(|source| ExternalBuffError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: ExternalBuffFile =
            serde_yaml::from_str(&text).map_err(|source| ExternalBuffError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        Self::from_file(file)
    }

    /// Builds the registry from its file, checking that names are unique.
    pub fn from_file(file: ExternalBuffFile) -> Result<Self, ExternalBuffError> {
        let mut names = BTreeSet::new();
        for spec in file.buffs.iter().chain(&file.debuffs) {
            if !names.insert(spec.name.as_str()) {
                return Err(ExternalBuffError::DuplicateName(spec.name.clone()));
            }
            if spec.stacks == Some(0) {
                return Err(ExternalBuffError::ZeroStacks {
                    name: spec.name.clone(),
                });
            }
        }
        for spec in &file.consumables {
            if !names.insert(spec.name.as_str()) {
                return Err(ExternalBuffError::DuplicateName(spec.name.clone()));
            }
        }
        Ok(Self {
            buffs: file.buffs,
            debuffs: file.debuffs,
            consumables: file.consumables,
        })
    }

    /// Checks that every entry names a spell of `db` that applies auras, and every consumable
    /// an item of `db` with a use effect.
    pub fn validate(&self, db: &SpellDb) -> Result<(), ExternalBuffError> {
        for spec in &self.consumables {
            let usable = db
                .consumable_item(spec.item)
                .is_some_and(|item| item.uses().next().is_some());
            if !usable {
                return Err(ExternalBuffError::UnknownConsumable {
                    name: spec.name.clone(),
                    item: spec.item,
                });
            }
        }
        for spec in self.entries() {
            let record = db
                .get(spec.spell)
                .ok_or_else(|| ExternalBuffError::UnknownSpell {
                    name: spec.name.clone(),
                    spell: spec.spell,
                })?;
            if !record.applies_aura() {
                return Err(ExternalBuffError::NoAuras {
                    name: spec.name.clone(),
                    spell: spec.spell,
                });
            }
        }
        Ok(())
    }

    pub fn buffs(&self) -> &[ExternalBuffSpec] {
        &self.buffs
    }

    pub fn debuffs(&self) -> &[ExternalBuffSpec] {
        &self.debuffs
    }

    pub fn consumables(&self) -> &[ConsumableSpec] {
        &self.consumables
    }

    /// The consumable called `name`.
    pub fn consumable(&self, name: &str) -> Option<&ConsumableSpec> {
        self.consumables.iter().find(|s| s.name == name)
    }

    /// The items of the consumables, sorted and unique (the exporter's item seeds).
    pub fn consumable_item_ids(&self) -> BTreeSet<u32> {
        self.consumables.iter().map(|s| s.item).collect()
    }

    /// Buffs first, then debuffs.
    pub fn entries(&self) -> impl Iterator<Item = &ExternalBuffSpec> {
        self.buffs.iter().chain(&self.debuffs)
    }

    /// The entry called `name`, with whether it is a debuff.
    pub fn get(&self, name: &str) -> Option<(&ExternalBuffSpec, bool)> {
        self.buffs
            .iter()
            .find(|s| s.name == name)
            .map(|s| (s, false))
            .or_else(|| {
                self.debuffs
                    .iter()
                    .find(|s| s.name == name)
                    .map(|s| (s, true))
            })
    }

    /// Every spell id the registry refers to, sorted and unique (the exporter's seeds).
    pub fn spell_ids(&self) -> BTreeSet<u32> {
        self.entries().map(|s| s.spell).collect()
    }

    /// The entries offered to `class` (buffs and debuffs; the faction is checked when a buff
    /// is selected, as the character's faction can change with its race).
    pub fn offered_to(
        &self,
        class: PlayerClass,
    ) -> impl Iterator<Item = (&ExternalBuffSpec, bool)> {
        self.buffs
            .iter()
            .map(|s| (s, false))
            .chain(self.debuffs.iter().map(|s| (s, true)))
            .filter(move |(s, _)| s.valid_for_class(class))
    }
}

/// One external buff a character is offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBuffEntry {
    pub spec: ExternalBuffSpec,
    /// Whether the entry is a target debuff.
    pub debuff: bool,
    /// The buff in the character's spell registry.
    pub buff: BuffId,
    /// How many stacks selecting it applies.
    pub stacks: u32,
    /// Whether the user selected it (the C++ `QPair<bool, ExternalBuff*>::first`). A selected
    /// buff is active unless the character's faction rules it out.
    pub selected: bool,
}

/// A character's external buffs. Port of `GeneralBuffs` (one setup).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneralBuffs {
    entries: Vec<ExternalBuffEntry>,
}

impl GeneralBuffs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an offered entry; its buff was added to the character's spell registry.
    ///
    /// # Panics
    /// Panics if an entry with the same name exists.
    pub fn add(&mut self, spec: ExternalBuffSpec, debuff: bool, buff: BuffId, stacks: u32) {
        assert!(
            self.get(&spec.name).is_none(),
            "external buff {:?} added twice",
            spec.name
        );
        self.entries.push(ExternalBuffEntry {
            spec,
            debuff,
            buff,
            stacks,
            selected: false,
        });
    }

    pub fn entries(&self) -> &[ExternalBuffEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&ExternalBuffEntry> {
        self.entries.iter().find(|e| e.spec.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ExternalBuffEntry> {
        self.entries.iter_mut().find(|e| e.spec.name == name)
    }

    /// The entries offered for `faction` (port of `GeneralBuffs::get_external_buffs`, which
    /// filtered by the character's faction; debuffs are never faction-bound in the C++ data).
    pub fn offered(&self, faction: Faction) -> impl Iterator<Item = &ExternalBuffEntry> {
        self.entries
            .iter()
            .filter(move |e| e.spec.valid_for_faction(faction))
    }

    /// Whether the user selected `name`. Port of `buff_active` / `debuff_active`.
    pub fn is_selected(&self, name: &str) -> bool {
        self.get(name).is_some_and(|e| e.selected)
    }

    /// The names of the selected buffs, in registry order.
    pub fn selected_buffs(&self) -> Vec<&str> {
        self.selected(false)
    }

    /// The names of the selected debuffs, in registry order.
    pub fn selected_debuffs(&self) -> Vec<&str> {
        self.selected(true)
    }

    fn selected(&self, debuff: bool) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.debuff == debuff && e.selected)
            .map(|e| e.spec.name.as_str())
            .collect()
    }

    /// The other entries sharing `name`'s mutex key. Port of `deactivate_mutex_buffs`'s lookup.
    pub fn mutex_peers(&self, name: &str) -> Vec<&ExternalBuffEntry> {
        let Some(key) = self.get(name).and_then(|e| e.spec.mutex.as_deref()) else {
            return Vec::new();
        };
        self.entries
            .iter()
            .filter(|e| e.spec.name != name && e.spec.mutex.as_deref() == Some(key))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spell::dbc::{AuraType, ImplicitTarget, SpellEffectName};
    use crate::spell::record::{EffectRecord, SpellRecord};

    const FILE: &str = r#"
buffs:
  - name: Greater Blessing of Kings
    spell: 25898
    faction: ALLIANCE
  - name: Strength of Earth Totem
    spell: 25362
    faction: HORDE
  - name: Battle Shout
    spell: 25289
    classes: [ROGUE, HUNTER]
  - name: Juju Power
    spell: 16323
    mutex: strength
  - name: Elixir of Giants
    spell: 11405
    mutex: strength
  - name: Grilled Squid
    spell: 18192
    mutex: food
debuffs:
  - name: Sunder Armor
    spell: 11971
  - name: Annihilator
    spell: 16928
    stacks: 2
"#;

    fn aura_record(id: u32, name: &str, on_enemy: bool) -> SpellRecord {
        let mut record = SpellRecord::new(id, name);
        let mut effect = EffectRecord::new(0, SpellEffectName::ApplyAura);
        effect.aura = AuraType::ModStat;
        effect.implicit_target = [
            if on_enemy {
                ImplicitTarget::UnitTargetEnemy
            } else {
                ImplicitTarget::UnitCaster
            },
            ImplicitTarget::None,
        ];
        record.effects.push(effect);
        record
    }

    fn db() -> SpellDb {
        let mut db = SpellDb::new();
        for (id, name) in [
            (25898, "Greater Blessing of Kings"),
            (25362, "Strength of Earth"),
            (25289, "Battle Shout"),
            (16323, "Juju Power"),
            (11405, "Greater Strength"),
            (18192, "Increased Agility"),
        ] {
            db.add(None, aura_record(id, name, false)).unwrap();
        }
        db.add(None, aura_record(11971, "Sunder Armor", true))
            .unwrap();
        db.add(None, aura_record(16928, "Armor Shatter", true))
            .unwrap();
        db
    }

    fn registry() -> ExternalBuffDb {
        ExternalBuffDb::from_file(serde_yaml::from_str(FILE).unwrap()).unwrap()
    }

    #[test]
    fn file_parses_and_validates() {
        let registry = registry();
        registry.validate(&db()).unwrap();
        assert_eq!(registry.buffs().len(), 6);
        assert_eq!(registry.debuffs().len(), 2);
        let (kings, debuff) = registry.get("Greater Blessing of Kings").unwrap();
        assert!(!debuff);
        assert_eq!(kings.faction, Some(Faction::Alliance));
        let (sunder, debuff) = registry.get("Sunder Armor").unwrap();
        assert!(debuff);
        assert_eq!(sunder.applied_stacks(5), 5);
        let (annihilator, _) = registry.get("Annihilator").unwrap();
        assert_eq!(annihilator.applied_stacks(3), 2);
        assert_eq!(kings.applied_stacks(0), 1);
        assert!(registry.get("Flask of the Titans").is_none());
        assert_eq!(
            registry.spell_ids(),
            BTreeSet::from([25898, 25362, 25289, 16323, 11405, 18192, 11971, 16928])
        );
    }

    /// Consumables are looked up by name, share the name space and need their item's use
    /// effect in the spell db.
    #[test]
    fn consumables_need_an_item_with_a_use() {
        let text = format!(
            "{FILE}consumables:
  - name: Thistle Tea
    item: 7676
    classes: [ROGUE]
"
        );
        let registry = ExternalBuffDb::from_file(serde_yaml::from_str(&text).unwrap()).unwrap();
        let tea = registry.consumable("Thistle Tea").unwrap();
        assert!(tea.valid_for_class(PlayerClass::Rogue));
        assert!(!tea.valid_for_class(PlayerClass::Warrior));
        assert_eq!(registry.consumable_item_ids(), BTreeSet::from([7676]));
        assert!(matches!(
            registry.validate(&db()),
            Err(ExternalBuffError::UnknownConsumable { item: 7676, .. })
        ));

        let clash = format!(
            "{FILE}consumables:
  - name: Juju Power
    item: 7676
"
        );
        assert!(matches!(
            ExternalBuffDb::from_file(serde_yaml::from_str(&clash).unwrap()),
            Err(ExternalBuffError::DuplicateName(name)) if name == "Juju Power"
        ));
    }

    #[test]
    fn availability_by_faction_and_class() {
        let registry = registry();
        let (kings, _) = registry.get("Greater Blessing of Kings").unwrap();
        assert!(kings.valid_for_faction(Faction::Alliance));
        assert!(!kings.valid_for_faction(Faction::Horde));
        let (juju, _) = registry.get("Juju Power").unwrap();
        assert!(juju.valid_for_faction(Faction::Horde));
        assert!(juju.valid_for_class(PlayerClass::Warrior));
        let (shout, _) = registry.get("Battle Shout").unwrap();
        assert!(!shout.valid_for_class(PlayerClass::Warrior));
        assert!(shout.valid_for_class(PlayerClass::Rogue));

        let warrior: Vec<&str> = registry
            .offered_to(PlayerClass::Warrior)
            .map(|(s, _)| s.name.as_str())
            .collect();
        assert_eq!(
            warrior,
            [
                "Greater Blessing of Kings",
                "Strength of Earth Totem",
                "Juju Power",
                "Elixir of Giants",
                "Grilled Squid",
                "Sunder Armor",
                "Annihilator"
            ]
        );
        assert_eq!(registry.offered_to(PlayerClass::Rogue).count(), 8);
    }

    #[test]
    fn validation_errors() {
        let mut file: ExternalBuffFile = serde_yaml::from_str(FILE).unwrap();
        file.debuffs.push(ExternalBuffSpec::new("Juju Power", 1));
        assert!(matches!(
            ExternalBuffDb::from_file(file).unwrap_err(),
            ExternalBuffError::DuplicateName(name) if name == "Juju Power"
        ));

        let mut file: ExternalBuffFile = serde_yaml::from_str(FILE).unwrap();
        file.buffs[0].stacks = Some(0);
        assert!(matches!(
            ExternalBuffDb::from_file(file).unwrap_err(),
            ExternalBuffError::ZeroStacks { .. }
        ));

        let mut file: ExternalBuffFile = serde_yaml::from_str(FILE).unwrap();
        file.buffs
            .push(ExternalBuffSpec::new("Flask of the Titans", 17626));
        let registry = ExternalBuffDb::from_file(file).unwrap();
        assert!(matches!(
            registry.validate(&db()).unwrap_err(),
            ExternalBuffError::UnknownSpell { spell: 17626, .. }
        ));

        let mut db = db();
        db.add(None, SpellRecord::new(17626, "Flask of the Titans"))
            .unwrap();
        assert!(matches!(
            registry.validate(&db).unwrap_err(),
            ExternalBuffError::NoAuras { spell: 17626, .. }
        ));
    }

    #[test]
    fn general_buffs_track_selection_and_mutex_groups() {
        let registry = registry();
        let mut general = GeneralBuffs::new();
        for (index, (spec, debuff)) in registry.offered_to(PlayerClass::Warrior).enumerate() {
            general.add(spec.clone(), debuff, BuffId(index as u32), 1);
        }
        assert_eq!(general.entries().len(), 7);
        assert!(!general.is_selected("Juju Power"));
        general.get_mut("Juju Power").unwrap().selected = true;
        general.get_mut("Sunder Armor").unwrap().selected = true;
        assert!(general.is_selected("Juju Power"));
        assert_eq!(general.selected_buffs(), ["Juju Power"]);
        assert_eq!(general.selected_debuffs(), ["Sunder Armor"]);

        let peers: Vec<&str> = general
            .mutex_peers("Elixir of Giants")
            .iter()
            .map(|e| e.spec.name.as_str())
            .collect();
        assert_eq!(peers, ["Juju Power"]);
        assert!(general.mutex_peers("Grilled Squid").is_empty());
        assert!(general.mutex_peers("Sunder Armor").is_empty());

        let horde: Vec<&str> = general
            .offered(Faction::Horde)
            .map(|e| e.spec.name.as_str())
            .collect();
        assert!(!horde.contains(&"Greater Blessing of Kings"));
        assert!(horde.contains(&"Strength of Earth Totem"));
        assert!(horde.contains(&"Sunder Armor"));
    }

    #[test]
    #[should_panic(expected = "added twice")]
    fn adding_an_entry_twice_panics() {
        let mut general = GeneralBuffs::new();
        general.add(
            ExternalBuffSpec::new("Juju Power", 16323),
            false,
            BuffId(0),
            1,
        );
        general.add(
            ExternalBuffSpec::new("Juju Power", 16323),
            false,
            BuffId(1),
            1,
        );
    }
}
