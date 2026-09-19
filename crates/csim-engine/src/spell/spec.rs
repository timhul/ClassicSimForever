//! Spell data schema and loader. Port of `Spells/SpellInfo.h` and `Spells/SpellFileReader.*`.
//!
//! One YAML file (see `data/spells/warrior.yaml`) holds a list of spell groups plus the sets of
//! spells that share a cooldown. Where the C++ reader kept effect and talent attributes as
//! `QMap<QString, QString>` and logged malformed entries with `qDebug`, every attribute here is
//! typed and every problem is a [`SpellDbError`] at load time.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::faction::PlayerClass;
use crate::item::{ItemStat, WeaponType};
use crate::phase::Phase;
use crate::proc::ProcSource;
use crate::resource::ResourceType;
use crate::stance::Stance;
use crate::target::Priority;

/// Sentinel for "the highest rank available" wherever a rank number is given.
pub const MAX_RANK: u32 = 0;

/// Flags on a spell group. Port of `SpellFlag` plus the additions from `TASKS.md` §3.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpellFlag {
    CannotBeBlocked,
    CannotBeDodged,
    CannotBeParried,
    CannotCrit,
    CannotMiss,
    OnMeleeCrit,
    OnMeleeDodge,
    OnMeleeHit,
    OnMeleeMiss,
    OnMeleeParry,
    #[serde(rename = "PASSIVE_SPELL")]
    Passive,
    /// The spell replaces the next mainhand swing (Heroic Strike).
    OnNextSwing,
    /// Casting the spell resets the swing timers (Slam).
    ResetsSwingTimers,
    /// Auto attacks are paused while the spell is being cast (Slam).
    StopsAttackDuringCast,
    /// The spell is performed automatically when combat starts (Anger Management).
    StartOfCombat,
}

/// Effect kinds. Port of `SpellEffect` plus the generic effects from `TASKS.md` §3.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpellEffect {
    AddComboPoints,
    ApplyAuraArmorPenetration,
    ApplyAuraGenericStat,
    ApplyAuraMeleeAttackPower,
    ApplyAuraMeleeAuraCritChance,
    ApplyAuraModArmor,
    ApplyAuraModDamageDonePhysical,
    ApplyAuraModDamageTaken,
    ApplyAuraModMeleeAttackSpeed,
    ApplyAuraModResistance,
    ApplyAuraModThreat,
    ApplyAuraPeriodicDamageFromWeapon,
    ApplyAuraPeriodicResourceGainRage,
    ApplyAuraPeriodicWeaponDamage,
    ApplyAuraShapeshiftBattleStance,
    ApplyAuraShapeshiftBerserkerStance,
    ApplyAuraShapeshiftDefensiveStance,
    ApplyMarkerBuff,
    AuraConsumeCharge,
    ConsumeComboPoints,
    ExtraAttackInstant,
    ExtraAttackOnNextSwing,
    GainResourceEnergy,
    GainResourceFocus,
    GainResourceMana,
    GainResourceRage,
    NextBatchResourceLossRage,
    NoEffect,
    NormalizedWeaponDamage,
    SchoolDamageArcane,
    SchoolDamageBlockValue,
    SchoolDamageConvertRage,
    SchoolDamageFire,
    SchoolDamageFrost,
    SchoolDamageHoly,
    SchoolDamageNature,
    SchoolDamagePhysical,
    SchoolDamageShadow,
    UseTrinket,
    WeaponDamage,
    WindfuryExtraAttack,
}

/// Who a buff or effect applies to. Port of `Affected` in `Spells/Buff.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Affected {
    #[serde(rename = "self")]
    Caster,
    Party,
    Raid,
    Target,
}

/// Which weapon an effect concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hand {
    Mainhand,
    Offhand,
}

/// How a spell interacts with the global cooldown. Port of `GcdBehavior`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GcdBehavior {
    /// Does not trigger a global cooldown.
    #[serde(rename = "none")]
    None,
    /// Triggers the normal global cooldown.
    Normal,
    /// Triggers the stance cooldown instead.
    Stance,
}

/// How a spell's cost is interpreted. Port of `ResourceCostType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceCostType {
    #[default]
    Absolute,
    #[serde(rename = "percent")]
    BasePercentage,
}

/// Comparison used by restrictions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Comparison {
    Eq,
    Neq,
    Less,
    Leq,
    Greater,
    Geq,
}

/// A condition that must hold for the spell to be available. Port of `RestrictionSpec` with the
/// restriction types interpreted in `Spell::Spell` plus the additions from `TASKS.md` §3.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum RestrictionSpec {
    /// `COMBO_POINTS`: the character's combo points compared to `value`.
    #[serde(rename = "COMBO_POINTS")]
    ComboPoints { value: u32, cmp: Comparison },
    /// `TARGET_HEALTH_PERCENTAGE`: the target's health fraction compared to `value` (0.2 = 20%).
    #[serde(rename = "TARGET_HEALTH_PERCENTAGE")]
    TargetHealthPercentage { value: f64, cmp: Comparison },
    /// `UNIT_STANCE`: the character's stance compared to `stance`.
    #[serde(rename = "UNIT_STANCE")]
    UnitStance { stance: Stance, cmp: Comparison },
    /// `BUFF_ACTIVE`: the named buff must be active on the character.
    #[serde(rename = "BUFF_ACTIVE")]
    BuffActive { buff: String },
    /// `OFFHAND_WEAPON_TYPE`: the type of the equipped offhand compared to `weapon_type`.
    #[serde(rename = "OFFHAND_WEAPON_TYPE")]
    OffhandWeaponType {
        weapon_type: WeaponType,
        cmp: Comparison,
    },
}

impl RestrictionSpec {
    /// The comparisons the runtime implements for this restriction.
    fn supported_comparisons(&self) -> &'static [Comparison] {
        match self {
            RestrictionSpec::ComboPoints { .. } => &[Comparison::Greater],
            RestrictionSpec::TargetHealthPercentage { .. } => &[Comparison::Leq],
            RestrictionSpec::UnitStance { .. } | RestrictionSpec::OffhandWeaponType { .. } => {
                &[Comparison::Eq, Comparison::Neq]
            }
            RestrictionSpec::BuffActive { .. } => &[],
        }
    }

    fn comparison(&self) -> Option<Comparison> {
        match self {
            RestrictionSpec::ComboPoints { cmp, .. }
            | RestrictionSpec::TargetHealthPercentage { cmp, .. }
            | RestrictionSpec::UnitStance { cmp, .. }
            | RestrictionSpec::OffhandWeaponType { cmp, .. } => Some(*cmp),
            RestrictionSpec::BuffActive { .. } => None,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            RestrictionSpec::ComboPoints { .. } => "COMBO_POINTS",
            RestrictionSpec::TargetHealthPercentage { .. } => "TARGET_HEALTH_PERCENTAGE",
            RestrictionSpec::UnitStance { .. } => "UNIT_STANCE",
            RestrictionSpec::BuffActive { .. } => "BUFF_ACTIVE",
            RestrictionSpec::OffhandWeaponType { .. } => "OFFHAND_WEAPON_TYPE",
        }
    }
}

/// One effect of a spell or buff. Port of `SpellEffectSpec`; the attribute map became typed
/// optional fields named after the XML attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpellEffectSpec {
    pub name: SpellEffect,
    /// Main magnitude; meaning depends on the effect (damage, percent, hundredths of percent...).
    #[serde(default)]
    pub value: f64,
    /// Damage range for effects rolling a flat amount (Revenge, Shield Slam).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Whether the effect re-rolls its own hit check instead of depending on the previous
    /// effect's result. `None` means "yes for the first effect, no for the others" (the C++
    /// reader marked the first effect of every list as independent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Affected>,
    /// Seconds between periodic ticks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_rate: Option<f64>,
    /// Alias of `tick_rate` used by periodic damage effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<f64>,
    /// Number of ticks for effects that distribute damage over a fixed number of ticks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks: Option<u32>,
    /// Duration in seconds for effects that carry their own duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// Fraction of weapon damage added per application (periodic weapon damage effects).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_coeff: Option<f64>,
    #[serde(default)]
    pub ap_dmg_mod: f64,
    #[serde(default)]
    pub sp_dmg_mod: f64,
    /// Threat added on top of the damage-based threat.
    #[serde(default)]
    pub innate_threat: u32,
    /// Buff the effect refers to (`AURA_CONSUME_CHARGE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buff: Option<String>,
    /// Stat changed by `APPLY_AURA_GENERIC_STAT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat: Option<ItemStat>,
    /// Weapon used by extra-attack effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<Hand>,
}

impl SpellEffectSpec {
    /// An effect with only a name, every attribute at its default.
    pub fn new(name: SpellEffect) -> Self {
        SpellEffectSpec {
            name,
            value: 0.0,
            min: None,
            max: None,
            independent: None,
            unit: None,
            tick_rate: None,
            period: None,
            ticks: None,
            duration: None,
            weapon_coeff: None,
            ap_dmg_mod: 0.0,
            sp_dmg_mod: 0.0,
            innate_threat: 0,
            buff: None,
            stat: None,
            hand: None,
        }
    }

    /// Whether the effect at `index` in its list rolls independently of the previous effects.
    pub fn is_independent(&self, index: usize) -> bool {
        self.independent.unwrap_or(index == 0)
    }

    fn validate(&self) -> Result<(), String> {
        match self.name {
            SpellEffect::AuraConsumeCharge if self.buff.is_none() => {
                Err("AURA_CONSUME_CHARGE requires `buff`".to_string())
            }
            SpellEffect::ApplyAuraGenericStat if self.stat.is_none() => {
                Err("APPLY_AURA_GENERIC_STAT requires `stat`".to_string())
            }
            SpellEffect::ExtraAttackInstant | SpellEffect::ExtraAttackOnNextSwing
                if self.hand.is_none() =>
            {
                Err(format!("{:?} requires `hand`", self.name))
            }
            SpellEffect::ApplyAuraPeriodicResourceGainRage
            | SpellEffect::ApplyAuraPeriodicDamageFromWeapon
            | SpellEffect::ApplyAuraPeriodicWeaponDamage
                if self.period.or(self.tick_rate).is_none_or(|p| p <= 0.0) =>
            {
                Err(format!("{:?} requires a positive `period`", self.name))
            }
            _ => match (self.min, self.max) {
                (Some(min), Some(max)) if min > max => {
                    Err(format!("min {min} is greater than max {max}"))
                }
                (Some(_), None) | (None, Some(_)) => {
                    Err("`min` and `max` must be given together".to_string())
                }
                _ => Ok(()),
            },
        }
    }
}

/// The buff a spell rank applies. Port of `BuffRankSpec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuffRankSpec {
    /// Display name; defaults to the spell group's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub unit: Affected,
    /// Debuff slot priority (target buffs only).
    #[serde(default = "invalid_priority")]
    pub priority: Priority,
    #[serde(default)]
    pub hidden: bool,
    /// Shared between all characters in the raid (target debuffs only).
    #[serde(default)]
    pub shared: bool,
    /// Duration in seconds; `None` means the buff lasts until removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default)]
    pub base_charges: u32,
    #[serde(default)]
    pub max_stacks: u32,
    #[serde(default)]
    pub effects: Vec<SpellEffectSpec>,
}

fn invalid_priority() -> Priority {
    Priority::Invalid
}

impl BuffRankSpec {
    pub fn is_permanent(&self) -> bool {
        self.duration.is_none()
    }
}

/// One rank of a spell. Port of `SpellRankSpec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpellRankSpec {
    pub rank: u32,
    pub resource: ResourceType,
    #[serde(default)]
    pub cost: u32,
    #[serde(default)]
    pub cost_type: ResourceCostType,
    #[serde(default)]
    pub cast_time_ms: u32,
    #[serde(default = "first_phase")]
    pub requires_phase: Phase,
    /// Character level at which the rank is learned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_req: Option<u32>,
    #[serde(default)]
    pub effects: Vec<SpellEffectSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buff: Option<BuffRankSpec>,
}

fn first_phase() -> Phase {
    Phase::MoltenCore
}

/// Which effect list a talent modification targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectTarget {
    #[default]
    Spell,
    Buff,
}

/// What a talent rank changes on a spell. Port of the `type` attribute of `<modified_by_talent>`
/// (`absolute_resource_cost_reduction`, `increase_value`, `add_effect`) plus the operations from
/// `TASKS.md` §3.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalentModification {
    AbsoluteResourceCostReduction {
        value: u32,
    },
    IncreaseValue {
        #[serde(default)]
        target: EffectTarget,
        effect: SpellEffect,
        value: f64,
    },
    IncreaseValuePercent {
        #[serde(default)]
        target: EffectTarget,
        effect: SpellEffect,
        value: f64,
    },
    AddEffect {
        #[serde(default)]
        target: EffectTarget,
        effect: SpellEffectSpec,
    },
    IncreaseBuffDurationPercent {
        value: f64,
    },
    CastTimeReductionMs {
        value: u32,
    },
    /// Hundredths of a percent.
    IncreaseCritChance {
        value: u32,
    },
    /// Fraction (0.08 = 8%).
    SetProcRate {
        value: f64,
    },
    /// Percent added to all damage the spell deals, including its periodic damage
    /// (Improved Rend).
    IncreaseDamagePercent {
        value: f64,
    },
}

/// One `<modified_by_talent>` entry. Port of `TalentRankSpec`.
///
/// `talent` and `rank` are followed by the fields of the [`TalentModification`] variant. (Serde
/// cannot reject unknown keys on this struct because of the flattened enum.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TalentModificationSpec {
    pub talent: String,
    pub rank: u32,
    #[serde(flatten)]
    pub modification: TalentModification,
}

/// Port of `StatisticsSpec` (a hint for how to present one effect's statistics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatisticsSpec {
    pub effect: SpellEffect,
    #[serde(rename = "type")]
    pub kind: String,
    pub display_name: String,
}

/// A spell and all its ranks. Port of `SpellRankGroupSpec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpellGroupSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub restricted_by_gcd: bool,
    pub causes_gcd: GcdBehavior,
    /// Base cooldown in seconds.
    #[serde(default)]
    pub cooldown: f64,
    /// Fraction of the resource cost still paid when the spell misses.
    #[serde(default = "default_resource_miss_cost_mod")]
    pub resource_miss_cost_mod: f64,
    /// The spell is only enabled while this talent has at least one rank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_talent: Option<String>,
    #[serde(default)]
    pub restrictions: Vec<RestrictionSpec>,
    #[serde(default)]
    pub flags: Vec<SpellFlag>,
    pub ranks: Vec<SpellRankSpec>,
    #[serde(default)]
    pub modified_by_talent: Vec<TalentModificationSpec>,
    #[serde(default)]
    pub statistics: Vec<StatisticsSpec>,
    /// Proc settings; only for passive spells (`PASSIVE_SPELL` flag), which are procs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proc: Option<ProcSpec>,
    /// A cast-time spell whose cast completes instantly while the character's casting time is
    /// suppressed (Nature's Swiftness and the like). Port of `SuppressibleCast`.
    #[serde(default)]
    pub suppressible_cast: bool,
    /// Names of the spells sharing a cooldown with this one (including this one). Filled by the
    /// loader from the file's `shared_cooldowns`, not read from the group itself.
    #[serde(skip)]
    pub shared_cooldowns: BTreeSet<String>,
}

/// How a passive spell procs. The `ON_MELEE_*` flags add their sources too; the spell group's
/// `cooldown` is the proc's internal cooldown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcSpec {
    /// Chance as a fraction (0.01 = 1%), or procs per minute when `ppm` is set. Talents may
    /// override it (`set_proc_rate`).
    #[serde(default = "always")]
    pub rate: f64,
    #[serde(default)]
    pub ppm: bool,
    /// The weapon whose speed a PPM rate is based on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<Hand>,
    #[serde(default)]
    pub sources: Vec<ProcSource>,
    /// The weapon on the triggering side must be one of these types (Sword Specialization).
    #[serde(default)]
    pub requires_weapon_type: Vec<WeaponType>,
    /// The named buff must be active on the character for the proc to fire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_buff: Option<String>,
    /// Procs performed (without their own roll) whenever this one procs.
    #[serde(default)]
    pub linked: Vec<String>,
}

fn always() -> f64 {
    1.0
}

impl ProcSpec {
    fn validate(&self) -> Result<(), String> {
        if self.ppm && self.hand.is_none() {
            return Err("a PPM proc needs `hand`".to_string());
        }
        if self.rate < 0.0 || (!self.ppm && self.rate > 1.0) {
            return Err(format!("proc rate {} is out of range", self.rate));
        }
        if self.sources.contains(&ProcSource::Manual) {
            return Err("MANUAL is not a proc source".to_string());
        }
        Ok(())
    }
}

fn default_resource_miss_cost_mod() -> f64 {
    0.25
}

impl SpellGroupSpec {
    pub fn has_flag(&self, flag: SpellFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// The sources a passive spell procs from: the `ON_MELEE_*` flags plus `proc.sources`,
    /// without duplicates. Port of the flag mapping in the `Proc` constructor.
    pub fn proc_sources(&self) -> Vec<ProcSource> {
        let mut sources: Vec<ProcSource> = self
            .flags
            .iter()
            .filter_map(|flag| match flag {
                SpellFlag::OnMeleeCrit => Some(ProcSource::MeleeCritical),
                SpellFlag::OnMeleeDodge => Some(ProcSource::MeleeDodge),
                SpellFlag::OnMeleeHit => Some(ProcSource::MeleeHit),
                SpellFlag::OnMeleeMiss => Some(ProcSource::MeleeMiss),
                SpellFlag::OnMeleeParry => Some(ProcSource::MeleeParry),
                _ => None,
            })
            .collect();
        if let Some(proc) = &self.proc {
            sources.extend(proc.sources.iter().copied());
        }
        sources.sort();
        sources.dedup();
        sources
    }

    pub fn rank(&self, rank: u32) -> Option<&SpellRankSpec> {
        if rank == MAX_RANK {
            self.ranks.iter().max_by_key(|spec| spec.rank)
        } else {
            self.ranks.iter().find(|spec| spec.rank == rank)
        }
    }

    pub fn max_rank(&self) -> u32 {
        self.ranks.iter().map(|spec| spec.rank).max().unwrap_or(0)
    }

    /// Talent modifications grouped by talent, in file order.
    pub fn talent_modifications(&self) -> BTreeMap<&str, Vec<&TalentModificationSpec>> {
        let mut map: BTreeMap<&str, Vec<&TalentModificationSpec>> = BTreeMap::new();
        for spec in &self.modified_by_talent {
            map.entry(spec.talent.as_str()).or_default().push(spec);
        }
        map
    }

    fn validate(&self) -> Result<(), SpellDbError> {
        let err = |message: String| SpellDbError::Invalid {
            spell: self.name.clone(),
            message,
        };

        if self.ranks.is_empty() {
            return Err(err("no ranks".to_string()));
        }
        if let Some(proc) = &self.proc {
            if !self.has_flag(SpellFlag::Passive) {
                return Err(err("only passive spells can have `proc`".to_string()));
            }
            proc.validate().map_err(err)?;
        }
        if self.has_flag(SpellFlag::Passive) && self.proc_sources().is_empty() {
            return Err(err("passive spell has no proc sources".to_string()));
        }
        let mut ranks = BTreeSet::new();
        for rank in &self.ranks {
            if rank.rank == MAX_RANK {
                return Err(err(format!("rank {MAX_RANK} is reserved for MAX_RANK")));
            }
            if !ranks.insert(rank.rank) {
                return Err(err(format!("rank {} is defined twice", rank.rank)));
            }
            for (index, effect) in rank.effects.iter().enumerate() {
                effect
                    .validate()
                    .map_err(|m| err(format!("rank {} effect {index}: {m}", rank.rank)))?;
            }
            if let Some(buff) = &rank.buff {
                for (index, effect) in buff.effects.iter().enumerate() {
                    effect
                        .validate()
                        .map_err(|m| err(format!("rank {} buff effect {index}: {m}", rank.rank)))?;
                }
                for effect in &buff.effects {
                    let periodic_damage = matches!(
                        effect.name,
                        SpellEffect::ApplyAuraPeriodicDamageFromWeapon
                            | SpellEffect::ApplyAuraPeriodicWeaponDamage
                    );
                    if periodic_damage && effect.ticks.is_none() && buff.duration.is_none() {
                        return Err(err(format!(
                            "rank {}: {:?} needs `ticks` or a buff duration",
                            rank.rank, effect.name
                        )));
                    }
                }
                if buff.shared && buff.unit != Affected::Target {
                    return Err(err(format!(
                        "rank {}: only target buffs can be shared",
                        rank.rank
                    )));
                }
            }
        }

        for restriction in &self.restrictions {
            if let Some(cmp) = restriction.comparison() {
                if !restriction.supported_comparisons().contains(&cmp) {
                    return Err(err(format!(
                        "restriction {} does not support comparison {cmp:?}",
                        restriction.type_name()
                    )));
                }
            }
        }

        let mut talent_ranks = BTreeSet::new();
        for spec in &self.modified_by_talent {
            let talent_err = |message: String| {
                err(format!(
                    "talent {:?} rank {}: {message}",
                    spec.talent, spec.rank
                ))
            };
            if spec.rank == 0 {
                return Err(talent_err("rank must be at least 1".to_string()));
            }
            if !talent_ranks.insert((spec.talent.as_str(), spec.rank)) {
                return Err(talent_err("defined twice".to_string()));
            }
            match &spec.modification {
                TalentModification::IncreaseValue { target, effect, .. }
                | TalentModification::IncreaseValuePercent { target, effect, .. } => {
                    for rank in &self.ranks {
                        let effects = match target {
                            EffectTarget::Spell => Some(&rank.effects),
                            EffectTarget::Buff => rank.buff.as_ref().map(|buff| &buff.effects),
                        };
                        let found = effects
                            .is_some_and(|effects| effects.iter().any(|e| e.name == *effect));
                        if !found {
                            return Err(talent_err(format!(
                                "rank {} has no {target:?} effect {effect:?}",
                                rank.rank
                            )));
                        }
                    }
                }
                TalentModification::AddEffect { target, effect } => {
                    effect.validate().map_err(talent_err)?;
                    if *target == EffectTarget::Buff && self.ranks.iter().any(|r| r.buff.is_none())
                    {
                        return Err(talent_err(
                            "targets the buff but a rank has none".to_string(),
                        ));
                    }
                }
                TalentModification::IncreaseBuffDurationPercent { .. } => {
                    if self.ranks.iter().any(|r| r.buff.is_none()) {
                        return Err(talent_err(
                            "targets the buff but a rank has none".to_string(),
                        ));
                    }
                }
                TalentModification::AbsoluteResourceCostReduction { value } => {
                    if self.ranks.iter().any(|r| r.cost < *value) {
                        return Err(talent_err("reduces the cost below zero".to_string()));
                    }
                }
                TalentModification::CastTimeReductionMs { value } => {
                    if self.ranks.iter().any(|r| r.cast_time_ms < *value) {
                        return Err(talent_err("reduces the cast time below zero".to_string()));
                    }
                }
                TalentModification::IncreaseCritChance { .. }
                | TalentModification::SetProcRate { .. }
                | TalentModification::IncreaseDamagePercent { .. } => {}
            }
        }

        Ok(())
    }
}

/// One spell data file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpellFileSpec {
    /// The class the spells belong to; `None` for class-independent spells (racials, items).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<PlayerClass>,
    #[serde(default)]
    pub spell_groups: Vec<SpellGroupSpec>,
    /// Groups of spell names that share one cooldown (the stances).
    #[serde(default)]
    pub shared_spell_cooldowns: Vec<Vec<String>>,
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
    #[error("spell {0:?} is defined twice")]
    Duplicate(String),
    #[error("spell {spell:?}: {message}")]
    Invalid { spell: String, message: String },
    #[error("shared cooldown refers to unknown spell {0:?}")]
    UnknownSharedCooldownSpell(String),
    #[error("spell {0:?} is in two shared cooldown groups")]
    SpellInTwoSharedCooldowns(String),
}

/// All loaded spell groups, shared immutably between simulations.
#[derive(Debug, Clone, Default)]
pub struct SpellDb {
    groups: Vec<Arc<SpellGroupSpec>>,
    by_name: HashMap<String, usize>,
    class_by_group: Vec<Option<PlayerClass>>,
}

impl SpellDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads every `*.yaml` / `*.yml` file in `spells_dir` (sorted by name).
    pub fn load(spells_dir: &Path) -> Result<Self, SpellDbError> {
        let mut db = Self::new();

        let mut paths: Vec<PathBuf> = fs::read_dir(spells_dir)
            .map_err(|source| SpellDbError::Io {
                path: spells_dir.to_path_buf(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        paths.sort();

        for path in paths {
            db.load_file(&path)?;
        }

        Ok(db)
    }

    /// Adds the spell groups of one YAML file.
    pub fn load_file(&mut self, path: &Path) -> Result<(), SpellDbError> {
        let text = fs::read_to_string(path).map_err(|source| SpellDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: SpellFileSpec =
            serde_yaml::from_str(&text).map_err(|source| SpellDbError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        self.add_file(file)
    }

    /// Validates and adds the spell groups of one file spec.
    pub fn add_file(&mut self, file: SpellFileSpec) -> Result<(), SpellDbError> {
        let mut groups = file.spell_groups;
        for group in &groups {
            group.validate()?;
            if self.by_name.contains_key(&group.name)
                || groups.iter().filter(|g| g.name == group.name).count() > 1
            {
                return Err(SpellDbError::Duplicate(group.name.clone()));
            }
        }

        let mut shared: HashMap<&str, BTreeSet<String>> = HashMap::new();
        for names in &file.shared_spell_cooldowns {
            let set: BTreeSet<String> = names.iter().cloned().collect();
            for name in names {
                if !groups.iter().any(|g| g.name == *name) {
                    return Err(SpellDbError::UnknownSharedCooldownSpell(name.clone()));
                }
                if shared.insert(name.as_str(), set.clone()).is_some() {
                    return Err(SpellDbError::SpellInTwoSharedCooldowns(name.clone()));
                }
            }
        }
        for group in &mut groups {
            if let Some(set) = shared.get(group.name.as_str()) {
                group.shared_cooldowns = set.clone();
            }
        }

        for group in groups {
            self.by_name.insert(group.name.clone(), self.groups.len());
            self.groups.push(Arc::new(group));
            self.class_by_group.push(file.class);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Arc<SpellGroupSpec>> {
        self.by_name.get(name).map(|&index| &self.groups[index])
    }

    /// The class a spell group was loaded for (`None` for class-independent files).
    pub fn class_of(&self, name: &str) -> Option<PlayerClass> {
        self.by_name
            .get(name)
            .and_then(|&index| self.class_by_group[index])
    }

    /// Every group, in load order.
    pub fn groups(&self) -> &[Arc<SpellGroupSpec>] {
        &self.groups
    }

    /// The groups belonging to `class` plus the class-independent ones.
    pub fn groups_for_class(&self, class: PlayerClass) -> Vec<&Arc<SpellGroupSpec>> {
        self.groups
            .iter()
            .zip(&self.class_by_group)
            .filter(|(_, group_class)| group_class.is_none_or(|c| c == class))
            .map(|(group, _)| group)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<SpellDb, SpellDbError> {
        let file: SpellFileSpec = serde_yaml::from_str(yaml).expect("valid yaml");
        let mut db = SpellDb::new();
        db.add_file(file)?;
        Ok(db)
    }

    const BATTLE_SHOUT: &str = r#"
class: WARRIOR
spell_groups:
  - name: Battle Shout
    icon: Assets/ability/Ability_warrior_battleshout.png
    causes_gcd: normal
    restricted_by_gcd: true
    ranks:
      - rank: 6
        resource: rage
        cost: 10
        buff:
          unit: party
          duration: 120
          effects:
            - name: APPLY_AURA_MELEE_ATTACK_POWER
              value: 193
      - rank: 7
        resource: rage
        cost: 10
        requires_phase: 5
        buff:
          unit: party
          duration: 120
          effects:
            - name: APPLY_AURA_MELEE_ATTACK_POWER
              value: 232
    modified_by_talent:
      - talent: Improved Battle Shout
        rank: 1
        type: increase_value_percent
        target: buff
        effect: APPLY_AURA_MELEE_ATTACK_POWER
        value: 5
      - talent: Booming Voice
        rank: 1
        type: increase_buff_duration_percent
        value: 10
"#;

    #[test]
    fn parses_a_spell_group_with_defaults() {
        let db = parse(BATTLE_SHOUT).unwrap();
        assert_eq!(db.len(), 1);
        let group = db.get("Battle Shout").unwrap();
        assert_eq!(db.class_of("Battle Shout"), Some(PlayerClass::Warrior));
        assert!(group.restricted_by_gcd);
        assert_eq!(group.causes_gcd, GcdBehavior::Normal);
        assert_eq!(group.cooldown, 0.0);
        assert_eq!(group.resource_miss_cost_mod, 0.25);
        assert_eq!(group.requires_talent, None);
        assert_eq!(group.max_rank(), 7);
        assert_eq!(group.rank(MAX_RANK).unwrap().rank, 7);
        assert_eq!(group.rank(6).unwrap().requires_phase, Phase::MoltenCore);
        assert_eq!(group.rank(7).unwrap().requires_phase, Phase::AhnQiraj);
        assert!(group.rank(8).is_none());

        let rank = group.rank(6).unwrap();
        assert_eq!(rank.cost_type, ResourceCostType::Absolute);
        assert_eq!(rank.cast_time_ms, 0);
        assert!(rank.effects.is_empty());
        let buff = rank.buff.as_ref().unwrap();
        assert_eq!(buff.unit, Affected::Party);
        assert_eq!(buff.priority, Priority::Invalid);
        assert_eq!(buff.duration, Some(120.0));
        assert!(!buff.is_permanent());
        assert_eq!(buff.effects[0].name, SpellEffect::ApplyAuraMeleeAttackPower);
        assert_eq!(buff.effects[0].value, 193.0);
        assert!(buff.effects[0].is_independent(0));

        let talents = group.talent_modifications();
        assert_eq!(talents.len(), 2);
        assert_eq!(
            talents["Improved Battle Shout"][0].modification,
            TalentModification::IncreaseValuePercent {
                target: EffectTarget::Buff,
                effect: SpellEffect::ApplyAuraMeleeAttackPower,
                value: 5.0,
            }
        );
        assert_eq!(
            talents["Booming Voice"][0].modification,
            TalentModification::IncreaseBuffDurationPercent { value: 10.0 }
        );
    }

    #[test]
    fn parses_restrictions_flags_and_add_effect() {
        let db = parse(
            r#"
spell_groups:
  - name: Berserker Rage
    causes_gcd: none
    cooldown: 30
    restrictions:
      - type: UNIT_STANCE
        stance: BERSERKER_STANCE
        cmp: eq
    flags: [CANNOT_MISS, CANNOT_CRIT]
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: self
          duration: 10
          effects:
            - name: NO_EFFECT
    modified_by_talent:
      - talent: Improved Berserker Rage
        rank: 1
        type: add_effect
        target: buff
        effect: { name: GAIN_RESOURCE_RAGE, value: 5 }
  - name: Execute
    causes_gcd: normal
    restricted_by_gcd: true
    resource_miss_cost_mod: 0.16
    restrictions:
      - { type: TARGET_HEALTH_PERCENTAGE, value: 0.2, cmp: leq }
      - { type: UNIT_STANCE, stance: DEFENSIVE_STANCE, cmp: neq }
      - { type: COMBO_POINTS, value: 0, cmp: greater }
      - { type: BUFF_ACTIVE, buff: Revenge Ready }
      - { type: OFFHAND_WEAPON_TYPE, weapon_type: SHIELD, cmp: eq }
    ranks:
      - rank: 5
        resource: rage
        cost: 15
        effects:
          - { name: SCHOOL_DAMAGE_PHYSICAL, value: 600 }
          - { name: SCHOOL_DAMAGE_CONVERT_RAGE, value: 15 }
          - { name: NEXT_BATCH_RESOURCE_LOSS_RAGE, independent: true }
    modified_by_talent:
      - { talent: Improved Execute, rank: 1, type: absolute_resource_cost_reduction, value: 2 }
      - { talent: Improved Execute, rank: 2, type: absolute_resource_cost_reduction, value: 5 }
"#,
        )
        .unwrap();

        let rage = db.get("Berserker Rage").unwrap();
        assert_eq!(db.class_of("Berserker Rage"), None);
        assert_eq!(rage.causes_gcd, GcdBehavior::None);
        assert!(!rage.restricted_by_gcd);
        assert_eq!(
            rage.restrictions,
            vec![RestrictionSpec::UnitStance {
                stance: Stance::Berserker,
                cmp: Comparison::Eq
            }]
        );
        assert!(rage.has_flag(SpellFlag::CannotCrit));
        assert!(rage.has_flag(SpellFlag::CannotMiss));
        assert!(!rage.has_flag(SpellFlag::Passive));
        let mut added = SpellEffectSpec::new(SpellEffect::GainResourceRage);
        added.value = 5.0;
        assert_eq!(
            rage.modified_by_talent[0].modification,
            TalentModification::AddEffect {
                target: EffectTarget::Buff,
                effect: added,
            }
        );

        let execute = db.get("Execute").unwrap();
        assert_eq!(execute.resource_miss_cost_mod, 0.16);
        assert_eq!(execute.restrictions.len(), 5);
        assert_eq!(
            execute.restrictions[3],
            RestrictionSpec::BuffActive {
                buff: "Revenge Ready".to_string()
            }
        );
        let effects = &execute.rank(5).unwrap().effects;
        assert!(effects[0].is_independent(0));
        assert!(!effects[1].is_independent(1));
        assert!(effects[2].is_independent(2));
        assert_eq!(execute.talent_modifications()["Improved Execute"].len(), 2);
    }

    #[test]
    fn shared_cooldowns_are_attached_to_every_member() {
        let db = parse(
            r#"
spell_groups:
  - { name: Battle Stance, causes_gcd: stance, ranks: [{ rank: 1, resource: rage }] }
  - { name: Defensive Stance, causes_gcd: stance, ranks: [{ rank: 1, resource: rage }] }
  - { name: Whirlwind, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }] }
shared_spell_cooldowns:
  - [Battle Stance, Defensive Stance]
"#,
        )
        .unwrap();
        let expected: BTreeSet<String> = ["Battle Stance", "Defensive Stance"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(db.get("Battle Stance").unwrap().shared_cooldowns, expected);
        assert_eq!(
            db.get("Defensive Stance").unwrap().shared_cooldowns,
            expected
        );
        assert!(db.get("Whirlwind").unwrap().shared_cooldowns.is_empty());
    }

    #[test]
    fn unknown_effect_names_are_rejected() {
        let result = serde_yaml::from_str::<SpellFileSpec>(
            r#"
spell_groups:
  - name: Bad
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        effects:
          - { name: SCHOOL_DAMAGE_CHAOS, value: 1 }
"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn unknown_attributes_are_rejected() {
        let result = serde_yaml::from_str::<SpellFileSpec>(
            r#"
spell_groups:
  - name: Bad
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        effects:
          - { name: NO_EFFECT, magnitude: 1 }
"#,
        );
        assert!(result.is_err());
    }

    fn assert_invalid(yaml: &str, expected: &str) {
        match parse(yaml) {
            Err(SpellDbError::Invalid { message, .. }) => {
                assert!(
                    message.contains(expected),
                    "expected {expected:?} in {message:?}"
                );
            }
            other => panic!("expected an Invalid error, got {other:?}"),
        }
    }

    #[test]
    fn validation_rejects_bad_ranks_and_effects() {
        assert_invalid(
            "spell_groups: [{ name: X, causes_gcd: normal, ranks: [] }]",
            "no ranks",
        );
        assert_invalid(
            "spell_groups: [{ name: X, causes_gcd: normal, ranks: [{ rank: 0, resource: rage }] }]",
            "reserved",
        );
        assert_invalid(
            "spell_groups: [{ name: X, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }, { rank: 1, resource: rage }] }]",
            "defined twice",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: AURA_CONSUME_CHARGE }]
"#,
            "requires `buff`",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: SCHOOL_DAMAGE_PHYSICAL, min: 10 }]
"#,
            "given together",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: self
          shared: true
"#,
            "only target buffs",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: self
          duration: 10
          effects: [{ name: APPLY_AURA_PERIODIC_RESOURCE_GAIN_RAGE, value: 1 }]
"#,
            "positive `period`",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks:
      - rank: 1
        resource: rage
        buff:
          unit: target
          priority: low
          effects: [{ name: APPLY_AURA_PERIODIC_WEAPON_DAMAGE, value: 20, period: 3 }]
"#,
            "needs `ticks`",
        );
    }

    #[test]
    fn validation_rejects_bad_restrictions_and_talents() {
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    restrictions: [{ type: TARGET_HEALTH_PERCENTAGE, value: 0.2, cmp: geq }]
    ranks: [{ rank: 1, resource: rage }]
"#,
            "does not support comparison",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks: [{ rank: 1, resource: rage, effects: [{ name: NO_EFFECT }] }]
    modified_by_talent:
      - { talent: T, rank: 1, type: increase_value, effect: GAIN_RESOURCE_RAGE, value: 1 }
"#,
            "has no Spell effect",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks: [{ rank: 1, resource: rage }]
    modified_by_talent:
      - { talent: T, rank: 1, type: add_effect, target: buff, effect: { name: NO_EFFECT } }
"#,
            "a rank has none",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks: [{ rank: 1, resource: rage, cost: 5 }]
    modified_by_talent:
      - { talent: T, rank: 1, type: absolute_resource_cost_reduction, value: 6 }
"#,
            "below zero",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: normal
    ranks: [{ rank: 1, resource: rage }]
    modified_by_talent:
      - { talent: T, rank: 1, type: increase_crit_chance, value: 1 }
      - { talent: T, rank: 1, type: increase_crit_chance, value: 2 }
"#,
            "defined twice",
        );
    }

    #[test]
    fn proc_specs_are_validated() {
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: none
    proc: { rate: 0.05, sources: [MAINHAND_SWING] }
    ranks: [{ rank: 1, resource: rage }]
"#,
            "only passive",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: none
    flags: [PASSIVE_SPELL]
    ranks: [{ rank: 1, resource: rage }]
"#,
            "no proc sources",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: none
    flags: [PASSIVE_SPELL]
    proc: { rate: 1.5, ppm: true, sources: [MAINHAND_SWING] }
    ranks: [{ rank: 1, resource: rage }]
"#,
            "needs `hand`",
        );
        assert_invalid(
            r#"
spell_groups:
  - name: X
    causes_gcd: none
    flags: [PASSIVE_SPELL]
    proc: { rate: 1.5, sources: [MAINHAND_SWING] }
    ranks: [{ rank: 1, resource: rage }]
"#,
            "out of range",
        );

        let db = parse(
            r#"
spell_groups:
  - name: Sword Specialization
    causes_gcd: none
    flags: [PASSIVE_SPELL, ON_MELEE_HIT]
    proc:
      sources: [MAINHAND_SWING, OFFHAND_SWING, MAINHAND_SWING]
      requires_weapon_type: [SWORD, TWOHAND_SWORD]
    ranks:
      - rank: 1
        resource: rage
        effects: [{ name: EXTRA_ATTACK_INSTANT, hand: mainhand }]
"#,
        )
        .unwrap();
        let group = db.get("Sword Specialization").unwrap();
        assert_eq!(
            group.proc_sources(),
            vec![
                ProcSource::MainhandSwing,
                ProcSource::OffhandSwing,
                ProcSource::MeleeHit
            ]
        );
        let proc = group.proc.as_ref().unwrap();
        assert_eq!(proc.rate, 1.0);
        assert!(!proc.ppm);
        assert_eq!(
            proc.requires_weapon_type,
            vec![WeaponType::Sword, WeaponType::TwohandSword]
        );
    }

    #[test]
    fn duplicate_and_unknown_shared_cooldown_spells_are_rejected() {
        let one =
            "spell_groups: [{ name: X, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }] }]";
        let mut db = parse(one).unwrap();
        let file: SpellFileSpec = serde_yaml::from_str(one).unwrap();
        assert!(matches!(db.add_file(file), Err(SpellDbError::Duplicate(name)) if name == "X"));

        assert!(matches!(
            parse(
                r#"
spell_groups: [{ name: X, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }] }]
shared_spell_cooldowns: [[X, Y]]
"#
            ),
            Err(SpellDbError::UnknownSharedCooldownSpell(name)) if name == "Y"
        ));
        assert!(matches!(
            parse(
                r#"
spell_groups:
  - { name: X, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }] }
  - { name: Y, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }] }
shared_spell_cooldowns: [[X, Y], [X]]
"#
            ),
            Err(SpellDbError::SpellInTwoSharedCooldowns(name)) if name == "X"
        ));
    }

    #[test]
    fn groups_for_class_includes_class_independent_groups() {
        let mut db = parse(BATTLE_SHOUT).unwrap();
        db.add_file(
            serde_yaml::from_str(
                "spell_groups: [{ name: Blood Fury, causes_gcd: none, ranks: [{ rank: 1, resource: rage }] }]",
            )
            .unwrap(),
        )
        .unwrap();
        db.add_file(
            serde_yaml::from_str(
                "class: ROGUE\nspell_groups: [{ name: Eviscerate, causes_gcd: normal, ranks: [{ rank: 1, resource: energy }] }]",
            )
            .unwrap(),
        )
        .unwrap();
        let names: Vec<&str> = db
            .groups_for_class(PlayerClass::Warrior)
            .into_iter()
            .map(|g| g.name.as_str())
            .collect();
        assert_eq!(names, vec!["Battle Shout", "Blood Fury"]);
    }
}
