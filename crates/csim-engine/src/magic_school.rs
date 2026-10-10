//! Magic schools. Port of `Spells/MagicSchools.h`.

use serde::{Deserialize, Serialize};

use crate::spell::dbc::SpellSchoolMask;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MagicSchool {
    Physical,
    Arcane,
    Fire,
    Frost,
    Nature,
    Shadow,
    Holy,
}

impl MagicSchool {
    /// Every school, in declaration order.
    pub const ALL: [MagicSchool; 7] = [
        MagicSchool::Physical,
        MagicSchool::Arcane,
        MagicSchool::Fire,
        MagicSchool::Frost,
        MagicSchool::Nature,
        MagicSchool::Shadow,
        MagicSchool::Holy,
    ];

    /// The non-physical schools.
    pub const MAGIC: [MagicSchool; 6] = [
        MagicSchool::Arcane,
        MagicSchool::Fire,
        MagicSchool::Frost,
        MagicSchool::Nature,
        MagicSchool::Shadow,
        MagicSchool::Holy,
    ];

    /// The school's bit of a `SpellSchoolMask`.
    pub fn school_mask(self) -> SpellSchoolMask {
        match self {
            MagicSchool::Physical => SpellSchoolMask::PHYSICAL,
            MagicSchool::Arcane => SpellSchoolMask::ARCANE,
            MagicSchool::Fire => SpellSchoolMask::FIRE,
            MagicSchool::Frost => SpellSchoolMask::FROST,
            MagicSchool::Nature => SpellSchoolMask::NATURE,
            MagicSchool::Shadow => SpellSchoolMask::SHADOW,
            MagicSchool::Holy => SpellSchoolMask::HOLY,
        }
    }

    /// The magic schools among `mask` (the misc value of a school-masked aura).
    pub fn magic_schools_of(mask: SpellSchoolMask) -> impl Iterator<Item = MagicSchool> {
        MagicSchool::MAGIC
            .into_iter()
            .filter(move |school| mask.intersects(school.school_mask()))
    }

    /// The school of a spell with `mask`: physical when it is among the schools (or none is
    /// set), else the first magic school of the mask.
    pub fn from_school_mask(mask: SpellSchoolMask) -> MagicSchool {
        const MAGIC_BITS: [(SpellSchoolMask, MagicSchool); 6] = [
            (SpellSchoolMask::HOLY, MagicSchool::Holy),
            (SpellSchoolMask::FIRE, MagicSchool::Fire),
            (SpellSchoolMask::NATURE, MagicSchool::Nature),
            (SpellSchoolMask::FROST, MagicSchool::Frost),
            (SpellSchoolMask::SHADOW, MagicSchool::Shadow),
            (SpellSchoolMask::ARCANE, MagicSchool::Arcane),
        ];
        if mask.is_physical() {
            return MagicSchool::Physical;
        }
        MAGIC_BITS
            .into_iter()
            .find(|(bit, _)| mask.intersects(*bit))
            .map_or(MagicSchool::Physical, |(_, school)| school)
    }

    pub fn name(self) -> &'static str {
        match self {
            MagicSchool::Physical => "Physical",
            MagicSchool::Arcane => "Arcane",
            MagicSchool::Fire => "Fire",
            MagicSchool::Frost => "Frost",
            MagicSchool::Nature => "Nature",
            MagicSchool::Shadow => "Shadow",
            MagicSchool::Holy => "Holy",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_snake_case() {
        let yaml = serde_yaml::to_string(&MagicSchool::Frost).unwrap();
        assert_eq!(yaml.trim(), "frost");
        let school: MagicSchool = serde_yaml::from_str("shadow").unwrap();
        assert_eq!(school, MagicSchool::Shadow);
    }

    #[test]
    fn school_of_a_school_mask() {
        assert_eq!(
            MagicSchool::from_school_mask(SpellSchoolMask::NATURE),
            MagicSchool::Nature
        );
        assert_eq!(
            MagicSchool::from_school_mask(SpellSchoolMask::PHYSICAL),
            MagicSchool::Physical
        );
        assert_eq!(
            MagicSchool::from_school_mask(SpellSchoolMask::from_bits(0)),
            MagicSchool::Physical
        );
        assert_eq!(
            MagicSchool::from_school_mask(SpellSchoolMask::SHADOW | SpellSchoolMask::FROST),
            MagicSchool::Frost
        );
    }

    #[test]
    fn magic_excludes_physical() {
        assert!(!MagicSchool::MAGIC.contains(&MagicSchool::Physical));
        assert_eq!(MagicSchool::ALL.len(), MagicSchool::MAGIC.len() + 1);
    }
}
