//! The table-shaped spell data model: one [`SpellRecord`] per spell id, one [`EffectRecord`] per
//! `SpellEffect` row, and the [`SpellDb`] that indexes them.
//!
//! The YAML files under `data/spells/` are generated from the client table dumps by
//! `csim-tables export-spells` (see `TASKS.md` Phase 3T and `data/SPELL_INSTRUCTIONS.md`). Field
//! names are the table columns in snake_case, values are stored exactly as the tables store them
//! (rage costs in tenths, durations in milliseconds, masks as raw words) and fields at their
//! default are omitted on export, so a record reads like a joined table row:
//!
//! ```yaml
//! build: 1.60.1.70009
//! class: WARRIOR
//! spells:
//!   - id: 12294
//!     name: Mortal Strike
//!     rank_text: Rank 1
//!     skill_line: 26
//!     attributes: [327696, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
//!     school_mask: 1
//!     duration_ms: 10000
//!     range_yd: 5
//!     power: [{ type: RAGE, cost: 300 }]
//!     cooldown: { category_recovery_ms: 6000, start_recovery_ms: 1500 }
//!     categories: { category: 971, start_recovery_category: 133, defense_type: MELEE }
//!     levels: { base: 40, spell: 40 }
//!     class_options: { set: 4, mask: [33554432, 0, 0, 0] }
//!     equipped_items: { class: 2, subclass_mask: 173555 }
//!     effects:
//!       - index: 1                     # effect 0, the healing debuff, is pruned (§1.10)
//!         effect: NORMALIZED_WEAPON_DMG
//!         base_points: 85
//!         implicit_target: [UNIT_TARGET_ENEMY, NONE]
//! ```
//!
//! Everything the client tables do *not* contain (scripted `DUMMY` effects, server-side proc
//! conditions, sim-only threat) lives in the hand-written [`crate::spell::overrides`], which
//! [`SpellDb`] loads from `data/spells/overrides/` and keeps next to the records: a record is
//! always the table row, and [`SpellDb::overrides`] answers what the sim adds to it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::faction::PlayerClass;
use crate::spell::Hand;
use crate::spell::dbc::{
    AuraState, AuraType, DefenseType, ImplicitTarget, Mechanic, PowerType, ProcFlags,
    ShapeshiftForm, SpellAttr0, SpellAttr1, SpellAttr2, SpellAttr3, SpellEffectName, SpellModOp,
    SpellSchoolMask,
};
use crate::spell::overrides::{OverrideError, OverrideFile, Overrides, SimFlag};

/// The subdirectory of the spell directory that holds the hand-written overrides.
pub const OVERRIDES_DIR: &str = "overrides";

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}

/// One `data/spells/*.yaml` file: the spells of one class (or the racials when `class` is absent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpellFile {
    /// The client build the records were exported from (`1.60.1.70009`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build: String,
    /// The class whose spellbook this is; `None` for class-independent spells (racials, the
    /// external buff auras).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<PlayerClass>,
    /// Whether characters learn these spells (`CharacterContext::learn_all`): `false` for the
    /// external buff auras of `externals.yaml`, which are only turned into buffs.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub learnable: bool,
    #[serde(default)]
    pub spells: Vec<SpellRecord>,
}

impl Default for SpellFile {
    fn default() -> Self {
        Self {
            build: String::new(),
            class: None,
            learnable: true,
            spells: Vec::new(),
        }
    }
}

/// A `SpellPower` row: what casting the spell costs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PowerCost {
    #[serde(rename = "type")]
    pub power_type: PowerType,
    /// `ManaCost`, in the power type's stored units (rage ×10).
    #[serde(default, skip_serializing_if = "is_default")]
    pub cost: i32,
    /// `PowerCostPct`: percent of base mana / health.
    #[serde(default, skip_serializing_if = "is_default")]
    pub cost_pct: f32,
    /// `ManaPerSecond` for channelled spells.
    #[serde(default, skip_serializing_if = "is_default")]
    pub per_second: i32,
}

impl PowerCost {
    /// The cost in displayed units (30 for Mortal Strike's stored 300 rage).
    pub fn displayed_cost(&self) -> f32 {
        self.cost as f32 / self.power_type.display_modifier() as f32
    }
}

/// The `SpellCooldowns` row, all in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cooldown {
    /// The spell's own cooldown.
    #[serde(default, skip_serializing_if = "is_default")]
    pub recovery_ms: u32,
    /// The cooldown shared by every spell of `categories.category`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub category_recovery_ms: u32,
    /// The global cooldown the cast triggers.
    #[serde(default, skip_serializing_if = "is_default")]
    pub start_recovery_ms: u32,
}

/// The `SpellCategories` row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Categories {
    /// Shared-cooldown group (`SpellCategory.ID`); 0 = none.
    #[serde(default, skip_serializing_if = "is_default")]
    pub category: u32,
    /// 133 = the spell is on the global cooldown.
    #[serde(default, skip_serializing_if = "is_default")]
    pub start_recovery_category: u32,
    /// Which hit table the spell rolls on.
    #[serde(default, skip_serializing_if = "is_default")]
    pub defense_type: DefenseType,
    #[serde(default, skip_serializing_if = "is_default")]
    pub mechanic: Mechanic,
    #[serde(default, skip_serializing_if = "is_default")]
    pub dispel_type: u32,
}

/// The global-cooldown category (`SpellCategories.StartRecoveryCategory`).
pub const GLOBAL_COOLDOWN_CATEGORY: u32 = 133;

/// The `SpellLevels` row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Levels {
    /// Learn level.
    #[serde(default, skip_serializing_if = "is_default")]
    pub base: u32,
    /// Level the per-level scaling counts from.
    #[serde(default, skip_serializing_if = "is_default")]
    pub spell: u32,
    /// Scaling cap; 0 = none.
    #[serde(default, skip_serializing_if = "is_default")]
    pub max: u32,
}

/// The `SpellAuraOptions` row: procs and stacking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct AuraOptions {
    /// Percent; 101 = "always".
    #[serde(default, skip_serializing_if = "is_default")]
    pub proc_chance: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub proc_charges: u32,
    /// Internal cooldown.
    #[serde(default, skip_serializing_if = "is_default")]
    pub proc_category_recovery_ms: u32,
    /// The events the aura reacts to.
    #[serde(default, skip_serializing_if = "is_default")]
    pub proc_type_mask: ProcFlags,
    /// `SpellProcsPerMinute.BaseProcRate`; 0 = flat chance.
    #[serde(default, skip_serializing_if = "is_default")]
    pub ppm: f32,
    /// `CumulativeAura`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_stacks: u32,
}

/// The `SpellClassOptions` row: the spell's family and family flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassOptions {
    /// `SpellClassSet` (Warrior 4).
    pub set: u32,
    /// `SpellClassMask_0..3`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub mask: [u32; 4],
}

impl ClassOptions {
    /// Whether a modifier's `EffectSpellClassMask` selects this spell.
    pub fn matches(&self, set: u32, mask: &[u32; 4]) -> bool {
        self.set == set && self.mask.iter().zip(mask).any(|(a, b)| a & b != 0)
    }
}

/// The `SpellEquippedItems` row: the weapon / shield the spell needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquippedItems {
    /// `ItemClass`: 2 weapon, 4 armor.
    pub class: i32,
    /// Bitmask over `ItemSubClass.SubClassID` (173555 = any melee weapon, 64 with class 4 = shield).
    #[serde(default, skip_serializing_if = "is_default")]
    pub subclass_mask: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub inv_type_mask: u32,
}

impl EquippedItems {
    /// `ItemClass` of weapons.
    pub const WEAPON: i32 = 2;
    /// `ItemClass` of armor (shields, held items, relics).
    pub const ARMOR: i32 = 4;
    /// `ItemSubClass` of a shield within the armor class.
    pub const SHIELD_SUBCLASS: u32 = 6;

    /// Whether the requirement is for a shield: the spell strikes with it (Shield Slam, Shield
    /// Bash), which makes it an off-hand attack.
    pub fn requires_shield(&self) -> bool {
        self.class == Self::ARMOR && self.subclass_mask & (1 << Self::SHIELD_SUBCLASS) != 0
    }

    /// Whether an item of `item_class` / `subclass` satisfies the requirement (an empty
    /// subclass mask accepts the whole class).
    pub fn accepts(&self, item_class: u32, subclass: u32) -> bool {
        u32::try_from(self.class).is_ok_and(|class| class == item_class)
            && (self.subclass_mask == 0 || self.subclass_mask & (1 << subclass) != 0)
    }
}

/// The `SpellAuraRestrictions` row: aura-state gates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraRestrictions {
    #[serde(default, skip_serializing_if = "is_default")]
    pub caster_aura_state: AuraState,
    #[serde(default, skip_serializing_if = "is_default")]
    pub target_aura_state: AuraState,
    #[serde(default, skip_serializing_if = "is_default")]
    pub exclude_caster_aura_state: AuraState,
    #[serde(default, skip_serializing_if = "is_default")]
    pub exclude_target_aura_state: AuraState,
    #[serde(default, skip_serializing_if = "is_default")]
    pub caster_aura_spell: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub target_aura_spell: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub exclude_caster_aura_spell: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub exclude_target_aura_spell: u32,
}

/// One `SpellEffect` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectRecord {
    /// `EffectIndex`; descriptions call it `$s<index + 1>`.
    pub index: u32,
    pub effect: SpellEffectName,
    /// The aura type for apply-aura effects.
    #[serde(default, skip_serializing_if = "is_default")]
    pub aura: AuraType,
    /// `EffectBasePointsF`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub base_points: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub real_points_per_level: f32,
    /// Damage range: value × (1 ± variance / 2).
    #[serde(default, skip_serializing_if = "is_default")]
    pub variance: f32,
    /// Value added per combo point.
    #[serde(default, skip_serializing_if = "is_default")]
    pub points_per_resource: f32,
    /// Tick interval of periodic auras.
    #[serde(default, skip_serializing_if = "is_default")]
    pub aura_period_ms: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub amplitude: f32,
    /// `EffectChainAmplitude`; 1 for almost every row.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub chain_amplitude: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub chain_targets: u32,
    /// The spell fired by `TRIGGER_SPELL`, `PROC_TRIGGER_SPELL` and `PERIODIC_TRIGGER_SPELL`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub trigger_spell: u32,
    /// Spell-power coefficient.
    #[serde(default, skip_serializing_if = "is_default")]
    pub bonus_coefficient: f32,
    /// Attack-power coefficient.
    #[serde(default, skip_serializing_if = "is_default")]
    pub bonus_coefficient_from_ap: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub mechanic: Mechanic,
    /// `EffectMiscValue_0/1`: aura-specific (school mask, stat, form, power type, `SpellModOp`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub misc_value: [i32; 2],
    /// `SpellRadius.Radius` of `EffectRadiusIndex_0/1`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub radius_yd: [f32; 2],
    /// `EffectSpellClassMask_0..3`: the spells a modifier aura affects.
    #[serde(default, skip_serializing_if = "is_default")]
    pub spell_class_mask: [u32; 4],
    #[serde(default, skip_serializing_if = "is_default")]
    pub implicit_target: [ImplicitTarget; 2],
    /// `EffectAttributes`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub attributes: u32,
}

impl EffectRecord {
    /// A minimal effect for building records in code and tests.
    pub fn new(index: u32, effect: SpellEffectName) -> Self {
        Self {
            index,
            effect,
            aura: AuraType::None,
            base_points: 0.0,
            real_points_per_level: 0.0,
            variance: 0.0,
            points_per_resource: 0.0,
            aura_period_ms: 0,
            amplitude: 0.0,
            chain_amplitude: 1.0,
            chain_targets: 0,
            trigger_spell: 0,
            bonus_coefficient: 0.0,
            bonus_coefficient_from_ap: 0.0,
            mechanic: Mechanic::None,
            misc_value: [0, 0],
            radius_yd: [0.0, 0.0],
            spell_class_mask: [0; 4],
            implicit_target: [ImplicitTarget::None, ImplicitTarget::None],
            attributes: 0,
        }
    }

    /// Whether the simulator has no use for the effect (`crate::spell::dbc::DISCARDED_AURA_IDS` /
    /// `DISCARDED_EFFECT_IDS`): the exporter drops such effects from the data files.
    pub fn is_discarded(&self) -> bool {
        if self.is_apply_aura() {
            self.aura.is_discarded()
        } else {
            self.effect.is_discarded()
        }
    }

    /// Whether the effect applies `aura` (any of the apply-aura effect kinds).
    pub fn is_apply_aura(&self) -> bool {
        matches!(
            self.effect,
            SpellEffectName::ApplyAura
                | SpellEffectName::ApplyAreaAuraParty
                | SpellEffectName::ApplyAreaAuraRaid
                | SpellEffectName::ApplyAreaAuraFriend
                | SpellEffectName::ApplyAreaAuraEnemy
                | SpellEffectName::ApplyAreaAuraPet
                | SpellEffectName::ApplyAreaAuraOwner
                | SpellEffectName::PersistentAreaAura
        )
    }

    /// Whether the effect is a periodic aura (ticks every `aura_period_ms`).
    pub fn is_periodic(&self) -> bool {
        self.is_apply_aura()
            && matches!(
                self.aura,
                AuraType::PeriodicDamage
                    | AuraType::PeriodicEnergize
                    | AuraType::PeriodicTriggerSpell
                    | AuraType::PeriodicDummy
                    | AuraType::PeriodicLeech
                    | AuraType::PeriodicDamagePercent
                    | AuraType::PeriodicHealthFunnel
                    | AuraType::PeriodicManaLeech
                    | AuraType::PeriodicTriggerSpellWithValue
                    | AuraType::ObsModHealth
                    | AuraType::ObsModPower
            )
    }

    /// Whether the effect is a spell modifier (`ADD_FLAT_MODIFIER` / `ADD_PCT_MODIFIER`).
    pub fn is_spell_modifier(&self) -> bool {
        self.is_apply_aura()
            && matches!(
                self.aura,
                AuraType::AddFlatModifier | AuraType::AddPctModifier
            )
    }

    /// Whether the effect is a proc trigger (`PROC_TRIGGER_SPELL` family).
    pub fn is_proc_trigger(&self) -> bool {
        self.is_apply_aura()
            && matches!(
                self.aura,
                AuraType::ProcTriggerSpell
                    | AuraType::ProcTriggerSpellWithValue
                    | AuraType::ProcTriggerDamage
            )
    }

    /// Whether the effect needs hand-written logic (`DUMMY` effects and auras, class scripts,
    /// `ADD_TARGET_TRIGGER` whose chance rule the server keeps).
    pub fn is_scripted(&self) -> bool {
        matches!(self.effect, SpellEffectName::Dummy)
            || (self.is_apply_aura()
                && matches!(
                    self.aura,
                    AuraType::Dummy | AuraType::PeriodicDummy | AuraType::AddTargetTrigger
                ))
    }

    /// Whether the first implicit target is the caster.
    pub fn targets_caster(&self) -> bool {
        matches!(
            self.implicit_target[0],
            ImplicitTarget::UnitCaster | ImplicitTarget::DestCaster | ImplicitTarget::SrcCaster
        )
    }

    /// Whether the effect hits the enemy target (directly or as an area around the caster).
    pub fn targets_enemy(&self) -> bool {
        self.implicit_target.iter().any(|t| {
            matches!(
                t,
                ImplicitTarget::UnitTargetEnemy
                    | ImplicitTarget::UnitSrcAreaEnemy
                    | ImplicitTarget::UnitDestAreaEnemy
                    | ImplicitTarget::UnitNearbyEnemy
                    | ImplicitTarget::UnitConeEnemy24
                    | ImplicitTarget::UnitCone180DegEnemy
                    | ImplicitTarget::DestTargetEnemy
            )
        })
    }

    /// Whether the effect applies to the caster's party or raid.
    pub fn targets_group(&self) -> bool {
        self.implicit_target.iter().any(|t| {
            matches!(
                t,
                ImplicitTarget::UnitCasterAreaParty
                    | ImplicitTarget::UnitCasterAreaRaid
                    | ImplicitTarget::UnitSrcAreaParty
                    | ImplicitTarget::UnitDestAreaParty
                    | ImplicitTarget::UnitTargetParty
                    | ImplicitTarget::UnitTargetRaid
                    | ImplicitTarget::UnitLasttargetAreaParty
            )
        }) || matches!(
            self.effect,
            SpellEffectName::ApplyAreaAuraParty | SpellEffectName::ApplyAreaAuraRaid
        )
    }

    /// `misc_value[0]` read as a school mask (auras 10, 13, 14, 22, 79, 87, ...).
    pub fn school_mask(&self) -> SpellSchoolMask {
        SpellSchoolMask::from_bits(self.misc_value[0] as u32)
    }

    /// `misc_value[0]` read as the modified operation of a spell modifier aura.
    pub fn mod_op(&self) -> SpellModOp {
        SpellModOp::from_id(self.misc_value[0] as u32)
    }

    /// `misc_value[0]` read as a power type (`ENERGIZE`, `PERIODIC_ENERGIZE`, `MOD_POWER_REGEN`).
    pub fn power_type(&self) -> PowerType {
        PowerType::from_id(self.misc_value[0])
    }

    /// `misc_value[0]` read as a form (`MOD_SHAPESHIFT`).
    pub fn shapeshift_form(&self) -> ShapeshiftForm {
        ShapeshiftForm::from_id(self.misc_value[0] as u32)
    }

    /// The effect's value for a caster of `caster_level`, before variance and combo points:
    /// `base_points + real_points_per_level × (min(level, levels.max) − levels.spell)`.
    pub fn value_at_level(&self, caster_level: u32, levels: &Levels) -> f32 {
        if self.real_points_per_level == 0.0 {
            return self.base_points;
        }
        let level = if levels.max > 0 {
            caster_level.min(levels.max)
        } else {
            caster_level
        };
        let delta = level as f32 - levels.spell as f32;
        self.base_points + self.real_points_per_level * delta.max(0.0)
    }

    /// The `(min, max)` damage range of `value` after `variance` (equal when there is none).
    pub fn variance_range(&self, value: f32) -> (f32, f32) {
        let spread = value * self.variance / 2.0;
        (value - spread, value + spread)
    }

    /// Whether this modifier effect's class mask selects a spell with `class_options`.
    pub fn class_mask_matches(&self, set: u32, class_options: &ClassOptions) -> bool {
        class_options.matches(set, &self.spell_class_mask)
    }
}

/// One spell, joined from every `Spell*` table (`data/SPELL_INSTRUCTIONS.md` §1.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpellRecord {
    /// `SpellName.ID`.
    pub id: u32,
    pub name: String,
    /// `Spell.NameSubtext_lang` ("Rank 7").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rank_text: String,
    /// `SkillLineAbility.SkillLine`; absent for hidden / triggered spells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_line: Option<u32>,
    /// `SkillLineAbility.ClassMask`: the class bit for trainable abilities, 0 for talent- or
    /// rune-granted ones.
    #[serde(default, skip_serializing_if = "is_default")]
    pub class_mask: u32,
    /// `SkillLineAbility.RaceMasks_0` (racials).
    #[serde(default, skip_serializing_if = "is_default")]
    pub race_mask: u32,
    /// `SkillLineAbility.SupercedesSpell`: the previous rank.
    #[serde(default, skip_serializing_if = "is_default")]
    pub supercedes: u32,
    /// `SkillLineAbility.AcquireMethod`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub acquire_method: u32,
    /// `SpellMisc.Attributes_0..16`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub attributes: [u32; 17],
    #[serde(default, skip_serializing_if = "is_default")]
    pub school_mask: SpellSchoolMask,
    /// `SpellCastTimes.Base`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub cast_time_ms: u32,
    /// `SpellDuration.Duration`; −1 = until cancelled, absent = no duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i32>,
    /// `SpellDuration.DurationPerResource`: the extra duration per combo point spent
    /// (finishers: Slice and Dice 3 s, Rupture 2 s); 0 for everything else.
    #[serde(default, skip_serializing_if = "is_default")]
    pub duration_per_resource_ms: i32,
    /// `SpellDuration.MaxDuration`, the cap of a per-combo-point duration; exported only with a
    /// `duration_per_resource_ms` (elsewhere it is a level-scaling cap the sim does not use).
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_duration_ms: i32,
    /// `SpellRange.RangeMax_0`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub range_yd: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub power: Vec<PowerCost>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cooldown: Cooldown,
    #[serde(default, skip_serializing_if = "is_default")]
    pub categories: Categories,
    /// `SpellShapeshift.ShapeshiftMask_0`; 0 = usable in any stance.
    #[serde(default, skip_serializing_if = "is_default")]
    pub shapeshift_mask: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub shapeshift_exclude: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub levels: Levels,
    #[serde(default, skip_serializing_if = "is_default")]
    pub aura_options: AuraOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_options: Option<ClassOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipped_items: Option<EquippedItems>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub aura_restrictions: AuraRestrictions,
    /// `SpellTargetRestrictions.MaxTargets`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_targets: u32,
    /// `SpellLabel` ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<u32>,
    /// `Spell.Description_lang`, informational only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectRecord>,
}

impl SpellRecord {
    /// A minimal record for building spells in code and tests.
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            rank_text: String::new(),
            skill_line: None,
            class_mask: 0,
            race_mask: 0,
            supercedes: 0,
            acquire_method: 0,
            attributes: [0; 17],
            school_mask: SpellSchoolMask::PHYSICAL,
            cast_time_ms: 0,
            duration_ms: None,
            duration_per_resource_ms: 0,
            max_duration_ms: 0,
            range_yd: 0.0,
            power: Vec::new(),
            cooldown: Cooldown::default(),
            categories: Categories::default(),
            shapeshift_mask: 0,
            shapeshift_exclude: 0,
            levels: Levels::default(),
            aura_options: AuraOptions::default(),
            class_options: None,
            equipped_items: None,
            aura_restrictions: AuraRestrictions::default(),
            max_targets: 0,
            labels: Vec::new(),
            description: String::new(),
            effects: Vec::new(),
        }
    }

    /// `Attributes_0` as flags.
    pub fn attr0(&self) -> SpellAttr0 {
        SpellAttr0::from_bits(self.attributes[0])
    }

    /// `Attributes_1` as flags.
    pub fn attr1(&self) -> SpellAttr1 {
        SpellAttr1::from_bits(self.attributes[1])
    }

    /// `Attributes_2` as flags.
    pub fn attr2(&self) -> SpellAttr2 {
        SpellAttr2::from_bits(self.attributes[2])
    }

    /// `Attributes_3` as flags.
    pub fn attr3(&self) -> SpellAttr3 {
        SpellAttr3::from_bits(self.attributes[3])
    }

    /// The caster must attack from behind the target (Backstab, Garrote, Ambush).
    pub fn requires_behind_target(&self) -> bool {
        self.attr2().contains(SpellAttr2::BEHIND_TARGET)
    }

    /// The weapon requirement (`SpellEquippedItems`, a weapon class) must be met by the weapon
    /// in `hand` itself: `MAIN_HAND` / `REQUIRES_OFF_HAND_WEAPON`, as the server's
    /// `Spell::CheckItems`. Other requirements are met by any equipped weapon.
    pub fn requires_weapon_in(&self, hand: Hand) -> bool {
        let weapon_class = self
            .equipped_items
            .is_some_and(|items| items.class == EquippedItems::WEAPON);
        weapon_class
            && self.attr3().contains(match hand {
                Hand::Mainhand => SpellAttr3::MAIN_HAND,
                Hand::Offhand => SpellAttr3::REQUIRES_OFF_HAND_WEAPON,
            })
    }

    /// The spell attacks with the off-hand weapon (retail's `OFF_ATTACK` attack type for
    /// `REQUIRES_OFF_HAND_WEAPON`): its weapon damage effects deal off-hand damage (Mutilate's
    /// off-hand strike).
    pub fn attacks_with_offhand(&self) -> bool {
        self.attr3().contains(SpellAttr3::REQUIRES_OFF_HAND_WEAPON)
    }

    /// Most of the cost comes back when the attack is missed, dodged or parried.
    pub fn refunds_power_on_miss(&self) -> bool {
        self.attr1().contains(SpellAttr1::DISCOUNT_POWER_ON_MISS)
    }

    /// Passive aura (talents, stance passives, proc auras).
    pub fn is_passive(&self) -> bool {
        self.attr0().contains(SpellAttr0::PASSIVE)
    }

    /// Hidden from the UI (marker buffs, proc payloads).
    pub fn is_hidden(&self) -> bool {
        self.attr0().contains(SpellAttr0::DO_NOT_DISPLAY)
    }

    /// Replaces the next white swing (Heroic Strike, Cleave).
    pub fn is_on_next_swing(&self) -> bool {
        self.attr0()
            .intersects(SpellAttr0::ON_NEXT_SWING_NO_DAMAGE | SpellAttr0::ON_NEXT_SWING)
    }

    /// Cannot be dodged, parried or blocked (Overpower).
    pub fn ignores_active_defense(&self) -> bool {
        self.attr0().contains(SpellAttr0::NO_ACTIVE_DEFENSE)
    }

    /// The rank number parsed from `rank_text` ("Rank 7" → 7).
    pub fn rank_number(&self) -> Option<u32> {
        self.rank_text.strip_prefix("Rank ")?.trim().parse().ok()
    }

    /// Whether the spell is in a class or racial skill line (as opposed to a hidden payload).
    pub fn is_in_spellbook(&self) -> bool {
        self.skill_line.is_some()
    }

    /// Whether the spell is an ability the player casts from the spellbook (what rotations
    /// name): in a skill line, displayed, not passive and not a triggered payload
    /// (`AcquireMethod` 3 without a class mask: the Flurry buff, Sweeping Strikes' extra hit).
    /// Hidden payloads (`DO_NOT_DISPLAY`) and passives are reached by id.
    pub fn is_ability(&self) -> bool {
        self.is_in_spellbook()
            && !self.is_hidden()
            && !self.is_passive()
            && !(self.acquire_method == 3 && self.class_mask == 0)
    }

    /// The learn level (`SpellLevels.BaseLevel`).
    pub fn learn_level(&self) -> u32 {
        self.levels.base
    }

    /// Whether the aura lasts until cancelled (`duration_ms` −1: stances).
    pub fn is_permanent(&self) -> bool {
        self.duration_ms == Some(-1)
    }

    /// The finite duration in milliseconds, if any.
    pub fn finite_duration_ms(&self) -> Option<u32> {
        self.duration_ms.and_then(|d| u32::try_from(d).ok())
    }

    /// The finite duration in milliseconds after spending `combo_points`:
    /// `duration + duration_per_resource × combo_points`, capped at `max_duration_ms` when
    /// there is one (Slice and Dice 6 s + 3 s per point, 21 s at 5).
    pub fn finite_duration_ms_with_combo_points(&self, combo_points: u32) -> Option<u32> {
        let base = self.finite_duration_ms()?;
        let Ok(per_point) = u32::try_from(self.duration_per_resource_ms) else {
            return Some(base);
        };
        let duration = base + per_point * combo_points;
        Some(match u32::try_from(self.max_duration_ms) {
            Ok(max) if max > 0 => duration.min(max),
            _ => duration,
        })
    }

    /// Whether the spell is on the global cooldown, and that cooldown's length.
    pub fn triggers_gcd(&self) -> bool {
        self.categories.start_recovery_category == GLOBAL_COOLDOWN_CATEGORY
            && self.cooldown.start_recovery_ms > 0
    }

    /// The primary resource cost (`OrderIndex` 0), if the spell costs anything.
    pub fn primary_power(&self) -> Option<&PowerCost> {
        self.power.first()
    }

    /// The cost of `power_type`, in stored units.
    pub fn power_cost(&self, power_type: PowerType) -> i32 {
        self.power
            .iter()
            .filter(|p| p.power_type == power_type)
            .map(|p| p.cost)
            .sum()
    }

    /// The stances the spell can be used in (`shapeshift_mask` decoded); empty = any.
    pub fn shapeshift_forms(&self) -> Vec<ShapeshiftForm> {
        ShapeshiftForm::from_mask(self.shapeshift_mask)
    }

    /// The effect with `index`.
    pub fn effect(&self, index: u32) -> Option<&EffectRecord> {
        self.effects.iter().find(|e| e.index == index)
    }

    /// The spells this spell's effects trigger, in effect order without duplicates.
    pub fn trigger_spells(&self) -> Vec<u32> {
        let mut ids = Vec::new();
        for effect in &self.effects {
            if effect.trigger_spell != 0 && !ids.contains(&effect.trigger_spell) {
                ids.push(effect.trigger_spell);
            }
        }
        ids
    }

    /// The spells an `OVERRIDE_ACTIONBAR_SPELLS` aura replaces, as `(replaced, replacement)`.
    pub fn actionbar_overrides(&self) -> Vec<(u32, u32)> {
        self.effects
            .iter()
            .filter(|e| e.is_apply_aura() && e.aura == AuraType::OverrideActionbarSpells)
            .map(|e| (e.misc_value[0] as u32, e.base_points as u32))
            .collect()
    }

    /// Whether the spell awards combo points (an `ENERGIZE` of combo points: a builder).
    pub fn awards_combo_points(&self) -> bool {
        self.effects.iter().any(|e| {
            e.effect == SpellEffectName::Energize && e.power_type() == PowerType::ComboPoints
        })
    }

    /// Whether the spell has an apply-aura effect at all.
    pub fn applies_aura(&self) -> bool {
        self.effects.iter().any(EffectRecord::is_apply_aura)
    }

    /// Whether the spell is a proc aura: it reacts to `proc_type_mask` events with a trigger
    /// or a scripted aura.
    pub fn is_proc_aura(&self) -> bool {
        !self.aura_options.proc_type_mask.is_empty()
            && self
                .effects
                .iter()
                .any(|e| e.is_proc_trigger() || (e.is_apply_aura() && e.aura == AuraType::Dummy))
    }

    /// Whether the spell needs hand-written logic for at least one effect.
    pub fn has_scripted_effects(&self) -> bool {
        self.effects.iter().any(EffectRecord::is_scripted)
    }

    /// Whether `modifier` (an `ADD_*_MODIFIER` effect of a spell in `set`) applies to this spell.
    pub fn is_modified_by(&self, set: u32, modifier: &EffectRecord) -> bool {
        self.class_options
            .as_ref()
            .is_some_and(|c| modifier.class_mask_matches(set, c))
    }

    /// Structural checks that do not need other spells: effect indices are ascending and unique
    /// (gaps are fine: the exporter drops the effects the simulator has no use for, keeping the
    /// table indices of the rest).
    pub fn validate(&self) -> Result<(), SpellDbError> {
        if self.name.is_empty() {
            return Err(SpellDbError::Invalid {
                spell: self.id,
                message: "the name is empty".into(),
            });
        }
        for pair in self.effects.windows(2) {
            if pair[1].index <= pair[0].index {
                return Err(SpellDbError::Invalid {
                    spell: self.id,
                    message: format!(
                        "effect indices must be ascending, found {} after {}",
                        pair[1].index, pair[0].index
                    ),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpellDbError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("spell {0} is defined twice")]
    Duplicate(u32),
    #[error("spell {spell}: {message}")]
    Invalid { spell: u32, message: String },
    #[error("spell {spell} ({field}) refers to unknown spell {target}")]
    UnknownReference {
        spell: u32,
        field: &'static str,
        target: u32,
    },
    #[error("files were exported from different builds: {0} and {1}")]
    BuildMismatch(String, String),
    #[error(transparent)]
    Override(#[from] OverrideError),
}

/// An effect the sim cannot interpret: reported, never fatal (the spell loads, casting it
/// fails).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub spell: u32,
    pub effect: u32,
    pub reason: String,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spell {} effect {}: {}",
            self.spell, self.effect, self.reason
        )
    }
}

/// Every loaded spell record, indexed by id, name, rank chain and class.
///
/// Records are shared through `Arc` so the runtime can hold them without copying.
#[derive(Debug, Clone, Default)]
pub struct SpellDb {
    build: Option<String>,
    spells: HashMap<u32, Arc<SpellRecord>>,
    by_name: HashMap<String, Vec<u32>>,
    by_class: BTreeMap<Option<PlayerClass>, Vec<u32>>,
    /// Ids from files with `learnable: false` (the external buff auras).
    unlearnable: HashSet<u32>,
    next_rank: HashMap<u32, u32>,
    /// The spells another spell's `TRIGGER_SPELL` effect casts (Mutilate's strikes).
    triggered: HashSet<u32>,
    overrides: Overrides,
}

impl SpellDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads every `*.yaml` / `*.yml` file directly in `spells_dir` (sorted by name), then the
    /// overrides in `spells_dir/overrides/`, and checks cross references.
    pub fn load(spells_dir: &Path) -> Result<Self, SpellDbError> {
        let mut db = Self::new();
        db.overrides = Overrides::load(&spells_dir.join(OVERRIDES_DIR))?;
        let mut paths: Vec<PathBuf> = fs::read_dir(spells_dir)
            .map_err(|source| SpellDbError::Io {
                path: spells_dir.to_path_buf(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        paths.sort();
        for path in paths {
            db.load_file(&path)?;
        }
        db.check_references()?;
        Ok(db)
    }

    /// Adds the records of one YAML file.
    pub fn load_file(&mut self, path: &Path) -> Result<(), SpellDbError> {
        let text = fs::read_to_string(path).map_err(|source| SpellDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: SpellFile = serde_yaml::from_str(&text).map_err(|source| SpellDbError::Yaml {
            path: path.to_path_buf(),
            source,
        })?;
        self.add_file(file)
    }

    /// Validates and adds the records of one file.
    pub fn add_file(&mut self, file: SpellFile) -> Result<(), SpellDbError> {
        if !file.build.is_empty() {
            match &self.build {
                Some(build) if *build != file.build => {
                    return Err(SpellDbError::BuildMismatch(
                        build.clone(),
                        file.build.clone(),
                    ));
                }
                Some(_) => {}
                None => self.build = Some(file.build.clone()),
            }
        }
        for record in &file.spells {
            record.validate()?;
            if self.spells.contains_key(&record.id)
                || file.spells.iter().filter(|r| r.id == record.id).count() > 1
            {
                return Err(SpellDbError::Duplicate(record.id));
            }
        }
        for record in file.spells {
            if !file.learnable {
                self.unlearnable.insert(record.id);
            }
            self.add_record(file.class, record);
        }
        Ok(())
    }

    /// Adds one record (no validation; `check_references` covers the cross references).
    pub fn add(
        &mut self,
        class: Option<PlayerClass>,
        record: SpellRecord,
    ) -> Result<(), SpellDbError> {
        record.validate()?;
        if self.spells.contains_key(&record.id) {
            return Err(SpellDbError::Duplicate(record.id));
        }
        self.add_record(class, record);
        Ok(())
    }

    fn add_record(&mut self, class: Option<PlayerClass>, record: SpellRecord) {
        let id = record.id;
        let names = self.by_name.entry(record.name.clone()).or_default();
        names.push(id);
        names.sort_unstable();
        self.by_class.entry(class).or_default().push(id);
        if record.supercedes != 0 {
            self.next_rank.insert(record.supercedes, id);
        }
        self.triggered.extend(
            record
                .trigger_spells()
                .into_iter()
                .filter(|&triggered| triggered != id),
        );
        self.spells.insert(id, Arc::new(record));
    }

    /// Whether another spell casts `id` through a `TRIGGER_SPELL` effect: a payload, even
    /// when it looks like a spellbook ability (Mutilate's strikes carry Mutilate's name,
    /// rank and skill line).
    pub fn is_triggered(&self, id: u32) -> bool {
        self.triggered.contains(&id)
    }

    /// The hand-written overrides.
    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

    /// Replaces the overrides (checked against the records by `check_references`).
    pub fn set_overrides(&mut self, overrides: Overrides) {
        self.overrides = overrides;
    }

    /// Adds the overrides of one file (checked against the records by `check_references`).
    pub fn add_overrides(&mut self, file: OverrideFile) -> Result<(), SpellDbError> {
        self.overrides.add_file(file)?;
        Ok(())
    }

    /// Checks that every `supercedes` and `trigger_spell` reference points at a loaded spell,
    /// and that every override names a loaded spell, an existing effect and existing spells.
    pub fn check_references(&self) -> Result<(), SpellDbError> {
        self.check_record_references()?;
        self.check_override_references()
    }

    fn check_override_references(&self) -> Result<(), SpellDbError> {
        for spell_override in self.overrides.all() {
            let id = spell_override.id;
            let Some(record) = self.spells.get(&id) else {
                return Err(SpellDbError::UnknownReference {
                    spell: id,
                    field: "override",
                    target: id,
                });
            };
            for script in &spell_override.effects {
                if record.effect(script.index).is_none() {
                    return Err(SpellDbError::Invalid {
                        spell: id,
                        message: format!(
                            "the override scripts effect {}, but the spell has {} effects",
                            script.index,
                            record.effects.len()
                        ),
                    });
                }
            }
            if let Some(proc) = spell_override.proc {
                let invalid = |message: &str| SpellDbError::Invalid {
                    spell: id,
                    message: message.to_owned(),
                };
                if proc.chance.is_some_and(|c| !(c > 0.0 && c <= 100.0)) {
                    return Err(invalid("the override's proc chance is not in (0, 100] %"));
                }
                if proc.ppm.is_some_and(|ppm| ppm <= 0.0) {
                    return Err(invalid("the override's procs per minute are not positive"));
                }
                if proc.chance.is_some() && proc.ppm.is_some() {
                    return Err(invalid(
                        "the override gives both a proc chance and procs per minute",
                    ));
                }
                if proc.chance_effect.is_some() && proc.has_rate() {
                    return Err(invalid(
                        "the override takes the proc chance from an effect and gives a rate",
                    ));
                }
            }
            if let Some(index) = spell_override.proc.and_then(|p| p.chance_effect)
                && !record
                    .effect(index)
                    .is_some_and(EffectRecord::is_apply_aura)
            {
                return Err(SpellDbError::Invalid {
                    spell: id,
                    message: format!(
                        "the override takes the proc chance from effect {index}, which is not an aura effect of the spell"
                    ),
                });
            }
            for target in spell_override.referenced_spells() {
                if !self.spells.contains_key(&target) {
                    return Err(SpellDbError::UnknownReference {
                        spell: id,
                        field: "override",
                        target,
                    });
                }
            }
        }
        Ok(())
    }

    fn check_record_references(&self) -> Result<(), SpellDbError> {
        let mut ids: Vec<u32> = self.spells.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let record = &self.spells[&id];
            if record.supercedes != 0 && !self.spells.contains_key(&record.supercedes) {
                return Err(SpellDbError::UnknownReference {
                    spell: id,
                    field: "supercedes",
                    target: record.supercedes,
                });
            }
            for target in record.trigger_spells() {
                if !self.spells.contains_key(&target) {
                    return Err(SpellDbError::UnknownReference {
                        spell: id,
                        field: "trigger_spell",
                        target,
                    });
                }
            }
            for (replaced, replacement) in record.actionbar_overrides() {
                for target in [replaced, replacement] {
                    if !self.spells.contains_key(&target) {
                        return Err(SpellDbError::UnknownReference {
                            spell: id,
                            field: "actionbar override",
                            target,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Every effect the sim cannot interpret, sorted by spell and effect: unknown effect or aura
    /// ids from a newer dump (unless an override script documents them), and scripted effects (`DUMMY`, `PERIODIC_DUMMY`,
    /// `OVERRIDE_CLASS_SCRIPTS`) without a script in the overrides. Spells the overrides mark
    /// `IGNORED` are skipped.
    pub fn unsupported(&self) -> Vec<Unsupported> {
        self.records()
            .into_iter()
            .filter(|record| !self.overrides.has_sim_flag(record.id, SimFlag::Ignored))
            .flat_map(|record| self.unsupported_effects(record))
            .collect()
    }

    /// The effects of `record` the sim cannot interpret (see [`SpellDb::unsupported`]), whether
    /// or not the spell is `IGNORED`.
    pub fn unsupported_effects(&self, record: &SpellRecord) -> Vec<Unsupported> {
        let mut report = Vec::new();
        for effect in &record.effects {
            if effect.is_discarded() {
                continue;
            }
            let scripted = self
                .overrides
                .effect_script(record.id, effect.index)
                .is_some();
            let reason = if !effect.effect.is_known() {
                Some(format!("unknown effect id {}", effect.effect.id()))
            } else if effect.is_apply_aura() && !effect.aura.is_known() && !scripted {
                // A script (NO_OP) documents an unknown aura as handled (Bloodthrill's 560).
                Some(format!("unknown aura id {}", effect.aura.id()))
            } else if effect.is_scripted() && !scripted {
                Some(format!(
                    "{} needs a script in the overrides",
                    if effect.is_apply_aura() {
                        format!("aura {}", effect.aura)
                    } else {
                        format!("effect {}", effect.effect)
                    }
                ))
            } else {
                None
            };
            if let Some(reason) = reason {
                report.push(Unsupported {
                    spell: record.id,
                    effect: effect.index,
                    reason,
                });
            }
        }
        report
    }

    /// The build the files were exported from.
    pub fn build(&self) -> Option<&str> {
        self.build.as_deref()
    }

    pub fn len(&self) -> usize {
        self.spells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spells.is_empty()
    }

    /// The record with `id`.
    pub fn get(&self, id: u32) -> Option<&Arc<SpellRecord>> {
        self.spells.get(&id)
    }

    /// Whether `id` is loaded.
    pub fn contains(&self, id: u32) -> bool {
        self.spells.contains_key(&id)
    }

    /// Every record, sorted by id.
    pub fn records(&self) -> Vec<&Arc<SpellRecord>> {
        let mut records: Vec<_> = self.spells.values().collect();
        records.sort_by_key(|r| r.id);
        records
    }

    /// The ids of every spell named `name` (all ranks and hidden payloads alike), ascending.
    pub fn ids_by_name(&self, name: &str) -> &[u32] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// The records named `name`, by ascending id.
    pub fn by_name(&self, name: &str) -> Vec<&Arc<SpellRecord>> {
        self.ids_by_name(name)
            .iter()
            .filter_map(|id| self.spells.get(id))
            .collect()
    }

    /// The class a spell was loaded for (`Some(None)` for class-independent files).
    pub fn class_of(&self, id: u32) -> Option<Option<PlayerClass>> {
        self.by_class
            .iter()
            .find(|(_, ids)| ids.contains(&id))
            .map(|(class, _)| *class)
    }

    /// The ids loaded for `class` (`None` = the class-independent files), in file order.
    pub fn ids_of_class(&self, class: Option<PlayerClass>) -> &[u32] {
        self.by_class.get(&class).map_or(&[], Vec::as_slice)
    }

    /// Whether characters learn spell `id` (`false` for the auras of a `learnable: false` file:
    /// the external buffs, which only ever become buffs).
    pub fn is_learnable(&self, id: u32) -> bool {
        !self.unlearnable.contains(&id)
    }

    /// The spells `class` can have: its own records plus the class-independent ones, restricted
    /// to skill-line entries (hidden payloads are reached through their triggers).
    pub fn spellbook(&self, class: PlayerClass) -> Vec<&Arc<SpellRecord>> {
        let mut records: Vec<_> = self
            .ids_of_class(Some(class))
            .iter()
            .chain(self.ids_of_class(None))
            .filter_map(|id| self.spells.get(id))
            .filter(|r| r.is_in_spellbook())
            .collect();
        records.sort_by_key(|r| r.id);
        records
    }

    /// The rank that supersedes `id`, if any.
    pub fn next_rank(&self, id: u32) -> Option<u32> {
        self.next_rank.get(&id).copied()
    }

    /// The first rank of the chain `id` belongs to.
    pub fn first_rank(&self, id: u32) -> u32 {
        let mut current = id;
        let mut seen = 0;
        while let Some(record) = self.spells.get(&current) {
            if record.supercedes == 0 || !self.spells.contains_key(&record.supercedes) {
                break;
            }
            current = record.supercedes;
            seen += 1;
            if seen > self.spells.len() {
                break; // a cycle in the data; stop rather than spin
            }
        }
        current
    }

    /// The whole rank chain `id` belongs to, from the first rank to the last.
    pub fn rank_chain(&self, id: u32) -> Vec<u32> {
        let mut chain = vec![self.first_rank(id)];
        while let Some(next) = self.next_rank(*chain.last().expect("non-empty")) {
            if chain.contains(&next) {
                break;
            }
            chain.push(next);
        }
        chain
    }

    /// The rank chains of every spell named `name`: one chain per independent line of ranks
    /// (Slam has two in Forever: the trainable one and Improved Slam's replacements), sorted by
    /// first rank id. Hidden payloads that share the name are single-element chains.
    pub fn rank_chains(&self, name: &str) -> Vec<Vec<u32>> {
        let mut chains: Vec<Vec<u32>> = Vec::new();
        for &id in self.ids_by_name(name) {
            let chain = self.rank_chain(id);
            if !chains.contains(&chain) {
                chains.push(chain);
            }
        }
        chains.sort();
        chains
    }

    /// The spells of family `set` whose class mask intersects `mask`, ascending: the targets of
    /// an `ADD_FLAT_MODIFIER` / `ADD_PCT_MODIFIER` effect.
    pub fn family_matches(&self, set: u32, mask: &[u32; 4]) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .spells
            .values()
            .filter(|r| r.class_options.is_some_and(|c| c.matches(set, mask)))
            .map(|r| r.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// The spells `modifier` (an effect of a spell of family `set`) applies to.
    pub fn modified_spells(&self, set: u32, modifier: &EffectRecord) -> Vec<u32> {
        self.family_matches(set, &modifier.spell_class_mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WARRIOR_YAML: &str = r#"
build: 1.60.1.70009
class: WARRIOR
spells:
  - id: 78
    name: Heroic Strike
    rank_text: Rank 1
    skill_line: 26
    class_mask: 1
    acquire_method: 2
    attributes: [327700, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    school_mask: PHYSICAL
    power: [{ type: RAGE, cost: 150 }]
    categories: { defense_type: MELEE }
    levels: { base: 1, spell: 1 }
    class_options: { set: 4, mask: [64, 0, 0, 0] }
    equipped_items: { class: 2, subclass_mask: 173555 }
    effects:
      - { index: 0, effect: WEAPON_DAMAGE_NOSCHOOL, base_points: 11, implicit_target: [UNIT_TARGET_ENEMY, NONE] }
  - id: 284
    name: Heroic Strike
    rank_text: Rank 2
    skill_line: 26
    class_mask: 1
    supercedes: 78
    attributes: [0x50014, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    power: [{ type: 1, cost: 150 }]
    levels: { base: 8, spell: 8 }
    class_options: { set: 4, mask: [64, 0, 0, 0] }
    effects:
      - { index: 0, effect: 17, base_points: 21, implicit_target: [6, 0] }
  - id: 12294
    name: Mortal Strike
    rank_text: Rank 1
    skill_line: 26
    attributes: [327696, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    duration_ms: 10000
    range_yd: 5
    power: [{ type: RAGE, cost: 300 }]
    cooldown: { category_recovery_ms: 6000, start_recovery_ms: 1500 }
    categories: { category: 971, start_recovery_category: 133, defense_type: MELEE }
    levels: { base: 40, spell: 40 }
    class_options: { set: 4, mask: [33554432, 0, 0, 0] }
    effects:
      - { index: 0, effect: APPLY_AURA, aura: 118, base_points: -50, misc_value: [127, 0], implicit_target: [UNIT_TARGET_ENEMY, NONE] }
      - { index: 1, effect: NORMALIZED_WEAPON_DMG, base_points: 85, implicit_target: [UNIT_TARGET_ENEMY, NONE] }
  - id: 12282
    name: Improved Heroic Strike
    skill_line: 26
    attributes: [0x1d0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    class_options: { set: 4 }
    effects:
      - { index: 0, effect: APPLY_AURA, aura: ADD_FLAT_MODIFIER, base_points: -10, misc_value: [14, 0], spell_class_mask: [64, 0, 0, 0], implicit_target: [UNIT_CASTER, NONE] }
  - id: 12834
    name: Deep Wounds
    skill_line: 26
    attributes: [0x1d0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    aura_options: { proc_chance: 100, proc_type_mask: 0x11154 }
    equipped_items: { class: 2, subclass_mask: 173555 }
    effects:
      - { index: 0, effect: APPLY_AURA, aura: PROC_TRIGGER_SPELL, trigger_spell: 12162, implicit_target: [UNIT_CASTER, NONE] }
  - id: 12162
    name: Deep Wounds
    attributes: [0x40190, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    categories: { mechanic: BLEED }
    effects:
      - { index: 0, effect: DUMMY, implicit_target: [UNIT_TARGET_ENEMY, NONE] }
  - id: 2458
    name: Berserker Stance
    skill_line: 256
    class_mask: 1
    duration_ms: -1
    cooldown: { category_recovery_ms: 1000 }
    categories: { category: 47, defense_type: MELEE }
    effects:
      - { index: 0, effect: APPLY_AURA, aura: MOD_SHAPESHIFT, misc_value: [19, 0], implicit_target: [UNIT_CASTER, NONE] }
  - id: 25288
    name: Revenge
    rank_text: Rank 6
    skill_line: 257
    class_mask: 1
    shapeshift_mask: 131072
    levels: { base: 60, spell: 60 }
    aura_restrictions: { caster_aura_state: DEFENSIVE }
    effects:
      - { index: 0, effect: SCHOOL_DAMAGE, base_points: 153, variance: 0.2, implicit_target: [UNIT_TARGET_ENEMY, NONE] }
  - id: 5242
    name: Battle Shout
    rank_text: Rank 2
    skill_line: 26
    class_mask: 1
    duration_ms: 180000
    levels: { base: 12, spell: 12, max: 21 }
    effects:
      - { index: 0, effect: APPLY_AURA, aura: MOD_ATTACK_POWER, base_points: 21, real_points_per_level: 0.3, radius_yd: [20, 0], implicit_target: [UNIT_CASTER_AREA_PARTY, NONE] }
"#;

    const RACIAL_YAML: &str = r#"
build: 1.60.1.70009
spells:
  - id: 20572
    name: Blood Fury
    skill_line: 125
    race_mask: 2
    acquire_method: 2
    duration_ms: 15000
    cooldown: { recovery_ms: 120000 }
    effects:
      - { index: 0, effect: APPLY_AURA, aura: MOD_ATTACK_POWER_PCT, base_points: 10, implicit_target: [UNIT_CASTER, NONE] }
"#;

    const OVERRIDES_YAML: &str = r#"
overrides:
  - id: 12834
    proc: { hit_mask: [CRITICAL] }
  - id: 12162
    effects: [{ index: 0, script: DEEP_WOUNDS_BLEED, params: { duration_spell: 2458 } }]
  - id: 78
    threat: { flat: 20 }
"#;

    fn db() -> SpellDb {
        let mut db = SpellDb::new();
        db.add_file(serde_yaml::from_str(WARRIOR_YAML).unwrap())
            .unwrap();
        db.add_file(serde_yaml::from_str(RACIAL_YAML).unwrap())
            .unwrap();
        let mut overrides = Overrides::new();
        overrides
            .add_file(serde_yaml::from_str(OVERRIDES_YAML).unwrap())
            .unwrap();
        db.set_overrides(overrides);
        db.check_references().unwrap();
        db
    }

    #[test]
    fn overrides_sit_next_to_the_records() {
        use crate::spell::overrides::{ProcHitMask, ScriptKind, SpellOverride};

        let db = db();
        assert_eq!(db.overrides().proc_hit_mask(12834), ProcHitMask::CRITICAL);
        assert_eq!(db.overrides().proc_hit_mask(12319), ProcHitMask::LANDED);
        assert_eq!(
            db.overrides().effect_script(12162, 0).unwrap().script,
            ScriptKind::DeepWoundsBleed
        );
        assert_eq!(db.overrides().threat(78).flat, 20.0);
        assert_eq!(
            db.get(78).unwrap().effects.len(),
            1,
            "records are untouched"
        );
        assert!(db.unsupported().is_empty(), "{:?}", db.unsupported());

        // A scripted effect without a script is reported, not rejected.
        let mut bare = SpellDb::new();
        bare.add_file(serde_yaml::from_str(WARRIOR_YAML).unwrap())
            .unwrap();
        bare.check_references().unwrap();
        let report = bare.unsupported();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].spell, 12162);
        assert_eq!(
            report[0].to_string(),
            "spell 12162 effect 0: effect DUMMY needs a script in the overrides"
        );
        let mut ignored = Overrides::new();
        let mut spell_override = SpellOverride::new(12162);
        spell_override.sim_flags.push(SimFlag::Ignored);
        ignored.add(spell_override).unwrap();
        bare.set_overrides(ignored);
        assert!(bare.unsupported().is_empty());

        // Unknown ids from a newer dump are reported too.
        let mut future = SpellRecord::new(1, "Future");
        let mut effect = EffectRecord::new(0, SpellEffectName::Unknown(999));
        future.effects.push(effect.clone());
        effect.index = 1;
        effect.effect = SpellEffectName::ApplyAura;
        effect.aura = AuraType::Unknown(466);
        future.effects.push(effect);
        bare.add(None, future).unwrap();
        let reasons: Vec<String> = bare
            .unsupported()
            .iter()
            .map(|u| u.reason.clone())
            .collect();
        assert_eq!(reasons, ["unknown effect id 999", "unknown aura id 466"]);
    }

    #[test]
    fn overrides_must_name_loaded_spells_and_effects() {
        use crate::spell::overrides::{EffectScript, ScriptKind, ScriptParams, SpellOverride};

        let mut db = SpellDb::new();
        db.add_file(serde_yaml::from_str(WARRIOR_YAML).unwrap())
            .unwrap();

        let mut overrides = Overrides::new();
        overrides.add(SpellOverride::new(4242)).unwrap();
        db.set_overrides(overrides);
        assert!(matches!(
            db.check_references(),
            Err(SpellDbError::UnknownReference {
                spell: 4242,
                field: "override",
                target: 4242
            })
        ));

        let mut overrides = Overrides::new();
        let mut spell_override = SpellOverride::new(12294);
        spell_override.effects.push(EffectScript {
            index: 2,
            script: ScriptKind::NoOp,
            params: ScriptParams::default(),
        });
        overrides.add(spell_override).unwrap();
        db.set_overrides(overrides);
        assert!(matches!(
            db.check_references(),
            Err(SpellDbError::Invalid { spell: 12294, .. })
        ));

        let mut overrides = Overrides::new();
        let mut spell_override = SpellOverride::new(2458);
        spell_override.stance_passive = Some(7381);
        overrides.add(spell_override).unwrap();
        db.set_overrides(overrides);
        assert!(matches!(
            db.check_references(),
            Err(SpellDbError::UnknownReference {
                spell: 2458,
                field: "override",
                target: 7381
            })
        ));
    }

    #[test]
    fn records_read_names_and_numbers_alike() {
        let db = db();
        let hs1 = db.get(78).unwrap();
        let hs2 = db.get(284).unwrap();
        assert_eq!(hs1.attributes[0], 0x50014);
        assert_eq!(hs2.attributes[0], 0x50014);
        assert_eq!(hs1.power[0].power_type, PowerType::Rage);
        assert_eq!(hs2.power[0].power_type, PowerType::Rage);
        assert_eq!(hs1.effects[0].effect, SpellEffectName::WeaponDamageNoschool);
        assert_eq!(hs2.effects[0].effect, SpellEffectName::WeaponDamageNoschool);
        assert_eq!(
            hs2.effects[0].implicit_target,
            [ImplicitTarget::UnitTargetEnemy, ImplicitTarget::None]
        );
        assert_eq!(hs1.school_mask, SpellSchoolMask::PHYSICAL);
        assert_eq!(hs2.school_mask, SpellSchoolMask::empty(), "omitted field");
        assert_eq!(
            db.get(12834).unwrap().aura_options.proc_type_mask,
            ProcFlags::DEAL_ANY_DAMAGE
        );
        assert_eq!(db.build(), Some("1.60.1.70009"));
        assert_eq!(db.len(), 10);
    }

    #[test]
    fn finisher_durations_grow_per_combo_point_up_to_the_cap() {
        // Slice and Dice (SpellDuration 185: 6 s + 3 s per point, max 21 s).
        let mut slice = SpellRecord::new(5171, "Slice and Dice");
        slice.duration_ms = Some(6_000);
        slice.duration_per_resource_ms = 3_000;
        slice.max_duration_ms = 21_000;
        assert_eq!(slice.finite_duration_ms_with_combo_points(0), Some(6_000));
        assert_eq!(slice.finite_duration_ms_with_combo_points(1), Some(9_000));
        assert_eq!(slice.finite_duration_ms_with_combo_points(5), Some(21_000));
        assert_eq!(slice.finite_duration_ms_with_combo_points(6), Some(21_000));
        // Without a per-point duration the combo points change nothing.
        let mut rend = SpellRecord::new(772, "Rend");
        rend.duration_ms = Some(9_000);
        assert_eq!(rend.finite_duration_ms_with_combo_points(5), Some(9_000));
        rend.duration_ms = Some(-1);
        assert_eq!(rend.finite_duration_ms_with_combo_points(5), None);
    }

    #[test]
    fn record_helpers_decode_the_table_columns() {
        let db = db();
        let hs = db.get(78).unwrap();
        assert!(hs.is_on_next_swing());
        assert!(!hs.is_passive());
        assert!(!hs.triggers_gcd());
        assert_eq!(hs.rank_number(), Some(1));
        assert_eq!(hs.primary_power().unwrap().displayed_cost(), 15.0);
        assert_eq!(hs.power_cost(PowerType::Rage), 150);
        assert_eq!(hs.power_cost(PowerType::Mana), 0);
        assert!(hs.is_in_spellbook());
        assert!(hs.is_ability());

        let ms = db.get(12294).unwrap();
        assert!(ms.triggers_gcd());
        assert_eq!(ms.finite_duration_ms(), Some(10_000));
        assert!(!ms.is_permanent());
        assert!(ms.applies_aura());
        assert!(!ms.has_scripted_effects());
        assert_eq!(ms.effect(1).unwrap().base_points, 85.0);
        assert!(ms.effect(2).is_none());
        assert!(ms.shapeshift_forms().is_empty());
        assert_eq!(ms.rank_number(), Some(1));

        let stance = db.get(2458).unwrap();
        assert!(stance.is_permanent());
        assert_eq!(stance.finite_duration_ms(), None);
        assert_eq!(
            stance.effects[0].shapeshift_form(),
            ShapeshiftForm::BerserkerStance
        );

        let revenge = db.get(25288).unwrap();
        assert_eq!(
            revenge.shapeshift_forms(),
            [ShapeshiftForm::DefensiveStance]
        );
        assert_eq!(
            revenge.aura_restrictions.caster_aura_state,
            AuraState::Defensive
        );
        let (min, max) = revenge.effects[0].variance_range(153.0);
        assert!((min - 137.7).abs() < 1e-3 && (max - 168.3).abs() < 1e-3);
        assert_eq!(revenge.rank_number(), Some(6));

        let payload = db.get(12162).unwrap();
        assert!(payload.is_hidden());
        assert!(!payload.is_in_spellbook());
        assert!(!payload.is_ability());
        assert!(payload.has_scripted_effects());
        assert_eq!(payload.categories.mechanic, Mechanic::Bleed);
        assert_eq!(payload.rank_number(), None);

        let deep_wounds = db.get(12834).unwrap();
        assert!(deep_wounds.is_passive() && deep_wounds.is_proc_aura());
        assert!(!deep_wounds.is_ability(), "passives are not cast");
        assert_eq!(deep_wounds.trigger_spells(), [12162]);
        assert!(deep_wounds.effects[0].is_proc_trigger());
        assert!(!deep_wounds.effects[0].is_scripted());
        assert!(deep_wounds.effects[0].targets_caster());
        assert!(!deep_wounds.effects[0].targets_enemy());

        let improved_hs = db.get(12282).unwrap();
        let modifier = &improved_hs.effects[0];
        assert!(modifier.is_spell_modifier());
        assert_eq!(modifier.mod_op(), SpellModOp::PowerCost0);
        assert!(hs.is_modified_by(4, modifier));
        assert!(!ms.is_modified_by(4, modifier));
        assert!(!hs.is_modified_by(5, modifier));
        assert!(!improved_hs.is_proc_aura());

        let shout = db.get(5242).unwrap();
        let ap = &shout.effects[0];
        assert!(ap.targets_group());
        assert!(!ap.is_periodic());
        assert_eq!(ap.value_at_level(12, &shout.levels), 21.0);
        assert!((ap.value_at_level(20, &shout.levels) - 23.4).abs() < 1e-5);
        assert!(
            (ap.value_at_level(60, &shout.levels) - 23.7).abs() < 1e-5,
            "capped at max level 21"
        );
        assert_eq!(
            ap.value_at_level(5, &shout.levels),
            21.0,
            "never below base"
        );
        assert_eq!(ms.effects[1].value_at_level(60, &ms.levels), 85.0);
        assert_eq!(ms.effects[0].school_mask(), SpellSchoolMask::ALL);
        assert_eq!(
            db.get(20572).unwrap().effects[0].power_type(),
            PowerType::Mana
        );
    }

    #[test]
    fn indexes_by_name_class_and_rank_chain() {
        let db = db();
        assert_eq!(db.ids_by_name("Heroic Strike"), [78, 284]);
        assert_eq!(db.ids_by_name("Deep Wounds"), [12162, 12834]);
        assert!(db.ids_by_name("Nope").is_empty());
        assert_eq!(db.by_name("Mortal Strike").len(), 1);
        assert_eq!(db.class_of(78), Some(Some(PlayerClass::Warrior)));
        assert_eq!(db.class_of(20572), Some(None));
        assert_eq!(db.class_of(1), None);
        assert_eq!(db.ids_of_class(None), [20572]);
        assert_eq!(db.ids_of_class(Some(PlayerClass::Rogue)), []);
        let spellbook: Vec<u32> = db
            .spellbook(PlayerClass::Warrior)
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(
            spellbook,
            [78, 284, 2458, 5242, 12282, 12294, 12834, 20572, 25288]
        );
        assert!(
            db.spellbook(PlayerClass::Rogue)
                .iter()
                .all(|r| r.id == 20572)
        );

        assert_eq!(db.next_rank(78), Some(284));
        assert_eq!(db.next_rank(284), None);
        assert_eq!(db.first_rank(284), 78);
        assert_eq!(db.first_rank(78), 78);
        assert_eq!(db.rank_chain(284), [78, 284]);
        assert_eq!(db.rank_chain(78), [78, 284]);
        assert_eq!(db.rank_chain(12294), [12294]);
        assert_eq!(db.rank_chains("Heroic Strike"), [vec![78, 284]]);
        assert_eq!(db.rank_chains("Deep Wounds"), [vec![12162], vec![12834]]);
        let records: Vec<u32> = db.records().iter().map(|r| r.id).collect();
        assert!(records.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn family_masks_resolve_modifier_targets() {
        let db = db();
        let improved_hs = db.get(12282).unwrap();
        assert_eq!(db.modified_spells(4, &improved_hs.effects[0]), [78, 284]);
        assert_eq!(db.family_matches(4, &[33554432, 0, 0, 0]), [12294]);
        assert!(db.family_matches(4, &[0, 0, 0, 0]).is_empty());
        assert!(db.family_matches(5, &[64, 0, 0, 0]).is_empty());
        assert!(db.get(12834).unwrap().class_options.is_none());
    }

    #[test]
    fn duplicates_and_bad_effect_indices_are_rejected() {
        let mut db = SpellDb::new();
        let mut file: SpellFile = serde_yaml::from_str(WARRIOR_YAML).unwrap();
        db.add_file(file.clone()).unwrap();
        assert!(matches!(
            db.add_file(file.clone()),
            Err(SpellDbError::Duplicate(78))
        ));
        assert!(matches!(
            db.add(None, SpellRecord::new(78, "Again")),
            Err(SpellDbError::Duplicate(78))
        ));
        file.spells[0].effects[0].index = 1;
        let mut fresh = SpellDb::new();
        fresh
            .add_file(file.clone())
            .expect("gaps in the effect indices are fine (pruned effects)");
        file.spells[0]
            .effects
            .push(EffectRecord::new(1, SpellEffectName::Dummy));
        let mut fresh = SpellDb::new();
        let err = fresh.add_file(file).unwrap_err();
        assert!(
            matches!(err, SpellDbError::Invalid { spell: 78, .. }),
            "{err}"
        );
        assert!(matches!(
            SpellDb::new().add(None, SpellRecord::new(1, "")),
            Err(SpellDbError::Invalid { spell: 1, .. })
        ));
        let mut mismatch = SpellDb::new();
        mismatch
            .add_file(serde_yaml::from_str(RACIAL_YAML).unwrap())
            .unwrap();
        let other: SpellFile = serde_yaml::from_str("build: 9.9.9.9\nspells: []").unwrap();
        assert!(matches!(
            mismatch.add_file(other),
            Err(SpellDbError::BuildMismatch(..))
        ));
        let unversioned: SpellFile = serde_yaml::from_str("spells: []").unwrap();
        mismatch.add_file(unversioned).unwrap();
    }

    #[test]
    fn dangling_references_are_reported() {
        let mut db = SpellDb::new();
        let mut orphan = SpellRecord::new(5, "Orphan");
        orphan.supercedes = 4;
        db.add(None, orphan).unwrap();
        assert!(matches!(
            db.check_references(),
            Err(SpellDbError::UnknownReference {
                spell: 5,
                field: "supercedes",
                target: 4
            })
        ));

        let mut db = SpellDb::new();
        let mut trigger = SpellRecord::new(6, "Trigger");
        let mut effect = EffectRecord::new(0, SpellEffectName::TriggerSpell);
        effect.trigger_spell = 7;
        trigger.effects.push(effect);
        db.add(None, trigger).unwrap();
        let err = db.check_references().unwrap_err();
        assert_eq!(
            err.to_string(),
            "spell 6 (trigger_spell) refers to unknown spell 7"
        );

        let mut db = SpellDb::new();
        let mut talent = SpellRecord::new(8, "Improved Slam");
        let mut replace = EffectRecord::new(0, SpellEffectName::ApplyAura);
        replace.aura = AuraType::OverrideActionbarSpells;
        replace.misc_value = [1464, 0];
        replace.base_points = 1310197.0;
        talent.effects.push(replace);
        assert_eq!(talent.actionbar_overrides(), [(1464, 1310197)]);
        db.add(None, talent).unwrap();
        assert!(matches!(
            db.check_references(),
            Err(SpellDbError::UnknownReference {
                spell: 8,
                field: "actionbar override",
                target: 1464
            })
        ));
        db.add(None, SpellRecord::new(1464, "Slam")).unwrap();
        db.add(None, SpellRecord::new(1310197, "Slam")).unwrap();
        db.check_references().unwrap();
    }

    #[test]
    fn serialization_omits_defaults_and_round_trips() {
        let db = db();
        let file = SpellFile {
            build: "1.60.1.70009".into(),
            class: Some(PlayerClass::Warrior),
            learnable: true,
            spells: vec![
                (**db.get(12294).unwrap()).clone(),
                (**db.get(284).unwrap()).clone(),
            ],
        };
        let yaml = serde_yaml::to_string(&file).unwrap();
        assert!(yaml.contains("effect: NORMALIZED_WEAPON_DMG"));
        assert!(yaml.contains("aura: 118"), "discarded auras stay numbers");
        assert!(yaml.contains("type: RAGE"));
        assert!(yaml.contains("defense_type: MELEE"));
        assert!(yaml.contains("- UNIT_TARGET_ENEMY\n"), "{yaml}");
        assert!(yaml.contains("- NONE\n"));
        assert!(!yaml.contains("chain_amplitude"), "default 1 is omitted");
        assert!(!yaml.contains("variance"), "default 0 is omitted");
        assert!(!yaml.contains("race_mask"));
        assert!(!yaml.contains("aura_options"));
        assert!(!yaml.contains("description"));
        let back: SpellFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, file);
    }

    #[test]
    fn load_reads_the_yaml_files_of_a_directory_only() {
        let dir = std::env::temp_dir().join(format!(
            "csim-spell-record-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let overrides = dir.join("overrides");
        fs::create_dir_all(&overrides).unwrap();
        fs::write(dir.join("warrior.yaml"), WARRIOR_YAML).unwrap();
        fs::write(dir.join("racials.yml"), RACIAL_YAML).unwrap();
        fs::write(dir.join("notes.txt"), "not yaml").unwrap();
        fs::write(overrides.join("warrior.yaml"), OVERRIDES_YAML).unwrap();

        let db = SpellDb::load(&dir).unwrap();
        assert_eq!(db.len(), 10);
        assert_eq!(db.build(), Some("1.60.1.70009"));
        assert_eq!(db.overrides().len(), 3);
        assert!(db.unsupported().is_empty());

        fs::write(overrides.join("bad.yaml"), "overrides: [{ id: 12834 }]").unwrap();
        assert!(matches!(
            SpellDb::load(&dir),
            Err(SpellDbError::Override(OverrideError::Duplicate(12834)))
        ));
        fs::remove_file(overrides.join("bad.yaml")).unwrap();

        fs::write(dir.join("broken.yaml"), "spells: [{ id: 1 }]").unwrap();
        assert!(matches!(
            SpellDb::load(&dir),
            Err(SpellDbError::Yaml { .. })
        ));
        fs::write(
            dir.join("broken.yaml"),
            "spells: [{ id: 1, name: Dangling, supercedes: 999 }]",
        )
        .unwrap();
        assert!(matches!(
            SpellDb::load(&dir),
            Err(SpellDbError::UnknownReference { spell: 1, .. })
        ));
        fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(SpellDb::load(&dir), Err(SpellDbError::Io { .. })));
    }

    #[test]
    fn module_doc_example_parses() {
        let file: SpellFile = serde_yaml::from_str(DOC_EXAMPLE).unwrap();
        assert_eq!(file.class, Some(PlayerClass::Warrior));
        let ms = &file.spells[0];
        assert_eq!(ms.id, 12294);
        assert_eq!(ms.school_mask, SpellSchoolMask::PHYSICAL);
        assert_eq!(ms.effects[0].index, 1);
        assert_eq!(ms.effects[0].effect, SpellEffectName::NormalizedWeaponDmg);
        assert_eq!(ms.categories.defense_type, DefenseType::Melee);
        assert_eq!(ms.equipped_items.unwrap().subclass_mask, 173555);
    }

    const DOC_EXAMPLE: &str = r#"
build: 1.60.1.70009
class: WARRIOR
spells:
  - id: 12294
    name: Mortal Strike
    rank_text: Rank 1
    skill_line: 26
    attributes: [327696, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    school_mask: 1
    duration_ms: 10000
    range_yd: 5
    power: [{ type: RAGE, cost: 300 }]
    cooldown: { category_recovery_ms: 6000, start_recovery_ms: 1500 }
    categories: { category: 971, start_recovery_category: 133, defense_type: MELEE }
    levels: { base: 40, spell: 40 }
    class_options: { set: 4, mask: [33554432, 0, 0, 0] }
    equipped_items: { class: 2, subclass_mask: 173555 }
    effects:
      - index: 1
        effect: NORMALIZED_WEAPON_DMG
        base_points: 85
        implicit_target: [UNIT_TARGET_ENEMY, NONE]"#;

    #[test]
    fn shipped_spell_data_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/spells");
        let db = SpellDb::load(&dir).unwrap();
        assert_eq!(db.build(), Some("1.60.1.70009"));
        assert!(db.len() > 240, "{}", db.len());
        assert!(db.ids_of_class(Some(PlayerClass::Warrior)).len() > 200);
        assert!(db.ids_of_class(None).len() > 30, "racials");
        assert!(db.get(355).is_none(), "Taunt is pruned from the export");
        assert!(db.get(694).is_some(), "Mocking Blow keeps its damage");

        let ms = db.get(12294).unwrap();
        assert_eq!(ms.name, "Mortal Strike");
        assert_eq!(ms.power[0].displayed_cost(), 30.0);
        assert_eq!(ms.cooldown.category_recovery_ms, 6000);
        assert_eq!(ms.categories.category, 971);
        assert_eq!(ms.effects.len(), 1, "the healing debuff is pruned");
        assert_eq!(ms.effects[0].index, 1);
        assert_eq!(ms.effects[0].effect, SpellEffectName::NormalizedWeaponDmg);
        assert_eq!(ms.effects[0].base_points, 85.0);
        assert_eq!(db.rank_chain(25286).len(), 9, "Heroic Strike ranks");
        assert_eq!(db.rank_chain(11605).len(), 5, "Slam ranks");
        assert_eq!(
            db.rank_chains("Slam").len(),
            6,
            "the trainable chain plus Improved Slam's five unchained replacements"
        );
        assert_eq!(
            db.modified_spells(4, &db.get(12282).unwrap().effects[0])
                .len(),
            9
        );
        assert!(db.get(412609).is_some(), "reached through the overrides");
        assert!(db.get(7381).is_some(), "stance passive");
        assert_eq!(db.overrides().stance_passive(2458), Some(7381));
        // Shield Slam strikes with the shield, Heroic Strike with the main-hand weapon.
        assert!(
            db.get(23925)
                .unwrap()
                .equipped_items
                .unwrap()
                .requires_shield()
        );
        assert!(!ms.equipped_items.unwrap().requires_shield());
        assert!(
            !db.get(78)
                .unwrap()
                .equipped_items
                .unwrap()
                .requires_shield()
        );

        let blood_fury = db.get(20572).unwrap();
        assert_eq!(blood_fury.race_mask, 2);
        assert_eq!(db.class_of(20572), Some(None));

        // The Rogue: finisher durations per combo point, Relentless Strikes kept by its
        // ADD_TARGET_TRIGGER aura, the Season of Discovery runes ignored.
        assert!(db.ids_of_class(Some(PlayerClass::Rogue)).len() > 150);
        let slice = db.get(6774).unwrap();
        assert_eq!(slice.name, "Slice and Dice");
        assert_eq!(slice.finite_duration_ms_with_combo_points(1), Some(9_000));
        assert_eq!(slice.finite_duration_ms_with_combo_points(5), Some(21_000));
        let rupture = db.get(11275).unwrap();
        assert_eq!(
            rupture.finite_duration_ms_with_combo_points(5),
            Some(16_000)
        );
        let relentless = db.get(14179).unwrap();
        assert_eq!(relentless.effects[0].aura, AuraType::AddTargetTrigger);
        assert_eq!(relentless.effects[0].trigger_spell, 14181);
        assert!(
            db.overrides().has_sim_flag(424785, SimFlag::Ignored),
            "Saber Slash"
        );
        assert_eq!(
            db.get(31016).unwrap().rank_number(),
            Some(9),
            "Eviscerate r9"
        );

        // The effects the sim cannot interpret yet; extend the overrides rather than this list.
        let mut pending: Vec<u32> = db.unsupported().iter().map(|u| u.spell).collect();
        pending.dedup();
        assert_eq!(
            pending,
            [
                12299, 13567, 14537, 18350, 24658, 24661, 28839, 29275, 29284, 29286, 402911,
                403196, 1287808, 1295744, 1317432, 1318325, 1318470, 1318514
            ],
            "Toughness (aura 466), Raging Blow, Devastate, and item              spells whose DUMMY effects wait for a script (Zandalarian              trinkets, Six Demon Bag, Arcanite Dragonling, creature-type damage bonuses, ...)"
        );
    }
}
