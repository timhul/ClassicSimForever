//! Magic schools. Port of `Spells/MagicSchools.h`.

use serde::{Deserialize, Serialize};

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
    fn magic_excludes_physical() {
        assert!(!MagicSchool::MAGIC.contains(&MagicSchool::Physical));
        assert_eq!(MagicSchool::ALL.len(), MagicSchool::MAGIC.len() + 1);
    }
}
