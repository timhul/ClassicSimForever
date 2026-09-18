//! Character tables: classes, races and power types.

use crate::table_row;

table_row! {
    /// `ChrClasses`: class ids, spell families and the melee stat conversions.
    ChrClassesRow, "ChrClasses" {
        name: String = "Name_lang",
        /// Upper-case token (`WARRIOR`).
        filename: String = "Filename",
        id: u32 = "ID",
        has_strength_attack_bonus: u32 = "HasStrengthAttackBonus",
        primary_stat_priority: u32 = "PrimaryStatPriority",
        /// `PowerType.PowerTypeEnum` of the class resource (Warrior 1 = rage).
        display_power: u32 = "DisplayPower",
        ranged_attack_power_per_agility: f32 = "RangedAttackPowerPerAgility",
        attack_power_per_agility: f32 = "AttackPowerPerAgility",
        attack_power_per_strength: f32 = "AttackPowerPerStrength",
        /// `SpellClassOptions.SpellClassSet` of the class family (Warrior 4).
        spell_class_set: u32 = "SpellClassSet",
        armor_type_mask: u32 = "ArmorTypeMask",
        flags: u32 = "Flags",
        starting_level: u32 = "StartingLevel",
    }
}

table_row! {
    /// `ChrRaces`: race ids and faction.
    ChrRacesRow, "ChrRaces" {
        id: u32 = "ID",
        client_prefix: String = "ClientPrefix",
        /// Upper-camel token (`NightElf`).
        client_file_string: String = "ClientFileString",
        name: String = "Name_lang",
        flags: u32 = "Flags",
        faction_id: u32 = "FactionID",
        /// 0 Alliance, 1 Horde.
        alliance: u32 = "Alliance",
        /// Bit position in playable race masks; −1 for non-playable races.
        playable_race_bit: i32 = "PlayableRaceBit",
        base_language: u32 = "BaseLanguage",
        creature_type: u32 = "CreatureType",
    }
}

table_row! {
    /// `PowerType`: resource enums and their storage scale (`display_modifier`: rage 10).
    PowerTypeRow, "PowerType" {
        name_global_string_tag: String = "NameGlobalStringTag",
        id: u32 = "ID",
        /// 0 mana, 1 rage, 2 focus, 3 energy, 4 combo points, ...
        power_type_enum: u32 = "PowerTypeEnum",
        min_power: i32 = "MinPower",
        max_base_power: i32 = "MaxBasePower",
        default_power: i32 = "DefaultPower",
        /// Stored values are this many times the displayed value.
        display_modifier: i32 = "DisplayModifier",
        regen_interrupt_time_ms: u32 = "RegenInterruptTimeMS",
        regen_peace: f32 = "RegenPeace",
        regen_combat: f32 = "RegenCombat",
        flags: u32 = "Flags",
    }
}
