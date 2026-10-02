//! The item database. Port of `Equipment/EquipmentDb/EquipmentDb.*` and the item file readers.
//!
//! Items are loaded from the exported YAML files of `data/items/` (each an [`ItemFile`]). Every
//! id has one version; a lookup for a phase returns it when the item is available in that phase.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::set_bonus::{SetBonusDb, SetBonusError};
use super::{EquipmentSlot, Item, ItemError, ItemFile, ItemSetFile, ItemSetSpec, ItemSpec};
use crate::enchant::{EnchantDb, EnchantDbError};
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
    #[error("{path}: exported from build {found}, other item files from {expected}")]
    BuildMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
    #[error("item {id} ({name}) is defined twice")]
    DuplicateItem { id: u32, name: String },
    #[error(transparent)]
    SetBonus(#[from] SetBonusError),
    #[error(transparent)]
    Enchant(#[from] EnchantDbError),
}

/// All known items, item sets and enchants.
#[derive(Debug, Clone, Default)]
pub struct EquipmentDb {
    items: HashMap<u32, Arc<Item>>,
    /// The client build of the exported item files.
    build: Option<String>,
    sets: SetBonusDb,
    enchants: EnchantDb,
}

impl EquipmentDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a database from item specs and set definitions.
    pub fn from_specs(
        items: Vec<ItemSpec>,
        sets: Vec<ItemSetSpec>,
    ) -> Result<Self, EquipmentDbError> {
        let mut db = Self::new();
        for spec in items {
            db.add_item(Item::from_spec(spec)?)?;
        }
        db.set_sets(SetBonusDb::new(sets)?);
        Ok(db)
    }

    /// Loads every `*.yaml` item file in `items_dir` (sorted by file name) and, when given, the
    /// item set file (`data/item_sets.yaml`) and the enchant file.
    pub fn load(
        items_dir: &Path,
        item_sets: Option<&Path>,
        enchants: Option<&Path>,
    ) -> Result<Self, EquipmentDbError> {
        let mut db = Self::new();

        for path in yaml_files(items_dir)? {
            db.load_item_file(&path)?;
        }

        if let Some(path) = item_sets {
            db.load_item_set_file(path)?;
        }

        if let Some(path) = enchants {
            db.set_enchants(EnchantDb::load(path)?);
        }

        Ok(db)
    }

    /// Adds the items of one exported item file ([`ItemFile`]).
    pub fn load_item_file(&mut self, path: &Path) -> Result<(), EquipmentDbError> {
        for spec in self.read_item_file(path)? {
            self.add_item(Item::from_spec(spec)?)?;
        }
        Ok(())
    }

    fn read_item_file(&mut self, path: &Path) -> Result<Vec<ItemSpec>, EquipmentDbError> {
        let yaml_error = |source| EquipmentDbError::Yaml {
            path: path.to_path_buf(),
            source,
        };
        let text = fs::read_to_string(path).map_err(|source| EquipmentDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: ItemFile = serde_yaml::from_str(&text).map_err(yaml_error)?;
        self.check_build(path, file.build)?;
        Ok(file.items)
    }

    /// Checks that an exported file comes from the same build as the files loaded before it.
    fn check_build(&mut self, path: &Path, build: String) -> Result<(), EquipmentDbError> {
        if build.is_empty() {
            return Ok(());
        }
        match &self.build {
            Some(expected) if *expected != build => Err(EquipmentDbError::BuildMismatch {
                path: path.to_path_buf(),
                expected: expected.clone(),
                found: build,
            }),
            Some(_) => Ok(()),
            None => {
                self.build = Some(build);
                Ok(())
            }
        }
    }

    /// Replaces the item sets with those of an exported [`ItemSetFile`].
    pub fn load_item_set_file(&mut self, path: &Path) -> Result<(), EquipmentDbError> {
        let text = fs::read_to_string(path).map_err(|source| EquipmentDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: ItemSetFile =
            serde_yaml::from_str(&text).map_err(|source| EquipmentDbError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        self.check_build(path, file.build)?;
        self.set_sets(SetBonusDb::new(file.sets)?);
        Ok(())
    }

    /// Adds one item. Fails if the id is already defined.
    pub fn add_item(&mut self, item: Item) -> Result<(), EquipmentDbError> {
        match self.items.entry(item.id()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                Err(EquipmentDbError::DuplicateItem {
                    id: item.id(),
                    name: item.name().to_string(),
                })
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Arc::new(item));
                Ok(())
            }
        }
    }

    pub fn set_sets(&mut self, sets: SetBonusDb) {
        self.sets = sets;
    }

    pub fn sets(&self) -> &SetBonusDb {
        &self.sets
    }

    pub fn set_enchants(&mut self, enchants: EnchantDb) {
        self.enchants = enchants;
    }

    pub fn enchants(&self) -> &EnchantDb {
        &self.enchants
    }

    /// The client build of the exported item files.
    pub fn build(&self) -> Option<&str> {
        self.build.as_deref()
    }

    /// The item, when it is available in `phase`.
    pub fn get_item(&self, item_id: u32, phase: Phase) -> Option<&Arc<Item>> {
        self.item(item_id)
            .filter(|item| item.available_for_phase(phase))
    }

    /// The item regardless of phase.
    pub fn item(&self, item_id: u32) -> Option<&Arc<Item>> {
        self.items.get(&item_id)
    }

    pub fn name_for_item_id(&self, item_id: u32) -> Option<&str> {
        self.item(item_id).map(|item| item.name())
    }

    /// The items that fit `slot` in `phase`, sorted by id.
    pub fn items_for_slot(&self, slot: EquipmentSlot, phase: Phase) -> Vec<Arc<Item>> {
        let mut ids: Vec<u32> = self.items.keys().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| self.get_item(id, phase))
            .filter(|item| item.fits(slot))
            .cloned()
            .collect()
    }

    /// Every item id, sorted.
    pub fn item_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.items.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Number of distinct item ids.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The `*.yaml` / `*.yml` files directly in `dir`, sorted by name.
fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>, EquipmentDbError> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|source| EquipmentDbError::Io {
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
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemSlot, ItemStat, ItemType, Quality, WeaponDamageSpec};
    use crate::magic_school::MagicSchool;

    fn spec(id: u32, name: &str, phase: Phase, slot: ItemSlot, item_type: ItemType) -> ItemSpec {
        ItemSpec {
            id,
            name: name.into(),
            phase,
            slot,
            item_type,
            quality: Quality::Rare,
            icon: 0,
            unique: false,
            req_lvl: 60,
            item_lvl: 60,
            boe: false,
            faction: None,
            class_restrictions: Vec::new(),
            damage: None,
            stats: Default::default(),
            effects: Vec::new(),
            set: None,
            limit_category: None,
            suffixes: Vec::new(),
            flavour_text: String::new(),
        }
    }

    fn weapon(id: u32, name: &str, phase: Phase, slot: ItemSlot, item_type: ItemType) -> ItemSpec {
        let mut spec = spec(id, name, phase, slot, item_type);
        spec.damage = Some(WeaponDamageSpec {
            min: 10,
            max: 20,
            speed: 2.0,
            school: MagicSchool::Physical,
        });
        spec
    }

    fn db() -> EquipmentDb {
        let mut helm = spec(
            100,
            "Helm",
            Phase::MoltenCore,
            ItemSlot::Head,
            ItemType::Plate,
        );
        helm.stats.insert(ItemStat::Strength, 10.0);

        EquipmentDb::from_specs(
            vec![
                helm,
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
            vec![ItemSetSpec {
                id: 1,
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
        assert!(db.get_item(100, Phase::Naxxramas).is_some());

        assert!(db.get_item(101, Phase::AhnQiraj).is_none());
        assert!(db.get_item(101, Phase::Naxxramas).is_some());
        assert!(db.get_item(999, Phase::Naxxramas).is_none());
        assert_eq!(db.item(101).unwrap().phase(), Phase::Naxxramas);
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
    fn duplicate_id_is_an_error() {
        let error = EquipmentDb::from_specs(
            vec![
                spec(1, "A", Phase::MoltenCore, ItemSlot::Head, ItemType::Plate),
                spec(1, "A", Phase::AhnQiraj, ItemSlot::Head, ItemType::Plate),
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
    fn shipped_item_database_loads() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let db = EquipmentDb::load(
            &root.join("items"),
            Some(&root.join("item_sets.yaml")),
            Some(&root.join("enchants.yaml")),
        )
        .unwrap();

        assert!(db.len() > 1200, "only {} items loaded", db.len());
        let ebon_hand = db.get_item(19170, Phase::MoltenCore).unwrap();
        assert_eq!(ebon_hand.name(), "Ebon Hand");
        assert_eq!(ebon_hand.effects()[0].spell, 18211);
        assert!(ebon_hand.is_weapon());

        // Sets and enchants are attached.
        assert!(db.sets().sets().len() > 100);
        assert_eq!(db.build(), Some("1.60.1.70170"));
        assert!(!db.enchants().is_empty());

        // Every weapon slot item carries weapon data, every set item exists.
        for slot in [
            EquipmentSlot::Mainhand,
            EquipmentSlot::Offhand,
            EquipmentSlot::Ranged,
        ] {
            for item in db.items_for_slot(slot, Phase::Naxxramas) {
                assert!(item.is_weapon(), "{} has no weapon data", item.name());
            }
        }
        // Every set has a member the database knows (sets list Common members, and Forever
        // variants, that are not exported).
        for set in db.sets().sets() {
            assert!(
                set.items.iter().any(|&item_id| db.item(item_id).is_some()),
                "set {} has no known member",
                set.name
            );
        }
    }

    #[test]
    fn loads_from_yaml_files() {
        let dir = std::env::temp_dir().join(format!("csim-db-test-{}", std::process::id()));
        let items_dir = dir.join("items");
        fs::create_dir_all(&items_dir).unwrap();
        fs::write(
            items_dir.join("weapons.yaml"),
            "items:\n- id: 1\n  name: Sword\n  phase: 1\n  slot: 1H\n  type: SWORD\n  quality: EPIC\n  damage: {min: 1, max: 2, speed: 2.0}\n",
        )
        .unwrap();
        fs::write(
            items_dir.join("armor.yml"),
            "items:\n- id: 2\n  name: Helm\n  phase: 2\n  slot: HEAD\n  type: PLATE\n  quality: EPIC\n  stats: {STAMINA: 10}\n",
        )
        .unwrap();
        fs::write(items_dir.join("notes.txt"), "ignored").unwrap();
        fs::write(
            dir.join("sets.yaml"),
            "sets:\n- id: 7\n  name: S\n  items: [1, 2]\n  bonuses:\n  - pieces: 2\n    spell: 15464\n",
        )
        .unwrap();

        fs::write(
            dir.join("enchants.yaml"),
            "- name: Crusader
  display_name: Crusader
  unique_name: Enchant Weapon - Crusader
  slots: [MAINHAND, OFFHAND]
",
        )
        .unwrap();

        let db = EquipmentDb::load(
            &items_dir,
            Some(&dir.join("sets.yaml")),
            Some(&dir.join("enchants.yaml")),
        )
        .unwrap();
        assert_eq!(db.enchants().len(), 1);
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
        let error = EquipmentDb::load(&items_dir, None, None).unwrap_err();
        assert!(matches!(error, EquipmentDbError::Yaml { .. }));

        let error = EquipmentDb::load(&dir.join("missing"), None, None).unwrap_err();
        assert!(matches!(error, EquipmentDbError::Io { .. }));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exported_files_share_one_build() {
        let dir = std::env::temp_dir().join(format!("csim-db-build-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("head.yaml"),
            "build: 1.60.1.70009
items:
  - id: 1
    name: Exported Helm
    phase: 1
    slot: HEAD
    type: PLATE
    quality: EPIC
    stats: {STAMINA: 20}
",
        )
        .unwrap();

        let db = EquipmentDb::load(&dir, None, None).unwrap();
        assert_eq!(db.build(), Some("1.60.1.70009"));
        assert_eq!(db.len(), 1);
        assert_eq!(db.item(1).unwrap().name(), "Exported Helm");

        fs::write(dir.join("legs.yaml"), "build: 9.9.9.9\nitems: []\n").unwrap();
        let error = EquipmentDb::load(&dir, None, None).unwrap_err();
        assert!(matches!(error, EquipmentDbError::BuildMismatch { .. }));

        fs::remove_dir_all(&dir).unwrap();
    }
}
