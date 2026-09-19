//! The effects and auras a damage simulator has no use for (`data/spells/overrides/discard.txt`).
//!
//! Crowd control, movement, immunities, healing, threat-less utility, phasing, mounts, … change
//! nothing in a single-target damage simulation. The exporter drops these effects from the
//! records it writes (`csim-tables export-spells`, `data/SPELL_INSTRUCTIONS.md` §1.9), and a
//! spell whose effects are all dropped — or that only triggers dropped spells — is left out of
//! the data files altogether (Taunt: `ATTACK_ME` + `MOD_TAUNT`; Mocking Blow keeps its
//! `SCHOOL_DAMAGE`). The engine uses the same lists to leave such effects out of the
//! "unsupported" report.
//!
//! Kept on purpose although `discard.txt` lists them as candidates: `MOD_THREAT` and
//! `MOD_TOTAL_THREAT` (the simulator tracks threat: stance passives, Defiance) and
//! `OVERRIDE_ACTIONBAR_SPELLS` (Improved Slam's and the runes' rank replacement).

use crate::spell::dbc::AuraType as A;
use crate::spell::dbc::SpellEffectName as E;

/// Aura types (`SpellEffect.EffectAura` of `APPLY_AURA` effects) that are discarded.
pub const DISCARDED_AURAS: &[A] = &[
    A::BindSight,
    A::ModPossess,
    A::ModConfuse,
    A::ModCharm,
    A::ModFear,
    A::PeriodicHeal,
    A::ModTaunt,
    A::ModStun,
    A::ModDamageTaken,
    A::DamageShield,
    A::ModStealthDetect,
    A::ModInvisibility,
    A::ModInvisibilityDetect,
    A::ModPacify,
    A::ModRoot,
    A::ModSilence,
    A::ReflectSpells,
    A::ModIncreaseSpeed,
    A::ModIncreaseMountedSpeed,
    A::ModDecreaseSpeed,
    A::EffectImmunity,
    A::StateImmunity,
    A::SchoolImmunity,
    A::DamageImmunity,
    A::DispelImmunity,
    A::TrackCreatures,
    A::TrackResources,
    A::ModCriticalHealingAmount,
    A::ModIncreaseSwimSpeed,
    A::ModPacifySilence,
    A::ModScale,
    A::FeignDeath,
    A::ModDisarm,
    A::ModStalked,
    A::SchoolAbsorb,
    A::ReflectSpellsSchool,
    A::ModLanguage,
    A::FarSight,
    A::MechanicImmunity,
    A::Mounted,
    A::SplitDamagePct,
    A::WaterBreathing,
    A::ModRegen,
    A::ChannelDeathItem,
    A::ModHealthRegenPercent,
    A::ModDetectRange,
    A::PreventsFleeing,
    A::ModUnattackable,
    A::InterruptRegen,
    A::Ghost,
    A::SpellMagnet,
    A::ManaShield,
    A::AurasVisible,
    A::WaterWalk,
    A::FeatherFall,
    A::Hover,
    A::AddTargetTrigger,
    A::InterceptMeleeRangedAttacks,
    A::OverrideClassScripts,
    A::ModHealing,
    A::ModRegenDuringCombat,
    A::ModMechanicResistance,
    A::ModHealingPct,
    A::Untrackable,
    A::Empathy,
    A::ModPossessPet,
    A::ModSpeedAlways,
    A::ModMountedSpeedAlways,
    A::ModManaRegenInterrupt,
    A::ModHealingDone,
    A::ModHealingDonePercent,
    A::ForceReaction,
    A::ModResistanceExclusive,
    A::ModChargeRecoveryRate,
    A::ReducePushback,
    A::TrackStealthed,
    A::ModDetectedRange,
    A::ModStealthLevel,
    A::ModWaterBreathing,
    A::ModReputationGain,
    A::AllowTalentSwapping,
    A::NoPvpCredit,
    A::ModHealthRegenInCombat,
    A::PowerBurn,
    A::SetFfaPvp,
    A::DetectAmore,
    A::ModSpeedNotStack,
    A::ModMountedSpeedNotStack,
    A::SpiritOfRedemption,
    A::AoeCharm,
    A::ModPowerDisplay,
    A::ModCriticalThreat,
    A::ModRating,
    A::ModFactionReputationGain,
    A::UseNormalMovementSpeed,
    A::MeleeSlow,
    A::ModXpPct,
    A::Fly,
    A::ModIncreaseVehicleFlightSpeed,
    A::ModIncreaseMountedFlightSpeed,
    A::ModIncreaseFlightSpeed,
    A::ModMountedFlightSpeedAlways,
    A::ModVehicleSpeedAlways,
    A::ModFlightSpeedNotStack,
    A::ArenaPreparation,
    A::ModDetaunt,
    A::RemoveTransmogCost,
    A::PreventRegeneratePower,
    A::DetectStealth,
    A::ModAoeDamageAvoidance,
    A::ModMechanicDuration,
    A::ModMechanicDurationNotStack,
    A::ModDispelResist,
    A::ControlVehicle,
    A::ModSpellHealingOfAttackPower,
    A::ModScale2,
    A::ForceMoveForward,
    A::ModFaction,
    A::ComprehendLanguage,
    A::ModAuraDurationByDispel,
    A::ModAuraDurationByDispelNotStack,
    A::CloneCaster,
    A::ModCombatResultChance,
    A::ConvertRune,
    A::ModSpeedSlowAll,
    A::ModDisarmOffhand,
    A::NoReagentUse,
    A::OverrideSummonedObject,
    A::ModHotPct,
    A::ScreenEffect,
    A::Phase,
    A::AbilityIgnoreAurastate,
    A::DisableCastingExceptAbilities,
    A::DisableAttackingExceptAbilities,
    A::SetVignette,
    A::ModImmuneAuraApplySchool,
    A::XRay,
    A::ModDamageDoneForMechanic,
    A::ModMaxAffectedTargets,
    A::ModDisarmRanged,
    A::InitializeImages,
    A::ModHonorGainPct,
    A::ModHealingReceived,
    A::Linked,
    A::Linked2,
    A::ModRecoveryRate,
    A::DeflectSpells,
    A::IgnoreHitDirection,
    A::PreventDurabilityLoss,
    A::ModXpQuestPct,
    A::OpenStable,
    A::OverrideSpells,
    A::PreventRegeneratePower2,
    A::ModPeriodicDamageTaken,
    A::ShareDamagePct,
    A::SchoolHealAbsorb,
    A::ModFakeInebriate,
    A::ModMinimumSpeed,
    A::CastWhileWalkingBySpellLabel,
    A::ModResilience,
    A::ModCreatureAoeDamageAvoidance,
    A::IgnoreCombat,
    A::AnimReplacementSet,
    A::PreventResurrection,
    A::UnderwaterWalking,
    A::SchoolAbsorbOverkill,
    A::Mastery,
    A::ModNoActions,
    A::InterfereTargetting,
    A::PhaseGroup,
    A::PhaseAlwaysVisible,
    A::CastWhileWalking,
    A::ForceWeather,
    A::MountRestrictions,
    A::ModVendorItemsPrices,
    A::ModDurabilityLoss,
    A::ModResurrectedHealthByGuildMember,
    A::ModMeleeDamageFromCaster,
    A::BypassArmorForCaster,
    A::ModMoneyGain,
    A::ModCurrencyGain,
];

/// Effect names (`SpellEffect.Effect`) that are discarded.
pub const DISCARDED_EFFECTS: &[E] = &[
    E::AttackMe,
    E::Dispel,
    E::DispelMechanic,
    E::InterruptCast,
    E::UpdatePlayerPhase,
];

impl AuraType {
    /// Whether an aura of this type is dropped from the data (see the module docs).
    pub fn is_discarded(self) -> bool {
        DISCARDED_AURAS.contains(&self)
    }
}

impl SpellEffectName {
    /// Whether an effect of this kind is dropped from the data (see the module docs).
    pub fn is_discarded(self) -> bool {
        DISCARDED_EFFECTS.contains(&self)
    }
}

use crate::spell::dbc::{AuraType, SpellEffectName};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crowd_control_and_utility_are_discarded_but_damage_threat_and_overrides_are_not() {
        assert!(A::ModStun.is_discarded());
        assert!(A::ModFear.is_discarded());
        assert!(A::ModIncreaseSpeed.is_discarded());
        assert!(A::MechanicImmunity.is_discarded());
        assert!(A::ModHealingPct.is_discarded());
        assert!(E::AttackMe.is_discarded());
        assert!(E::InterruptCast.is_discarded());
        assert!(!A::ModThreat.is_discarded());
        assert!(!A::ModTotalThreat.is_discarded());
        assert!(!A::OverrideActionbarSpells.is_discarded());
        assert!(!A::ModAttackPower.is_discarded());
        assert!(!A::PeriodicDamage.is_discarded());
        assert!(!A::Dummy.is_discarded());
        assert!(!E::SchoolDamage.is_discarded());
        assert!(!E::Dummy.is_discarded());
        assert!(!E::TriggerSpell.is_discarded());
        assert!(!E::ApplyAura.is_discarded());
    }

    #[test]
    fn the_lists_are_sorted_and_unique() {
        let ids: Vec<u32> = DISCARDED_AURAS.iter().map(|a| a.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids, sorted);
        assert!(DISCARDED_AURAS.iter().all(|a| a.is_known()));
        assert!(DISCARDED_EFFECTS.iter().all(|e| e.is_known()));
    }
}
