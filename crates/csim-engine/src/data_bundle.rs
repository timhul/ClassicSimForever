//! Every shipped data file the engine builds characters from, loaded once and shared read-only
//! by the sim threads (TASKS.md §1.5, the `Arc<DataBundle>`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buff::external::{ExternalBuffDb, ExternalBuffError};
use crate::character::{ClassDb, ClassSpecError};
use crate::files::{Files, FsFiles};
use crate::item::{EquipmentDb, EquipmentDbError};
use crate::race::{RaceDb, RaceDbError};
use crate::rotation::{RotationDb, RotationSpecError};
use crate::spell::record::{SpellDb, SpellDbError};
use crate::talent::{TalentDb, TalentSpecError};

/// Why the data directory could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum DataBundleError {
    #[error("spells: {0}")]
    Spells(#[from] SpellDbError),
    #[error("items: {0}")]
    Equipment(#[from] EquipmentDbError),
    #[error("classes: {0}")]
    Classes(#[from] ClassSpecError),
    #[error("races: {0}")]
    Races(#[from] RaceDbError),
    #[error("talents: {0}")]
    Talents(#[from] TalentSpecError),
    #[error("external buffs: {0}")]
    ExternalBuffs(#[from] ExternalBuffError),
    #[error("rotations: {0}")]
    Rotations(#[from] RotationSpecError),
}

/// The databases of a `data/` directory.
#[derive(Debug)]
pub struct DataBundle {
    pub spells: SpellDb,
    /// Items, item sets and enchants.
    pub equipment: Arc<EquipmentDb>,
    pub classes: ClassDb,
    pub races: RaceDb,
    pub talents: TalentDb,
    pub external_buffs: ExternalBuffDb,
    pub rotations: RotationDb,
}

impl DataBundle {
    /// Loads and cross-validates `dir` laid out as the repository's `data/`: `spells/`,
    /// `items/`, `item_sets.yaml`, `enchants.yaml`, `classes/`, `races.yaml`, `talents/`,
    /// `external_buffs.yaml` and `rotations/`.
    pub fn load(dir: &Path) -> Result<Self, DataBundleError> {
        Self::load_from(&FsFiles, dir)
    }

    /// [`Self::load`] from `files` (`dir` is `""` for files rooted at the data directory).
    pub fn load_from(files: &dyn Files, dir: &Path) -> Result<Self, DataBundleError> {
        let spells = SpellDb::load_from(files, &dir.join("spells"))?;
        let equipment = EquipmentDb::load_from(
            files,
            &dir.join("items"),
            Some(&dir.join("item_sets.yaml")),
            Some(&dir.join("enchants.yaml")),
        )?;
        let classes = ClassDb::load_from(files, &dir.join("classes"), Some(equipment.enchants()))?;
        let races = RaceDb::load_from(files, &dir.join("races.yaml"))?;
        let talents = TalentDb::load_from(files, &dir.join("talents"))?;
        let external_buffs = ExternalBuffDb::load_from(files, &dir.join("external_buffs.yaml"))?;
        external_buffs.validate(&spells)?;
        let rotations = RotationDb::load_from(files, &dir.join("rotations"))?;
        Ok(DataBundle {
            spells,
            equipment: Arc::new(equipment),
            classes,
            races,
            talents,
            external_buffs,
            rotations,
        })
    }

    /// The game client build the data was exported from (`1.60.1.70205`): the spell files'
    /// (the item files are exported with them).
    pub fn build(&self) -> Option<&str> {
        self.spells.build()
    }

    /// The repository's `data/` directory (for tests and tools run from the workspace).
    pub fn repository_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_data_names_one_build() {
        let data = DataBundle::load(&DataBundle::repository_dir()).unwrap();
        let build = data.build().expect("the spell files name their build");
        assert!(build.starts_with("1.60."), "{build}");
        assert_eq!(data.equipment.build(), Some(build), "items and spells");
    }

    #[test]
    fn spells_and_items_carry_their_icons() {
        let data = DataBundle::load(&DataBundle::repository_dir()).unwrap();
        // Bloodthirst (rank 4): `SpellMisc.SpellIconFileDataID`, named by the listfile.
        let bloodthirst = data.spells.get(23894).unwrap();
        assert_eq!(bloodthirst.icon, 136012);
        assert_eq!(
            bloodthirst.icon_name.as_deref(),
            Some("spell_nature_bloodlust")
        );
        // High Warlord's Bludgeon: `Item.IconFileDataID`.
        let bludgeon = data.equipment.item(18866).unwrap().spec();
        assert_eq!(bludgeon.icon, 133057);
        assert_eq!(bludgeon.icon_name.as_deref(), Some("inv_hammer_20"));
    }

    #[test]
    fn the_data_loads_from_memory_as_from_disk() {
        let dir = DataBundle::repository_dir();
        let disk = DataBundle::load(&dir).unwrap();
        let files = crate::files::yaml_tree(&dir);
        let memory = DataBundle::load_from(&files, Path::new("")).unwrap();
        assert_eq!(memory.spells.len(), disk.spells.len());
        assert_eq!(memory.equipment.len(), disk.equipment.len());
        assert_eq!(memory.classes.len(), disk.classes.len());
        assert_eq!(memory.races.specs(), disk.races.specs());
        assert_eq!(memory.talents.len(), disk.talents.len());
        assert_eq!(memory.rotations.len(), disk.rotations.len());
        assert_eq!(
            memory.spells.get(23894).unwrap(),
            disk.spells.get(23894).unwrap()
        );
        // A missing directory fails as on disk.
        let error = DataBundle::load_from(&files, Path::new("nowhere")).unwrap_err();
        assert!(error.to_string().contains("nowhere"), "{error}");
    }
}
