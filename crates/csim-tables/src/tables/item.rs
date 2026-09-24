//! `Item*` tables (`data/ITEM_INSTRUCTIONS.md` §1.2): item identity and stats, the per item
//! level budget / armor / damage tables, item spells, sets, unique groups and random suffixes.

use crate::row::TableRow;
use crate::table_row;

table_row! {
    /// `Item`: the physical identity of every item (present for all items, unlike
    /// `ItemSparse`).
    ItemRow, "Item" {
        id: u32 = "ID",
        /// 2 weapon, 4 armor; everything else is not equipment.
        class_id: u32 = "ClassID",
        subclass_id: u32 = "SubclassID",
        material: u32 = "Material",
        inventory_type: u32 = "InventoryType",
        icon_file_data_id: u32 = "IconFileDataID",
    }
}

table_row! {
    /// `ItemSparse`: name, level, quality and stats. Only part of the items have a row in the
    /// dump (ITEM_INSTRUCTIONS §1.1).
    ItemSparseRow, "ItemSparse" {
        id: u32 = "ID",
        name: String = "Display_lang",
        /// Flavour text.
        description: String = "Description_lang",
        /// Weapon damage range: average × (1 ± variance / 2).
        dmg_variance: f32 = "DmgVariance",
        /// → `ItemLimitCategory.ID` (unique-equipped group); 0 = none.
        limit_category: u32 = "LimitCategory",
        /// Bonus armor (same value as `bonusStat` 50).
        quality_modifier: f32 = "QualityModifier",
        /// Share of the `RandPropPoints` budget per stat, in 1/10000.
        stat_percent_editor: [i32; 10] = "StatPercentEditor_",
        /// `ItemModType` per stat; −1 = unused.
        stat_modifier_bonus_stat: [i32; 10] = "StatModifier_bonusStat_",
        /// 1 = "Unique".
        max_count: u32 = "MaxCount",
        /// The two 32-bit halves of the race mask; all bits = every race.
        allowable_race: [u32; 2] = "AllowableRace_",
        /// Retail `ItemFlags`, `ItemFlags2`, ...: `flags[0] & 0x10` deprecated,
        /// `flags[0] & 0x80000` unique-equipped.
        flags: [u32; 5] = "Flags_",
        min_faction_id: u32 = "MinFactionID",
        min_reputation: u32 = "MinReputation",
        required_skill: u32 = "RequiredSkill",
        required_skill_rank: u32 = "RequiredSkillRank",
        /// → `ItemNameDescription.ID` (grey subtitle).
        item_name_description_id: u32 = "ItemNameDescriptionID",
        /// → `ItemSet.ID`; 0 = none.
        item_set: u32 = "ItemSet",
        /// Weapon speed in ms.
        item_delay: u32 = "ItemDelay",
        item_level: u32 = "ItemLevel",
        /// Bit `1 << (ChrClasses.ID − 1)`; −1 or 32767 = every class.
        allowable_class: i32 = "AllowableClass",
        /// 0 none, 1 BoP, 2 BoE, 3 BoU, 4 quest, 5 account.
        bonding: u32 = "Bonding",
        /// School of the white damage: 0 physical, 1–6 magic schools.
        damage_type: u32 = "DamageType",
        required_level: u32 = "RequiredLevel",
        inventory_type: u32 = "InventoryType",
        /// 0 poor … 5 legendary, 6 artifact.
        overall_quality_id: u32 = "OverallQualityID",
    }
}

table_row! {
    /// `ItemSubClass`: weapon / armor sub-class names, keyed by (`ClassID`, `SubClassID`).
    ItemSubClassRow, "ItemSubClass" {
        display_name: String = "DisplayName_lang",
        verbose_name: String = "VerboseName_lang",
        id: u32 = "ID",
        class_id: u32 = "ClassID",
        sub_class_id: u32 = "SubClassID",
    }
}

table_row! {
    /// `RandPropPoints`: the stat budget per item level (`id` = item level), one column per
    /// quality tier and slot group (ITEM_INSTRUCTIONS §1.4).
    RandPropPointsRow, "RandPropPoints" {
        id: u32 = "ID",
        /// Quality 4–6.
        epic: [u32; 5] = "Epic_",
        /// Quality 3.
        superior: [u32; 5] = "Superior_",
        /// Quality 0–2.
        good: [u32; 5] = "Good_",
    }
}

table_row! {
    /// `ItemArmorTotal`: base armor per item level and material.
    ItemArmorTotalRow, "ItemArmorTotal" {
        id: u32 = "ID",
        item_level: u32 = "ItemLevel",
        cloth: f32 = "Cloth",
        leather: f32 = "Leather",
        mail: f32 = "Mail",
        plate: f32 = "Plate",
    }
}

table_row! {
    /// `ItemArmorQuality`: armor multiplier per quality (`id` = item level).
    ItemArmorQualityRow, "ItemArmorQuality" {
        id: u32 = "ID",
        quality_mod: [f32; 7] = "Qualitymod_",
    }
}

table_row! {
    /// `ItemArmorShield`: shield armor per item level and quality.
    ItemArmorShieldRow, "ItemArmorShield" {
        id: u32 = "ID",
        quality: [f32; 7] = "Quality_",
        item_level: u32 = "ItemLevel",
    }
}

table_row! {
    /// `ArmorLocation`: armor share of a slot (`id` = `InventoryType`). The four material
    /// columns are identical per row; `modifier` is the cloak column.
    ArmorLocationRow, "ArmorLocation" {
        id: u32 = "ID",
        cloth_modifier: f32 = "Clothmodifier",
        leather_modifier: f32 = "Leathermodifier",
        chain_modifier: f32 = "Chainmodifier",
        plate_modifier: f32 = "Platemodifier",
        modifier: f32 = "Modifier",
    }
}

/// The weapon DPS tables (`ItemDamage*`), which share one layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemDamageTable {
    OneHand,
    TwoHand,
    Ranged,
    Wand,
    Thrown,
}

impl ItemDamageTable {
    pub const ALL: [ItemDamageTable; 5] = [
        ItemDamageTable::OneHand,
        ItemDamageTable::TwoHand,
        ItemDamageTable::Ranged,
        ItemDamageTable::Wand,
        ItemDamageTable::Thrown,
    ];

    /// The table name.
    pub fn table(self) -> &'static str {
        match self {
            ItemDamageTable::OneHand => ItemDamageOneHandRow::TABLE,
            ItemDamageTable::TwoHand => ItemDamageTwoHandRow::TABLE,
            ItemDamageTable::Ranged => ItemDamageRangedRow::TABLE,
            ItemDamageTable::Wand => ItemDamageWandRow::TABLE,
            ItemDamageTable::Thrown => ItemDamageThrownRow::TABLE,
        }
    }
}

/// One row of an `ItemDamage*` table: weapon DPS per quality at an item level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemDamageRow {
    pub item_level: u32,
    /// DPS by `OverallQualityID` (float, not rounded).
    pub quality: [f32; 7],
}

macro_rules! item_damage_table {
    ($($name:ident, $table:literal;)*) => {
        $(
            table_row! {
                #[doc = concat!("`", $table, "`: weapon DPS per item level and quality.")]
                $name, $table {
                    id: u32 = "ID",
                    item_level: u32 = "ItemLevel",
                    quality: [f32; 7] = "Quality_",
                }
            }

            impl From<$name> for ItemDamageRow {
                fn from(row: $name) -> Self {
                    Self {
                        item_level: row.item_level,
                        quality: row.quality,
                    }
                }
            }
        )*
    };
}

item_damage_table! {
    ItemDamageOneHandRow, "ItemDamageOneHand";
    ItemDamageTwoHandRow, "ItemDamageTwoHand";
    ItemDamageRangedRow, "ItemDamageRanged";
    ItemDamageWandRow, "ItemDamageWand";
    ItemDamageThrownRow, "ItemDamageThrown";
}

table_row! {
    /// `ItemEffect`: a spell of an item (ITEM_INSTRUCTIONS §1.7).
    ItemEffectRow, "ItemEffect" {
        id: u32 = "ID",
        /// Slot 0..4 of the old `ItemSparse.Spell*` arrays.
        legacy_slot_index: u32 = "LegacySlotIndex",
        /// 0 on use, 1 on equip, 2 chance on hit, 5 no-delay use, 6 learn spell.
        trigger_type: u32 = "TriggerType",
        /// −1 unlimited, 0 n/a.
        charges: i32 = "Charges",
        /// Own cooldown in ms; −1 = none.
        cooldown_ms: i32 = "CoolDownMSec",
        /// Cooldown shared with `spell_category_id` in ms; −1 = none.
        category_cooldown_ms: i32 = "CategoryCoolDownMSec",
        spell_category_id: u32 = "SpellCategoryID",
        spell_id: u32 = "SpellID",
        player_condition_id: u32 = "PlayerConditionID",
    }
}

table_row! {
    /// `ItemXItemEffect`: which effects an item has.
    ItemXItemEffectRow, "ItemXItemEffect" {
        id: u32 = "ID",
        item_effect_id: u32 = "ItemEffectID",
        item_id: u32 = "ItemID",
    }
}

table_row! {
    /// `ItemSet`: set name and members (`item_ids`, 0 = unused).
    ItemSetRow, "ItemSet" {
        id: u32 = "ID",
        name: String = "Name_lang",
        set_flags: u32 = "SetFlags",
        required_skill: u32 = "RequiredSkill",
        required_skill_rank: u32 = "RequiredSkillRank",
        item_ids: [u32; 17] = "ItemID_",
    }
}

table_row! {
    /// `ItemSetSpell`: a set bonus — `spell_id` once `threshold` pieces are worn.
    ItemSetSpellRow, "ItemSetSpell" {
        id: u32 = "ID",
        chr_spec_id: u32 = "ChrSpecID",
        spell_id: u32 = "SpellID",
        threshold: u32 = "Threshold",
        item_set_id: u32 = "ItemSetID",
    }
}

table_row! {
    /// `ItemLimitCategory`: unique-equipped groups (at most `quantity` equipped).
    ItemLimitCategoryRow, "ItemLimitCategory" {
        id: u32 = "ID",
        name: String = "Name_lang",
        quantity: u32 = "Quantity",
        flags: u32 = "Flags",
    }
}

table_row! {
    /// `ItemNameDescription`: subtitle strings, also the random-suffix names.
    ItemNameDescriptionRow, "ItemNameDescription" {
        id: u32 = "ID",
        description: String = "Description_lang",
        color: i32 = "Color",
    }
}

table_row! {
    /// `ItemXBonusTree`: the bonus trees (random-suffix pools) of an item.
    ItemXBonusTreeRow, "ItemXBonusTree" {
        id: u32 = "ID",
        item_bonus_tree_id: u32 = "ItemBonusTreeID",
        item_id: u32 = "ItemID",
    }
}

table_row! {
    /// `ItemBonusTreeNode`: one choice of a bonus tree (one suffix for context 0).
    ItemBonusTreeNodeRow, "ItemBonusTreeNode" {
        id: u32 = "ID",
        item_context: u32 = "ItemContext",
        child_item_bonus_tree_id: u32 = "ChildItemBonusTreeID",
        /// → `ItemBonus.ParentItemBonusListID`.
        child_item_bonus_list_id: u32 = "ChildItemBonusListID",
        child_item_level_selector_id: u32 = "ChildItemLevelSelectorID",
        parent_item_bonus_tree_id: u32 = "ParentItemBonusTreeID",
    }
}

table_row! {
    /// `ItemBonus`: one bonus of a bonus list. `bonus_type` 2 = stat (`value[0]` bonusStat,
    /// `value[1]` budget share in 1/10000), 5 = name suffix (`value[0]` →
    /// `ItemNameDescription.ID`).
    ItemBonusRow, "ItemBonus" {
        id: u32 = "ID",
        value: [i32; 4] = "Value_",
        parent_item_bonus_list_id: u32 = "ParentItemBonusListID",
        bonus_type: u32 = "Type",
        order_index: u32 = "OrderIndex",
    }
}
