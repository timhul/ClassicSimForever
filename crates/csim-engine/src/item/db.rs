//! The item database. Port of `Equipment/EquipmentDb/EquipmentDb.*` and the item file readers.
//!
//! Items are loaded from YAML files that each hold a list of [`ItemSpec`]s. The same item id may
//! appear in several content phases (items that were changed by a patch); lookups take the
//! current phase and return the newest version available in it, like the C++ database did.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::set_bonus::{ItemInTwoSets, SetBonusDb, SetSpec};
use super::{EquipmentSlot, Item, ItemError, ItemSpec};
use crate::phase::Phase;

/// Errors while loading the item database.
#[derive(Debug, thiserror::Error)]
pub enum EquipmentDbError {
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
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error("item {id} ({name}) is defined twice for phase {phase:?}")]
    DuplicateItem { id: u32, name: String, phase: Phase },
    #[error(transparent)]
    ItemInTwoSets(#[from] ItemInTwoSets),
}

/// All known items and item sets.
#[derive(Debug, Clone, Default)]
pub struct EquipmentDb {
    /// Every version of an item, sorted by ascending phase.
    items: HashMap<u32, Vec<Arc<Item>>>,
    sets: SetBonusDb,
}

impl EquipmentDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a database from item specs and set definitions.
    pub fn from_specs(items: Vec<ItemSpec>, sets: Vec<SetSpec>) -> Result<Self, EquipmentDbError> {
        let mut db = Self::new();
        for spec in items {
            db.add_item(Item::from_spec(spec)?)?;
        }
        db.set_sets(SetBonusDb::new(sets)?);
        Ok(db)
    }

    /// Loads every `*.yaml` item file in `items_dir` (sorted by file name) and, when given, the
    /// set bonus file.
    pub fn load(items_dir: &Path, set_bonuses: Option<&Path>) -> Result<Self, EquipmentDbError> {
        let mut db = Self::new();

        let mut paths: Vec<PathBuf> = fs::read_dir(items_dir)
            .map_err(|source| EquipmentDbError::Io {
                path: items_dir.to_path_buf(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        paths.sort();

        for path in paths {
            db.load_item_file(&path)?;
        }

        if let Some(path) = set_bonuses {
            db.load_set_bonus_file(path)?;
        }

        Ok(db)
    }

    /// Adds the items of one YAML file (a list of item specs).
    pub fn load_item_file(&mut self, path: &Path) -> Result<(), EquipmentDbError> {
        let text = fs::read_to_string(path).map_err(|source| EquipmentDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let specs: Vec<ItemSpec> =
            serde_yaml::from_str(&text).map_err(|source| EquipmentDbError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        for spec in specs {
            self.add_item(Item::from_spec(spec)?)?;
        }
        Ok(())
    }

    /// Replaces the item sets with those of a YAML file (a list of set specs).
    pub fn load_set_bonus_file(&mut self, path: &Path) -> Result<(), EquipmentDbError> {
        let text = fs::read_to_string(path).map_err(|source| EquipmentDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let sets: Vec<SetSpec> =
            serde_yaml::from_str(&text).map_err(|source| EquipmentDbError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        self.set_sets(SetBonusDb::new(sets)?);
        Ok(())
    }

    /// Adds one item version. Fails if the same id is already defined for its phase.
    pub fn add_item(&mut self, item: Item) -> Result<(), EquipmentDbError> {
        let versions = self.items.entry(item.id()).or_default();
        if versions
            .iter()
            .any(|existing| existing.phase() == item.phase())
        {
            return Err(EquipmentDbError::DuplicateItem {
                id: item.id(),
                name: item.name().to_string(),
                phase: item.phase(),
            });
        }
        let position = versions.partition_point(|existing| existing.phase() < item.phase());
        versions.insert(position, Arc::new(item));
        Ok(())
    }

    pub fn set_sets(&mut self, sets: SetBonusDb) {
        self.sets = sets;
    }

    pub fn sets(&self) -> &SetBonusDb {
        &self.sets
    }

    /// The newest version of an item available in `phase`.
    pub fn get_item(&self, item_id: u32, phase: Phase) -> Option<&Arc<Item>> {
        self.items
            .get(&item_id)?
            .iter()
            .rev()
            .find(|item| item.available_for_phase(phase))
    }

    /// The newest version of an item regardless of phase.
    pub fn get_item_any_phase(&self, item_id: u32) -> Option<&Arc<Item>> {
        self.items.get(&item_id)?.last()
    }

    /// Every version of an item, oldest phase first.
    pub fn item_versions(&self, item_id: u32) -> &[Arc<Item>] {
        self.items.get(&item_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn name_for_item_id(&self, item_id: u32) -> Option<&str> {
        self.get_item_any_phase(item_id).map(|item| item.name())
    }

    /// The items that fit `slot` in `phase` (one version per id), sorted by id.
    pub fn items_for_slot(&self, slot: EquipmentSlot, phase: Phase) -> Vec<Arc<Item>> {
        let mut ids: Vec<u32> = self.items.keys().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| self.get_item(id, phase))
            .filter(|item| item.fits(slot))
            .cloned()
            .collect()
    }

    /// Number of distinct item ids.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemSlot, ItemStat, ItemType, Quality, WeaponDamageSpec};

    fn spec(id: u32, name: &str, phase: Phase, slot: ItemSlot, item_type: ItemType) -> ItemSpec {
        ItemSpec {
            id,
            name: name.into(),
            phase,
            slot,
            item_type,
            quality: Quality::Rare,
            unique: false,
            req_lvl: 60,
            item_lvl: 60,
            boe: false,
            icon: String::new(),
            faction: None,
            class_restrictions: Vec::new(),
            damage: None,
            stats: Default::default(),
            procs: Vec::new(),
            uses: Vec::new(),
            modifies: Vec::new(),
            mutex: Vec::new(),
            random_affixes: Vec::new(),
            special_equip_effects: Vec::new(),
            source: String::new(),
            flavour_text: String::new(),
        }
    }

    fn weapon(id: u32, name: &str, phase: Phase, slot: ItemSlot, item_type: ItemType) -> ItemSpec {
        let mut spec = spec(id, name, phase, slot, item_type);
        spec.damage = Some(WeaponDamageSpec {
            min: 10,
            max: 20,
            speed: 2.0,
        });
        spec
    }

    fn db() -> EquipmentDb {
        let mut helm_p1 = spec(
            100,
            "Helm",
            Phase::MoltenCore,
            ItemSlot::Head,
            ItemType::Plate,
        );
        helm_p1.stats.insert(ItemStat::Strength, 10.0);
        let mut helm_p5 = spec(
            100,
            "Helm",
            Phase::AhnQiraj,
            ItemSlot::Head,
            ItemType::Plate,
        );
        helm_p5.stats.insert(ItemStat::Strength, 20.0);

        EquipmentDb::from_specs(
            vec![
                helm_p5,
                helm_p1,
                spec(
                    101,
                    "Naxx Helm",
                    Phase::Naxxramas,
                    ItemSlot::Head,
                    ItemType::Plate,
                ),
                weapon(
                    200,
                    "Sword",
                    Phase::MoltenCore,
                    ItemSlot::OneHand,
                    ItemType::Sword,
                ),
                weapon(
                    201,
                    "Axe",
                    Phase::DireMaul,
                    ItemSlot::Mainhand,
                    ItemType::Axe,
                ),
                weapon(
                    202,
                    "Shield",
                    Phase::MoltenCore,
                    ItemSlot::Offhand,
                    ItemType::Shield,
                ),
                weapon(
                    203,
                    "Bow",
                    Phase::MoltenCore,
                    ItemSlot::Ranged,
                    ItemType::Bow,
                ),
                spec(
                    300,
                    "Ring",
                    Phase::MoltenCore,
                    ItemSlot::Ring,
                    ItemType::Ring,
                ),
            ],
            vec![SetSpec {
                name: "Set".into(),
                items: vec![100, 300],
                bonuses: Vec::new(),
            }],
        )
        .unwrap()
    }

    #[test]
    fn lookups_respect_phase() {
        let db = db();
        assert_eq!(db.len(), 7);

        let helm = db.get_item(100, Phase::MoltenCore).unwrap();
        assert_eq!(helm.stats().get_strength(), 10);
        let helm = db.get_item(100, Phase::BlackwingLair).unwrap();
        assert_eq!(helm.stats().get_strength(), 10);
        let helm = db.get_item(100, Phase::AhnQiraj).unwrap();
        assert_eq!(helm.stats().get_strength(), 20);
        let helm = db.get_item(100, Phase::Naxxramas).unwrap();
        assert_eq!(helm.stats().get_strength(), 20);

        assert!(db.get_item(101, Phase::AhnQiraj).is_none());
        assert!(db.get_item(101, Phase::Naxxramas).is_some());
        assert!(db.get_item(999, Phase::Naxxramas).is_none());

        assert_eq!(
            db.get_item_any_phase(100).unwrap().stats().get_strength(),
            20
        );
        assert_eq!(db.item_versions(100).len(), 2);
        assert_eq!(db.item_versions(100)[0].phase(), Phase::MoltenCore);
        assert_eq!(db.name_for_item_id(201), Some("Axe"));
        assert_eq!(db.name_for_item_id(1), None);
    }

    #[test]
    fn items_for_slot() {
        let db = db();
        let ids = |slot, phase| -> Vec<u32> {
            db.items_for_slot(slot, phase)
                .iter()
                .map(|item| item.id())
                .collect()
        };

        assert_eq!(ids(EquipmentSlot::Head, Phase::MoltenCore), vec![100]);
        assert_eq!(ids(EquipmentSlot::Head, Phase::Naxxramas), vec![100, 101]);
        assert_eq!(ids(EquipmentSlot::Mainhand, Phase::MoltenCore), vec![200]);
        assert_eq!(
            ids(EquipmentSlot::Mainhand, Phase::DireMaul),
            vec![200, 201]
        );
        assert_eq!(
            ids(EquipmentSlot::Offhand, Phase::Naxxramas),
            vec![200, 202]
        );
        assert_eq!(ids(EquipmentSlot::Ranged, Phase::Naxxramas), vec![203]);
        assert_eq!(ids(EquipmentSlot::Ring1, Phase::Naxxramas), vec![300]);
        assert_eq!(ids(EquipmentSlot::Ring2, Phase::Naxxramas), vec![300]);
        assert!(ids(EquipmentSlot::Trinket1, Phase::Naxxramas).is_empty());
    }

    #[test]
    fn sets_are_available() {
        let db = db();
        assert_eq!(db.sets().set_for_item(300).unwrap().name, "Set");
        assert!(db.sets().set_for_item(200).is_none());
    }

    #[test]
    fn duplicate_phase_version_is_an_error() {
        let error = EquipmentDb::from_specs(
            vec![
                spec(1, "A", Phase::MoltenCore, ItemSlot::Head, ItemType::Plate),
                spec(1, "A", Phase::MoltenCore, ItemSlot::Head, ItemType::Plate),
            ],
            Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            EquipmentDbError::DuplicateItem { id: 1, .. }
        ));
    }

    #[test]
    fn invalid_items_are_reported() {
        let error = EquipmentDb::from_specs(
            vec![spec(
                1,
                "A",
                Phase::MoltenCore,
                ItemSlot::Mainhand,
                ItemType::Sword,
            )],
            Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            EquipmentDbError::Item(ItemError::MissingDamage { .. })
        ));
    }

    #[test]
    fn loads_from_yaml_files() {
        let dir = std::env::temp_dir().join(format!("csim-db-test-{}", std::process::id()));
        let items_dir = dir.join("items");
        fs::create_dir_all(&items_dir).unwrap();
        fs::write(
            items_dir.join("weapons.yaml"),
            "- id: 1\n  name: Sword\n  phase: 1\n  slot: 1H\n  type: SWORD\n  quality: EPIC\n  damage: {min: 1, max: 2, speed: 2.0}\n",
        )
        .unwrap();
        fs::write(
            items_dir.join("armor.yml"),
            "- id: 2\n  name: Helm\n  phase: 2\n  slot: HEAD\n  type: PLATE\n  quality: EPIC\n  stats: {STAMINA: 10}\n",
        )
        .unwrap();
        fs::write(items_dir.join("notes.txt"), "ignored").unwrap();
        fs::write(
            dir.join("sets.yaml"),
            "- name: S\n  items: [1, 2]\n  bonuses:\n    - pieces: 2\n      stat: HIT_CHANCE\n      value: 0.01\n",
        )
        .unwrap();

        let db = EquipmentDb::load(&items_dir, Some(&dir.join("sets.yaml"))).unwrap();
        assert_eq!(db.len(), 2);
        assert!(db.get_item(1, Phase::MoltenCore).unwrap().is_weapon());
        assert_eq!(
            db.get_item(2, Phase::DireMaul)
                .unwrap()
                .stats()
                .get_stamina(),
            10
        );
        assert_eq!(db.sets().set_for_item(2).unwrap().name, "S");

        fs::write(items_dir.join("broken.yaml"), "- id: x\n").unwrap();
        let error = EquipmentDb::load(&items_dir, None).unwrap_err();
        assert!(matches!(error, EquipmentDbError::Yaml { .. }));

        let error = EquipmentDb::load(&dir.join("missing"), None).unwrap_err();
        assert!(matches!(error, EquipmentDbError::Io { .. }));

        fs::remove_dir_all(&dir).unwrap();
    }
}
