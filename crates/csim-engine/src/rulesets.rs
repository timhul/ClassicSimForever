//! Encounter rulesets. Port of `Rulesets/Rulesets.h` and `Rulesets/RulesetControl.*`.
//!
//! A ruleset changes the fight to model a boss mechanic that affects the whole encounter:
//!
//! * **Vaelastrasz**: Essence of the Red (spell 23513, a `PERIODIC_ENERGIZE` aura: mana, rage
//!   and energy every second) is cast on the character when combat starts, and the fight starts
//!   with the boss at 30 % health, so the execute phase is the last two thirds of it.
//! * **Loatheb**: Fungal Bloom. The C++ gave every character +100 % melee crit and turned
//!   glancing blows off.
//!
//! The C++ `RulesetControl` mutated the character and the `SimSettings` when the ruleset
//! changed. Here the ruleset travels in [`SimParams`](crate::character::SimParams):
//! `Character::set_sim` applies the stat change, `CharacterContext::set_sim` also learns and
//! enables the ruleset's spells, and the target's start health is resolved by
//! [`SimSettings::sim_params`](crate::sim_settings::SimSettings::sim_params).

use serde::{Deserialize, Serialize};

/// Essence of the Red, the Vaelastrasz ruleset's start-of-combat spell.
pub const ESSENCE_OF_THE_RED: u32 = 23513;

/// Melee crit (out of 10 000) the Loatheb ruleset adds.
pub const LOATHEB_MELEE_CRIT: u32 = 10_000;

/// The target's health in percent when a Vaelastrasz fight starts: below 20 % for two thirds
/// of it.
pub const VAELASTRASZ_START_HEALTH_PERCENT: u32 = 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Ruleset {
    #[default]
    Standard,
    Vaelastrasz,
    Loatheb,
}

impl Ruleset {
    pub const ALL: [Ruleset; 3] = [Ruleset::Standard, Ruleset::Vaelastrasz, Ruleset::Loatheb];

    pub fn name(self) -> &'static str {
        match self {
            Ruleset::Standard => "Standard",
            Ruleset::Vaelastrasz => "Vaelastrasz",
            Ruleset::Loatheb => "Loatheb",
        }
    }

    /// Whether glancing blows occur.
    pub fn glancing_blows(self) -> bool {
        self != Ruleset::Loatheb
    }

    /// Melee crit (out of 10 000) added to the character as an aura.
    pub fn melee_aura_crit(self) -> u32 {
        match self {
            Ruleset::Loatheb => LOATHEB_MELEE_CRIT,
            Ruleset::Standard | Ruleset::Vaelastrasz => 0,
        }
    }

    /// The target's start health in percent the ruleset imposes, replacing the configured
    /// one (`target_start_health_percent`).
    pub fn target_start_health_percent(self) -> Option<u32> {
        match self {
            Ruleset::Vaelastrasz => Some(VAELASTRASZ_START_HEALTH_PERCENT),
            Ruleset::Standard | Ruleset::Loatheb => None,
        }
    }

    /// Game ids of the spells the ruleset enables on the character (performed at the start of
    /// combat).
    pub fn spells(self) -> &'static [u32] {
        match self {
            Ruleset::Vaelastrasz => &[ESSENCE_OF_THE_RED],
            Ruleset::Standard | Ruleset::Loatheb => &[],
        }
    }

    /// Every spell any ruleset uses (the spell exporter's seeds).
    pub fn all_spells() -> impl Iterator<Item = u32> {
        Self::ALL
            .into_iter()
            .flat_map(|ruleset| ruleset.spells().iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_screaming_names() {
        assert_eq!(
            serde_yaml::from_str::<Ruleset>("LOATHEB").unwrap(),
            Ruleset::Loatheb
        );
        assert_eq!(
            serde_yaml::to_string(&Ruleset::Vaelastrasz).unwrap().trim(),
            "VAELASTRASZ"
        );
        assert!(serde_yaml::from_str::<Ruleset>("ONYXIA").is_err());
    }

    #[test]
    fn standard_changes_nothing() {
        let ruleset = Ruleset::default();
        assert_eq!(ruleset, Ruleset::Standard);
        assert!(ruleset.glancing_blows());
        assert_eq!(ruleset.melee_aura_crit(), 0);
        assert_eq!(ruleset.target_start_health_percent(), None);
        assert!(ruleset.spells().is_empty());
    }

    #[test]
    fn loatheb_crits_and_never_glances() {
        assert!(!Ruleset::Loatheb.glancing_blows());
        assert_eq!(Ruleset::Loatheb.melee_aura_crit(), 10_000);
        assert_eq!(Ruleset::Loatheb.target_start_health_percent(), None);
    }

    #[test]
    fn vaelastrasz_executes_for_two_thirds_with_essence_of_the_red() {
        assert!(Ruleset::Vaelastrasz.glancing_blows());
        assert_eq!(Ruleset::Vaelastrasz.target_start_health_percent(), Some(30));
        assert_eq!(Ruleset::Vaelastrasz.spells(), &[ESSENCE_OF_THE_RED]);
        assert_eq!(
            Ruleset::all_spells().collect::<Vec<_>>(),
            [ESSENCE_OF_THE_RED]
        );
    }
}
