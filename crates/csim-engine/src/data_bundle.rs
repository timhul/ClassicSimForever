//! Every shipped data file the engine builds characters from, loaded once and shared read-only
//! by the sim threads (TASKS.md §1.5, the `Arc<DataBundle>`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buff::external::{ExternalBuffDb, ExternalBuffError};
use crate::character::{ClassDb, ClassSpecError};
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
        let spells = SpellDb::load(&dir.join("spells"))?;
        let equipment = EquipmentDb::load(
            &dir.join("items"),
            Some(&dir.join("item_sets.yaml")),
            Some(&dir.join("enchants.yaml")),
        )?;
        let classes = ClassDb::load(&dir.join("classes"), Some(equipment.enchants()))?;
        let races = RaceDb::load(&dir.join("races.yaml"))?;
        let talents = TalentDb::load(&dir.join("talents"))?;
        let external_buffs = ExternalBuffDb::load(&dir.join("external_buffs.yaml"))?;
        external_buffs.validate(&spells)?;
        let rotations = RotationDb::load(&dir.join("rotations"))?;
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

    /// The repository's `data/` directory (for tests and tools run from the workspace).
    pub fn repository_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
