//! `Spell*` tables (`data/SPELL_INSTRUCTIONS.md` §1.2 and §1.7).

use crate::table_row;

table_row! {
    /// `SpellName`: the existence test for "spell is real" and its display name.
    SpellNameRow, "SpellName" {
        id: u32 = "ID",
        name: String = "Name_lang",
    }
}

table_row! {
    /// `Spell`: tooltip texts with `$` tokens; `name_subtext` is the "Rank N" string.
    SpellRow, "Spell" {
        id: u32 = "ID",
        name_subtext: String = "NameSubtext_lang",
        description: String = "Description_lang",
        aura_description: String = "AuraDescription_lang",
    }
}

table_row! {
    /// `SpellMisc`: attribute flags, school, and the cast time / duration / range indexes.
    SpellMiscRow, "SpellMisc" {
        id: u32 = "ID",
        /// `Attributes_0..16` = retail `SpellAttr0..16` bit flags.
        attributes: [u32; 17] = "Attributes_",
        difficulty_id: u32 = "DifficultyID",
        /// → `SpellCastTimes.ID`.
        casting_time_index: u32 = "CastingTimeIndex",
        /// → `SpellDuration.ID`.
        duration_index: u32 = "DurationIndex",
        /// → `SpellRange.ID`.
        range_index: u32 = "RangeIndex",
        /// 1 physical, 2 holy, 4 fire, 8 nature, 16 frost, 32 shadow, 64 arcane.
        school_mask: u32 = "SchoolMask",
        /// Missile speed.
        speed: f32 = "Speed",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellEffect`: one row per effect of a spell — what the spell does.
    SpellEffectRow, "SpellEffect" {
        id: u32 = "ID",
        /// `AuraType` when `effect` is an apply-aura effect.
        effect_aura: u32 = "EffectAura",
        /// 0 for every player spell; the other rows are ignored.
        difficulty_id: u32 = "DifficultyID",
        effect_index: u32 = "EffectIndex",
        /// Retail `SpellEffectName`.
        effect: u32 = "Effect",
        effect_amplitude: f32 = "EffectAmplitude",
        effect_attributes: u32 = "EffectAttributes",
        /// Tick interval in ms for periodic auras.
        effect_aura_period: u32 = "EffectAuraPeriod",
        /// Spell-power coefficient.
        effect_bonus_coefficient: f32 = "EffectBonusCoefficient",
        effect_chain_amplitude: f32 = "EffectChainAmplitude",
        effect_chain_targets: u32 = "EffectChainTargets",
        effect_item_type: u32 = "EffectItemType",
        effect_mechanic: u32 = "EffectMechanic",
        /// Value added per combo point.
        effect_points_per_resource: f32 = "EffectPointsPerResource",
        /// Value added per level above `SpellLevels.SpellLevel`.
        effect_real_points_per_level: f32 = "EffectRealPointsPerLevel",
        /// Spell fired by `TRIGGER_SPELL` / `PROC_TRIGGER_SPELL` / `PERIODIC_TRIGGER_SPELL`.
        effect_trigger_spell: u32 = "EffectTriggerSpell",
        /// Attack-power coefficient.
        bonus_coefficient_from_ap: f32 = "BonusCoefficientFromAP",
        /// Damage range: value × (1 ± variance / 2).
        variance: f32 = "Variance",
        /// The value (exact float, no die offset).
        effect_base_points: f32 = "EffectBasePointsF",
        /// Aura-specific: school mask, stat index, form id, power type, `SpellModOp`, ...
        effect_misc_value: [i32; 2] = "EffectMiscValue_",
        /// → `SpellRadius.ID`.
        effect_radius_index: [u32; 2] = "EffectRadiusIndex_",
        /// Family flags matched against `SpellClassOptions.SpellClassMask_*` (auras 107/108).
        effect_spell_class_mask: [u32; 4] = "EffectSpellClassMask_",
        /// 1 caster, 6 enemy, 20 party area, 22 caster position, 56 raid, ...
        implicit_target: [u32; 2] = "ImplicitTarget_",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellPower`: resource cost. Rage values are ×10 (`PowerType.DisplayModifier`).
    SpellPowerRow, "SpellPower" {
        id: u32 = "ID",
        order_index: u32 = "OrderIndex",
        mana_cost: i32 = "ManaCost",
        mana_cost_per_level: i32 = "ManaCostPerLevel",
        mana_per_second: i32 = "ManaPerSecond",
        /// Percent of base mana.
        power_cost_pct: f32 = "PowerCostPct",
        power_cost_max_pct: f32 = "PowerCostMaxPct",
        power_pct_per_second: f32 = "PowerPctPerSecond",
        /// `PowerType.PowerTypeEnum`: 0 mana, 1 rage, 2 focus, 3 energy, 4 combo points, −2 health.
        power_type: i32 = "PowerType",
        required_aura_spell_id: u32 = "RequiredAuraSpellID",
        optional_cost: i32 = "OptionalCost",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellCooldowns`: all values in ms.
    SpellCooldownsRow, "SpellCooldowns" {
        id: u32 = "ID",
        difficulty_id: u32 = "DifficultyID",
        /// Cooldown shared with `SpellCategories.Category`.
        category_recovery_time: u32 = "CategoryRecoveryTime",
        /// The spell's own cooldown.
        recovery_time: u32 = "RecoveryTime",
        /// The global cooldown the spell triggers.
        start_recovery_time: u32 = "StartRecoveryTime",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellCategories`: shared-cooldown group, hit table, mechanic.
    SpellCategoriesRow, "SpellCategories" {
        id: u32 = "ID",
        difficulty_id: u32 = "DifficultyID",
        /// Shared cooldown group (`SpellCategory.ID`).
        category: u32 = "Category",
        /// 0 none, 1 magic, 2 melee, 3 ranged.
        defense_type: u32 = "DefenseType",
        dispel_type: u32 = "DispelType",
        mechanic: u32 = "Mechanic",
        prevention_type: u32 = "PreventionType",
        /// 133 = on the global cooldown.
        start_recovery_category: u32 = "StartRecoveryCategory",
        charge_category: u32 = "ChargeCategory",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellCategory`: the shared-cooldown group definitions.
    SpellCategoryRow, "SpellCategory" {
        id: u32 = "ID",
        name: String = "Name_lang",
        flags: u32 = "Flags",
        max_charges: u32 = "MaxCharges",
        charge_recovery_time: u32 = "ChargeRecoveryTime",
    }
}

table_row! {
    /// `SpellDuration`: ms, −1 = until cancelled.
    SpellDurationRow, "SpellDuration" {
        id: u32 = "ID",
        duration: i32 = "Duration",
        max_duration: i32 = "MaxDuration",
        /// Extra duration per combo point.
        duration_per_resource: i32 = "DurationPerResource",
    }
}

table_row! {
    /// `SpellCastTimes`: ms.
    SpellCastTimesRow, "SpellCastTimes" {
        id: u32 = "ID",
        base: i32 = "Base",
        minimum: i32 = "Minimum",
    }
}

table_row! {
    /// `SpellRange`: yards.
    SpellRangeRow, "SpellRange" {
        id: u32 = "ID",
        display_name: String = "DisplayName_lang",
        flags: u32 = "Flags",
        range_min: [f32; 2] = "RangeMin_",
        range_max: [f32; 2] = "RangeMax_",
    }
}

table_row! {
    /// `SpellRadius`: yards.
    SpellRadiusRow, "SpellRadius" {
        id: u32 = "ID",
        radius: f32 = "Radius",
        radius_per_level: f32 = "RadiusPerLevel",
        radius_min: f32 = "RadiusMin",
        radius_max: f32 = "RadiusMax",
    }
}

table_row! {
    /// `SpellLevels`: learn level and per-level scaling bounds.
    SpellLevelsRow, "SpellLevels" {
        id: u32 = "ID",
        difficulty_id: u32 = "DifficultyID",
        /// Scaling cap (0 = none).
        max_level: u32 = "MaxLevel",
        max_passive_aura_level: u32 = "MaxPassiveAuraLevel",
        /// Learn level.
        base_level: u32 = "BaseLevel",
        /// Level used for per-level scaling.
        spell_level: u32 = "SpellLevel",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellAuraOptions`: procs and stacking.
    SpellAuraOptionsRow, "SpellAuraOptions" {
        id: u32 = "ID",
        difficulty_id: u32 = "DifficultyID",
        /// Max stacks.
        cumulative_aura: u32 = "CumulativeAura",
        /// Internal cooldown in ms.
        proc_category_recovery: u32 = "ProcCategoryRecovery",
        /// Percent; 101 = "n/a, always".
        proc_chance: u32 = "ProcChance",
        proc_charges: u32 = "ProcCharges",
        /// → `SpellProcsPerMinute.ID`.
        spell_procs_per_minute_id: u32 = "SpellProcsPerMinuteID",
        /// Retail `ProcFlags` (two 32-bit words).
        proc_type_mask: [u32; 2] = "ProcTypeMask_",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellProcsPerMinute`: PPM rates.
    SpellProcsPerMinuteRow, "SpellProcsPerMinute" {
        id: u32 = "ID",
        base_proc_rate: f32 = "BaseProcRate",
        flags: u32 = "Flags",
    }
}

table_row! {
    /// `SpellClassOptions`: class family and the 128-bit family flags used by modifiers.
    SpellClassOptionsRow, "SpellClassOptions" {
        id: u32 = "ID",
        spell_id: u32 = "SpellID",
        modal_next_spell: u32 = "ModalNextSpell",
        /// Class family (`ChrClasses.SpellClassSet`, Warrior 4).
        spell_class_set: u32 = "SpellClassSet",
        spell_class_mask: [u32; 4] = "SpellClassMask_",
    }
}

table_row! {
    /// `SpellShapeshift`: stance/form requirement.
    SpellShapeshiftRow, "SpellShapeshift" {
        id: u32 = "ID",
        spell_id: u32 = "SpellID",
        stance_bar_order: i32 = "StanceBarOrder",
        shapeshift_exclude: [u32; 2] = "ShapeshiftExclude_",
        /// Bit `1 << (SpellShapeshiftForm.ID − 1)`.
        shapeshift_mask: [u32; 2] = "ShapeshiftMask_",
    }
}

table_row! {
    /// `SpellShapeshiftForm`: form definitions (17 Battle, 18 Defensive, 19 Berserker Stance, ...).
    SpellShapeshiftFormRow, "SpellShapeshiftForm" {
        id: u32 = "ID",
        name: String = "Name_lang",
        creature_type: u32 = "CreatureType",
        flags: u32 = "Flags",
        bonus_action_bar: i32 = "BonusActionBar",
        combat_round_time: u32 = "CombatRoundTime",
        damage_variance: f32 = "DamageVariance",
        preset_spell_id: [u32; 8] = "PresetSpellID_",
    }
}

table_row! {
    /// `SpellAuraRestrictions`: aura-state gates (target ≤ 20 %, after dodge/parry/block, ...).
    SpellAuraRestrictionsRow, "SpellAuraRestrictions" {
        id: u32 = "ID",
        difficulty_id: u32 = "DifficultyID",
        caster_aura_state: u32 = "CasterAuraState",
        target_aura_state: u32 = "TargetAuraState",
        exclude_caster_aura_state: u32 = "ExcludeCasterAuraState",
        exclude_target_aura_state: u32 = "ExcludeTargetAuraState",
        caster_aura_spell: u32 = "CasterAuraSpell",
        target_aura_spell: u32 = "TargetAuraSpell",
        exclude_caster_aura_spell: u32 = "ExcludeCasterAuraSpell",
        exclude_target_aura_spell: u32 = "ExcludeTargetAuraSpell",
        caster_aura_type: u32 = "CasterAuraType",
        target_aura_type: u32 = "TargetAuraType",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellEquippedItems`: required weapon / shield.
    SpellEquippedItemsRow, "SpellEquippedItems" {
        id: u32 = "ID",
        spell_id: u32 = "SpellID",
        /// `ItemClass`: 2 weapon, 4 armor; −1 none.
        equipped_item_class: i32 = "EquippedItemClass",
        equipped_item_inv_types: u32 = "EquippedItemInvTypes",
        /// Bitmask over `ItemSubClass.SubClassID`.
        equipped_item_subclass: u32 = "EquippedItemSubclass",
    }
}

table_row! {
    /// `SpellTargetRestrictions`: target count and type limits.
    SpellTargetRestrictionsRow, "SpellTargetRestrictions" {
        id: u32 = "ID",
        difficulty_id: u32 = "DifficultyID",
        cone_degrees: f32 = "ConeDegrees",
        max_targets: u32 = "MaxTargets",
        max_target_level: u32 = "MaxTargetLevel",
        target_creature_type: u32 = "TargetCreatureType",
        targets: u32 = "Targets",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `SpellLabel`: tag ids per spell.
    SpellLabelRow, "SpellLabel" {
        id: u32 = "ID",
        label_id: u32 = "LabelID",
        spell_id: u32 = "SpellID",
    }
}

table_row! {
    /// `Curve`: talent rank curves.
    CurveRow, "Curve" {
        id: u32 = "ID",
        curve_type: u32 = "Type",
        flags: u32 = "Flags",
    }
}

table_row! {
    /// `CurvePoint`: `pos[0]` = rank, `pos[1]` = value.
    CurvePointRow, "CurvePoint" {
        id: u32 = "ID",
        pos: [f32; 2] = "Pos_",
        curve_id: u32 = "CurveID",
        order_index: u32 = "OrderIndex",
    }
}
