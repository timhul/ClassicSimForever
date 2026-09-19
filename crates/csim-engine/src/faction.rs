//! Factions. Port of `Faction/AvailableFactions.h`.
//!
//! The C++ enum had a `Neutral` value used for items available to both factions; the Rust
//! code models that as `Option<Faction>` instead.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Faction {
    Alliance,
    Horde,
}

impl Faction {
    pub fn opposite(self) -> Faction {
        match self {
            Faction::Alliance => Faction::Horde,
            Faction::Horde => Faction::Alliance,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Faction::Alliance => "Alliance",
            Faction::Horde => "Horde",
        }
    }
}

/// The classes a character can be. Port of the class name strings used throughout the C++ code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlayerClass {
    Druid,
    Hunter,
    Mage,
    Paladin,
    Priest,
    Rogue,
    Shaman,
    Warlock,
    Warrior,
}

impl PlayerClass {
    pub fn name(self) -> &'static str {
        match self {
            PlayerClass::Druid => "Druid",
            PlayerClass::Hunter => "Hunter",
            PlayerClass::Mage => "Mage",
            PlayerClass::Paladin => "Paladin",
            PlayerClass::Priest => "Priest",
            PlayerClass::Rogue => "Rogue",
            PlayerClass::Shaman => "Shaman",
            PlayerClass::Warlock => "Warlock",
            PlayerClass::Warrior => "Warrior",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names() {
        assert_eq!(
            serde_yaml::from_str::<Faction>("HORDE").unwrap(),
            Faction::Horde
        );
        assert_eq!(
            serde_yaml::from_str::<PlayerClass>("WARRIOR").unwrap(),
            PlayerClass::Warrior
        );
        assert_eq!(Faction::Alliance.opposite(), Faction::Horde);
    }
}
