//! Stances and shapeshift forms. Port of `Stance` in `Spells/SpellInfo.h`.

use serde::{Deserialize, Serialize};

/// The stance (Warrior) or form (Druid) a character is in. Serialized with the names used by the
/// spell data (`BATTLE_STANCE`, `BEAR_FORM`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stance {
    #[serde(rename = "CASTER_FORM")]
    Caster,
    #[serde(rename = "BATTLE_STANCE")]
    Battle,
    #[serde(rename = "DEFENSIVE_STANCE")]
    Defensive,
    #[serde(rename = "BERSERKER_STANCE")]
    Berserker,
    #[serde(rename = "BEAR_FORM")]
    Bear,
    #[serde(rename = "CAT_FORM")]
    Cat,
    #[serde(rename = "MOONKIN_FORM")]
    Moonkin,
}

impl Stance {
    pub const ALL: [Stance; 7] = [
        Stance::Caster,
        Stance::Battle,
        Stance::Defensive,
        Stance::Berserker,
        Stance::Bear,
        Stance::Cat,
        Stance::Moonkin,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stance::Caster => "Caster Form",
            Stance::Battle => "Battle Stance",
            Stance::Defensive => "Defensive Stance",
            Stance::Berserker => "Berserker Stance",
            Stance::Bear => "Bear Form",
            Stance::Cat => "Cat Form",
            Stance::Moonkin => "Moonkin Form",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_data_file_names() {
        assert_eq!(
            serde_yaml::from_str::<Stance>("BERSERKER_STANCE").unwrap(),
            Stance::Berserker
        );
        assert_eq!(
            serde_yaml::to_string(&Stance::Bear).unwrap().trim(),
            "BEAR_FORM"
        );
    }
}
