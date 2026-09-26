//! Spell availability and outcome enums. Port of `SpellStatus` / `SpellResult` in
//! `Spells/Spell.h` and the status descriptions in `Statistics/StatisticsRotationExecutor.cpp`.

use crate::stance::Stance;

/// Why a spell can or cannot be cast right now. Port of `SpellStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpellStatus {
    Available,
    BuffInactive,
    CastInProgress,
    InBattleStance,
    InBerserkerStance,
    InDefensiveStance,
    InCasterForm,
    InBearForm,
    InCatForm,
    InMoonkinForm,
    IncorrectWeaponType,
    InsufficientComboPoints,
    InsufficientResources,
    /// The spell must be used from behind the target and the character faces it (tanking).
    NotBehindTarget,
    NotEnabled,
    NotInExecuteRange,
    /// The sim does not model the spell (`IGNORED` override, unsupported effects).
    NotSupported,
    OnCooldown,
    OnGcd,
    OnStanceCooldown,
    OnTrinketCooldown,
    OvercapResource,
}

impl SpellStatus {
    /// Every status, in declaration order.
    pub const ALL: [SpellStatus; 22] = [
        SpellStatus::Available,
        SpellStatus::BuffInactive,
        SpellStatus::CastInProgress,
        SpellStatus::InBattleStance,
        SpellStatus::InBerserkerStance,
        SpellStatus::InDefensiveStance,
        SpellStatus::InCasterForm,
        SpellStatus::InBearForm,
        SpellStatus::InCatForm,
        SpellStatus::InMoonkinForm,
        SpellStatus::IncorrectWeaponType,
        SpellStatus::InsufficientComboPoints,
        SpellStatus::InsufficientResources,
        SpellStatus::NotBehindTarget,
        SpellStatus::NotEnabled,
        SpellStatus::NotInExecuteRange,
        SpellStatus::NotSupported,
        SpellStatus::OnCooldown,
        SpellStatus::OnGcd,
        SpellStatus::OnStanceCooldown,
        SpellStatus::OnTrinketCooldown,
        SpellStatus::OvercapResource,
    ];

    pub fn is_available(self) -> bool {
        self == SpellStatus::Available
    }

    /// The status reported when a spell is unusable because the character is in `stance`.
    pub fn in_stance(stance: Stance) -> SpellStatus {
        match stance {
            Stance::Caster => SpellStatus::InCasterForm,
            Stance::Battle => SpellStatus::InBattleStance,
            Stance::Defensive => SpellStatus::InDefensiveStance,
            Stance::Berserker => SpellStatus::InBerserkerStance,
            Stance::Bear => SpellStatus::InBearForm,
            Stance::Cat => SpellStatus::InCatForm,
            Stance::Moonkin => SpellStatus::InMoonkinForm,
        }
    }

    /// The stance a stance status refers to, if it is one.
    pub fn stance(self) -> Option<Stance> {
        match self {
            SpellStatus::InCasterForm => Some(Stance::Caster),
            SpellStatus::InBattleStance => Some(Stance::Battle),
            SpellStatus::InDefensiveStance => Some(Stance::Defensive),
            SpellStatus::InBerserkerStance => Some(Stance::Berserker),
            SpellStatus::InBearForm => Some(Stance::Bear),
            SpellStatus::InCatForm => Some(Stance::Cat),
            SpellStatus::InMoonkinForm => Some(Stance::Moonkin),
            _ => None,
        }
    }

    /// Text used in rotation executor statistics.
    pub fn description(self) -> &'static str {
        match self {
            SpellStatus::Available => "Available",
            SpellStatus::BuffInactive => "FAIL: Depends on inactive buff",
            SpellStatus::CastInProgress => "FAIL: Cast in progress",
            SpellStatus::InBattleStance => "FAIL: In Battle Stance",
            SpellStatus::InBerserkerStance => "FAIL: In Berserker Stance",
            SpellStatus::InDefensiveStance => "FAIL: In Defensive Stance",
            SpellStatus::InCasterForm => "FAIL: In Caster Form",
            SpellStatus::InBearForm => "FAIL: In Bear Form",
            SpellStatus::InCatForm => "FAIL: In Cat Form",
            SpellStatus::InMoonkinForm => "FAIL: In Moonkin Form",
            SpellStatus::IncorrectWeaponType => "FAIL: Incorrect weapon type",
            SpellStatus::InsufficientComboPoints => "FAIL: Insufficient combo points",
            SpellStatus::InsufficientResources => "FAIL: Insufficient resources",
            SpellStatus::NotBehindTarget => "FAIL: Not behind the target",
            SpellStatus::NotEnabled => "FAIL: Not enabled",
            SpellStatus::NotInExecuteRange => "FAIL: Not in execute range",
            SpellStatus::NotSupported => "FAIL: Not modelled by the simulator",
            SpellStatus::OnCooldown => "FAIL: On spell cooldown",
            SpellStatus::OnGcd => "FAIL: On global cooldown",
            SpellStatus::OnStanceCooldown => "FAIL: On stance cooldown",
            SpellStatus::OnTrinketCooldown => "FAIL: On shared trinket cooldown",
            SpellStatus::OvercapResource => "FAIL: Cast would exceed resource cap",
        }
    }
}

/// Aggregate outcome of a spell's effect chain. Port of `SpellResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpellResult {
    /// No effect has been performed yet (before the first cast of an iteration).
    #[default]
    Undetermined,
    /// Every effect failed.
    Failure,
    /// Some effects succeeded and some failed.
    PartialSuccess,
    /// Every effect succeeded.
    Success,
}

impl SpellResult {
    /// The result of a chain whose first effect succeeded (`true`) or failed (`false`).
    pub fn from_first(success: bool) -> SpellResult {
        if success {
            SpellResult::Success
        } else {
            SpellResult::Failure
        }
    }

    /// Folds in the outcome of one more effect. Port of the loop in `Spell::spell_effect`.
    pub fn merge(self, success: bool) -> SpellResult {
        match (self, success) {
            (SpellResult::Undetermined, _) => SpellResult::from_first(success),
            (SpellResult::Success, false) | (SpellResult::Failure, true) => {
                SpellResult::PartialSuccess
            }
            (result, _) => result,
        }
    }

    /// Whether the result lets the spell's buff be applied (success or partial success).
    pub fn applies_buff(self) -> bool {
        matches!(self, SpellResult::Success | SpellResult::PartialSuccess)
    }

    pub fn is_failure(self) -> bool {
        self == SpellResult::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stance_statuses_round_trip() {
        for stance in Stance::ALL {
            let status = SpellStatus::in_stance(stance);
            assert_eq!(status.stance(), Some(stance));
            assert!(!status.is_available());
        }
        assert_eq!(SpellStatus::OnGcd.stance(), None);
        assert!(SpellStatus::Available.is_available());
    }

    #[test]
    fn every_status_has_a_description() {
        let descriptions: std::collections::HashSet<&str> =
            SpellStatus::ALL.iter().map(|s| s.description()).collect();
        assert_eq!(descriptions.len(), SpellStatus::ALL.len());
        for status in SpellStatus::ALL {
            assert_eq!(
                status.description().starts_with("FAIL: "),
                !status.is_available()
            );
        }
    }

    #[test]
    fn results_merge_like_the_effect_chain() {
        assert_eq!(SpellResult::from_first(true), SpellResult::Success);
        assert_eq!(SpellResult::from_first(false), SpellResult::Failure);
        assert_eq!(
            SpellResult::Success.merge(false),
            SpellResult::PartialSuccess
        );
        assert_eq!(
            SpellResult::Failure.merge(true),
            SpellResult::PartialSuccess
        );
        assert_eq!(SpellResult::Success.merge(true), SpellResult::Success);
        assert_eq!(SpellResult::Failure.merge(false), SpellResult::Failure);
        assert_eq!(
            SpellResult::PartialSuccess.merge(true),
            SpellResult::PartialSuccess
        );
        assert_eq!(
            SpellResult::PartialSuccess.merge(false),
            SpellResult::PartialSuccess
        );
        assert_eq!(SpellResult::Undetermined.merge(true), SpellResult::Success);

        assert!(SpellResult::Success.applies_buff());
        assert!(SpellResult::PartialSuccess.applies_buff());
        assert!(!SpellResult::Failure.applies_buff());
        assert!(!SpellResult::Undetermined.applies_buff());
        assert!(SpellResult::Failure.is_failure());
        assert_eq!(SpellResult::default(), SpellResult::Undetermined);
    }
}
