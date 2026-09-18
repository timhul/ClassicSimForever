//! Content phases. Port of `Phases/ContentPhase.*` and `Phases/PhaseRequirer.*`.
//!
//! Serialized as the phase number (`1` = Molten Core).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Phase {
    MoltenCore = 1,
    DireMaul,
    BlackwingLair,
    ZulGurub,
    AhnQiraj,
    Naxxramas,
}

/// The phase number is outside `1..=6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unknown content phase {0}")]
pub struct UnknownPhase(pub u8);

impl Phase {
    pub const ALL: [Phase; 6] = [
        Phase::MoltenCore,
        Phase::DireMaul,
        Phase::BlackwingLair,
        Phase::ZulGurub,
        Phase::AhnQiraj,
        Phase::Naxxramas,
    ];

    pub fn number(self) -> u8 {
        self as u8
    }

    pub fn description(self) -> &'static str {
        match self {
            Phase::MoltenCore => "Phase 1: Molten Core, Onyxia",
            Phase::DireMaul => "Phase 2: Dire Maul",
            Phase::BlackwingLair => "Phase 3: Blackwing Lair",
            Phase::ZulGurub => "Phase 4: Zul'Gurub",
            Phase::AhnQiraj => "Phase 5: Ahn'Qiraj",
            Phase::Naxxramas => "Phase 6: Naxxramas",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Phase::MoltenCore => "MC",
            Phase::DireMaul => "DM",
            Phase::BlackwingLair => "BWL",
            Phase::ZulGurub => "ZG",
            Phase::AhnQiraj => "AQ",
            Phase::Naxxramas => "Naxx",
        }
    }

    /// Whether content requiring `self` is available in `current`.
    pub fn available_in(self, current: Phase) -> bool {
        self <= current
    }
}

impl TryFrom<u8> for Phase {
    type Error = UnknownPhase;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|phase| phase.number() == value)
            .ok_or(UnknownPhase(value))
    }
}

impl From<Phase> for u8 {
    fn from(phase: Phase) -> u8 {
        phase.number()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_numbers_round_trip() {
        for phase in Phase::ALL {
            assert_eq!(Phase::try_from(phase.number()), Ok(phase));
        }
        assert_eq!(Phase::try_from(0), Err(UnknownPhase(0)));
        assert_eq!(Phase::try_from(7), Err(UnknownPhase(7)));
    }

    #[test]
    fn serde_uses_numbers() {
        assert_eq!(serde_yaml::from_str::<Phase>("5").unwrap(), Phase::AhnQiraj);
        assert_eq!(
            serde_yaml::to_string(&Phase::BlackwingLair).unwrap().trim(),
            "3"
        );
        assert!(serde_yaml::from_str::<Phase>("9").is_err());
    }

    #[test]
    fn availability() {
        assert!(Phase::MoltenCore.available_in(Phase::Naxxramas));
        assert!(Phase::AhnQiraj.available_in(Phase::AhnQiraj));
        assert!(!Phase::Naxxramas.available_in(Phase::AhnQiraj));
    }
}
