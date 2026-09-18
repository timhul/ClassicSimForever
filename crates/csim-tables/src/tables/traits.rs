//! `Trait*` tables: the retail talent system used by Forever (`data/TALENT_INSTRUCTIONS.md`).

use crate::table_row;

table_row! {
    /// `TraitTree`: one tree per class (Warrior 1117).
    TraitTreeRow, "TraitTree" {
        title_text: String = "TitleText_lang",
        id: u32 = "ID",
        trait_system_id: u32 = "TraitSystemID",
        first_trait_node_id: u32 = "FirstTraitNodeID",
        flags: u32 = "Flags",
    }
}

table_row! {
    /// `TraitNode`: one node = one talent; `pos_x` / `pos_y` give tab, tier and column.
    TraitNodeRow, "TraitNode" {
        id: u32 = "ID",
        trait_tree_id: u32 = "TraitTreeID",
        pos_x: i32 = "PosX",
        pos_y: i32 = "PosY",
        node_type: u32 = "Type",
        flags: u32 = "Flags",
        trait_sub_tree_id: u32 = "TraitSubTreeID",
    }
}

table_row! {
    /// `TraitNodeEntry`: `max_ranks` and the definition (spell) of a node.
    TraitNodeEntryRow, "TraitNodeEntry" {
        id: u32 = "ID",
        trait_definition_id: u32 = "TraitDefinitionID",
        max_ranks: u32 = "MaxRanks",
        node_entry_type: u32 = "NodeEntryType",
    }
}

table_row! {
    /// `TraitNodeXTraitNodeEntry`: node ↔ entry (1:1 in this build).
    TraitNodeXTraitNodeEntryRow, "TraitNodeXTraitNodeEntry" {
        id: u32 = "ID",
        trait_node_id: u32 = "TraitNodeID",
        trait_node_entry_id: u32 = "TraitNodeEntryID",
        index: u32 = "_Index",
    }
}

table_row! {
    /// `TraitDefinition`: the talent's spell.
    TraitDefinitionRow, "TraitDefinition" {
        override_name: String = "OverrideName_lang",
        override_subtext: String = "OverrideSubtext_lang",
        override_description: String = "OverrideDescription_lang",
        id: u32 = "ID",
        spell_id: u32 = "SpellID",
        override_icon: u32 = "OverrideIcon",
        overrides_spell_id: u32 = "OverridesSpellID",
        visible_spell_id: u32 = "VisibleSpellID",
    }
}

table_row! {
    /// `TraitDefinitionEffectPoints`: per-rank value curve of one effect of the talent spell.
    TraitDefinitionEffectPointsRow, "TraitDefinitionEffectPoints" {
        id: u32 = "ID",
        trait_definition_id: u32 = "TraitDefinitionID",
        effect_index: u32 = "EffectIndex",
        /// 0 = replace the effect's base points.
        operation_type: u32 = "OperationType",
        /// → `CurvePoint.CurveID`.
        curve_id: u32 = "CurveID",
    }
}

table_row! {
    /// `TraitEdge`: prerequisite (`left` must be maxed before `right`).
    TraitEdgeRow, "TraitEdge" {
        id: u32 = "ID",
        visual_style: u32 = "VisualStyle",
        left_trait_node_id: u32 = "LeftTraitNodeID",
        right_trait_node_id: u32 = "RightTraitNodeID",
        edge_type: u32 = "Type",
    }
}

table_row! {
    /// `TraitNodeGroup`: tab groups and tier-band groups.
    TraitNodeGroupRow, "TraitNodeGroup" {
        id: u32 = "ID",
        trait_tree_id: u32 = "TraitTreeID",
        flags: u32 = "Flags",
    }
}

table_row! {
    /// `TraitNodeGroupXTraitNode`: group membership.
    TraitNodeGroupXTraitNodeRow, "TraitNodeGroupXTraitNode" {
        id: u32 = "ID",
        trait_node_group_id: u32 = "TraitNodeGroupID",
        trait_node_id: u32 = "TraitNodeID",
        index: u32 = "_Index",
    }
}

table_row! {
    /// `TraitNodeGroupXTraitCond`: tier gating condition attached to a group.
    TraitNodeGroupXTraitCondRow, "TraitNodeGroupXTraitCond" {
        id: u32 = "ID",
        trait_cond_id: u32 = "TraitCondID",
        trait_node_group_id: u32 = "TraitNodeGroupID",
    }
}

table_row! {
    /// `TraitNodeGroupDisplayInfo`: tab group → skill line (Arms/Fury/Protection).
    TraitNodeGroupDisplayInfoRow, "TraitNodeGroupDisplayInfo" {
        id: u32 = "ID",
        trait_node_group_id: u32 = "TraitNodeGroupID",
        skill_line_id: u32 = "SkillLineID",
        order_index: u32 = "OrderIndex",
        trait_tree_id: u32 = "TraitTreeID",
    }
}

table_row! {
    /// `TraitCond`: "spent N points in group G" conditions.
    TraitCondRow, "TraitCond" {
        id: u32 = "ID",
        cond_type: u32 = "CondType",
        trait_tree_id: u32 = "TraitTreeID",
        granted_ranks: u32 = "GrantedRanks",
        /// The group whose spent points are counted.
        trait_node_group_id: u32 = "TraitNodeGroupID",
        trait_node_id: u32 = "TraitNodeID",
        trait_node_entry_id: u32 = "TraitNodeEntryID",
        trait_currency_id: u32 = "TraitCurrencyID",
        spent_amount_required: u32 = "SpentAmountRequired",
        flags: u32 = "Flags",
        required_level: u32 = "RequiredLevel",
    }
}

table_row! {
    /// `TraitCurrency`: talent points (3820, `sourced_max` 51).
    TraitCurrencyRow, "TraitCurrency" {
        id: u32 = "ID",
        currency_type: u32 = "Type",
        flags: u32 = "Flags",
        sourced_max: u32 = "SourcedMax",
    }
}

table_row! {
    /// `TraitCurrencySource`: one point per level from 10.
    TraitCurrencySourceRow, "TraitCurrencySource" {
        requirement: String = "Requirement_lang",
        id: u32 = "ID",
        trait_currency_id: u32 = "TraitCurrencyID",
        amount: u32 = "Amount",
        player_level: u32 = "PlayerLevel",
        trait_node_entry_id: u32 = "TraitNodeEntryID",
        order_index: u32 = "OrderIndex",
    }
}
