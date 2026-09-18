//! Enchants. Port of `Class/Common/Enchants/EnchantName.*`.
//!
//! Only the enchant identifiers are defined here for now; the enchant data (stats, procs, slot
//! and class availability) is added in Phase 2.6.

use serde::{Deserialize, Serialize};

/// Identifier of a permanent or temporary enchant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EnchantName {
    ArcanumOfFocus,
    ArcanumOfRapidity,
    Biznicks247x128Accurascope,
    BrilliantManaOil,
    BrilliantWizardOil,
    ConsecratedSharpeningStone,
    Crusader,
    DeathsEmbrace,
    DenseSharpeningStone,
    DenseWeightstone,
    ElementalSharpeningStone,
    Enchant2HWeaponAgility,
    Enchant2HWeaponSuperiorImpact,
    EnchantBootsAgility,
    EnchantBootsGreaterAgility,
    EnchantBootsMinorSpeed,
    EnchantBootsSpirit,
    EnchantBracerGreaterIntellect,
    EnchantBracerGreaterStrength,
    EnchantBracerManaRegeneration,
    EnchantBracerMinorAgility,
    EnchantBracerSuperiorStrength,
    EnchantChestGreaterStats,
    EnchantChestMajorMana,
    EnchantChestStats,
    EnchantCloakLesserAgility,
    EnchantGlovesFirePower,
    EnchantGlovesFrostPower,
    EnchantGlovesShadowPower,
    EnchantGlovesGreaterAgility,
    EnchantGlovesGreaterStrength,
    EnchantGlovesMinorHaste,
    EnchantGlovesSuperiorAgility,
    EnchantWeaponAgility,
    EnchantWeaponSpellPower,
    EnchantWeaponStrength,
    FalconsCall,
    FieryWeapon,
    InstantPoison,
    IronCounterweight,
    LesserArcanumOfVoracityAgility,
    LesserArcanumOfVoracityIntellect,
    LesserArcanumOfVoracitySpirit,
    LesserArcanumOfVoracityStamina,
    LesserArcanumOfVoracityStrength,
    LesserManaOil,
    MightOfTheScourge,
    PowerOfTheScourge,
    PresenceOfSight,
    HoodooHex,
    ShadowOil,
    SniperScope,
    SolidWeightstone,
    SuperiorStriking,
    WindfuryTotem,
    ZandalarSignetOfMight,
    ZandalarSignetOfMojo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_variant_names() {
        assert_eq!(
            serde_yaml::from_str::<EnchantName>("Crusader").unwrap(),
            EnchantName::Crusader
        );
        assert_eq!(
            serde_yaml::to_string(&EnchantName::Enchant2HWeaponAgility)
                .unwrap()
                .trim(),
            "Enchant2HWeaponAgility"
        );
    }
}
