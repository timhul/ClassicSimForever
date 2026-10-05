//! Typed rows of the tables the exporters read.
//!
//! Each struct maps a subset of the table's columns (the ones the sim needs) by header name; the
//! meaning of the columns is documented in `data/SPELL_INSTRUCTIONS.md` (spells),
//! `data/TALENT_INSTRUCTIONS.md` (Trait tables) and `data/ITEM_INSTRUCTIONS.md`. Columns that
//! the dump only carries for the UI or for retail-only systems are left out.

mod chr;
mod item;
mod skill;
mod spell;
mod traits;

pub use chr::*;
pub use item::*;
pub use skill::*;
pub use spell::*;
pub use traits::*;

/// Every table [`crate::Tables`] loads, in load order.
pub const ALL_TABLES: &[&str] = &[
    SpellNameRow::TABLE,
    SpellRow::TABLE,
    SpellMiscRow::TABLE,
    SpellEffectRow::TABLE,
    SpellPowerRow::TABLE,
    SpellCooldownsRow::TABLE,
    SpellCategoriesRow::TABLE,
    SpellCategoryRow::TABLE,
    SpellDurationRow::TABLE,
    SpellCastTimesRow::TABLE,
    SpellRangeRow::TABLE,
    SpellRadiusRow::TABLE,
    SpellLevelsRow::TABLE,
    SpellAuraOptionsRow::TABLE,
    SpellProcsPerMinuteRow::TABLE,
    SpellClassOptionsRow::TABLE,
    SpellShapeshiftRow::TABLE,
    SpellShapeshiftFormRow::TABLE,
    SpellAuraRestrictionsRow::TABLE,
    SpellEquippedItemsRow::TABLE,
    SpellTargetRestrictionsRow::TABLE,
    SpellLabelRow::TABLE,
    SpellItemEnchantmentRow::TABLE,
    SkillLineRow::TABLE,
    SkillLineAbilityRow::TABLE,
    SkillRaceClassInfoRow::TABLE,
    SkillLineXTraitTreeRow::TABLE,
    TalentTabRow::TABLE,
    TraitTreeRow::TABLE,
    TraitNodeRow::TABLE,
    TraitNodeEntryRow::TABLE,
    TraitNodeXTraitNodeEntryRow::TABLE,
    TraitDefinitionRow::TABLE,
    TraitDefinitionEffectPointsRow::TABLE,
    TraitEdgeRow::TABLE,
    TraitNodeGroupRow::TABLE,
    TraitNodeGroupXTraitNodeRow::TABLE,
    TraitNodeGroupXTraitCondRow::TABLE,
    TraitNodeGroupDisplayInfoRow::TABLE,
    TraitCondRow::TABLE,
    TraitCurrencyRow::TABLE,
    TraitCurrencySourceRow::TABLE,
    CurveRow::TABLE,
    CurvePointRow::TABLE,
    ChrClassesRow::TABLE,
    ChrRacesRow::TABLE,
    PlayerExpectedStatRow::TABLE,
    CharBaseInfoRow::TABLE,
    PowerTypeRow::TABLE,
    ItemRow::TABLE,
    ItemSparseRow::TABLE,
    ItemSubClassRow::TABLE,
    RandPropPointsRow::TABLE,
    ItemArmorTotalRow::TABLE,
    ItemArmorQualityRow::TABLE,
    ItemArmorShieldRow::TABLE,
    ArmorLocationRow::TABLE,
    ItemDamageOneHandRow::TABLE,
    ItemDamageTwoHandRow::TABLE,
    ItemDamageRangedRow::TABLE,
    ItemDamageWandRow::TABLE,
    ItemDamageThrownRow::TABLE,
    ItemEffectRow::TABLE,
    ItemXItemEffectRow::TABLE,
    ItemSetRow::TABLE,
    ItemSetSpellRow::TABLE,
    ItemLimitCategoryRow::TABLE,
    ItemNameDescriptionRow::TABLE,
    ItemXBonusTreeRow::TABLE,
    ItemBonusTreeNodeRow::TABLE,
    ItemBonusRow::TABLE,
];

use crate::row::TableRow;
