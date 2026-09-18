//! The simulated target. Port of `Target/Target.*`.
//!
//! Only [`CreatureType`] is defined here for now; the `Target` itself is added in Phase 2.2.

use serde::{Deserialize, Serialize};

/// Creature type of the target; several stats depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CreatureType {
    Beast,
    Demon,
    Dragonkin,
    Elemental,
    Giant,
    Humanoid,
    Mechanical,
    Undead,
}

impl CreatureType {
    /// Every creature type, in declaration order.
    pub const ALL: [CreatureType; 8] = [
        CreatureType::Beast,
        CreatureType::Demon,
        CreatureType::Dragonkin,
        CreatureType::Elemental,
        CreatureType::Giant,
        CreatureType::Humanoid,
        CreatureType::Mechanical,
        CreatureType::Undead,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            CreatureType::Beast => "Beast",
            CreatureType::Demon => "Demon",
            CreatureType::Dragonkin => "Dragonkin",
            CreatureType::Elemental => "Elemental",
            CreatureType::Giant => "Giant",
            CreatureType::Humanoid => "Humanoid",
            CreatureType::Mechanical => "Mechanical",
            CreatureType::Undead => "Undead",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creature_type_serde_uses_display_names() {
        let parsed: CreatureType = serde_yaml::from_str("Dragonkin").unwrap();
        assert_eq!(parsed, CreatureType::Dragonkin);
        assert_eq!(
            serde_yaml::to_string(&CreatureType::Undead).unwrap().trim(),
            "Undead"
        );
        for creature_type in CreatureType::ALL {
            assert_eq!(creature_type.name(), format!("{creature_type:?}"));
        }
    }
}
