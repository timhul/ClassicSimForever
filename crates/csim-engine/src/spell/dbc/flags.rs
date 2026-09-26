//! Bit-flag columns: `SpellMisc.Attributes_0` to `Attributes_3`,
//! `SpellAuraOptions.ProcTypeMask_0` and `SpellMisc.SchoolMask`.

use super::dbc_flags;

dbc_flags! {
    /// Retail `SpellAttr0`, the `Attributes_0` word of `SpellMisc`.
    ///
    /// `Attributes_2..16` are the retail `SpellAttr2..16` words; the engine keeps them as raw
    /// numbers until a specific bit is needed.
    SpellAttr0 {
        PROC_FAILURE_BURNS_CHARGE = 0x0000_0001 => "PROC_FAILURE_BURNS_CHARGE",
        USES_RANGED_SLOT = 0x0000_0002 => "USES_RANGED_SLOT",
        /// Classic-style on-next-swing attack (Heroic Strike, Cleave).
        ON_NEXT_SWING_NO_DAMAGE = 0x0000_0004 => "ON_NEXT_SWING_NO_DAMAGE",
        DO_NOT_LOG_IMMUNE_MISSES = 0x0000_0008 => "DO_NOT_LOG_IMMUNE_MISSES",
        /// Uses weapon / melee rules.
        IS_ABILITY = 0x0000_0010 => "IS_ABILITY",
        IS_TRADESKILL = 0x0000_0020 => "IS_TRADESKILL",
        /// Passive aura (talents, stance passives, procs).
        PASSIVE = 0x0000_0040 => "PASSIVE",
        /// Hidden from the UI (marker buffs, proc payloads).
        DO_NOT_DISPLAY = 0x0000_0080 => "DO_NOT_DISPLAY",
        DO_NOT_LOG = 0x0000_0100 => "DO_NOT_LOG",
        HELD_ITEM_ONLY = 0x0000_0200 => "HELD_ITEM_ONLY",
        ON_NEXT_SWING = 0x0000_0400 => "ON_NEXT_SWING",
        WEARER_CASTS_PROC_TRIGGER = 0x0000_0800 => "WEARER_CASTS_PROC_TRIGGER",
        SERVER_ONLY = 0x0000_1000 => "SERVER_ONLY",
        ALLOW_ITEM_SPELL_IN_PVP = 0x0000_2000 => "ALLOW_ITEM_SPELL_IN_PVP",
        ONLY_INDOORS = 0x0000_4000 => "ONLY_INDOORS",
        ONLY_OUTDOORS = 0x0000_8000 => "ONLY_OUTDOORS",
        NOT_SHAPESHIFTED = 0x0001_0000 => "NOT_SHAPESHIFTED",
        ONLY_STEALTHED = 0x0002_0000 => "ONLY_STEALTHED",
        DO_NOT_SHEATH = 0x0004_0000 => "DO_NOT_SHEATH",
        SCALES_WITH_CREATURE_LEVEL = 0x0008_0000 => "SCALES_WITH_CREATURE_LEVEL",
        CANCELS_AUTO_ATTACK_COMBAT = 0x0010_0000 => "CANCELS_AUTO_ATTACK_COMBAT",
        /// Cannot be dodged, parried or blocked (Overpower).
        NO_ACTIVE_DEFENSE = 0x0020_0000 => "NO_ACTIVE_DEFENSE",
        TRACK_TARGET_IN_CAST_PLAYER_ONLY = 0x0040_0000 => "TRACK_TARGET_IN_CAST_PLAYER_ONLY",
        ALLOW_CAST_WHILE_DEAD = 0x0080_0000 => "ALLOW_CAST_WHILE_DEAD",
        ALLOW_WHILE_MOUNTED = 0x0100_0000 => "ALLOW_WHILE_MOUNTED",
        COOLDOWN_ON_EVENT = 0x0200_0000 => "COOLDOWN_ON_EVENT",
        /// The aura is shown as a debuff even on the caster (Death Wish).
        AURA_IS_DEBUFF = 0x0400_0000 => "AURA_IS_DEBUFF",
        ALLOW_WHILE_SITTING = 0x0800_0000 => "ALLOW_WHILE_SITTING",
        NOT_IN_COMBAT_ONLY_PEACEFUL = 0x1000_0000 => "NOT_IN_COMBAT_ONLY_PEACEFUL",
        NO_IMMUNITIES = 0x2000_0000 => "NO_IMMUNITIES",
        HEARTBEAT_RESIST = 0x4000_0000 => "HEARTBEAT_RESIST",
        NO_AURA_CANCEL = 0x8000_0000 => "NO_AURA_CANCEL",
    }
}

dbc_flags! {
    /// Retail `SpellAttr1`, the `Attributes_1` word of `SpellMisc`. Only the bits the engine
    /// reads are named.
    SpellAttr1 {
        /// Most of the cost is refunded when the attack is missed, dodged or parried (the
        /// single-target warrior attacks; not Whirlwind, Cleave or Thunder Clap).
        DISCOUNT_POWER_ON_MISS = 0x0800_0000 => "DISCOUNT_POWER_ON_MISS",
    }
}

dbc_flags! {
    /// Retail `SpellAttr2`, the `Attributes_2` word of `SpellMisc`. Only the bits the engine
    /// reads are named.
    SpellAttr2 {
        /// The caster must be behind the target (Backstab, Garrote, Ambush).
        BEHIND_TARGET = 0x0010_0000 => "BEHIND_TARGET",
    }
}

dbc_flags! {
    /// Retail `SpellAttr3`, the `Attributes_3` word of `SpellMisc`. Only the bits the engine
    /// reads are named.
    SpellAttr3 {
        /// The main-hand weapon must meet the `SpellEquippedItems` requirement (Backstab: a
        /// dagger in the main hand, not just anywhere).
        MAIN_HAND = 0x0000_0400 => "MAIN_HAND",
        /// An off-hand weapon meeting the requirement is needed, and the spell's weapon damage
        /// is the off hand's (Mutilate's off-hand strike).
        REQUIRES_OFF_HAND_WEAPON = 0x0100_0000 => "REQUIRES_OFF_HAND_WEAPON",
    }
}

dbc_flags! {
    /// Retail `ProcFlags`, the `ProcTypeMask_0` word of `SpellAuraOptions`: which events a
    /// proc aura (or a charge-consuming buff) reacts to. `DEAL_*` = done by the aura's owner,
    /// `TAKE_*` = suffered by the owner.
    ProcFlags {
        KILLED = 0x0000_0001 => "KILLED",
        KILL = 0x0000_0002 => "KILL",
        /// Melee auto attack done (Unbridled Wrath, Flurry charge use).
        DEAL_MELEE_SWING = 0x0000_0004 => "DEAL_MELEE_SWING",
        TAKE_MELEE_SWING = 0x0000_0008 => "TAKE_MELEE_SWING",
        /// Melee ability done (Sweeping Strikes).
        DEAL_MELEE_ABILITY = 0x0000_0010 => "DEAL_MELEE_ABILITY",
        TAKE_MELEE_ABILITY = 0x0000_0020 => "TAKE_MELEE_ABILITY",
        DEAL_RANGED_ATTACK = 0x0000_0040 => "DEAL_RANGED_ATTACK",
        TAKE_RANGED_ATTACK = 0x0000_0080 => "TAKE_RANGED_ATTACK",
        DEAL_RANGED_ABILITY = 0x0000_0100 => "DEAL_RANGED_ABILITY",
        TAKE_RANGED_ABILITY = 0x0000_0200 => "TAKE_RANGED_ABILITY",
        DEAL_HELPFUL_ABILITY = 0x0000_0400 => "DEAL_HELPFUL_ABILITY",
        TAKE_HELPFUL_ABILITY = 0x0000_0800 => "TAKE_HELPFUL_ABILITY",
        DEAL_HARMFUL_ABILITY = 0x0000_1000 => "DEAL_HARMFUL_ABILITY",
        TAKE_HARMFUL_ABILITY = 0x0000_2000 => "TAKE_HARMFUL_ABILITY",
        DEAL_HELPFUL_SPELL = 0x0000_4000 => "DEAL_HELPFUL_SPELL",
        TAKE_HELPFUL_SPELL = 0x0000_8000 => "TAKE_HELPFUL_SPELL",
        DEAL_HARMFUL_SPELL = 0x0001_0000 => "DEAL_HARMFUL_SPELL",
        TAKE_HARMFUL_SPELL = 0x0002_0000 => "TAKE_HARMFUL_SPELL",
        DEAL_HARMFUL_PERIODIC = 0x0004_0000 => "DEAL_HARMFUL_PERIODIC",
        TAKE_HARMFUL_PERIODIC = 0x0008_0000 => "TAKE_HARMFUL_PERIODIC",
        TAKE_ANY_DAMAGE = 0x0010_0000 => "TAKE_ANY_DAMAGE",
        DEAL_HELPFUL_PERIODIC = 0x0020_0000 => "DEAL_HELPFUL_PERIODIC",
        /// Restricts `DEAL_MELEE_SWING` to the main hand.
        MAIN_HAND_WEAPON_SWING = 0x0040_0000 => "MAIN_HAND_WEAPON_SWING",
        /// Restricts `DEAL_MELEE_SWING` to the off hand (Dual Wield Specialization).
        OFF_HAND_WEAPON_SWING = 0x0080_0000 => "OFF_HAND_WEAPON_SWING",
        DEATH = 0x0100_0000 => "DEATH",
        JUMP = 0x0200_0000 => "JUMP",
        PROC_CLONE_SPELL = 0x0400_0000 => "PROC_CLONE_SPELL",
        ENTER_COMBAT = 0x0800_0000 => "ENTER_COMBAT",
        ENCOUNTER_START = 0x1000_0000 => "ENCOUNTER_START",
        CAST_ENDED = 0x2000_0000 => "CAST_ENDED",
        LOOTED = 0x4000_0000 => "LOOTED",
        TAKE_HELPFUL_PERIODIC = 0x8000_0000 => "TAKE_HELPFUL_PERIODIC",
    }
}

impl ProcFlags {
    /// Any damaging attack or spell done by the owner (`0x11154`: Deep Wounds, Flurry).
    pub const DEAL_ANY_DAMAGE: Self = Self(0x0001_1154);
    /// Any melee or ranged hit taken (`0x2a8`: Shield Specialization).
    pub const TAKE_ANY_ATTACK: Self = Self(0x0000_02a8);

    /// Whether any `DEAL_*` bit is set.
    pub const fn deals(self) -> bool {
        self.intersects(Self::from_bits(0x0025_5554))
    }

    /// Whether any `TAKE_*` bit is set.
    pub const fn takes(self) -> bool {
        self.intersects(Self::from_bits(0x801a_aaa8))
    }
}

dbc_flags! {
    /// Retail `SpellSchoolMask`, the `SchoolMask` column of `SpellMisc` and the misc value of
    /// school-masked auras (`MOD_DAMAGE_PERCENT_DONE`, `MOD_THREAT`, ...).
    SpellSchoolMask {
        PHYSICAL = 0x01 => "PHYSICAL",
        HOLY = 0x02 => "HOLY",
        FIRE = 0x04 => "FIRE",
        NATURE = 0x08 => "NATURE",
        FROST = 0x10 => "FROST",
        SHADOW = 0x20 => "SHADOW",
        ARCANE = 0x40 => "ARCANE",
    }
}

impl SpellSchoolMask {
    /// Every school (`127`), the misc value stance passives and Death Wish use.
    pub const ALL: Self = Self(0x7f);
    /// Every magic school (everything but physical).
    pub const MAGIC: Self = Self(0x7e);

    /// Whether physical is among the schools.
    pub const fn is_physical(self) -> bool {
        self.intersects(Self::PHYSICAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warrior_attribute_words_decode() {
        let heroic_strike = SpellAttr0::from_bits(0x50014);
        assert!(heroic_strike.contains(SpellAttr0::ON_NEXT_SWING_NO_DAMAGE));
        assert!(heroic_strike.contains(SpellAttr0::IS_ABILITY));
        assert!(!heroic_strike.contains(SpellAttr0::PASSIVE));
        assert_eq!(
            heroic_strike.names(),
            [
                "ON_NEXT_SWING_NO_DAMAGE",
                "IS_ABILITY",
                "NOT_SHAPESHIFTED",
                "DO_NOT_SHEATH"
            ]
        );
        let passive = SpellAttr0::from_bits(0x1d0);
        assert!(passive.contains(SpellAttr0::PASSIVE | SpellAttr0::DO_NOT_DISPLAY));
        let overpower = SpellAttr0::from_bits(0x250010);
        assert!(overpower.contains(SpellAttr0::NO_ACTIVE_DEFENSE));
        let death_wish = SpellAttr0::from_bits(0xc040010);
        assert!(death_wish.contains(SpellAttr0::AURA_IS_DEBUFF));
        assert_eq!(SpellAttr0::from_bits(0x50014).unknown_bits(), 0);
        assert_eq!(SpellAttr0::NAMED.len(), 32);
        assert_eq!(SpellAttr0::from_name("PASSIVE"), Some(SpellAttr0::PASSIVE));
        assert_eq!(
            "PASSIVE | DO_NOT_DISPLAY".parse::<SpellAttr0>().unwrap(),
            SpellAttr0::PASSIVE | SpellAttr0::DO_NOT_DISPLAY
        );
    }

    #[test]
    fn proc_masks_decode() {
        let deep_wounds = ProcFlags::from_bits(0x11154);
        assert_eq!(deep_wounds, ProcFlags::DEAL_ANY_DAMAGE);
        assert_eq!(
            deep_wounds.names(),
            [
                "DEAL_MELEE_SWING",
                "DEAL_MELEE_ABILITY",
                "DEAL_RANGED_ATTACK",
                "DEAL_RANGED_ABILITY",
                "DEAL_HARMFUL_ABILITY",
                "DEAL_HARMFUL_SPELL"
            ]
        );
        assert!(deep_wounds.deals());
        assert!(!deep_wounds.takes());
        let enrage = ProcFlags::from_bits(0x222a8);
        assert!(enrage.takes());
        assert!(!enrage.deals());
        assert!(enrage.contains(ProcFlags::TAKE_ANY_ATTACK));
        let dual_wield = ProcFlags::from_bits(0x800004);
        assert_eq!(
            dual_wield.names(),
            ["DEAL_MELEE_SWING", "OFF_HAND_WEAPON_SWING"]
        );
        assert_eq!(ProcFlags::from_bits(0x4).to_string(), "DEAL_MELEE_SWING");
        assert_eq!(ProcFlags::NAMED.len(), 32);
        assert_eq!(
            serde_yaml::from_str::<ProcFlags>("[DEAL_MELEE_SWING, OFF_HAND_WEAPON_SWING]").unwrap(),
            dual_wield
        );
        assert_eq!(
            serde_yaml::to_string(&dual_wield).unwrap().trim(),
            "8388612"
        );
    }

    #[test]
    fn school_masks_decode() {
        assert!(SpellSchoolMask::from_bits(1).is_physical());
        assert_eq!(SpellSchoolMask::from_bits(127), SpellSchoolMask::ALL);
        assert_eq!(SpellSchoolMask::ALL.names().len(), 7);
        assert!(!SpellSchoolMask::MAGIC.is_physical());
        assert_eq!(SpellSchoolMask::from_bits(4).to_string(), "FIRE");
        assert_eq!(
            serde_yaml::from_str::<SpellSchoolMask>("FIRE").unwrap(),
            SpellSchoolMask::FIRE
        );
        assert_eq!(SpellSchoolMask::from_bits(0x80).unknown_bits(), 0x80);
        assert_eq!(
            SpellSchoolMask::from_bits(0x81).to_string(),
            "PHYSICAL | 0x80"
        );
    }
}
