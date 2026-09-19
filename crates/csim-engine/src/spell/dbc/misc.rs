//! The smaller table enums: spell modifier operations, implicit targets, hit tables, power
//! types, mechanics, aura states and shapeshift forms.

use super::dbc_enum;

dbc_enum! {
    /// Retail `SpellModOp`: `EffectMiscValue_0` of `ADD_FLAT_MODIFIER` / `ADD_PCT_MODIFIER` auras.
    SpellModOp: u32 {
        /// Damage / healing % (Improved Revenge).
        HealingAndDamage = 0 => "HEALING_AND_DAMAGE",
        /// Aura duration.
        Duration = 1 => "DURATION",
        /// Threat.
        Threat = 2 => "THREAT",
        /// Effect 1 value.
        PointsIndex0 = 3 => "POINTS_INDEX_0",
        ProcCharges = 4 => "PROC_CHARGES",
        Range = 5 => "RANGE",
        Radius = 6 => "RADIUS",
        /// Crit chance (Improved Overpower).
        CritChance = 7 => "CRIT_CHANCE",
        /// Every effect value (Improved Bloodrage, Improved Charge).
        Points = 8 => "POINTS",
        ResistPushback = 9 => "RESIST_PUSHBACK",
        /// Cast time in ms (Improved Slam).
        ChangeCastTime = 10 => "CHANGE_CAST_TIME",
        /// Cooldown in ms (Improved Intercept).
        Cooldown = 11 => "COOLDOWN",
        /// Effect 2 value.
        PointsIndex1 = 12 => "POINTS_INDEX_1",
        ResistDispelChance = 13 => "RESIST_DISPEL_CHANCE",
        /// Resource cost, raw units (Improved Heroic Strike −10 = 1 rage).
        PowerCost0 = 14 => "POWER_COST_0",
        /// Crit damage bonus % (Impale).
        CritDamageAndHealing = 15 => "CRIT_DAMAGE_AND_HEALING",
        HitChance = 16 => "HIT_CHANCE",
        ChainTargets = 17 => "CHAIN_TARGETS",
        ProcChance = 18 => "PROC_CHANCE",
        Period = 19 => "PERIOD",
        ChainAmplitude = 20 => "CHAIN_AMPLITUDE",
        /// Global cooldown in ms (Improved Slam).
        StartCooldown = 21 => "START_COOLDOWN",
        /// Periodic damage / healing % (Improved Rend).
        PeriodicHealingAndDamage = 22 => "PERIODIC_HEALING_AND_DAMAGE",
        /// Effect 3 value.
        PointsIndex2 = 23 => "POINTS_INDEX_2",
        BonusCoefficient = 24 => "BONUS_COEFFICIENT",
        TriggerDamage = 25 => "TRIGGER_DAMAGE",
        /// Procs per minute.
        ProcFrequency = 26 => "PROC_FREQUENCY",
        Amplitude = 27 => "AMPLITUDE",
        DispelResistance = 28 => "DISPEL_RESISTANCE",
        CrowdDamage = 29 => "CROWD_DAMAGE",
        PowerCostPct = 30 => "POWER_COST_PCT",
        PointsIndex3 = 32 => "POINTS_INDEX_3",
        PointsIndex4 = 33 => "POINTS_INDEX_4",
        PowerCost1 = 34 => "POWER_COST_1",
        ChainJumpDistance = 35 => "CHAIN_JUMP_DISTANCE",
        AreaTriggerMaxSummons = 36 => "AREA_TRIGGER_MAX_SUMMONS",
        MaxAuraStacks = 37 => "MAX_AURA_STACKS",
        ProcCooldown = 38 => "PROC_COOLDOWN",
        PowerCost2 = 39 => "POWER_COST_2",
    }
}

dbc_enum! {
    /// Retail `Targets`: `ImplicitTarget_0/1` of `SpellEffect`.
    ImplicitTarget: u32 {
        None = 0 => "NONE",
        /// The caster (self buffs, passives).
        UnitCaster = 1 => "UNIT_CASTER",
        UnitNearbyEnemy = 2 => "UNIT_NEARBY_ENEMY",
        UnitNearbyAlly = 3 => "UNIT_NEARBY_ALLY",
        UnitNearbyParty = 4 => "UNIT_NEARBY_PARTY",
        UnitPet = 5 => "UNIT_PET",
        /// The enemy target (attacks, debuffs).
        UnitTargetEnemy = 6 => "UNIT_TARGET_ENEMY",
        UnitSrcAreaEntry = 7 => "UNIT_SRC_AREA_ENTRY",
        UnitDestAreaEntry = 8 => "UNIT_DEST_AREA_ENTRY",
        DestHome = 9 => "DEST_HOME",
        UnitSrcAreaUnk11 = 11 => "UNIT_SRC_AREA_UNK_11",
        /// Enemies around the caster (Whirlwind with `SRC_CASTER`).
        UnitSrcAreaEnemy = 15 => "UNIT_SRC_AREA_ENEMY",
        UnitDestAreaEnemy = 16 => "UNIT_DEST_AREA_ENEMY",
        DestDb = 17 => "DEST_DB",
        DestCaster = 18 => "DEST_CASTER",
        /// The caster's party within `EffectRadiusIndex` (Battle Shout).
        UnitCasterAreaParty = 20 => "UNIT_CASTER_AREA_PARTY",
        /// A friendly target.
        UnitTargetAlly = 21 => "UNIT_TARGET_ALLY",
        /// The caster's position (area spells' first target).
        SrcCaster = 22 => "SRC_CASTER",
        GameobjectTarget = 23 => "GAMEOBJECT_TARGET",
        UnitConeEnemy24 = 24 => "UNIT_CONE_ENEMY_24",
        UnitTargetAny = 25 => "UNIT_TARGET_ANY",
        GameobjectItemTarget = 26 => "GAMEOBJECT_ITEM_TARGET",
        UnitMaster = 27 => "UNIT_MASTER",
        DestDynobjEnemy = 28 => "DEST_DYNOBJ_ENEMY",
        DestDynobjAlly = 29 => "DEST_DYNOBJ_ALLY",
        UnitSrcAreaAlly = 30 => "UNIT_SRC_AREA_ALLY",
        UnitDestAreaAlly = 31 => "UNIT_DEST_AREA_ALLY",
        DestCasterSummon = 32 => "DEST_CASTER_SUMMON",
        UnitSrcAreaParty = 33 => "UNIT_SRC_AREA_PARTY",
        UnitDestAreaParty = 34 => "UNIT_DEST_AREA_PARTY",
        UnitTargetParty = 35 => "UNIT_TARGET_PARTY",
        DestCasterUnk36 = 36 => "DEST_CASTER_UNK_36",
        UnitLasttargetAreaParty = 37 => "UNIT_LASTTARGET_AREA_PARTY",
        UnitNearbyEntry = 38 => "UNIT_NEARBY_ENTRY",
        DestCasterFishing = 39 => "DEST_CASTER_FISHING",
        GameobjectNearbyEntry = 40 => "GAMEOBJECT_NEARBY_ENTRY",
        DestCasterFrontRight = 41 => "DEST_CASTER_FRONT_RIGHT",
        DestCasterBackRight = 42 => "DEST_CASTER_BACK_RIGHT",
        DestCasterBackLeft = 43 => "DEST_CASTER_BACK_LEFT",
        DestCasterFrontLeft = 44 => "DEST_CASTER_FRONT_LEFT",
        UnitTargetChainhealAlly = 45 => "UNIT_TARGET_CHAINHEAL_ALLY",
        DestNearbyEntry = 46 => "DEST_NEARBY_ENTRY",
        DestCasterFront = 47 => "DEST_CASTER_FRONT",
        DestCasterBack = 48 => "DEST_CASTER_BACK",
        DestCasterRight = 49 => "DEST_CASTER_RIGHT",
        DestCasterLeft = 50 => "DEST_CASTER_LEFT",
        GameobjectSrcArea = 51 => "GAMEOBJECT_SRC_AREA",
        GameobjectDestArea = 52 => "GAMEOBJECT_DEST_AREA",
        DestTargetEnemy = 53 => "DEST_TARGET_ENEMY",
        UnitCone180DegEnemy = 54 => "UNIT_CONE_180_DEG_ENEMY",
        DestCasterFrontLeap = 55 => "DEST_CASTER_FRONT_LEAP",
        /// The caster's raid (Rallying Cry).
        UnitCasterAreaRaid = 56 => "UNIT_CASTER_AREA_RAID",
        UnitTargetRaid = 57 => "UNIT_TARGET_RAID",
        UnitNearbyRaid = 58 => "UNIT_NEARBY_RAID",
        UnitConeAlly = 59 => "UNIT_CONE_ALLY",
        UnitConeEntry = 60 => "UNIT_CONE_ENTRY",
        UnitTargetAreaRaidClass = 61 => "UNIT_TARGET_AREA_RAID_CLASS",
        DestCasterGround = 62 => "DEST_CASTER_GROUND",
        DestTargetAny = 63 => "DEST_TARGET_ANY",
        DestTargetFront = 64 => "DEST_TARGET_FRONT",
        DestTargetBack = 65 => "DEST_TARGET_BACK",
        DestTargetRight = 66 => "DEST_TARGET_RIGHT",
        DestTargetLeft = 67 => "DEST_TARGET_LEFT",
        DestTargetFrontRight = 68 => "DEST_TARGET_FRONT_RIGHT",
        DestTargetBackRight = 69 => "DEST_TARGET_BACK_RIGHT",
        DestTargetBackLeft = 70 => "DEST_TARGET_BACK_LEFT",
        DestTargetFrontLeft = 71 => "DEST_TARGET_FRONT_LEFT",
        DestCasterRandom = 72 => "DEST_CASTER_RANDOM",
        DestCasterRadius = 73 => "DEST_CASTER_RADIUS",
        DestTargetRandom = 74 => "DEST_TARGET_RANDOM",
        DestTargetRadius = 75 => "DEST_TARGET_RADIUS",
        DestChannelTarget = 76 => "DEST_CHANNEL_TARGET",
        UnitChannelTarget = 77 => "UNIT_CHANNEL_TARGET",
        DestDestFront = 78 => "DEST_DEST_FRONT",
        DestDestBack = 79 => "DEST_DEST_BACK",
        DestDestRight = 80 => "DEST_DEST_RIGHT",
        DestDestLeft = 81 => "DEST_DEST_LEFT",
        DestDestFrontRight = 82 => "DEST_DEST_FRONT_RIGHT",
        DestDestBackRight = 83 => "DEST_DEST_BACK_RIGHT",
        DestDestBackLeft = 84 => "DEST_DEST_BACK_LEFT",
        DestDestFrontLeft = 85 => "DEST_DEST_FRONT_LEFT",
        DestDestRandom = 86 => "DEST_DEST_RANDOM",
        DestDest = 87 => "DEST_DEST",
        DestDynobjNone = 88 => "DEST_DYNOBJ_NONE",
        DestTraj = 89 => "DEST_TRAJ",
        UnitTargetMinipet = 90 => "UNIT_TARGET_MINIPET",
        DestDestRadius = 91 => "DEST_DEST_RADIUS",
        UnitSummoner = 92 => "UNIT_SUMMONER",
        CorpseSrcAreaEnemy = 93 => "CORPSE_SRC_AREA_ENEMY",
        UnitVehicle = 94 => "UNIT_VEHICLE",
        UnitTargetPassenger = 95 => "UNIT_TARGET_PASSENGER",
        UnitConeCasterToDestEnemy = 104 => "UNIT_CONE_CASTER_TO_DEST_ENEMY",
    }
}

dbc_enum! {
    /// `SpellCategories.DefenseType`: which hit table the spell rolls on.
    DefenseType: u32 {
        None = 0 => "NONE",
        Magic = 1 => "MAGIC",
        Melee = 2 => "MELEE",
        Ranged = 3 => "RANGED",
    }
}

dbc_enum! {
    /// Retail `Powers`: `SpellPower.PowerType`, `EffectMiscValue_0` of `ENERGIZE` and
    /// `PERIODIC_ENERGIZE`.
    PowerType: i32 {
        /// Costs health (Bloodrage: `PowerCostPct` 20).
        Health = -2 => "HEALTH",
        Mana = 0 => "MANA",
        /// Stored in tenths (`PowerType.DisplayModifier` 10).
        Rage = 1 => "RAGE",
        Focus = 2 => "FOCUS",
        Energy = 3 => "ENERGY",
        /// Forever gives Warriors combo points (Overpower costs 1).
        ComboPoints = 4 => "COMBO_POINTS",
        Runes = 5 => "RUNES",
        RunicPower = 6 => "RUNIC_POWER",
        SoulShards = 7 => "SOUL_SHARDS",
        LunarPower = 8 => "LUNAR_POWER",
        HolyPower = 9 => "HOLY_POWER",
        AlternatePower = 10 => "ALTERNATE_POWER",
        Maelstrom = 11 => "MAELSTROM",
        Chi = 12 => "CHI",
        Insanity = 13 => "INSANITY",
        ArcaneCharges = 16 => "ARCANE_CHARGES",
        Fury = 17 => "FURY",
        Pain = 18 => "PAIN",
        Essence = 19 => "ESSENCE",
        Happiness = 27 => "HAPPINESS",
    }
}

dbc_enum! {
    /// Retail `Mechanics`: `SpellCategories.Mechanic` and `SpellEffect.EffectMechanic`.
    Mechanic: u32 {
        None = 0 => "NONE",
        Charm = 1 => "CHARM",
        Disoriented = 2 => "DISORIENTED",
        Disarm = 3 => "DISARM",
        Distract = 4 => "DISTRACT",
        Fear = 5 => "FEAR",
        Grip = 6 => "GRIP",
        Root = 7 => "ROOT",
        SlowAttack = 8 => "SLOW_ATTACK",
        Silence = 9 => "SILENCE",
        Sleep = 10 => "SLEEP",
        Snare = 11 => "SNARE",
        Stun = 12 => "STUN",
        Freeze = 13 => "FREEZE",
        Knockout = 14 => "KNOCKOUT",
        Bleed = 15 => "BLEED",
        Bandage = 16 => "BANDAGE",
        Polymorph = 17 => "POLYMORPH",
        Banish = 18 => "BANISH",
        Shield = 19 => "SHIELD",
        Shackle = 20 => "SHACKLE",
        Mount = 21 => "MOUNT",
        Infected = 22 => "INFECTED",
        Turn = 23 => "TURN",
        Horror = 24 => "HORROR",
        Invulnerability = 25 => "INVULNERABILITY",
        Interrupt = 26 => "INTERRUPT",
        Daze = 27 => "DAZE",
        Discovery = 28 => "DISCOVERY",
        ImmuneShield = 29 => "IMMUNE_SHIELD",
        Sapped = 30 => "SAPPED",
        Enraged = 31 => "ENRAGED",
        Wounded = 32 => "WOUNDED",
        Taunted = 36 => "TAUNTED",
    }
}

dbc_enum! {
    /// Retail `AuraStateType`: `SpellAuraRestrictions.Caster/TargetAuraState`.
    AuraState: u32 {
        None = 0 => "NONE",
        /// Caster dodged, parried or blocked recently (Revenge).
        Defensive = 1 => "DEFENSIVE",
        /// Target below 20 % health (Execute).
        Wounded20Percent = 2 => "WOUNDED_20_PERCENT",
        Unbalanced = 3 => "UNBALANCED",
        Frozen = 4 => "FROZEN",
        Marked = 5 => "MARKED",
        Wounded25Percent = 6 => "WOUNDED_25_PERCENT",
        Defensive2 = 7 => "DEFENSIVE_2",
        Banished = 8 => "BANISHED",
        Dazed = 9 => "DAZED",
        /// Caster just killed something (Victory Rush).
        Victorious = 10 => "VICTORIOUS",
        Rampage = 11 => "RAMPAGE",
        FaerieFire = 12 => "FAERIE_FIRE",
        Wounded35Percent = 13 => "WOUNDED_35_PERCENT",
        RaidEncounter2 = 14 => "RAID_ENCOUNTER_2",
        DruidPeriodicHeal = 15 => "DRUID_PERIODIC_HEAL",
        RoguePoisoned = 16 => "ROGUE_POISONED",
        /// Caster is enraged (Raging Blow, Enraged Regeneration).
        Enraged = 17 => "ENRAGED",
        Bleed = 18 => "BLEED",
        Vulnerable = 19 => "VULNERABLE",
        ArenaPreparation = 20 => "ARENA_PREPARATION",
        WoundHealth2080 = 21 => "WOUND_HEALTH_20_80",
        RaidEncounter = 22 => "RAID_ENCOUNTER",
        Healthy75Percent = 23 => "HEALTHY_75_PERCENT",
        WoundHealth3580 = 24 => "WOUND_HEALTH_35_80",
    }
}

dbc_enum! {
    /// `SpellShapeshiftForm.ID`: `EffectMiscValue_0` of `MOD_SHAPESHIFT`; bit `1 << (id - 1)` of
    /// `SpellShapeshift.ShapeshiftMask_0`.
    ShapeshiftForm: u32 {
        None = 0 => "NONE",
        CatForm = 1 => "CAT_FORM",
        TreeOfLife = 2 => "TREE_OF_LIFE",
        TravelForm = 3 => "TRAVEL_FORM",
        AquaticForm = 4 => "AQUATIC_FORM",
        BearForm = 5 => "BEAR_FORM",
        Ambient = 7 => "AMBIENT",
        DireBearForm = 8 => "DIRE_BEAR_FORM",
        CreatureBear = 14 => "CREATURE_BEAR",
        GhostWolf = 16 => "GHOST_WOLF",
        BattleStance = 17 => "BATTLE_STANCE",
        DefensiveStance = 18 => "DEFENSIVE_STANCE",
        BerserkerStance = 19 => "BERSERKER_STANCE",
        SwiftFlightForm = 27 => "SWIFT_FLIGHT_FORM",
        Shadowform = 28 => "SHADOWFORM",
        FlightForm = 29 => "FLIGHT_FORM",
        Stealth = 30 => "STEALTH",
        MoonkinForm = 31 => "MOONKIN_FORM",
        SpiritOfRedemption = 32 => "SPIRIT_OF_REDEMPTION",
    }
}

impl PowerType {
    /// `PowerType.DisplayModifier`: stored amounts are this many times the displayed amount
    /// (rage costs and gains are in tenths).
    pub const fn display_modifier(self) -> i32 {
        match self {
            Self::Rage => 10,
            _ => 1,
        }
    }
}

impl ShapeshiftForm {
    /// The bit of this form in `SpellShapeshift.ShapeshiftMask_0` (`None` for `NONE` / unknown
    /// forms beyond 32).
    pub const fn mask_bit(self) -> Option<u32> {
        match self.id() {
            0 => None,
            id if id <= 32 => Some(1 << (id - 1)),
            _ => None,
        }
    }

    /// The forms whose bits are set in a `ShapeshiftMask_0` word, ascending.
    pub fn from_mask(mask: u32) -> Vec<Self> {
        (1..=32u32)
            .filter(|id| mask & (1 << (id - 1)) != 0)
            .map(Self::from_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_mod_ops() {
        assert_eq!(SpellModOp::from_id(14), SpellModOp::PowerCost0);
        assert_eq!(SpellModOp::from_id(15), SpellModOp::CritDamageAndHealing);
        assert_eq!(
            SpellModOp::from_id(22),
            SpellModOp::PeriodicHealingAndDamage
        );
        assert_eq!(SpellModOp::from_id(21), SpellModOp::StartCooldown);
        assert_eq!(SpellModOp::from_id(31), SpellModOp::Unknown(31));
        let ids: Vec<u32> = SpellModOp::KNOWN.iter().map(|m| m.id()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn documented_targets() {
        assert_eq!(ImplicitTarget::from_id(1), ImplicitTarget::UnitCaster);
        assert_eq!(ImplicitTarget::from_id(6), ImplicitTarget::UnitTargetEnemy);
        assert_eq!(
            ImplicitTarget::from_id(20),
            ImplicitTarget::UnitCasterAreaParty
        );
        assert_eq!(ImplicitTarget::from_id(22), ImplicitTarget::SrcCaster);
        assert_eq!(
            ImplicitTarget::from_id(15),
            ImplicitTarget::UnitSrcAreaEnemy
        );
        assert_eq!(
            ImplicitTarget::from_id(56),
            ImplicitTarget::UnitCasterAreaRaid
        );
        let ids: Vec<u32> = ImplicitTarget::KNOWN.iter().map(|m| m.id()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn defense_types_and_mechanics() {
        assert_eq!(DefenseType::from_id(2), DefenseType::Melee);
        assert_eq!(DefenseType::from_id(2).name(), Some("MELEE"));
        assert_eq!(Mechanic::from_id(15), Mechanic::Bleed);
        assert_eq!(Mechanic::from_id(12), Mechanic::Stun);
        assert_eq!(Mechanic::from_id(26), Mechanic::Interrupt);
        assert_eq!(AuraState::from_id(1), AuraState::Defensive);
        assert_eq!(AuraState::from_id(2), AuraState::Wounded20Percent);
        assert_eq!(AuraState::from_id(17), AuraState::Enraged);
        assert_eq!(AuraState::from_id(10), AuraState::Victorious);
    }

    #[test]
    fn power_types_are_signed() {
        assert_eq!(PowerType::from_id(-2), PowerType::Health);
        assert_eq!(PowerType::from_id(1), PowerType::Rage);
        assert_eq!(PowerType::from_id(4), PowerType::ComboPoints);
        assert_eq!(PowerType::Health.id(), -2);
        assert_eq!(
            serde_yaml::from_str::<PowerType>("-2").unwrap(),
            PowerType::Health
        );
        assert_eq!(
            serde_yaml::from_str::<PowerType>("RAGE").unwrap(),
            PowerType::Rage
        );
        assert_eq!(
            serde_yaml::to_string(&PowerType::Health).unwrap().trim(),
            "HEALTH"
        );
        assert_eq!(
            serde_yaml::from_str::<PowerType>("99").unwrap(),
            PowerType::Unknown(99)
        );
        assert!(serde_yaml::from_str::<PowerType>("4294967296").is_err());
    }

    #[test]
    fn shapeshift_forms_map_to_mask_bits() {
        assert_eq!(ShapeshiftForm::from_id(17), ShapeshiftForm::BattleStance);
        assert_eq!(ShapeshiftForm::BattleStance.mask_bit(), Some(65536));
        assert_eq!(ShapeshiftForm::DefensiveStance.mask_bit(), Some(131072));
        assert_eq!(ShapeshiftForm::BerserkerStance.mask_bit(), Some(262144));
        assert_eq!(ShapeshiftForm::None.mask_bit(), None);
        assert_eq!(ShapeshiftForm::Unknown(40).mask_bit(), None);
        assert_eq!(
            ShapeshiftForm::from_mask(327680),
            [
                ShapeshiftForm::BattleStance,
                ShapeshiftForm::BerserkerStance
            ]
        );
        assert_eq!(
            ShapeshiftForm::from_mask(196608),
            [
                ShapeshiftForm::BattleStance,
                ShapeshiftForm::DefensiveStance
            ]
        );
        assert_eq!(
            ShapeshiftForm::from_mask(1 << 25),
            [ShapeshiftForm::Unknown(26)]
        );
        assert!(ShapeshiftForm::from_mask(0).is_empty());
    }
}
