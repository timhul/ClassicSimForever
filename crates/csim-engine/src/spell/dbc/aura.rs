//! `SpellEffect.EffectAura`: the aura applied by an `APPLY_AURA`-family effect.

use super::dbc_enum;

dbc_enum! {
    /// Retail `AuraType`, the `EffectAura` column of `SpellEffect`.
    ///
    /// The engine interprets the values listed in `data/SPELL_INSTRUCTIONS.md` §1.7 (the ones with
    /// a doc comment below say how); the rest are named for reporting and are no-ops. The
    /// values a damage simulator has no use for are not named at all (`super::discard`): they
    /// load as `Unknown(id)` and the exporter drops them.
    AuraType: u32 {
        None = 0 => "NONE",
        /// Bleed / DoT tick (`EffectAuraPeriod` ms), Rend.
        PeriodicDamage = 3 => "PERIODIC_DAMAGE",
        /// Scripted; what it does is in the description and in `data/spells/overrides/*.yaml`.
        Dummy = 4 => "DUMMY",
        ModAttackspeed = 9 => "MOD_ATTACKSPEED",
        /// Threat done %, misc = school mask (stance passives, Defiance).
        ModThreat = 10 => "MOD_THREAT",
        ModDamageDone = 13 => "MOD_DAMAGE_DONE",
        ModStealth = 16 => "MOD_STEALTH",
        ObsModHealth = 20 => "OBS_MOD_HEALTH",
        ObsModPower = 21 => "OBS_MOD_POWER",
        /// Resistance, misc 1 = armor (Sunder Armor −450).
        ModResistance = 22 => "MOD_RESISTANCE",
        PeriodicTriggerSpell = 23 => "PERIODIC_TRIGGER_SPELL",
        /// Periodic resource gain, misc = power type (Bloodrage 1 rage/s).
        PeriodicEnergize = 24 => "PERIODIC_ENERGIZE",
        /// Stat, misc −1 all / 0 Str / 1 Agi / 2 Sta / 3 Int / 4 Spi.
        ModStat = 29 => "MOD_STAT",
        ModSkill = 30 => "MOD_SKILL",
        ModIncreaseHealth = 34 => "MOD_INCREASE_HEALTH",
        ModIncreaseEnergy = 35 => "MOD_INCREASE_ENERGY",
        /// Stance/form, misc = `SpellShapeshiftForm.ID`.
        ModShapeshift = 36 => "MOD_SHAPESHIFT",
        /// Proc: `EffectTriggerSpell` fires on `SpellAuraOptions.ProcTypeMask` events.
        ProcTriggerSpell = 42 => "PROC_TRIGGER_SPELL",
        ProcTriggerDamage = 43 => "PROC_TRIGGER_DAMAGE",
        ModParryPercent = 47 => "MOD_PARRY_PERCENT",
        PeriodicTriggerSpellFromClient = 48 => "PERIODIC_TRIGGER_SPELL_FROM_CLIENT",
        ModDodgePercent = 49 => "MOD_DODGE_PERCENT",
        ModBlockPercent = 51 => "MOD_BLOCK_PERCENT",
        /// Melee crit chance % (Cruelty).
        ModWeaponCritPercent = 52 => "MOD_WEAPON_CRIT_PERCENT",
        PeriodicLeech = 53 => "PERIODIC_LEECH",
        ModHitChance = 54 => "MOD_HIT_CHANCE",
        ModSpellHitChance = 55 => "MOD_SPELL_HIT_CHANCE",
        Transform = 56 => "TRANSFORM",
        ModSpellCritChance = 57 => "MOD_SPELL_CRIT_CHANCE",
        ModDamageDoneCreature = 59 => "MOD_DAMAGE_DONE_CREATURE",
        PeriodicHealthFunnel = 62 => "PERIODIC_HEALTH_FUNNEL",
        PeriodicManaLeech = 64 => "PERIODIC_MANA_LEECH",
        ModCastingSpeedNotStack = 65 => "MOD_CASTING_SPEED_NOT_STACK",
        ModSpellCritChanceSchool = 71 => "MOD_SPELL_CRIT_CHANCE_SCHOOL",
        ModPowerCostSchoolPct = 72 => "MOD_POWER_COST_SCHOOL_PCT",
        ModPowerCostSchool = 73 => "MOD_POWER_COST_SCHOOL",
        /// Damage done %, misc = school mask (Death Wish, Two-Handed Weapon Specialization).
        ModDamagePercentDone = 79 => "MOD_DAMAGE_PERCENT_DONE",
        ModPercentStat = 80 => "MOD_PERCENT_STAT",
        ModBaseResistance = 83 => "MOD_BASE_RESISTANCE",
        /// Power regeneration, misc = power type (Anger Management).
        ModPowerRegen = 85 => "MOD_POWER_REGEN",
        /// Damage taken %, misc = school mask.
        ModDamagePercentTaken = 87 => "MOD_DAMAGE_PERCENT_TAKEN",
        PeriodicDamagePercent = 89 => "PERIODIC_DAMAGE_PERCENT",
        ModSkillTalent = 98 => "MOD_SKILL_TALENT",
        /// Flat melee attack power (Battle Shout).
        ModAttackPower = 99 => "MOD_ATTACK_POWER",
        ModResistancePct = 101 => "MOD_RESISTANCE_PCT",
        ModMeleeAttackPowerVersus = 102 => "MOD_MELEE_ATTACK_POWER_VERSUS",
        ModTotalThreat = 103 => "MOD_TOTAL_THREAT",
        /// Flat spell modifier: misc = `SpellModOp`, `EffectSpellClassMask` = affected spells.
        AddFlatModifier = 107 => "ADD_FLAT_MODIFIER",
        /// Percent spell modifier: misc = `SpellModOp`, `EffectSpellClassMask` = affected spells.
        AddPctModifier = 108 => "ADD_PCT_MODIFIER",
        /// Casts `EffectTriggerSpell` on the target of the spells in `EffectSpellClassMask`
        /// (Relentless Strikes on finishers). Scripted: the chance rule is in the overrides.
        AddTargetTrigger = 109 => "ADD_TARGET_TRIGGER",
        ModPowerRegenPercent = 110 => "MOD_POWER_REGEN_PERCENT",
        ModRangedDamageTaken = 113 => "MOD_RANGED_DAMAGE_TAKEN",
        ModRangedDamageTakenPct = 114 => "MOD_RANGED_DAMAGE_TAKEN_PCT",
        ModOffhandDamagePct = 122 => "MOD_OFFHAND_DAMAGE_PCT",
        ModTargetResistance = 123 => "MOD_TARGET_RESISTANCE",
        ModRangedAttackPower = 124 => "MOD_RANGED_ATTACK_POWER",
        ModMeleeDamageTaken = 125 => "MOD_MELEE_DAMAGE_TAKEN",
        ModMeleeDamageTakenPct = 126 => "MOD_MELEE_DAMAGE_TAKEN_PCT",
        RangedAttackPowerAttackerBonus = 127 => "RANGED_ATTACK_POWER_ATTACKER_BONUS",
        ModRangedAttackPowerVersus = 131 => "MOD_RANGED_ATTACK_POWER_VERSUS",
        ModIncreaseEnergyPercent = 132 => "MOD_INCREASE_ENERGY_PERCENT",
        ModIncreaseHealthPercent = 133 => "MOD_INCREASE_HEALTH_PERCENT",
        ModTotalStatPercentage = 137 => "MOD_TOTAL_STAT_PERCENTAGE",
        ModMeleeHaste = 138 => "MOD_MELEE_HASTE",
        ModRangedHaste = 140 => "MOD_RANGED_HASTE",
        ModBaseResistancePct = 142 => "MOD_BASE_RESISTANCE_PCT",
        SafeFall = 144 => "SAFE_FALL",
        ModPetTalentPoints = 145 => "MOD_PET_TALENT_POINTS",
        AllowTamePetType = 146 => "ALLOW_TAME_PET_TYPE",
        MechanicImmunityMask = 147 => "MECHANIC_IMMUNITY_MASK",
        ModShieldBlockvaluePct = 150 => "MOD_SHIELD_BLOCKVALUE_PCT",
        PetDamageMulti = 157 => "PET_DAMAGE_MULTI",
        ModCritDamageBonus = 163 => "MOD_CRIT_DAMAGE_BONUS",
        MeleeAttackPowerAttackerBonus = 165 => "MELEE_ATTACK_POWER_ATTACKER_BONUS",
        /// Attack power % (Blood Fury).
        ModAttackPowerPct = 166 => "MOD_ATTACK_POWER_PCT",
        ModRangedAttackPowerPct = 167 => "MOD_RANGED_ATTACK_POWER_PCT",
        ModDamageDoneVersus = 168 => "MOD_DAMAGE_DONE_VERSUS",
        ModSpellDamageOfStatPercent = 174 => "MOD_SPELL_DAMAGE_OF_STAT_PERCENT",
        ModSpellHealingOfStatPercent = 175 => "MOD_SPELL_HEALING_OF_STAT_PERCENT",
        ModMaxPowerPct = 178 => "MOD_MAX_POWER_PCT",
        ModFlatSpellDamageVersus = 180 => "MOD_FLAT_SPELL_DAMAGE_VERSUS",
        ModAttackerMeleeHitChance = 184 => "MOD_ATTACKER_MELEE_HIT_CHANCE",
        ModAttackerRangedHitChance = 185 => "MOD_ATTACKER_RANGED_HIT_CHANCE",
        ModAttackerSpellHitChance = 186 => "MOD_ATTACKER_SPELL_HIT_CHANCE",
        ModAttackerMeleeCritChance = 187 => "MOD_ATTACKER_MELEE_CRIT_CHANCE",
        ModAttackerRangedCritChance = 188 => "MOD_ATTACKER_RANGED_CRIT_CHANCE",
        ModMeleeRangedHaste = 192 => "MOD_MELEE_RANGED_HASTE",
        ModTargetAbsorbSchool = 194 => "MOD_TARGET_ABSORB_SCHOOL",
        ModTargetAbilityAbsorbSchool = 195 => "MOD_TARGET_ABILITY_ABSORB_SCHOOL",
        ModCooldown = 196 => "MOD_COOLDOWN",
        ModAttackerSpellAndWeaponCritChance = 197 => "MOD_ATTACKER_SPELL_AND_WEAPON_CRIT_CHANCE",
        IgnoreCombatResult = 202 => "IGNORE_COMBAT_RESULT",
        ModAttackerMeleeCritDamage = 203 => "MOD_ATTACKER_MELEE_CRIT_DAMAGE",
        ModAttackerRangedCritDamage = 204 => "MOD_ATTACKER_RANGED_CRIT_DAMAGE",
        ModSchoolCritDmgTaken = 205 => "MOD_SCHOOL_CRIT_DMG_TAKEN",
        ModRangedAttackPowerOfStatPercent = 212 => "MOD_RANGED_ATTACK_POWER_OF_STAT_PERCENT",
        ModRageFromDamageDealt = 213 => "MOD_RAGE_FROM_DAMAGE_DEALT",
        HasteSpells = 216 => "HASTE_SPELLS",
        ModMeleeHaste2 = 217 => "MOD_MELEE_HASTE_2",
        AddPctModifierBySpellLabel = 218 => "ADD_PCT_MODIFIER_BY_SPELL_LABEL",
        AddFlatModifierBySpellLabel = 219 => "ADD_FLAT_MODIFIER_BY_SPELL_LABEL",
        ModAbilitySchoolMask = 220 => "MOD_ABILITY_SCHOOL_MASK",
        /// Scripted periodic tick.
        PeriodicDummy = 226 => "PERIODIC_DUMMY",
        PeriodicTriggerSpellWithValue = 227 => "PERIODIC_TRIGGER_SPELL_WITH_VALUE",
        ModMaxHealth = 230 => "MOD_MAX_HEALTH",
        ProcTriggerSpellWithValue = 231 => "PROC_TRIGGER_SPELL_WITH_VALUE",
        ChangeModelForAllHumanoids = 233 => "CHANGE_MODEL_FOR_ALL_HUMANOIDS",
        ModSpellDamageOfAttackPower = 237 => "MOD_SPELL_DAMAGE_OF_ATTACK_POWER",
        ModExpertise = 240 => "MOD_EXPERTISE",
        ModSpellDamageFromHealing = 242 => "MOD_SPELL_DAMAGE_FROM_HEALING",
        ModIncreaseHealth2 = 250 => "MOD_INCREASE_HEALTH_2",
        ModEnemyDodge = 251 => "MOD_ENEMY_DODGE",
        ModBlockCritChance = 253 => "MOD_BLOCK_CRIT_CHANCE",
        ModMechanicDamageTakenPercent = 255 => "MOD_MECHANIC_DAMAGE_TAKEN_PERCENT",
        ModTargetResistBySpellClass = 257 => "MOD_TARGET_RESIST_BY_SPELL_CLASS",
        ModArmorPctFromStat = 268 => "MOD_ARMOR_PCT_FROM_STAT",
        ModIgnoreTargetResist = 269 => "MOD_IGNORE_TARGET_RESIST",
        ModSchoolMaskDamageFromCaster = 270 => "MOD_SCHOOL_MASK_DAMAGE_FROM_CASTER",
        ModSpellDamageFromCaster = 271 => "MOD_SPELL_DAMAGE_FROM_CASTER",
        ModBlockValuePct = 272 => "MOD_BLOCK_VALUE_PCT",
        ModBlockValueFlat = 274 => "MOD_BLOCK_VALUE_FLAT",
        ModIgnoreShapeshift = 275 => "MOD_IGNORE_SHAPESHIFT",
        /// Armor penetration % (Weaponmaster maces).
        ModArmorPenetrationPct = 280 => "MOD_ARMOR_PENETRATION_PCT",
        ModBaseHealthPct = 282 => "MOD_BASE_HEALTH_PCT",
        /// Crit chance % for spells and attacks (Berserker Stance, Recklessness).
        ModCritPct = 290 => "MOD_CRIT_PCT",
        SetPowerType = 296 => "SET_POWER_TYPE",
        TriggerSpellOnPowerPct = 297 => "TRIGGER_SPELL_ON_POWER_PCT",
        ModDamageDoneVersusAurastate = 303 => "MOD_DAMAGE_DONE_VERSUS_AURASTATE",
        ModCritChanceForCaster = 306 => "MOD_CRIT_CHANCE_FOR_CASTER",
        ModCritChanceForCasterWithAbilities = 308 => "MOD_CRIT_CHANCE_FOR_CASTER_WITH_ABILITIES",
        ModSpellPowerPct = 317 => "MOD_SPELL_POWER_PCT",
        /// Melee attack speed % (Flurry, Berserking).
        ModMeleeHaste3 = 319 => "MOD_MELEE_HASTE_3",
        ModRangedHaste2 = 320 => "MOD_RANGED_HASTE_2",
        TriggerSpellOnPowerAmount = 328 => "TRIGGER_SPELL_ON_POWER_AMOUNT",
        ModPowerGainPct = 329 => "MOD_POWER_GAIN_PCT",
        /// Replaces the action-bar spell misc with the spell in base points (runes, Improved Slam).
        OverrideActionbarSpells = 332 => "OVERRIDE_ACTIONBAR_SPELLS",
        OverrideActionbarSpellsTriggered = 333 => "OVERRIDE_ACTIONBAR_SPELLS_TRIGGERED",
        ModAutoattackRange = 334 => "MOD_AUTOATTACK_RANGE",
        ModCritChanceForCasterPet = 339 => "MOD_CRIT_CHANCE_FOR_CASTER_PET",
        ModSpellCategoryCooldown = 341 => "MOD_SPELL_CATEGORY_COOLDOWN",
        ModMeleeRangedHaste2 = 342 => "MOD_MELEE_RANGED_HASTE_2",
        ModAutoattackDamage = 344 => "MOD_AUTOATTACK_DAMAGE",
        EnableAltPower = 346 => "ENABLE_ALT_POWER",
        ModSpellCooldownByHaste = 347 => "MOD_SPELL_COOLDOWN_BY_HASTE",
        /// Max power, misc = power type (Boundless Rage); retail name unverified.
        ModMaxPower = 418 => "MOD_MAX_POWER",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_round_trip() {
        for aura in AuraType::KNOWN {
            assert_eq!(AuraType::from_id(aura.id()), *aura);
            assert_eq!(AuraType::from_name(aura.name().unwrap()), Some(*aura));
            assert_eq!(aura.to_string().parse::<AuraType>().unwrap(), *aura);
        }
        let ids: Vec<u32> = AuraType::KNOWN.iter().map(|a| a.id()).collect();
        assert!(
            ids.windows(2).all(|w| w[0] < w[1]),
            "ids are unique and ascending"
        );
    }

    #[test]
    fn values_seen_on_player_spells_have_the_documented_names() {
        let seen = [
            (3, "PERIODIC_DAMAGE"),
            (4, "DUMMY"),
            (10, "MOD_THREAT"),
            (13, "MOD_DAMAGE_DONE"),
            (22, "MOD_RESISTANCE"),
            (23, "PERIODIC_TRIGGER_SPELL"),
            (24, "PERIODIC_ENERGIZE"),
            (29, "MOD_STAT"),
            (30, "MOD_SKILL"),
            (36, "MOD_SHAPESHIFT"),
            (42, "PROC_TRIGGER_SPELL"),
            (52, "MOD_WEAPON_CRIT_PERCENT"),
            (54, "MOD_HIT_CHANCE"),
            (55, "MOD_SPELL_HIT_CHANCE"),
            (65, "MOD_CASTING_SPEED_NOT_STACK"),
            (79, "MOD_DAMAGE_PERCENT_DONE"),
            (85, "MOD_POWER_REGEN"),
            (87, "MOD_DAMAGE_PERCENT_TAKEN"),
            (99, "MOD_ATTACK_POWER"),
            (107, "ADD_FLAT_MODIFIER"),
            (108, "ADD_PCT_MODIFIER"),
            (122, "MOD_OFFHAND_DAMAGE_PCT"),
            (124, "MOD_RANGED_ATTACK_POWER"),
            (137, "MOD_TOTAL_STAT_PERCENTAGE"),
            (142, "MOD_BASE_RESISTANCE_PCT"),
            (166, "MOD_ATTACK_POWER_PCT"),
            (226, "PERIODIC_DUMMY"),
            (280, "MOD_ARMOR_PENETRATION_PCT"),
            (290, "MOD_CRIT_PCT"),
            (319, "MOD_MELEE_HASTE_3"),
            (332, "OVERRIDE_ACTIONBAR_SPELLS"),
            (418, "MOD_MAX_POWER"),
        ];
        for (id, name) in seen {
            assert_eq!(AuraType::from_id(id).name(), Some(name), "{id}");
        }
        // Toughness' second effect: no retail name known, loads as unknown.
        assert_eq!(AuraType::from_id(466), AuraType::Unknown(466));
        assert_eq!(AuraType::Unknown(466).to_string(), "UNKNOWN_466");
    }

    #[test]
    fn serde_accepts_both_forms() {
        assert_eq!(
            serde_yaml::from_str::<AuraType>("PROC_TRIGGER_SPELL").unwrap(),
            AuraType::ProcTriggerSpell
        );
        assert_eq!(
            serde_yaml::from_str::<AuraType>("42").unwrap(),
            AuraType::ProcTriggerSpell
        );
        assert_eq!(
            serde_yaml::from_str::<AuraType>("466").unwrap(),
            AuraType::Unknown(466)
        );
        assert_eq!(
            serde_yaml::to_string(&AuraType::Unknown(466))
                .unwrap()
                .trim(),
            "466"
        );
    }
}
