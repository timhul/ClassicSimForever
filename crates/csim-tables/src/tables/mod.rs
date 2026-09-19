//! Typed rows of the tables the exporters read.
//!
//! Each struct maps a subset of the table's columns (the ones the sim needs) by header name; the
//! meaning of the columns is documented in `data/SPELL_INSTRUCTIONS.md` (spells),
//! `data/TALENT_INSTRUCTIONS.md` (Trait tables) and `data/ITEM_INSTRUCTIONS.md`. Columns that
//! the dump only carries for the UI or for retail-only systems are left out.

mod chr;
mod skill;
mod spell;
mod traits;

pub use chr::*;
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
    SkillLineRow::TABLE,
    SkillLineAbilityRow::TABLE,
    SkillRaceClassInfoRow::TABLE,
    SkillLineXTraitTreeRow::TABLE,
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
];

use crate::row::TableRow;
