//! Hand-written spell overrides: what the client tables do not contain.
//!
//! `data/spells/overrides/*.yaml` is the only hand-authored spell data. It adds, per spell id,
//! the things retail keeps server-side or in scripts: which hit results a proc reacts to
//! (`spell_proc.HitMask`), what a `DUMMY` effect does, sim-only threat numbers, sim behaviour
//! flags, the link from a stance to its passive, and reactions to combat events.
//!
//! ```yaml
//! defaults:
//!   proc_hit_mask: [NORMAL, CRITICAL]    # what a proc reacts to unless overridden
//! overrides:
//!   - id: 12834                          # Deep Wounds (talent): crits only
//!     proc: { hit_mask: [CRITICAL] }
//!   - id: 12322                          # Unbridled Wrath: the rank value is the proc chance
//!     proc: { chance_effect: 0 }
//!   - id: 12162                          # Deep Wounds payload (DUMMY)
//!     effects: [{ index: 0, script: DEEP_WOUNDS_BLEED, params: { duration_spell: 412609 } }]
//!   - id: 12319                          # Flurry (talent DUMMY)
//!     proc: { hit_mask: [CRITICAL] }
//!     effects: [{ index: 0, script: TRIGGER_WITH_VALUE, params: { spell: 12966, effect: 0 } }]
//!   - id: 10612                          # Windfury Totem passive: payload id in base points
//!     effects: [{ index: 0, script: TRIGGER_SPELL, params: { spell: 10610 } }]
//!   - id: 23881                          # Bloodthirst: 35 % of attack power
//!     effects: [{ index: 1, script: ATTACK_POWER_PERCENT_DAMAGE }]
//!   - id: 25286                          # Heroic Strike r9
//!     threat: { flat: 145 }
//!   - id: 11605                          # Slam
//!     sim_flags: [RESETS_SWING_TIMERS, STOPS_ATTACK_DURING_CAST]
//!   - id: 2458                           # Berserker Stance -> its passive
//!     stance_passive: 7381
//!   - id: 7384                           # Overpower: a combo point when the target dodges
//!     on_event: [{ source: MELEE_DODGE, script: ADD_COMBO_POINTS, params: { value: 1 } }]
//!   - id: 12292                          # Sweeping Strikes: multi-target, no-op here
//!     sim_flags: [IGNORED]
//! ```
//!
//! The *mechanics* behind a script are a closed Rust enum ([`ScriptKind`]); the mapping from a
//! spell id to a script is data, so a re-tuned or renumbered spell only needs its override
//! changed. Overrides never replace table values: the [`crate::spell::record::SpellRecord`] stays
//! the table row and the override is consulted next to it.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::proc::ProcSource;
use crate::spell::Hand;
use crate::spell::dbc::{PowerType, dbc_flags};
use crate::target::{CreatureTypes, Priority};

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

dbc_flags! {
    /// Retail `ProcFlagsHit` (the server-side `spell_proc.HitMask`): which outcomes of a
    /// `proc_type_mask` event let a proc fire.
    ProcHitMask {
        /// A plain hit (glancing blows included).
        NORMAL = 0x0001 => "NORMAL",
        CRITICAL = 0x0002 => "CRITICAL",
        MISS = 0x0004 => "MISS",
        FULL_RESIST = 0x0008 => "FULL_RESIST",
        PARTIAL_RESIST = 0x0010 => "PARTIAL_RESIST",
        DODGE = 0x0020 => "DODGE",
        PARRY = 0x0040 => "PARRY",
        BLOCK = 0x0080 => "BLOCK",
        EVADE = 0x0100 => "EVADE",
        IMMUNE = 0x0200 => "IMMUNE",
        DEFLECT = 0x0400 => "DEFLECT",
        ABSORB = 0x0800 => "ABSORB",
        REFLECT = 0x1000 => "REFLECT",
        INTERRUPT = 0x2000 => "INTERRUPT",
        FULL_BLOCK = 0x4000 => "FULL_BLOCK",
    }
}

impl ProcHitMask {
    /// Retail's default when `spell_proc` has no row: a landed hit, normal or critical.
    pub const LANDED: Self = Self(0x0003);
}

/// Per-file defaults that every override may leave implicit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideDefaults {
    /// Hit results a proc reacts to unless its override says otherwise.
    #[serde(default = "default_proc_hit_mask")]
    pub proc_hit_mask: ProcHitMask,
}

fn default_proc_hit_mask() -> ProcHitMask {
    ProcHitMask::LANDED
}

impl Default for OverrideDefaults {
    fn default() -> Self {
        Self {
            proc_hit_mask: ProcHitMask::LANDED,
        }
    }
}

/// One `data/spells/overrides/*.yaml` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideFile {
    #[serde(default, skip_serializing_if = "is_default")]
    pub defaults: OverrideDefaults,
    #[serde(default)]
    pub overrides: Vec<SpellOverride>,
}

/// The hand-written mechanics a `DUMMY` effect or an event reaction can name.
///
/// The interpreter (Phase 3T.6 onwards) implements each of these once; the data decides which
/// spell and effect they attach to. Adding a mechanic means adding a variant here and its logic
/// there, never a spell-specific branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScriptKind {
    /// Damage equal to `base_points` % of attack power (Bloodthirst's second effect).
    AttackPowerPercentDamage,
    /// Execute: `base_points` damage plus `chain_amplitude × 10` per rage point spent beyond
    /// the cost; consumes all rage.
    Execute,
    /// Deep Wounds: `base_points` % of the average weapon damage bleeds over the duration of
    /// `params.duration_spell`, one independent stack per application.
    DeepWoundsBleed,
    /// Casts `params.spell` with effect `params.effect`'s value replaced by this effect's value
    /// (Flurry's talent value into its haste buff, Enrage's into its damage buff).
    TriggerWithValue,
    /// Casts `params.spell` when the proc fires, forwarding nothing: what a server-side
    /// `DUMMY` proc aura, or a `PROC_TRIGGER_SPELL` aura without a trigger spell, does
    /// (Windfury Totem's party aura casting its payload).
    TriggerSpell,
    /// Rage retained when changing stance is raised by `base_points` (Tactical Mastery).
    StanceRageRetained,
    /// `base_points` (or `params.value`) of `params.resource` (rage by default) every
    /// `params.period_ms` while in combat (Anger Management).
    PeriodicResourceGain,
    /// Off-hand rage generation raised by `base_points` % (Dual Wield Specialization).
    OffhandRagePercent,
    /// Gain `base_points` (or `params.value`, in stored units) of `params.resource` whenever
    /// `params.spell` is used (Improved Berserker Rage's rage on Berserker Rage).
    GainResourceOnUse,
    /// Extra attacks from `params.spell` (Sword Specialization style `ADD_EXTRA_ATTACKS` payloads
    /// with a weapon condition on the aura).
    ExtraAttack,
    /// While the aura is up the character has the proc aura `params.spell`, a hidden aura the
    /// server applies (its `ProcTypeMask`, weapon requirement, internal cooldown and payload
    /// come from its record), firing with this effect's value as its chance in percent
    /// (Weaponmaster's sword extra attack).
    EnableProc,
    /// While the aura is up the character has the hidden aura `params.spell` the server applies
    /// (gated by its own equipment requirement), with its effect `params.effect` set to this
    /// effect's value (Weaponmaster's axe/polearm crit and mace/staff armor penetration).
    EnableAura,
    /// Grants `params.value` combo points to the character.
    AddComboPoints,
    /// Finishes the cooldown of `params.spell`, or of every spell of the spell's family in
    /// `params.family_mask` (Preparation), when the spell is cast. As an event reaction (no
    /// runtime yet) it names `params.spell`.
    ResetCooldown,
    /// Weapon-damage bonus `base_points` % with the weapon types the aura requires
    /// (Weaponmaster's per-weapon bonuses).
    WeaponTypeDamagePercent,
    /// Crit chance `base_points` % with the weapon types the aura requires.
    WeaponTypeCritPercent,
    /// A `MOD_CRIT_PCT` aura whose `base_points` % crit applies to spells and melee abilities
    /// but not to auto attacks (Axe and Sword Specialization).
    AbilityCritPercent,
    /// Ability `spell` also strikes with the off-hand weapon (Raging Blows: Whirlwind).
    OffhandCopy,
    /// Against the `params.creature_types`, the spell's weapon damage gains `base_points`
    /// times itself (Spearing Strike: 40 % weapon damage plus 2 × 40 % against giants and
    /// dragonkin).
    ExtraWeaponDamageVsCreatureTypes,
    /// An `ENERGIZE` effect gives `params.value` times its amount while a two-hand weapon is
    /// equipped (Unbridled Wrath: 1 rage, 2 with a two-hander).
    TwoHandEnergizeMultiplier,
    /// A finisher's attack power share, which the tables only mention in the description:
    /// `params.value` % of attack power per combo point spent, or the `params.per_combo_point`
    /// entry (% of attack power for 1 to 5 points). Without `params.effect` the effect deals it
    /// with the spell's direct damage (Eviscerate); with it, the share is spread over the ticks
    /// of that periodic aura effect (Rupture).
    ComboPointApDamage,
    /// A bleed's attack power share: `params.value` % of attack power added to every tick of
    /// this periodic aura effect (Garrote).
    AttackPowerPerTick,
    /// The attack power coefficient the tables leave at 0 (`BonusCoefficientFromAP`):
    /// `params.value` times attack power added to this effect's damage, the hit of a direct
    /// damage effect or every tick (per stack) of a periodic damage aura (the poisons, TASKS.md
    /// decision 2).
    ApCoefficient,
    /// While the main-hand weapon's subclass is in `params.weapon_subclass_mask`, this effect's
    /// value replaces the value of effect `params.effect` (Ghostly Strike: 180 % weapon damage
    /// instead of 125 % with a dagger).
    WeaponTypeValue,
    /// The spells this one triggers deal `base_points` % more damage while one of the caster's
    /// poisons is on the target (Mutilate).
    DamagePercentVsPoisoned,
    /// An armor reduction on the target that shares one slot with the other exclusive ones:
    /// only the strongest applies (Sunder Armor and Expose Armor, forever-bugs #112). Goes on
    /// the `MOD_RESISTANCE` aura effect itself.
    ExclusiveArmorReduction,
    /// The spells of `params.family_mask` deal `base_points` % more damage while the target's
    /// health is below effect `params.effect`'s value in percent (Quietus: 2-10 % below 35 %).
    DamagePercentBelowHealth,
    /// Explicitly does nothing (documented no-op, keeps the effect out of the unsupported list).
    NoOp,
}

/// Parameters a script may need beyond its effect row. Which ones are required depends on the
/// [`ScriptKind`]; see [`EffectScript::validate`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptParams {
    /// Another spell the script refers to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spell: Option<u32>,
    /// An effect index in `spell`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<u32>,
    /// A plain amount.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// A spell whose duration the script uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_spell: Option<u32>,
    /// A tick interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_ms: Option<u32>,
    /// A resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<PowerType>,
    /// Creature types the script applies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creature_types: Option<CreatureTypes>,
    /// One amount per combo point spent, for 1 to 5 points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_combo_point: Option<[f64; 5]>,
    /// Weapon subclasses, as a `SpellEquippedItems` subclass mask (32768 = dagger).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_subclass_mask: Option<u32>,
    /// The spells the script applies to, as a class mask of the spell's family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_mask: Option<[u32; 4]>,
}

impl ScriptParams {
    /// The attack power percent of a `COMBO_POINT_AP_DAMAGE` script after spending
    /// `combo_points`: `value` per point, or the `per_combo_point` entry for the count (5 for
    /// more). 0 without combo points.
    pub fn combo_point_ap_percent(&self, combo_points: u32) -> f64 {
        if combo_points == 0 {
            return 0.0;
        }
        match (self.value, self.per_combo_point) {
            (Some(per_point), _) => per_point * f64::from(combo_points),
            (None, Some(table)) => table[(combo_points.min(5) - 1) as usize],
            (None, None) => 0.0,
        }
    }
}

/// The aura effect that enables a hidden aura the server applies (`ENABLE_PROC`,
/// `ENABLE_AURA`): the hidden aura is a passive of the character while the enabling aura is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnablingAura {
    /// The enabling spell.
    pub spell: u32,
    /// Its effect whose value the hidden aura uses.
    pub effect: u32,
    /// The hidden aura's effect that takes the value (`ENABLE_AURA`); `None` when the value is
    /// the hidden proc aura's chance in percent (`ENABLE_PROC`).
    pub target_effect: Option<u32>,
}

/// A script attached to one effect of the spell.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectScript {
    /// The `EffectRecord::index` the script replaces.
    pub index: u32,
    pub script: ScriptKind,
    #[serde(default, skip_serializing_if = "is_default")]
    pub params: ScriptParams,
}

impl EffectScript {
    /// Checks that the script has the parameters it needs.
    pub fn validate(&self) -> Result<(), String> {
        let p = &self.params;
        let need = |ok: bool, what: &str| {
            if ok {
                Ok(())
            } else {
                Err(format!("{:?} needs params.{what}", self.script))
            }
        };
        match self.script {
            ScriptKind::TriggerWithValue => {
                need(p.spell.is_some(), "spell")?;
                need(p.effect.is_some(), "effect")
            }
            ScriptKind::DeepWoundsBleed => need(p.duration_spell.is_some(), "duration_spell"),
            ScriptKind::PeriodicResourceGain => {
                need(p.period_ms.is_some_and(|ms| ms > 0), "period_ms (> 0)")
            }
            ScriptKind::GainResourceOnUse => {
                need(p.spell.is_some(), "spell")?;
                need(p.resource.is_some(), "resource")
            }
            ScriptKind::ResetCooldown => need(
                p.spell.is_some() || p.family_mask.is_some(),
                "spell or family_mask",
            ),
            ScriptKind::ExtraAttack
            | ScriptKind::TriggerSpell
            | ScriptKind::OffhandCopy
            | ScriptKind::EnableProc => need(p.spell.is_some(), "spell"),
            ScriptKind::EnableAura => {
                need(p.spell.is_some(), "spell")?;
                need(p.effect.is_some(), "effect")
            }
            ScriptKind::ExtraWeaponDamageVsCreatureTypes => need(
                p.creature_types.is_some_and(|t| !t.is_empty()),
                "creature_types (not empty)",
            ),
            ScriptKind::AddComboPoints | ScriptKind::TwoHandEnergizeMultiplier => {
                need(p.value.is_some_and(|v| v > 0.0), "value (> 0)")
            }
            ScriptKind::ComboPointApDamage => need(
                p.value.is_some() != p.per_combo_point.is_some(),
                "value or per_combo_point (one of them)",
            ),
            ScriptKind::AttackPowerPerTick => need(p.value.is_some(), "value"),
            ScriptKind::ApCoefficient => need(p.value.is_some_and(|v| v > 0.0), "value (> 0)"),
            ScriptKind::DamagePercentBelowHealth => {
                need(p.effect.is_some(), "effect")?;
                need(
                    p.family_mask.is_some_and(|mask| mask != [0; 4]),
                    "family_mask (not 0)",
                )
            }
            ScriptKind::WeaponTypeValue => {
                need(p.effect.is_some(), "effect")?;
                need(
                    p.weapon_subclass_mask.is_some_and(|mask| mask != 0),
                    "weapon_subclass_mask (not 0)",
                )
            }
            ScriptKind::AttackPowerPercentDamage
            | ScriptKind::Execute
            | ScriptKind::StanceRageRetained
            | ScriptKind::OffhandRagePercent
            | ScriptKind::WeaponTypeDamagePercent
            | ScriptKind::WeaponTypeCritPercent
            | ScriptKind::AbilityCritPercent
            | ScriptKind::DamagePercentVsPoisoned
            | ScriptKind::ExclusiveArmorReduction
            | ScriptKind::NoOp => Ok(()),
        }
    }
}

/// A script run when a combat event involving the spell's owner happens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventScript {
    /// The event.
    pub source: ProcSource,
    pub script: ScriptKind,
    #[serde(default, skip_serializing_if = "is_default")]
    pub params: ScriptParams,
}

/// Proc conditions the client tables do not carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcOverride {
    /// Hit results the proc fires on; the file default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_mask: Option<ProcHitMask>,
    /// The aura effect whose value is the proc chance in percent, for talents whose rank value
    /// is the chance (Unbridled Wrath 12/24/36/48/60 %) rather than the payload's value: the
    /// table's `ProcChance` is the max-rank number. An effect without a value gives its value
    /// per combo point (`points_per_resource`: Revealed Flaw's 5 %, with
    /// `chance_per_combo_point`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chance_effect: Option<u32>,
    /// The weapon a scripted proc is bound to: only that hand's swings and abilities trigger
    /// it (Windfury Totem's scripted proc fires off the main-hand weapon it enchants, never
    /// off off-hand swings). Absent, the `ProcTypeMask` decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<Hand>,
    /// The proc chance in percent where the server, not the table, decides: an item's chance
    /// on hit (its payload's `ProcChance` is 101, "handled by the effect"), or a chance the
    /// tooltip divides by a racial multiplier the sim's target never has (Hand of Justice:
    /// 3 % against Dwarves, 1 % otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chance: Option<f64>,
    /// Procs per minute (chance = ppm × the triggering weapon's speed / 60) where the server
    /// decides, as [`ProcOverride::chance`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ppm: Option<f64>,
    /// A spell (any rank) whose aura the character must have up on the target for the proc to
    /// fire (Bloodthrill: main-hand attacks against enemies afflicted by your Rend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_aura: Option<u32>,
    /// The proc fires on finishing moves ([`ProcSource::Finisher`]: a finisher spent its
    /// combo points) instead of the events its `ProcTypeMask` names (Ruthlessness, Relentless
    /// Strikes, Improved Expose Armor).
    #[serde(default, skip_serializing_if = "is_default")]
    pub finisher: bool,
    /// The spells whose events trigger the proc, as a `SpellClassMask` of the proc's class
    /// family (the server's `spell_proc.SpellFamilyMask`: Puncturing Wounds on Backstab,
    /// Thousand Cuts on Rupture's ticks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_mask: Option<[u32; 4]>,
    /// As `family_mask`, the `SpellClassMask` of this aura effect of the proc (the server's
    /// default when `spell_proc` names no mask: Head Rush, Revealed Flaw).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_mask_effect: Option<u32>,
    /// The aura effect whose value is the least number of combo points the finisher must have
    /// spent (Improved Expose Armor's `$m3`: 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo_points_effect: Option<u32>,
    /// The chance is per combo point the finisher spent (Relentless Strikes: 20 % per point).
    #[serde(default, skip_serializing_if = "is_default")]
    pub chance_per_combo_point: bool,
    /// Only the events of a spell that awards combo points (a builder: Seal Fate on the
    /// critical strikes of Sinister Strike, Backstab, Mutilate's strikes, ...).
    #[serde(default, skip_serializing_if = "is_default")]
    pub builder: bool,
}

impl ProcOverride {
    /// Whether the override gives the proc its rate (a chance or procs per minute).
    pub fn has_rate(&self) -> bool {
        self.chance.is_some() || self.ppm.is_some()
    }
}

/// Threat the client tables do not carry (innate threat of Heroic Strike, Revenge, Shield Slam).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreatOverride {
    /// Threat added on a successful cast.
    #[serde(default, skip_serializing_if = "is_default")]
    pub flat: f64,
    /// Multiplier on the spell's damage-based threat.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub modifier: f64,
}

fn one() -> f64 {
    1.0
}

fn is_one(value: &f64) -> bool {
    *value == 1.0
}

impl Default for ThreatOverride {
    fn default() -> Self {
        Self {
            flat: 0.0,
            modifier: 1.0,
        }
    }
}

/// Sim behaviour that is neither in the tables nor a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SimFlag {
    /// The sim does not model the spell (multi-target, PvP, movement); it loads but never casts.
    Ignored,
    /// A successful cast restarts both swing timers (Slam).
    ResetsSwingTimers,
    /// Auto attacks pause while the spell is being cast (Slam).
    StopsAttackDuringCast,
    /// Casting it discards a queued on-next-swing spell (Slam vs Heroic Strike).
    CancelsNextSwingQueue,
    /// The passive is active from the start of combat (Anger Management).
    StartOfCombat,
    /// The spell's damage never crits (Rend's bleed in Classic).
    CannotCrit,
    /// While the buff is active the character is enraged (the `ENRAGED` caster aura state;
    /// the client tables do not carry the enrage mechanic).
    Enrage,
}

/// Everything hand-written about one spell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpellOverride {
    pub id: u32,
    /// Free text for the maintainer (why the override exists).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proc: Option<ProcOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectScript>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threat: Option<ThreatOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sim_flags: Vec<SimFlag>,
    /// The hidden passive that carries a stance's numbers (`SpellShapeshiftForm.PresetSpellID`
    /// is empty in the dump).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stance_passive: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_event: Vec<EventScript>,
    /// Priority of the spell's debuff when the target's debuff slots are full.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debuff_priority: Option<Priority>,
    /// Whether the spell's debuff is one instance shared by the raid (Sunder Armor) rather than
    /// one per caster; by default a stacking debuff is shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debuff_shared: Option<bool>,
    /// The spells whose buffs end when this spell's buff ends (the server removes Jom Gabbar's
    /// permanent attack power stacks with the trinket's aura).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ends_auras: Vec<u32>,
}

impl SpellOverride {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    /// The script attached to effect `index`, if any.
    pub fn effect_script(&self, index: u32) -> Option<&EffectScript> {
        self.effects.iter().find(|e| e.index == index)
    }

    pub fn has_sim_flag(&self, flag: SimFlag) -> bool {
        self.sim_flags.contains(&flag)
    }

    /// Whether the sim leaves the spell unused.
    pub fn is_ignored(&self) -> bool {
        self.has_sim_flag(SimFlag::Ignored)
    }

    /// The spells this override refers to (targets that must exist), without duplicates.
    pub fn referenced_spells(&self) -> Vec<u32> {
        let mut ids = Vec::new();
        let mut push = |id: Option<u32>| {
            if let Some(id) = id
                && id != 0
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        };
        push(self.stance_passive);
        for script in &self.effects {
            push(script.params.spell);
            push(script.params.duration_spell);
        }
        for event in &self.on_event {
            push(event.params.spell);
            push(event.params.duration_spell);
        }
        for &id in &self.ends_auras {
            push(Some(id));
        }
        push(self.proc.and_then(|p| p.target_aura));
        ids
    }

    /// Structural checks that do not need the spell records: no duplicate effect scripts,
    /// every script has its parameters, sane numbers.
    pub fn validate(&self) -> Result<(), OverrideError> {
        let invalid = |message: String| OverrideError::Invalid {
            spell: self.id,
            message,
        };
        if self.id == 0 {
            return Err(invalid("the spell id is 0".into()));
        }
        for (i, script) in self.effects.iter().enumerate() {
            if self.effects[..i].iter().any(|e| e.index == script.index) {
                return Err(invalid(format!("effect {} has two scripts", script.index)));
            }
            script.validate().map_err(invalid)?;
        }
        for event in &self.on_event {
            // The engine runs event reactions in `CharacterContext::run_event_scripts`; a
            // script it does not run there is refused here rather than silently ignored.
            if event.script != ScriptKind::AddComboPoints {
                return Err(invalid(format!(
                    "on_event does not support {:?} (only ADD_COMBO_POINTS)",
                    event.script
                )));
            }
            if event.source == ProcSource::Manual {
                return Err(invalid("on_event cannot react to MANUAL".into()));
            }
            EffectScript {
                index: 0,
                script: event.script,
                params: event.params,
            }
            .validate()
            .map_err(invalid)?;
        }
        if let Some(threat) = &self.threat
            && (threat.flat < 0.0 || threat.modifier < 0.0)
        {
            return Err(invalid("threat must not be negative".into()));
        }
        let mut flags = self.sim_flags.clone();
        flags.sort();
        flags.dedup();
        if flags.len() != self.sim_flags.len() {
            return Err(invalid("a sim flag is listed twice".into()));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OverrideError {
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
    #[error("spell {0} has two overrides")]
    Duplicate(u32),
    #[error("override for spell {spell}: {message}")]
    Invalid { spell: u32, message: String },
    #[error("override defaults: {0}")]
    InvalidDefaults(String),
}

/// The overrides of every file under `data/spells/overrides/`, by spell id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    defaults: OverrideDefaults,
    by_id: HashMap<u32, SpellOverride>,
}

impl Overrides {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads every `*.yaml` / `*.yml` file directly in `dir` (sorted by name). A missing
    /// directory means "no overrides".
    pub fn load(dir: &Path) -> Result<Self, OverrideError> {
        let mut overrides = Self::new();
        if !dir.exists() {
            return Ok(overrides);
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|source| OverrideError::Io {
                path: dir.to_path_buf(),
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
            overrides.load_file(&path)?;
        }
        Ok(overrides)
    }

    /// Adds the overrides of one YAML file.
    pub fn load_file(&mut self, path: &Path) -> Result<(), OverrideError> {
        let text = fs::read_to_string(path).map_err(|source| OverrideError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file: OverrideFile =
            serde_yaml::from_str(&text).map_err(|source| OverrideError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        self.add_file(file)
    }

    /// Validates and adds one file. The last file's `defaults` win; they must agree when set in
    /// several files.
    pub fn add_file(&mut self, file: OverrideFile) -> Result<(), OverrideError> {
        if file.defaults.proc_hit_mask.is_empty() {
            return Err(OverrideError::InvalidDefaults(
                "proc_hit_mask must name at least one hit result".into(),
            ));
        }
        for spell in &file.overrides {
            spell.validate()?;
            if self.by_id.contains_key(&spell.id)
                || file.overrides.iter().filter(|o| o.id == spell.id).count() > 1
            {
                return Err(OverrideError::Duplicate(spell.id));
            }
        }
        self.defaults = file.defaults;
        for spell in file.overrides {
            self.by_id.insert(spell.id, spell);
        }
        Ok(())
    }

    /// Adds one override.
    pub fn add(&mut self, spell: SpellOverride) -> Result<(), OverrideError> {
        spell.validate()?;
        if self.by_id.contains_key(&spell.id) {
            return Err(OverrideError::Duplicate(spell.id));
        }
        self.by_id.insert(spell.id, spell);
        Ok(())
    }

    pub fn defaults(&self) -> &OverrideDefaults {
        &self.defaults
    }

    pub fn set_defaults(&mut self, defaults: OverrideDefaults) {
        self.defaults = defaults;
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The override of `id`, if any.
    pub fn get(&self, id: u32) -> Option<&SpellOverride> {
        self.by_id.get(&id)
    }

    /// Every override, sorted by spell id.
    pub fn all(&self) -> Vec<&SpellOverride> {
        let ids: BTreeMap<u32, &SpellOverride> =
            self.by_id.iter().map(|(id, o)| (*id, o)).collect();
        ids.into_values().collect()
    }

    /// The hit results the proc aura `id` fires on.
    pub fn proc_hit_mask(&self, id: u32) -> ProcHitMask {
        self.get(id)
            .and_then(|o| o.proc.and_then(|p| p.hit_mask))
            .unwrap_or(self.defaults.proc_hit_mask)
    }

    /// The aura effect whose `ENABLE_PROC` / `ENABLE_AURA` script enables the hidden aura `id`,
    /// if any.
    pub fn enabled_by(&self, id: u32) -> Option<EnablingAura> {
        self.all().into_iter().find_map(|o| {
            o.effects.iter().find_map(|e| {
                let target_effect = match e.script {
                    ScriptKind::EnableProc => None,
                    ScriptKind::EnableAura => Some(e.params.effect?),
                    _ => return None,
                };
                (e.params.spell == Some(id)).then_some(EnablingAura {
                    spell: o.id,
                    effect: e.index,
                    target_effect,
                })
            })
        })
    }

    /// The script attached to effect `index` of spell `id`, if any.
    pub fn effect_script(&self, id: u32, index: u32) -> Option<&EffectScript> {
        self.get(id).and_then(|o| o.effect_script(index))
    }

    /// The innate threat of spell `id` (the default when there is no override).
    pub fn threat(&self, id: u32) -> ThreatOverride {
        self.get(id).and_then(|o| o.threat).unwrap_or_default()
    }

    /// Whether spell `id` carries `flag`.
    pub fn has_sim_flag(&self, id: u32, flag: SimFlag) -> bool {
        self.get(id).is_some_and(|o| o.has_sim_flag(flag))
    }

    /// The passive that carries the numbers of stance spell `id`.
    pub fn stance_passive(&self, id: u32) -> Option<u32> {
        self.get(id).and_then(|o| o.stance_passive)
    }

    /// The event reactions of spell `id`.
    pub fn event_scripts(&self, id: u32) -> &[EventScript] {
        self.get(id).map_or(&[], |o| o.on_event.as_slice())
    }

    /// The debuff priority of spell `id`, if overridden.
    pub fn debuff_priority(&self, id: u32) -> Option<Priority> {
        self.get(id).and_then(|o| o.debuff_priority)
    }

    /// Whether spell `id`'s debuff is raid-shared, if overridden.
    pub fn debuff_shared(&self, id: u32) -> Option<bool> {
        self.get(id).and_then(|o| o.debuff_shared)
    }

    /// The spells whose buffs end with spell `id`'s buff (`ends_auras`).
    pub fn ends_auras(&self, id: u32) -> &[u32] {
        self.get(id).map_or(&[], |o| o.ends_auras.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::CreatureType;

    const WARRIOR: &str = r#"
defaults:
  proc_hit_mask: [NORMAL, CRITICAL]
overrides:
  - id: 12834
    note: Deep Wounds procs on crits only (server-side spell_proc)
    proc: { hit_mask: [CRITICAL] }
  - id: 12162
    effects: [{ index: 0, script: DEEP_WOUNDS_BLEED, params: { duration_spell: 412609 } }]
  - id: 12319
    proc: { hit_mask: CRITICAL }
    effects: [{ index: 0, script: TRIGGER_WITH_VALUE, params: { spell: 12966, effect: 0 } }]
  - id: 23881
    effects: [{ index: 1, script: ATTACK_POWER_PERCENT_DAMAGE }]
  - id: 25286
    threat: { flat: 145 }
  - id: 11605
    sim_flags: [RESETS_SWING_TIMERS, STOPS_ATTACK_DURING_CAST, CANCELS_NEXT_SWING_QUEUE]
  - id: 2458
    stance_passive: 7381
  - id: 7384
    on_event: [{ source: MELEE_DODGE, script: ADD_COMBO_POINTS, params: { value: 1 } }]
  - id: 12292
    sim_flags: [IGNORED]
  - id: 11597
    debuff_priority: high
  - id: 10612
    proc: { hand: mainhand }
  - id: 1289682
    proc: { chance_effect: 0, hand: mainhand, target_aura: 772 }
"#;

    fn overrides() -> Overrides {
        let mut overrides = Overrides::new();
        overrides
            .add_file(serde_yaml::from_str(WARRIOR).unwrap())
            .unwrap();
        overrides
    }

    #[test]
    fn overrides_are_looked_up_by_spell_id_with_file_defaults() {
        let o = overrides();
        assert_eq!(o.len(), 12);
        assert_eq!(o.proc_hit_mask(12834), ProcHitMask::CRITICAL);
        assert_eq!(
            o.get(10612).and_then(|w| w.proc?.hand),
            Some(Hand::Mainhand),
            "a scripted proc bound to a weapon"
        );
        assert_eq!(o.get(12834).and_then(|d| d.proc?.hand), None);
        assert_eq!(o.proc_hit_mask(12319), ProcHitMask::CRITICAL, "single name");
        assert_eq!(o.proc_hit_mask(12322), ProcHitMask::LANDED, "no override");
        assert_eq!(
            o.proc_hit_mask(2458),
            ProcHitMask::LANDED,
            "override without proc"
        );
        let bleed = o.effect_script(12162, 0).unwrap();
        assert_eq!(bleed.script, ScriptKind::DeepWoundsBleed);
        assert_eq!(bleed.params.duration_spell, Some(412609));
        assert!(o.effect_script(12162, 1).is_none());
        assert!(o.effect_script(23881, 0).is_none());
        assert_eq!(
            o.effect_script(23881, 1).unwrap().script,
            ScriptKind::AttackPowerPercentDamage
        );
        let flurry = o.effect_script(12319, 0).unwrap();
        assert_eq!(
            (flurry.params.spell, flurry.params.effect),
            (Some(12966), Some(0))
        );
        assert_eq!(o.threat(25286).flat, 145.0);
        assert_eq!(o.threat(25286).modifier, 1.0);
        assert_eq!(o.threat(1).flat, 0.0);
        assert!(o.has_sim_flag(11605, SimFlag::ResetsSwingTimers));
        assert!(o.has_sim_flag(11605, SimFlag::CancelsNextSwingQueue));
        assert!(!o.has_sim_flag(11605, SimFlag::Ignored));
        assert!(o.get(12292).unwrap().is_ignored());
        assert_eq!(o.stance_passive(2458), Some(7381));
        assert_eq!(o.stance_passive(71), None);
        let dodge = &o.event_scripts(7384)[0];
        assert_eq!(dodge.source, ProcSource::MeleeDodge);
        assert_eq!(dodge.script, ScriptKind::AddComboPoints);
        assert_eq!(dodge.params.value, Some(1.0));
        assert!(o.event_scripts(12834).is_empty());
        assert_eq!(o.debuff_priority(11597), Some(Priority::High));
        assert_eq!(o.debuff_priority(25286), None);
        assert_eq!(
            o.get(12834).unwrap().note,
            "Deep Wounds procs on crits only (server-side spell_proc)"
        );
        let ids: Vec<u32> = o.all().iter().map(|s| s.id).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(o.get(2458).unwrap().referenced_spells(), [7381]);
        assert_eq!(o.get(12319).unwrap().referenced_spells(), [12966]);
        assert_eq!(o.get(12162).unwrap().referenced_spells(), [412609]);
        assert!(o.get(25286).unwrap().referenced_spells().is_empty());
        assert_eq!(
            o.get(1289682).and_then(|b| b.proc?.target_aura),
            Some(772),
            "a proc on the character's aura on the target"
        );
        assert_eq!(o.get(1289682).unwrap().referenced_spells(), [772]);
    }

    #[test]
    fn missing_defaults_fall_back_to_the_constants() {
        let file: OverrideFile = serde_yaml::from_str("overrides: []").unwrap();
        assert_eq!(file.defaults, OverrideDefaults::default());
        assert_eq!(file.defaults.proc_hit_mask, ProcHitMask::LANDED);
        let empty: OverrideFile = serde_yaml::from_str("{}").unwrap();
        assert!(empty.overrides.is_empty());
        let mut o = Overrides::new();
        o.add_file(file).unwrap();
        assert!(o.is_empty());
        assert_eq!(o.proc_hit_mask(1), ProcHitMask::LANDED);
        o.set_defaults(OverrideDefaults {
            proc_hit_mask: ProcHitMask::NORMAL,
        });
        assert_eq!(o.defaults().proc_hit_mask, ProcHitMask::NORMAL);
    }

    #[test]
    fn scripts_are_checked_for_their_parameters() {
        let script = |kind: ScriptKind, params: ScriptParams| EffectScript {
            index: 0,
            script: kind,
            params,
        };
        let none = ScriptParams::default();
        assert!(
            script(ScriptKind::TriggerWithValue, none)
                .validate()
                .is_err()
        );
        assert!(
            script(
                ScriptKind::TriggerWithValue,
                ScriptParams {
                    spell: Some(1),
                    ..none
                }
            )
            .validate()
            .is_err()
        );
        assert!(
            script(
                ScriptKind::TriggerWithValue,
                ScriptParams {
                    spell: Some(1),
                    effect: Some(0),
                    ..none
                }
            )
            .validate()
            .is_ok()
        );
        assert!(
            script(ScriptKind::DeepWoundsBleed, none)
                .validate()
                .is_err()
        );
        assert!(
            script(ScriptKind::PeriodicResourceGain, none)
                .validate()
                .is_err()
        );
        assert!(
            script(
                ScriptKind::PeriodicResourceGain,
                ScriptParams {
                    period_ms: Some(0),
                    ..none
                }
            )
            .validate()
            .is_err()
        );
        assert!(
            script(
                ScriptKind::PeriodicResourceGain,
                ScriptParams {
                    period_ms: Some(3000),
                    ..none
                }
            )
            .validate()
            .is_ok()
        );
        assert!(
            script(ScriptKind::GainResourceOnUse, none)
                .validate()
                .is_err()
        );
        assert!(
            script(
                ScriptKind::GainResourceOnUse,
                ScriptParams {
                    spell: Some(18499),
                    resource: Some(PowerType::Rage),
                    ..none
                }
            )
            .validate()
            .is_ok()
        );
        assert!(script(ScriptKind::ExtraAttack, none).validate().is_err());
        assert!(script(ScriptKind::ResetCooldown, none).validate().is_err());
        assert!(script(ScriptKind::OffhandCopy, none).validate().is_err());
        assert!(script(ScriptKind::EnableProc, none).validate().is_err());
        assert!(
            script(ScriptKind::ExtraWeaponDamageVsCreatureTypes, none)
                .validate()
                .is_err()
        );
        assert!(
            script(
                ScriptKind::ExtraWeaponDamageVsCreatureTypes,
                ScriptParams {
                    creature_types: Some(CreatureTypes::default()),
                    ..none
                }
            )
            .validate()
            .is_err()
        );
        assert!(
            script(
                ScriptKind::ExtraWeaponDamageVsCreatureTypes,
                ScriptParams {
                    creature_types: Some([CreatureType::Giant].into_iter().collect()),
                    ..none
                }
            )
            .validate()
            .is_ok()
        );
        assert!(
            script(
                ScriptKind::EnableAura,
                ScriptParams {
                    spell: Some(1),
                    ..none
                }
            )
            .validate()
            .is_err()
        );
        assert!(
            script(
                ScriptKind::AddComboPoints,
                ScriptParams {
                    value: Some(0.0),
                    ..none
                }
            )
            .validate()
            .is_err()
        );
        let err = script(ScriptKind::AddComboPoints, none)
            .validate()
            .unwrap_err();
        assert_eq!(err, "AddComboPoints needs params.value (> 0)");
        for kind in [
            ScriptKind::AttackPowerPercentDamage,
            ScriptKind::Execute,
            ScriptKind::StanceRageRetained,
            ScriptKind::OffhandRagePercent,
            ScriptKind::WeaponTypeDamagePercent,
            ScriptKind::WeaponTypeCritPercent,
            ScriptKind::AbilityCritPercent,
            ScriptKind::NoOp,
        ] {
            assert!(script(kind, none).validate().is_ok(), "{kind:?}");
        }
    }

    #[test]
    fn invalid_overrides_are_rejected() {
        let parse = |yaml: &str| -> Result<(), OverrideError> {
            Overrides::new().add_file(serde_yaml::from_str(yaml).unwrap())
        };
        assert!(matches!(
            parse("overrides: [{ id: 1 }, { id: 1 }]"),
            Err(OverrideError::Duplicate(1))
        ));
        assert!(matches!(
            parse("overrides: [{ id: 0 }]"),
            Err(OverrideError::Invalid { spell: 0, .. })
        ));
        assert!(matches!(
            parse(
                "overrides: [{ id: 1, effects: [{ index: 0, script: NO_OP }, { index: 0, script: NO_OP }] }]"
            ),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse("overrides: [{ id: 1, effects: [{ index: 0, script: DEEP_WOUNDS_BLEED }] }]"),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse(
                "overrides: [{ id: 1, on_event: [{ source: MELEE_HIT, script: ADD_COMBO_POINTS }] }]"
            ),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse(
                "overrides: [{ id: 1, on_event: [{ source: MELEE_HIT, script: RESET_COOLDOWN, params: { spell: 2 } }] }]"
            ),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse(
                "overrides: [{ id: 1, on_event: [{ source: MANUAL, script: ADD_COMBO_POINTS, params: { value: 1 } }] }]"
            ),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse("overrides: [{ id: 1, threat: { flat: -1 } }]"),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse("overrides: [{ id: 1, sim_flags: [IGNORED, IGNORED] }]"),
            Err(OverrideError::Invalid { spell: 1, .. })
        ));
        assert!(matches!(
            parse("defaults: { proc_hit_mask: [] }"),
            Err(OverrideError::InvalidDefaults(_))
        ));
        // Unknown keys and unknown scripts are YAML errors: the files are hand-written.
        assert!(
            serde_yaml::from_str::<OverrideFile>("overrides: [{ id: 1, scritp: NO_OP }]").is_err()
        );
        assert!(
            serde_yaml::from_str::<OverrideFile>(
                "overrides: [{ id: 1, effects: [{ index: 0, script: FROBNICATE }] }]"
            )
            .is_err()
        );
        assert!(serde_yaml::from_str::<OverrideFile>(
            "overrides: [{ id: 1, effects: [{ index: 0, script: NO_OP, params: { bogus: 1 } }] }]"
        )
        .is_err());
        let mut o = overrides();
        assert!(matches!(
            o.add(SpellOverride::new(12834)),
            Err(OverrideError::Duplicate(12834))
        ));
        o.add(SpellOverride::new(1)).unwrap();
        assert_eq!(o.len(), 13);
    }

    #[test]
    fn serialization_round_trips_and_omits_defaults() {
        let file: OverrideFile = serde_yaml::from_str(WARRIOR).unwrap();
        let yaml = serde_yaml::to_string(&file).unwrap();
        assert!(
            !yaml.contains("defaults"),
            "defaults equal to the constants are omitted"
        );
        assert!(yaml.contains("script: DEEP_WOUNDS_BLEED"));
        assert!(yaml.contains("source: MELEE_DODGE"));
        assert!(yaml.contains("- RESETS_SWING_TIMERS"));
        assert!(!yaml.contains("modifier"), "threat modifier 1 is omitted");
        assert!(!yaml.contains("params: {}"));
        let back: OverrideFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, file);
    }

    #[test]
    fn load_reads_a_directory_and_tolerates_its_absence() {
        let dir = std::env::temp_dir().join(format!(
            "csim-overrides-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(Overrides::load(&dir).unwrap().is_empty());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("warrior.yaml"), WARRIOR).unwrap();
        fs::write(
            dir.join("racials.yml"),
            "overrides: [{ id: 20572, sim_flags: [IGNORED] }]",
        )
        .unwrap();
        fs::write(dir.join("README.md"), "not yaml").unwrap();
        let o = Overrides::load(&dir).unwrap();
        assert_eq!(o.len(), 13);
        assert!(o.has_sim_flag(20572, SimFlag::Ignored));
        fs::write(dir.join("bad.yaml"), "overrides: [{ id: 12834 }]").unwrap();
        assert!(matches!(
            Overrides::load(&dir),
            Err(OverrideError::Duplicate(12834))
        ));
        fs::write(dir.join("bad.yaml"), "overrides: [{ nope: 1 }]").unwrap();
        assert!(matches!(
            Overrides::load(&dir),
            Err(OverrideError::Yaml { .. })
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_repository_overrides_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/spells/overrides");
        let o = Overrides::load(&dir).unwrap();
        assert!(o.len() > 40, "{}", o.len());
        assert_eq!(o.proc_hit_mask(12834), ProcHitMask::CRITICAL);
        assert_eq!(
            o.effect_script(12162, 0).unwrap().script,
            ScriptKind::DeepWoundsBleed
        );
        assert_eq!(o.threat(25286).flat, 145.0);
        assert_eq!(o.threat(25288).flat, 355.0);
        assert_eq!(o.stance_passive(2458), Some(7381));
        assert!(o.has_sim_flag(11605, SimFlag::ResetsSwingTimers));
        assert!(!o.has_sim_flag(1310200, SimFlag::ResetsSwingTimers));
        assert_eq!(o.event_scripts(11585)[0].script, ScriptKind::AddComboPoints);
        let ruthlessness = o.get(14156).and_then(|r| r.proc).unwrap();
        assert!(ruthlessness.finisher);
        let expose = o.get(14168).and_then(|r| r.proc).unwrap();
        assert_eq!(expose.family_mask, Some([524288, 0, 0, 0]));
        assert_eq!(expose.combo_points_effect, Some(2));
    }
}
