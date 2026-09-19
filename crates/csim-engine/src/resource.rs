//! Character resources. Port of `Resource/*`.
//!
//! Only the resource kinds are defined for now; the regenerating resources themselves follow in
//! Phase 4.2.

use serde::{Deserialize, Serialize};

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
}
