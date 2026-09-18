//! `SkillLine*` tables (`data/SPELL_INSTRUCTIONS.md` §1.3, §1.5, §1.6).

use crate::table_row;

table_row! {
    /// `SkillLine`: `category_id` 7 = class skills, 9 = racials/secondary, 6 = weapon skills.
    SkillLineRow, "SkillLine" {
        display_name: String = "DisplayName_lang",
        id: u32 = "ID",
        category_id: u32 = "CategoryID",
        parent_skill_line_id: u32 = "ParentSkillLineID",
        flags: u32 = "Flags",
    }
}

table_row! {
    /// `SkillLineAbility`: skill line → spell, with rank chains and acquisition info.
    SkillLineAbilityRow, "SkillLineAbility" {
        id: u32 = "ID",
        skill_line: u32 = "SkillLine",
        spell: u32 = "Spell",
        min_skill_line_rank: u32 = "MinSkillLineRank",
        /// `1 << (ChrClasses.ID − 1)` for trainable abilities; 0 = talent/rune granted;
        /// all bits = any class (racials).
        class_mask: u32 = "ClassMask",
        /// Previous rank.
        supercedes_spell: u32 = "SupercedesSpell",
        /// 0 trainer, 1 auto-learned, 2 granted, 3 Forever baseline additions.
        acquire_method: u32 = "AcquireMethod",
        flags: u32 = "Flags",
        /// `1 << (ChrRaces.ID − 1)`; the second word holds the Forever race Skyborne.
        race_masks: [u32; 2] = "RaceMasks_",
    }
}

table_row! {
    /// `SkillRaceClassInfo`: which classes / races own a skill line.
    SkillRaceClassInfoRow, "SkillRaceClassInfo" {
        id: u32 = "ID",
        skill_id: u32 = "SkillID",
        class_mask: u32 = "ClassMask",
        flags: u32 = "Flags",
        availability: u32 = "Availability",
        min_level: u32 = "MinLevel",
        race_masks: [u32; 2] = "RaceMasks_",
    }
}

table_row! {
    /// `SkillLineXTraitTree`: class skill line → talent tree.
    SkillLineXTraitTreeRow, "SkillLineXTraitTree" {
        id: u32 = "ID",
        skill_line_id: u32 = "SkillLineID",
        trait_tree_id: u32 = "TraitTreeID",
        variant: u32 = "Variant",
    }
}
