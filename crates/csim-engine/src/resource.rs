//! Character resources. Port of `Resource/*`.
//!
//! Only the resource kinds are defined for now; the regenerating resources themselves follow in
//! Phase 4.2.

use serde::{Deserialize, Serialize};

use crate::spell::dbc::PowerType;

/// The resource a spell costs / a character uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    Mana,
    Rage,
    Energy,
    Focus,
}

impl ResourceType {
    pub const ALL: [ResourceType; 4] = [
        ResourceType::Mana,
        ResourceType::Rage,
        ResourceType::Energy,
        ResourceType::Focus,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ResourceType::Mana => "Mana",
            ResourceType::Rage => "Rage",
            ResourceType::Energy => "Energy",
            ResourceType::Focus => "Focus",
        }
    }

    /// The resource behind a table power type; `None` for health, combo points and the power
    /// types no supported class uses.
    pub fn from_power_type(power: PowerType) -> Option<ResourceType> {
        match power {
            PowerType::Mana => Some(ResourceType::Mana),
            PowerType::Rage => Some(ResourceType::Rage),
            PowerType::Focus => Some(ResourceType::Focus),
            PowerType::Energy => Some(ResourceType::Energy),
            _ => None,
        }
    }

    pub fn power_type(self) -> PowerType {
        match self {
            ResourceType::Mana => PowerType::Mana,
            ResourceType::Rage => PowerType::Rage,
            ResourceType::Energy => PowerType::Energy,
            ResourceType::Focus => PowerType::Focus,
        }
    }

    /// Converts a stored table amount (rage in tenths) to the displayed amount, rounded.
    pub fn from_stored_amount(self, stored: f64) -> u32 {
        (stored / f64::from(self.power_type().display_modifier()))
            .round()
            .max(0.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_lowercase_names() {
        assert_eq!(
            serde_yaml::from_str::<ResourceType>("rage").unwrap(),
            ResourceType::Rage
        );
        assert!(serde_yaml::from_str::<ResourceType>("Rage").is_err());
    }

    #[test]
    fn power_types_map_to_resources() {
        assert_eq!(
            ResourceType::from_power_type(PowerType::Rage),
            Some(ResourceType::Rage)
        );
        assert_eq!(ResourceType::from_power_type(PowerType::ComboPoints), None);
        assert_eq!(ResourceType::from_power_type(PowerType::Health), None);
        assert_eq!(ResourceType::Rage.from_stored_amount(300.0), 30);
        assert_eq!(ResourceType::Rage.from_stored_amount(15.0), 2);
        assert_eq!(ResourceType::Mana.from_stored_amount(300.0), 300);
        for resource in ResourceType::ALL {
            assert_eq!(
                ResourceType::from_power_type(resource.power_type()),
                Some(resource)
            );
        }
    }
}
