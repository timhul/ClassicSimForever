//! The effect interpreter. Port of `Spells/Effect.*`, keyed on the client tables' `Effect` /
//! `EffectAura` vocabulary ([`SpellEffectName`], [`AuraType`]).
//!
//! An [`Effect`] is one [`EffectRecord`] (a `SpellEffect` row) plus its runtime state: the
//! current value (talent rank values replace the base points), the script the overrides
//! attached to a `DUMMY`, the last roll and the damage it produced. Direct effects are
//! [`Effect::perform`]ed when the spell is cast; aura effects are [`Effect::apply_aura`]ed when
//! the spell's buff becomes active and reverted when it ends. Everything the effect needs from
//! the world goes through [`EffectHost`], implemented by the spell context (Phase 4) and by the
//! test worlds.
//!
//! Compared to the C++ `Effect`, results are reported back instead of pushed into the spell
//! ([`EffectOutcome`]), and there is no spell-specific code: what a `DUMMY` does is decided by
//! the [`ScriptKind`] the data names.

use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult, SpellResistKind, SpellRoll};
use crate::item::{ItemStat, WeaponType};
use crate::magic_school::MagicSchool;
use crate::resource::ResourceType;
use crate::spell::SpellResult;
use crate::spell::dbc::{
    AuraType, DefenseType, PowerType, SpellEffectName, SpellModOp, SpellSchoolMask,
};
use crate::spell::modifiers::{SpellModifier, SpellModifiers};
use crate::spell::overrides::{EffectScript, ScriptKind};
use crate::spell::record::{ClassOptions, EffectRecord, EquippedItems, Levels, SpellRecord};
use crate::stance::Stance;
use crate::stats::CharacterStats;
use crate::stats::character_stats::ATTRIBUTE_COUNT;
use crate::target::{CreatureType, CreatureTypes, Target};

/// What an effect needs from the world. Port of the `Character` / `CombatRoll` / `Spell` calls
/// made by `Effect.cpp`, plus the hooks the table auras need.
pub trait EffectHost {
    fn caster_level(&self) -> u32;
    fn combo_points(&self) -> u32;
    fn gain_combo_points(&mut self, amount: u32);
    fn spend_combo_points(&mut self);

    fn resource_level(&self, resource: ResourceType) -> u32;
    /// Gains `amount` of `resource`, returning how much was actually gained (caps).
    fn gain_resource(&mut self, resource: ResourceType, amount: u32) -> u32;
    /// Changes the regeneration rate of `resource` by `percent` (`MOD_POWER_REGEN_PERCENT`:
    /// Adrenaline Rush).
    fn adjust_power_regen_percent(&mut self, _resource: ResourceType, _percent: i32) {}
    /// Changes the maximum of `resource` by `amount` (`MOD_INCREASE_ENERGY`: Vigor).
    fn adjust_max_power(&mut self, _resource: ResourceType, _amount: i32) {}
    /// Changes by `percent` the share of spirit regen that continues inside the five-second
    /// rule (`MOD_MANA_REGEN_INTERRUPT`: Reverence).
    fn adjust_mana_regen_while_casting(&mut self, _percent: i32) {}

    fn melee_ap(&self) -> u32;
    /// The caster's spell damage of `school`, the target's flat damage taken of it included
    /// (Judgement of the Crusader). A host without spells has none.
    fn spell_damage(&self, _school: MagicSchool) -> u32 {
        0
    }
    /// The caster's maximum health (`HEALTH_LEECH`: Touch of the Grave).
    fn max_health(&self) -> u32;
    /// A uniformly random value in `[min, max]`.
    fn random_in_range(&mut self, min: f64, max: f64) -> f64;
    /// Random mainhand damage normalized to the weapon type's standard speed.
    fn random_normalized_mh_dmg(&mut self) -> f64;
    /// Random mainhand damage including the attack power contribution of the weapon speed.
    fn random_non_normalized_mh_dmg(&mut self) -> f64;
    /// Rolls a mainhand melee ability on the special attack table, with `extra_crit` (hundredths
    /// of a percent) added to the character's crit chance for this roll, or no crit chance at
    /// all when `can_crit` is false.
    fn roll_melee_ability(
        &mut self,
        included: IncludedOutcomes,
        extra_crit: u32,
        can_crit: bool,
    ) -> PhysicalAttackResult;
    /// Rolls a spell of `school` on the magic table (the spell hit and the target's resistance),
    /// with `extra_crit` (hundredths of a percent) added to the character's spell crit chance
    /// for the school, or no crit chance at all when `can_crit` is false. A host without a
    /// magic table lands every spell.
    fn roll_spell(
        &mut self,
        _school: MagicSchool,
        _kind: SpellResistKind,
        _extra_crit: u32,
        _can_crit: bool,
    ) -> SpellRoll {
        SpellRoll::HIT
    }

    fn stats_mut(&mut self) -> &mut CharacterStats;
    fn target_mut(&mut self) -> &mut Target;
    fn target_creature_type(&self) -> CreatureType;
    /// Attack speed changes go through the character so pending swings are re-timed.
    fn increase_melee_attack_speed(&mut self, percent: u32);
    fn decrease_melee_attack_speed(&mut self, percent: u32);
    fn swap_stance(&mut self, stance: Stance);
    /// The aura of form `stance` ended: back to caster form if the character is still in it
    /// (Stealth broken), nothing if another form took over.
    fn leave_stance(&mut self, _stance: Stance) {}
    /// The active spell modifiers (`ADD_FLAT_MODIFIER` / `ADD_PCT_MODIFIER` auras).
    fn spell_modifiers(&self) -> &SpellModifiers;
    fn spell_modifiers_mut(&mut self) -> &mut SpellModifiers;
    /// Grants `count` extra main-hand attacks (`ADD_EXTRA_ATTACKS`).
    fn add_extra_attacks(&mut self, count: u32);
    /// Changes the rage kept when the character changes stance (`STANCE_RAGE_RETAINED`).
    fn adjust_stance_rage_retained(&mut self, delta: i32);
    /// Changes the off-hand damage multiplier by `percent` points (`MOD_OFFHAND_DAMAGE_PCT`).
    fn adjust_offhand_damage_percent(&mut self, percent: i32);
    /// Changes the off-hand rage generation by `percent` points (`OFFHAND_RAGE_PERCENT`).
    fn adjust_offhand_rage_percent(&mut self, percent: i32);
    /// Adds (`apply`) or removes an off-hand copy of ability `spell` (`OFFHAND_COPY`: Raging
    /// Blows makes Whirlwind also strike with the off hand).
    fn adjust_offhand_copy(&mut self, spell: u32, apply: bool);
    /// The judgement of the caster's active seal (`SEAL_JUDGEMENT`), `None` without a seal.
    fn seal_judgement(&self) -> Option<u32> {
        None
    }
    /// While the caster has the aura `enabling_aura`, starts the duration of each of its seals'
    /// judgements that is up on the target again (`REFRESH_JUDGEMENTS`: Sacred Arbiter).
    fn refresh_seal_judgements(&mut self, _enabling_aura: u32) {}
    /// Adds (`apply`) or removes `percent` % more damage of `spell`'s school while the
    /// caster's aura of `spell` is up (`SCHOOL_DAMAGE_PERCENT_WHILE_AURA`: Consecrated Ground).
    fn adjust_school_damage_while_aura(&mut self, _spell: u32, _percent: f64, _apply: bool) {}
    /// Multiplies (`apply`) or divides the main-hand weapon damage by `factor`
    /// (`ATTACK_SPEED_DAMAGE_PENALTY`: Seal of the Crusader).
    fn adjust_mainhand_damage_multiplier(&mut self, _factor: f64, _apply: bool) {}
    /// Adds (`apply`) or removes the return of `percent` % of the judged seal's mana cost by
    /// each judgement, at the chance the value of effect `chance_effect` of `spell`'s aura
    /// gives (`JUDGED_SEAL_MANA_RETURN`: Sanctified Judgement).
    fn adjust_judged_seal_mana_return(
        &mut self,
        _spell: u32,
        _chance_effect: u32,
        _percent: f64,
        _apply: bool,
    ) {
    }
    /// Adds (`apply`) or removes a gain of `amount` of `resource` when ability `spell` is used
    /// (`GAIN_RESOURCE_ON_USE`: Improved Berserker Rage).
    fn adjust_resource_on_use(
        &mut self,
        spell: u32,
        resource: ResourceType,
        amount: u32,
        apply: bool,
    );
    /// Replaces spell `replaced` by `replacement` on the action bar while `apply` is true
    /// (`OVERRIDE_ACTIONBAR_SPELLS`: Improved Slam, Vanguard).
    fn override_actionbar_spell(&mut self, replaced: u32, replacement: u32, apply: bool);
    /// Whether the main hand holds a two-hand weapon (`TWO_HAND_ENERGIZE_MULTIPLIER`).
    fn has_two_hand_weapon(&self) -> bool {
        false
    }
    /// The type of the main-hand weapon, if one is equipped (`WEAPON_TYPE_VALUE`).
    fn mainhand_weapon_type(&self) -> Option<WeaponType> {
        None
    }
    /// Whether one of the caster's poisons is on the target (`DAMAGE_PERCENT_VS_POISONED`).
    fn target_poisoned_by_caster(&self) -> bool {
        false
    }
}

/// How an effect relates to the effects before it in the chain. Port of `Dependency`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dependency {
    /// Always performed, always rolls its own hit check.
    Independent,
    /// Performed only if the chain so far is not a complete failure; reuses the first roll.
    PartialSuccess,
    /// Performed only if every previous effect succeeded; reuses the first roll.
    FullSuccess,
}

/// The state of the effect chain a dependent effect is performed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainState {
    /// The spell's aggregate result so far.
    pub result: SpellResult,
    /// The first effect's roll, reused by dependent effects.
    pub previous: Option<PhysicalAttackResult>,
    /// The first effect's roll on the magic table, reused by dependent effects.
    pub previous_spell: Option<SpellRoll>,
    /// The spell's resource cost in displayed units (Execute converts the rage above it).
    pub resource_cost: u32,
    /// Crit chance added by modifiers, hundredths of a percent.
    pub extra_crit: u32,
    /// The spell is the strike of an attack that landed already (Mutilate's weapon strikes):
    /// its roll cannot be avoided, only crit.
    pub hit_guaranteed: bool,
}

/// What the spell needs to know after an effect was performed.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectOutcome {
    /// Whether the effect did its thing (a failed hit check makes it `false`).
    pub success: bool,
    /// The attack table result if this effect made a fresh roll (`None` when it was skipped,
    /// reused the previous result or needs no roll).
    pub rolled: Option<PhysicalAttackResult>,
    /// The magic table result if this effect made a fresh roll on it.
    pub spell_roll: Option<SpellRoll>,
    /// Resource gained by the effect, for the resource statistics.
    pub resource_gained: Option<(ResourceType, u32)>,
    /// A spell to cast now (`TRIGGER_SPELL`).
    pub trigger: Option<u32>,
    /// Flat threat added by a `THREAT` effect.
    pub threat: f64,
    /// The spell consumes every remaining point of its resource (Execute).
    pub consumes_all_resource: bool,
    /// Percent added to the damage of the spells this one triggers (Mutilate's strikes
    /// against a poisoned target).
    pub triggered_damage_percent: f64,
    /// The multiplier on the damage of `trigger` (`TRIGGER_SPELL_DAMAGE_PERCENT`: Judgement of
    /// Command's half damage).
    pub trigger_damage_scale: Option<f64>,
    /// The effect cast the judgement of the caster's seal (`JUDGE_SEAL`).
    pub judged_seal: bool,
}

impl EffectOutcome {
    const SKIPPED: EffectOutcome = EffectOutcome {
        success: false,
        rolled: None,
        spell_roll: None,
        resource_gained: None,
        trigger: None,
        threat: 0.0,
        consumes_all_resource: false,
        triggered_damage_percent: 0.0,
        trigger_damage_scale: None,
        judged_seal: false,
    };

    fn plain(success: bool) -> Self {
        EffectOutcome {
            success,
            ..EffectOutcome::SKIPPED
        }
    }

    fn rolled(success: bool, rolled: Option<PhysicalAttackResult>) -> Self {
        EffectOutcome {
            success,
            rolled,
            ..EffectOutcome::SKIPPED
        }
    }

    /// A roll on the magic table: `fresh` when the effect made it, not when it reused the
    /// chain's.
    fn spell(roll: SpellRoll, fresh: bool) -> Self {
        EffectOutcome {
            success: roll.landed(),
            spell_roll: fresh.then_some(roll),
            ..EffectOutcome::SKIPPED
        }
    }
}

/// Royalgiraffe's classification of a spell on the magic table: non-binary when it only deals
/// damage (direct or periodic, a health leech included; dummies and empty effects aside),
/// binary when it does anything else too (a slow, a debuff), which makes it land fully or not
/// at all.
pub fn spell_resist_kind(spell: &SpellRecord) -> SpellResistKind {
    let only_damage = spell.effects.iter().all(|effect| match effect.effect {
        SpellEffectName::SchoolDamage
        | SpellEffectName::HealthLeech
        | SpellEffectName::Dummy
        | SpellEffectName::None => true,
        _ if effect.is_apply_aura() => matches!(
            effect.aura,
            AuraType::PeriodicDamage | AuraType::PeriodicLeech | AuraType::Dummy | AuraType::None
        ),
        _ => false,
    });
    if only_damage {
        SpellResistKind::NonBinary
    } else {
        SpellResistKind::Binary
    }
}

/// One runtime effect of a spell or of its buff. Port of `Effect`.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    record: EffectRecord,
    script: Option<EffectScript>,
    /// The spell the effect belongs to (`SpellName.ID`), the source of modifier auras.
    spell: u32,
    class_options: Option<ClassOptions>,
    /// The spell's `SpellEquippedItems` requirement: the weapon types a weapon-type aura
    /// applies to.
    equipped_items: Option<EquippedItems>,
    levels: Levels,
    defense: DefenseType,
    /// The spell's school, for the magic table and its resistance.
    school: MagicSchool,
    /// How the target's resistance applies to the spell on the magic table.
    resist_kind: SpellResistKind,
    /// The current base points: the table value, or the talent rank value.
    value: f64,
    /// The table value.
    base_value: f64,
    /// Points the cast that applied the effect's aura added to the value: combo points spent
    /// times `EffectPointsPerResource`, the attack power share of a finisher's or a bleed's
    /// ticks. Taken when the aura is applied, so a later change cannot unbalance its removal.
    cast_bonus: f64,
    /// The value that replaces the base points while the main-hand weapon's subclass is in
    /// the mask (`WEAPON_TYPE_VALUE`: Ghostly Strike's 180 % with a dagger).
    weapon_type_value: Option<(u32, f64)>,
    /// The target health fraction below which a `DAMAGE_PERCENT_BELOW_HEALTH` aura applies:
    /// the table value of the effect its script names (Quietus' 35 %).
    health_threshold: Option<f64>,
    dependency: Dependency,
    included: IncludedOutcomes,
    can_crit: bool,
    /// A `WEAPON_PERCENT_DAMAGE` effect of a spell that also has a weapon damage effect: it
    /// scales that effect's damage (Raging Blow, Spearing Strike) instead of dealing its own.
    scales_weapon_damage: bool,
    /// Result of the last hit check (own roll or inherited).
    pub last_result: Option<PhysicalAttackResult>,
    /// Result of the last roll on the magic table (own roll or inherited).
    pub last_spell: Option<SpellRoll>,
    /// Damage produced by the last perform; the spell collects and zeroes it.
    pub damage_dealt: f64,
    /// The part of the last perform's damage that the spell's weapon damage multiplier does
    /// not scale: the flat damage and spell power of a magic school weapon strike (Holy
    /// Strike's 93 + 0.429 × spell power beside its 50 % weapon damage). Collected with
    /// `damage_dealt`.
    pub flat_damage: f64,
    reroll_result: bool,
    /// The roll of the perform in progress cannot be avoided ([`ChainState::hit_guaranteed`]).
    hit_guaranteed: bool,
    effect_success: bool,
}

impl Effect {
    /// Builds the effect `record` of `spell`, with `script` if the overrides attached one.
    /// `cannot_crit` comes from the `CANNOT_CRIT` sim flag.
    pub fn new(
        record: &EffectRecord,
        spell: &SpellRecord,
        script: Option<EffectScript>,
        cannot_crit: bool,
    ) -> Self {
        // The spell sets the chain position: the first direct effect rolls, the rest follow it.
        let dependency = if record.index == 0 {
            Dependency::Independent
        } else {
            Dependency::PartialSuccess
        };
        // A ranged attack can miss and crit, never be dodged, parried or blocked
        // (`MeleeSpellHitResult` of the server for `RANGED_ATTACK`: Hammer of Wrath).
        let no_active_defense =
            spell.ignores_active_defense() || spell.categories.defense_type == DefenseType::Ranged;
        let scales_weapon_damage = record.effect == SpellEffectName::WeaponPercentDamage
            && spell.effects.iter().any(|e| {
                matches!(
                    e.effect,
                    SpellEffectName::NormalizedWeaponDmg
                        | SpellEffectName::WeaponDamage
                        | SpellEffectName::WeaponDamageNoschool
                )
            });
        let health_threshold = script
            .filter(|s| s.script == ScriptKind::DamagePercentBelowHealth)
            .and_then(|s| spell.effect(s.params.effect?))
            .map(|threshold| f64::from(threshold.base_points) / 100.0);
        Effect {
            value: record.base_points as f64,
            base_value: record.base_points as f64,
            cast_bonus: 0.0,
            weapon_type_value: None,
            health_threshold,
            record: record.clone(),
            script,
            spell: spell.id,
            class_options: spell.class_options,
            equipped_items: spell.equipped_items,
            levels: spell.levels,
            defense: spell.categories.defense_type,
            school: MagicSchool::from_school_mask(spell.school_mask),
            resist_kind: spell_resist_kind(spell),
            dependency,
            included: IncludedOutcomes {
                dodge: !no_active_defense,
                parry: !no_active_defense,
                block: !no_active_defense,
                miss: true,
            },
            can_crit: !cannot_crit,
            scales_weapon_damage,
            last_result: None,
            last_spell: None,
            damage_dealt: 0.0,
            flat_damage: 0.0,
            reroll_result: true,
            hit_guaranteed: false,
            effect_success: false,
        }
    }

    pub fn record(&self) -> &EffectRecord {
        &self.record
    }

    /// The weapon types the spell's weapon requirement accepts; every weapon type without one.
    pub fn weapon_types(&self) -> Vec<WeaponType> {
        WeaponType::ALL
            .into_iter()
            .filter(|weapon_type| {
                self.equipped_items
                    .filter(|items| items.class > 0)
                    .is_none_or(|items| {
                        let (class, subclass) = weapon_type.item_class_subclass();
                        items.accepts(class, subclass)
                    })
            })
            .collect()
    }

    pub fn index(&self) -> u32 {
        self.record.index
    }

    pub fn kind(&self) -> SpellEffectName {
        self.record.effect
    }

    pub fn aura(&self) -> AuraType {
        self.record.aura
    }

    pub fn script(&self) -> Option<&EffectScript> {
        self.script.as_ref()
    }

    pub fn script_kind(&self) -> Option<ScriptKind> {
        self.script.map(|s| s.script)
    }

    /// The spell the effect belongs to.
    pub fn spell(&self) -> u32 {
        self.spell
    }

    pub fn is_aura(&self) -> bool {
        self.record.is_apply_aura()
    }

    /// The current base points (before per-level scaling and modifiers).
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The table base points.
    pub fn base_value(&self) -> f64 {
        self.base_value
    }

    /// Replaces the base points (talent rank values, `TRIGGER_WITH_VALUE`).
    pub fn set_value(&mut self, value: f64) {
        self.value = value;
    }

    /// Restores the table base points.
    pub fn reset_value(&mut self) {
        self.value = self.base_value;
    }

    /// The points the cast that applied the aura added (see `cast_bonus`).
    pub fn cast_bonus(&self) -> f64 {
        self.cast_bonus
    }

    /// Sets the points the cast applying the aura adds to its value.
    pub fn set_cast_bonus(&mut self, bonus: f64) {
        self.cast_bonus = bonus;
    }

    /// Makes `value` the base points while the main-hand weapon's subclass is in
    /// `subclass_mask` (`WEAPON_TYPE_VALUE`).
    pub fn set_weapon_type_value(&mut self, subclass_mask: u32, value: f64) {
        self.weapon_type_value = Some((subclass_mask, value));
    }

    /// Whether the effect is a periodic aura (its value is a tick: `PERIODIC_DAMAGE`).
    pub fn is_periodic_aura(&self) -> bool {
        self.record.is_apply_aura() && self.record.is_periodic()
    }

    /// The value for a caster of `level`: base points plus per-level scaling
    /// (`EffectRealPointsPerLevel × (min(level, max) − spell level)`).
    pub fn value_at_level(&self, level: u32) -> f64 {
        let per_level = self.record.real_points_per_level as f64;
        if per_level == 0.0 {
            return self.value;
        }
        let level = if self.levels.max > 0 {
            level.min(self.levels.max)
        } else {
            level
        };
        let delta = (f64::from(level) - f64::from(self.levels.spell)).max(0.0);
        self.value + per_level * delta
    }

    /// The value the caster of `host` gets: per-level scaling plus the character's
    /// `POINTS` / `POINTS_INDEX_n` modifiers.
    /// The factor the effect applies to the spell's weapon damage effects. A
    /// `WEAPON_PERCENT_DAMAGE` effect scales them like the server's `weaponDamagePercentMod`:
    /// "deals 40% weapon damage" is the normalized weapon damage effect times 0.4 (a percent
    /// effect alone on its spell deals the weapon damage share itself). An
    /// `EXTRA_WEAPON_DAMAGE_VS_CREATURE_TYPES` script adds its value times the damage against
    /// its creature types. `None` for every other effect.
    pub fn weapon_damage_multiplier(&self, host: &impl EffectHost) -> Option<f64> {
        if self.scales_weapon_damage {
            return Some(self.effective_value(host) / 100.0);
        }
        let script = self
            .script
            .filter(|s| s.script == ScriptKind::ExtraWeaponDamageVsCreatureTypes)?;
        let applies = script
            .params
            .creature_types
            .is_some_and(|types| types.contains(host.target_creature_type()));
        Some(if applies {
            1.0 + self.effective_value(host)
        } else {
            1.0
        })
    }

    pub fn effective_value(&self, host: &impl EffectHost) -> f64 {
        let weapon_type_value = self.weapon_type_value.filter(|(mask, _)| {
            host.mainhand_weapon_type().is_some_and(|weapon| {
                let (_, subclass) = weapon.item_class_subclass();
                mask & (1 << subclass) != 0
            })
        });
        let base = match weapon_type_value {
            Some((_, value)) => value,
            None => self.value_at_level(host.caster_level()),
        } + self.cast_bonus;
        host.spell_modifiers()
            .effect_value(self.class_options.as_ref(), self.record.index, base)
    }

    /// The value in displayed resource units (rage in tenths in the tables).
    pub fn resource_amount(&self, host: &impl EffectHost, resource: ResourceType) -> u32 {
        resource.from_stored_amount(self.effective_value(host))
    }

    pub fn dependency(&self) -> Dependency {
        self.dependency
    }

    /// Sets how the effect relates to the effects before it in its spell's chain.
    pub fn set_dependency(&mut self, dependency: Dependency) {
        self.dependency = dependency;
    }

    pub fn included_outcomes(&self) -> IncludedOutcomes {
        self.included
    }

    pub fn can_crit(&self) -> bool {
        self.can_crit
    }

    pub fn was_successful(&self) -> bool {
        self.effect_success
    }

    /// Performs the effect as part of a chain. `chain.result` is the spell's result so far and
    /// `chain.previous` the first effect's roll, which dependent effects reuse.
    /// Port of `Effect::perform_effect(int)`.
    pub fn perform(&mut self, host: &mut impl EffectHost, chain: &ChainState) -> EffectOutcome {
        match self.dependency {
            Dependency::Independent => self.reroll_result = true,
            Dependency::FullSuccess => {
                if chain.result != SpellResult::Success {
                    return EffectOutcome::SKIPPED;
                }
                self.reroll_result = false;
                self.last_result = chain.previous;
                self.last_spell = chain.previous_spell;
            }
            Dependency::PartialSuccess => {
                if chain.result == SpellResult::Failure {
                    return EffectOutcome::SKIPPED;
                }
                self.reroll_result = false;
                self.last_result = chain.previous;
                self.last_spell = chain.previous_spell;
            }
        }
        self.hit_guaranteed = chain.hit_guaranteed;
        let outcome = self.perform_internal(host, chain.resource_cost, chain.extra_crit);
        self.hit_guaranteed = false;
        self.effect_success = outcome.success;
        outcome
    }

    /// Performs the effect on its own, always rolling. Port of `Effect::perform_effect()`.
    pub fn perform_independent(
        &mut self,
        host: &mut impl EffectHost,
        resource_cost: u32,
        extra_crit: u32,
    ) -> EffectOutcome {
        self.reroll_result = true;
        let outcome = self.perform_internal(host, resource_cost, extra_crit);
        self.effect_success = outcome.success;
        outcome
    }

    fn perform_internal(
        &mut self,
        host: &mut impl EffectHost,
        resource_cost: u32,
        extra_crit: u32,
    ) -> EffectOutcome {
        use SpellEffectName as E;
        match self.record.effect {
            // A direct effect that casts a spell the tables do not name (Vanish's `SANCTUARY`:
            // the server puts the Rogue back in Stealth).
            _ if !self.record.is_apply_aura()
                && self.script_kind() == Some(ScriptKind::TriggerSpell) =>
            {
                EffectOutcome {
                    trigger: self.script.and_then(|s| s.params.spell),
                    ..EffectOutcome::plain(true)
                }
            }
            // Damage to the caster itself (Demonic Rune's 800 shadow damage) is not simulated.
            E::SchoolDamage if self.record.targets_caster() && !self.record.targets_enemy() => {
                EffectOutcome::plain(true)
            }
            E::SchoolDamage => {
                let outcome = self.roll_attack(host, extra_crit, self.can_crit);
                if outcome.success {
                    self.damage_dealt = self.direct_damage(host);
                }
                outcome
            }
            // Damage of `value` % of the caster's maximum health (Touch of the Grave); the
            // healing is not modelled.
            E::HealthLeech => {
                let outcome = self.roll_attack(host, extra_crit, self.can_crit);
                if outcome.success {
                    self.damage_dealt =
                        f64::from(host.max_health()) * self.effective_value(host) / 100.0;
                }
                outcome
            }
            E::WeaponDamageNoschool | E::WeaponDamage => {
                let (hit, rolled) = self.roll_melee(host, extra_crit);
                if hit {
                    let weapon = host.random_non_normalized_mh_dmg();
                    self.deal_weapon_damage(host, weapon);
                }
                EffectOutcome::rolled(hit, rolled)
            }
            E::NormalizedWeaponDmg => {
                let (hit, rolled) = self.roll_melee(host, extra_crit);
                if hit {
                    let weapon = host.random_normalized_mh_dmg();
                    self.deal_weapon_damage(host, weapon);
                }
                EffectOutcome::rolled(hit, rolled)
            }
            E::WeaponPercentDamage => {
                let (hit, rolled) = self.roll_melee(host, extra_crit);
                // With a weapon damage effect beside it, the spell applies the percentage to that
                // effect's damage (`weapon_damage_multiplier`).
                if hit && !self.scales_weapon_damage {
                    self.damage_dealt =
                        host.random_non_normalized_mh_dmg() * self.effective_value(host) / 100.0;
                    self.flat_damage = self.spell_power_damage(host);
                }
                EffectOutcome::rolled(hit, rolled)
            }
            E::Energize => {
                // Combo points are not a resource pool: Bloodthrill's payload opens the
                // Overpower window with one.
                if self.record.power_type() == PowerType::ComboPoints {
                    let amount = self.effective_value(host).round().max(0.0) as u32;
                    host.gain_combo_points(amount);
                    return EffectOutcome::plain(true);
                }
                let Some(resource) = ResourceType::from_power_type(self.record.power_type()) else {
                    return EffectOutcome::plain(true);
                };
                let mut amount = self.resource_amount(host, resource);
                if let Some(script) = self.script
                    && script.script == ScriptKind::TwoHandEnergizeMultiplier
                    && host.has_two_hand_weapon()
                {
                    // Validated as present and positive when the overrides were loaded.
                    let multiplier = script.params.value.unwrap_or(1.0);
                    amount = (f64::from(amount) * multiplier).round() as u32;
                }
                let gained = host.gain_resource(resource, amount);
                EffectOutcome {
                    resource_gained: (gained > 0).then_some((resource, gained)),
                    ..EffectOutcome::plain(true)
                }
            }
            E::Threat => EffectOutcome {
                threat: self.effective_value(host),
                ..EffectOutcome::plain(true)
            },
            // A hostile trigger of a melee spell strikes the target: the spell must land first
            // (Mutilate's weapon strikes), like the server's hit check of the whole spell.
            E::TriggerSpell if self.is_strike_trigger() => {
                let (hit, rolled) = self.roll_melee_with(host, extra_crit, false);
                EffectOutcome {
                    trigger: (hit && self.record.trigger_spell != 0)
                        .then_some(self.record.trigger_spell),
                    ..EffectOutcome::rolled(hit, rolled)
                }
            }
            E::TriggerSpell => EffectOutcome {
                trigger: (self.record.trigger_spell != 0).then_some(self.record.trigger_spell),
                ..EffectOutcome::plain(true)
            },
            E::AddExtraAttacks => {
                host.add_extra_attacks(self.effective_value(host).round().max(0.0) as u32);
                EffectOutcome::plain(true)
            }
            E::Dummy | E::ScriptEffect => self.perform_script(host, resource_cost, extra_crit),
            // A melee debuff (Rend, Sunder Armor) must land on the attack table before it is
            // applied; an aura cannot crit.
            _ if self.is_melee_debuff() => {
                let (hit, rolled) = self.roll_melee_with(host, extra_crit, false);
                EffectOutcome::rolled(hit, rolled)
            }
            // A damage-over-time on the magic table (Deadly Poison) must land its hit roll;
            // its ticks roll their own partial resists.
            _ if self.is_spell_damage_debuff() => self.roll_attack(host, extra_crit, false),
            // Aura effects act through the buff they belong to.
            _ if self.record.is_apply_aura() => EffectOutcome::plain(true),
            // Everything else (dispels, interrupts, taunts, heals, summons, ...) has no
            // effect on a single-target damage simulation.
            _ => EffectOutcome::plain(true),
        }
    }

    /// The scripted direct effects.
    fn perform_script(
        &mut self,
        host: &mut impl EffectHost,
        resource_cost: u32,
        extra_crit: u32,
    ) -> EffectOutcome {
        match self.script_kind() {
            Some(ScriptKind::AttackPowerPercentDamage) => {
                let (hit, rolled) = self.roll_melee(host, extra_crit);
                if hit {
                    self.damage_dealt =
                        f64::from(host.melee_ap()) * self.effective_value(host) / 100.0;
                }
                EffectOutcome::rolled(hit, rolled)
            }
            Some(ScriptKind::Execute) => {
                let (hit, rolled) = self.roll_melee(host, extra_crit);
                if hit {
                    let rage = host.resource_level(ResourceType::Rage);
                    let extra = f64::from(rage.saturating_sub(resource_cost));
                    self.damage_dealt = self.effective_value(host)
                        + extra * f64::from(self.record.chain_amplitude) * 10.0;
                }
                EffectOutcome {
                    consumes_all_resource: true,
                    ..EffectOutcome::rolled(hit, rolled)
                }
            }
            // Applied by the spell to its weapon damage (`weapon_damage_multiplier`).
            Some(ScriptKind::ExtraWeaponDamageVsCreatureTypes) => EffectOutcome::plain(true),
            // A finisher's attack power share dealt with its direct damage (Eviscerate); a
            // share of a periodic aura (`params.effect`) is added to its ticks by the spell.
            Some(ScriptKind::ComboPointApDamage) if self.script_target().is_none() => {
                let (hit, rolled) = self.roll_melee(host, extra_crit);
                if hit {
                    let percent = self.script.map_or(0.0, |s| {
                        s.params.combo_point_ap_percent(host.combo_points())
                    });
                    self.damage_dealt = f64::from(host.melee_ap()) * percent / 100.0;
                }
                EffectOutcome::rolled(hit, rolled)
            }
            // The judgement of the active seal; nothing without one.
            Some(ScriptKind::JudgeSeal) => match host.seal_judgement() {
                Some(judgement) => EffectOutcome {
                    trigger: Some(judgement),
                    judged_seal: true,
                    ..EffectOutcome::plain(true)
                },
                None => EffectOutcome::plain(true),
            },
            Some(ScriptKind::TriggerSpellDamagePercent) => {
                let params = self.script.map(|s| s.params).unwrap_or_default();
                EffectOutcome {
                    trigger: params.spell,
                    trigger_damage_scale: params.value.map(|percent| percent / 100.0),
                    ..EffectOutcome::plain(true)
                }
            }
            Some(ScriptKind::RefreshJudgements) => {
                if let Some(aura) = self.script.and_then(|s| s.params.spell) {
                    host.refresh_seal_judgements(aura);
                }
                EffectOutcome::plain(true)
            }
            Some(ScriptKind::DamagePercentVsPoisoned) => EffectOutcome {
                triggered_damage_percent: if host.target_poisoned_by_caster() {
                    self.effective_value(host)
                } else {
                    0.0
                },
                ..EffectOutcome::plain(true)
            },
            // Aura-side scripts (bleeds, periodic gains, triggers with a value) are run by the
            // buff, periodic and proc systems; `NO_OP` and unscripted dummies do nothing.
            _ => EffectOutcome::plain(true),
        }
    }

    /// The effect a script acts on (`params.effect`), if it names one.
    pub fn script_target(&self) -> Option<u32> {
        self.script.and_then(|s| s.params.effect)
    }

    /// Whether the effect rolls an attack table when it is performed on its own: weapon
    /// damage, damage and debuffs of a melee spell, a melee spell's hostile trigger, damage and
    /// damage-over-time of a spell on the magic table.
    pub fn rolls_attack(&self) -> bool {
        use SpellEffectName as E;
        let melee = self.rolls_melee_table();
        self.weapon_damage_kind()
            || match self.record.effect {
                E::SchoolDamage | E::HealthLeech => melee || self.rolls_spell_table(),
                _ => {
                    self.is_melee_debuff()
                        || self.is_strike_trigger()
                        || self.is_spell_damage_debuff()
                }
            }
    }

    /// Whether the spell rolls the special attack table: a melee spell, or a ranged one
    /// (without dodge, parry and block: see [`Effect::new`]), which crits for double as melee
    /// attacks do. A paladin has no ranged weapon: the melee hit and crit chances apply.
    pub fn rolls_melee_table(&self) -> bool {
        matches!(self.defense, DefenseType::Melee | DefenseType::Ranged)
    }

    /// Whether the spell's damage rolls on the magic table (spell hit, resistance, spell crit):
    /// a spell whose `DefenseType` is magic, or none with a magic school (a physical one, such
    /// as Sweeping Strikes' copy of a hit, always lands).
    pub fn rolls_spell_table(&self) -> bool {
        match self.defense {
            DefenseType::Magic => true,
            DefenseType::None => self.school != MagicSchool::Physical,
            _ => false,
        }
    }

    /// A damage-over-time aura on the target of a spell on the magic table (Deadly Poison,
    /// Blaze): it must land the spell's hit roll before the buff carrying it is applied.
    pub fn is_spell_damage_debuff(&self) -> bool {
        self.rolls_spell_table()
            && self.record.is_apply_aura()
            && self.record.targets_enemy()
            && matches!(
                self.record.aura,
                AuraType::PeriodicDamage | AuraType::PeriodicLeech
            )
    }

    /// The spell's school.
    pub fn school(&self) -> MagicSchool {
        self.school
    }

    /// Whether the effect deals weapon damage (or scales it).
    pub fn weapon_damage_kind(&self) -> bool {
        use SpellEffectName as E;
        matches!(
            self.record.effect,
            E::NormalizedWeaponDmg
                | E::WeaponDamage
                | E::WeaponDamageNoschool
                | E::WeaponPercentDamage
        )
    }

    /// A hostile `TRIGGER_SPELL` of a melee spell: the triggered spell is a strike of the
    /// attack, cast only when the attack lands (Mutilate).
    pub fn is_strike_trigger(&self) -> bool {
        self.record.effect == SpellEffectName::TriggerSpell
            && self.defense == DefenseType::Melee
            && self.record.targets_enemy()
    }

    /// `SCHOOL_DAMAGE`: the level-scaled value with its variance, combo points, attack
    /// power coefficient and spell power coefficient.
    fn direct_damage(&mut self, host: &mut impl EffectHost) -> f64 {
        let mut damage = self.rolled_value(host);
        damage += f64::from(self.record.points_per_resource) * f64::from(host.combo_points());
        damage += self.ap_coefficient() * f64::from(host.melee_ap());
        damage += self.spell_power_damage(host);
        damage
    }

    /// The level-scaled value, rolled within its variance.
    fn rolled_value(&self, host: &mut impl EffectHost) -> f64 {
        let base = self.effective_value(host);
        if self.record.variance > 0.0 {
            let (min, max) = self.record.variance_range(base as f32);
            host.random_in_range(f64::from(min), f64::from(max))
        } else {
            base
        }
    }

    /// The damage of a weapon damage effect: the weapon's plus the effect's flat value. Of a
    /// physical spell, the spell's weapon damage multiplier scales both (Raging Blow). Of a
    /// magic school spell (a holy weapon strike), it scales the weapon damage only: the flat
    /// value, rolled within its variance, and the spell power are added whole
    /// ([`Effect::flat_damage`]), as Holy Strike's tooltip reads.
    fn deal_weapon_damage(&mut self, host: &mut impl EffectHost, weapon: f64) {
        if self.school == MagicSchool::Physical {
            self.damage_dealt = weapon + self.effective_value(host);
        } else {
            self.damage_dealt = weapon;
            self.flat_damage = self.rolled_value(host) + self.spell_power_damage(host);
        }
    }

    /// The spell power share of the effect's damage: its `EffectBonusCoefficient` times the
    /// caster's spell damage of the spell's school. A physical spell has none.
    pub fn spell_power_damage(&self, host: &impl EffectHost) -> f64 {
        if self.school == MagicSchool::Physical {
            return 0.0;
        }
        f64::from(self.record.bonus_coefficient) * f64::from(host.spell_damage(self.school))
    }

    /// The attack power coefficient of the effect's damage: the table's
    /// `BonusCoefficientFromAP`, or an `AP_COEFFICIENT` script's where the table has none.
    pub fn ap_coefficient(&self) -> f64 {
        match self.script {
            Some(script) if script.script == ScriptKind::ApCoefficient => {
                script.params.value.unwrap_or(0.0)
            }
            _ => f64::from(self.record.bonus_coefficient_from_ap),
        }
    }

    /// Rolls the table of the effect's spell: the special attack table for a melee or ranged
    /// spell ([`Effect::rolls_melee_table`]), the magic table for a spell on it
    /// ([`Effect::rolls_spell_table`]); any other spell (a physical spell without a defense
    /// type) always lands.
    fn roll_attack(
        &mut self,
        host: &mut impl EffectHost,
        extra_crit: u32,
        can_crit: bool,
    ) -> EffectOutcome {
        if self.rolls_melee_table() {
            let (hit, rolled) = self.roll_melee_with(host, extra_crit, can_crit);
            EffectOutcome::rolled(hit, rolled)
        } else if self.rolls_spell_table() {
            let (roll, fresh) = self.roll_spell(host, extra_crit, can_crit);
            EffectOutcome::spell(roll, fresh)
        } else {
            EffectOutcome::plain(true)
        }
    }

    /// Rolls (or reuses) the spell's result on the magic table; whether the roll is fresh.
    fn roll_spell(
        &mut self,
        host: &mut impl EffectHost,
        extra_crit: u32,
        can_crit: bool,
    ) -> (SpellRoll, bool) {
        if !self.reroll_result {
            let reused = match (self.last_spell, self.last_result) {
                (Some(roll), _) => roll,
                (None, Some(result)) if result.is_success() => SpellRoll::HIT,
                (None, _) => SpellRoll::MISS,
            };
            return (reused, false);
        }
        let roll = host.roll_spell(self.school, self.resist_kind, extra_crit, can_crit);
        self.last_spell = Some(roll);
        (roll, true)
    }

    /// A debuff of a melee spell (Rend, Sunder Armor): a hostile spell on the melee defense
    /// table (`DmgClass` melee) rolls it whatever its effects, like the server's
    /// `MeleeSpellHitResult`. The tables have no flag of their own for this.
    pub fn is_melee_debuff(&self) -> bool {
        self.defense == DefenseType::Melee
            && self.record.is_apply_aura()
            && self.record.targets_enemy()
    }

    /// Rolls (or reuses) the mainhand ability result. Port of `Effect::roll_mh_melee_ability`.
    fn roll_melee(
        &mut self,
        host: &mut impl EffectHost,
        extra_crit: u32,
    ) -> (bool, Option<PhysicalAttackResult>) {
        self.roll_melee_with(host, extra_crit, self.can_crit)
    }

    /// [`Effect::roll_melee`] with an explicit `can_crit`.
    fn roll_melee_with(
        &mut self,
        host: &mut impl EffectHost,
        extra_crit: u32,
        can_crit: bool,
    ) -> (bool, Option<PhysicalAttackResult>) {
        if !self.reroll_result {
            let hit = self
                .last_result
                .is_some_and(PhysicalAttackResult::is_success);
            return (hit, None);
        }
        let included = if self.hit_guaranteed {
            IncludedOutcomes::NONE
        } else {
            self.included
        };
        let result = host.roll_melee_ability(included, extra_crit, can_crit);
        self.last_result = Some(result);
        (result.is_success(), Some(result))
    }

    /// Applies the effect as an aura (when its buff becomes active). `on_target` says whether
    /// the buff is a debuff on the enemy. Port of `apply_aura_effect`.
    pub fn apply_aura(&self, host: &mut impl EffectHost, on_target: bool) {
        self.apply_or_remove_aura(host, on_target, true);
    }

    /// Reverts [`Effect::apply_aura`]. Port of `remove_aura_effect`.
    pub fn remove_aura(&self, host: &mut impl EffectHost, on_target: bool) {
        self.apply_or_remove_aura(host, on_target, false);
    }

    fn apply_or_remove_aura(&self, host: &mut impl EffectHost, on_target: bool, apply: bool) {
        use AuraType as A;
        if !self.record.is_apply_aura() {
            return;
        }
        let value = self.effective_value(host);
        let rounded = value.round() as i32;
        let signed = if apply { rounded } else { -rounded };
        let hundredths = (value * 100.0).round() as i32;
        let school = self.record.school_mask();
        let physical = school.is_physical();
        let magic = school.intersects(SpellSchoolMask::MAGIC);
        // Crit with the weapon types the spell requires only (Weaponmaster: axes, polearms).
        if self.script_kind() == Some(ScriptKind::WeaponTypeCritPercent) {
            let crit = hundredths.max(0) as u32;
            for weapon_type in self.weapon_types() {
                if apply {
                    host.stats_mut()
                        .increase_crit_for_weapon_type(weapon_type, crit);
                } else {
                    host.stats_mut()
                        .decrease_crit_for_weapon_type(weapon_type, crit);
                }
            }
            return;
        }
        match self.record.aura {
            A::ModAttackPower => adjust(
                host.stats_mut(),
                signed,
                |s, v| s.increase_melee_ap(v),
                |s, v| s.decrease_melee_ap(v),
            ),
            A::ModRangedAttackPower => adjust(
                host.stats_mut(),
                signed,
                |s, v| s.increase_ranged_ap(v),
                |s, v| s.decrease_ranged_ap(v),
            ),
            A::ModAttackPowerPct => multiplier(
                host.stats_mut(),
                apply,
                rounded,
                CharacterStats::add_ap_multiplier,
                CharacterStats::remove_ap_multiplier,
            ),
            A::ModStat => {
                let stats: Vec<ItemStat> = match self.record.misc_value[0] {
                    -1 => vec![
                        ItemStat::Strength,
                        ItemStat::Agility,
                        ItemStat::Stamina,
                        ItemStat::Intellect,
                        ItemStat::Spirit,
                    ],
                    0 => vec![ItemStat::Strength],
                    1 => vec![ItemStat::Agility],
                    2 => vec![ItemStat::Stamina],
                    3 => vec![ItemStat::Intellect],
                    4 => vec![ItemStat::Spirit],
                    _ => Vec::new(),
                };
                for stat in stats {
                    adjust(
                        host.stats_mut(),
                        signed,
                        |s, v| s.increase_stat(stat, v),
                        |s, v| s.decrease_stat(stat, v),
                    );
                }
            }
            A::ModIncreaseHealth | A::ModIncreaseHealth2 | A::ModMaxHealth if !on_target => adjust(
                host.stats_mut(),
                signed,
                |s, v| s.increase_health(v),
                |s, v| s.decrease_health(v),
            ),
            A::ModIncreaseHealthPercent if !on_target => multiplier(
                host.stats_mut(),
                apply,
                rounded,
                CharacterStats::add_health_mod,
                CharacterStats::remove_health_mod,
            ),
            A::ModTotalStatPercentage => {
                for stat in total_stat_percentage_stats(self.record.misc_value) {
                    let stats = host.stats_mut();
                    match (stat, apply) {
                        (ItemStat::Strength, true) => stats.add_strength_mod(rounded),
                        (ItemStat::Strength, false) => stats.remove_strength_mod(rounded),
                        (ItemStat::Agility, true) => stats.add_agility_mod(rounded),
                        (ItemStat::Agility, false) => stats.remove_agility_mod(rounded),
                        (ItemStat::Stamina, true) => stats.add_stamina_mod(rounded),
                        (ItemStat::Stamina, false) => stats.remove_stamina_mod(rounded),
                        (ItemStat::Intellect, true) => stats.add_intellect_mod(rounded),
                        (ItemStat::Intellect, false) => stats.remove_intellect_mod(rounded),
                        (ItemStat::Spirit, true) => stats.add_spirit_mod(rounded),
                        (ItemStat::Spirit, false) => stats.remove_spirit_mod(rounded),
                        _ => unreachable!("not an attribute: {stat:?}"),
                    }
                }
            }
            A::ModResistance | A::ModArmorNoMods if school.is_physical() => {
                if on_target && self.script_kind() == Some(ScriptKind::ExclusiveArmorReduction) {
                    host.target_mut()
                        .change_exclusive_armor_reduction(self.spell, signed);
                } else if on_target {
                    change_target_armor(host, signed);
                } else {
                    adjust(
                        host.stats_mut(),
                        signed,
                        |s, v| s.increase_armor(v),
                        |s, v| s.decrease_armor(v),
                    );
                }
            }
            A::ModBaseResistancePct if school.is_physical() => {
                multiplier(
                    host.stats_mut(),
                    apply,
                    rounded,
                    CharacterStats::add_armor_mod,
                    CharacterStats::remove_armor_mod,
                );
            }
            A::ModDamageDone if !on_target => {
                if physical {
                    adjust(
                        host.stats_mut(),
                        signed,
                        |s, v| s.increase_flat_physical_damage_bonus(v),
                        |s, v| s.decrease_flat_physical_damage_bonus(v),
                    );
                }
                // Spell damage: of every magic school at once (gear's "Increase Spell Dam"),
                // else of each school of the mask.
                if school.contains(SpellSchoolMask::MAGIC) {
                    adjust(
                        host.stats_mut(),
                        signed,
                        |s, v| s.increase_base_spell_damage(v),
                        |s, v| s.decrease_base_spell_damage(v),
                    );
                } else {
                    for magic in MagicSchool::magic_schools_of(school) {
                        adjust(
                            host.stats_mut(),
                            signed,
                            |s, v| s.increase_spell_damage_vs_school(v, magic),
                            |s, v| s.decrease_spell_damage_vs_school(v, magic),
                        );
                    }
                }
            }
            // Flat damage the target takes from the magic schools of the mask (Judgement of
            // the Crusader's holy): spell damage of every attacker, scaled by each spell's
            // coefficient like the caster's own.
            A::ModDamageTaken if on_target => {
                for magic in MagicSchool::magic_schools_of(school) {
                    let stats = host.target_mut().stats_mut();
                    if signed >= 0 {
                        stats.increase_spell_damage_vs_school(signed as u32, magic);
                    } else {
                        stats.decrease_spell_damage_vs_school(signed.unsigned_abs(), magic);
                    }
                }
            }
            // Spell damage of the schools of the mask as a percentage of an attribute
            // (Champion of the Light: intellect).
            A::ModSpellDamageOfStatPercent if !on_target => {
                let attribute = self.record.misc_value[1];
                if (0..ATTRIBUTE_COUNT as i32).contains(&attribute) {
                    for magic in MagicSchool::magic_schools_of(school) {
                        host.stats_mut().change_spell_damage_of_stat_percent(
                            magic,
                            attribute as usize,
                            signed,
                        );
                    }
                }
            }
            A::ModDamagePercentDone => {
                if physical {
                    if apply {
                        host.stats_mut().increase_total_phys_dmg_mod(rounded);
                    } else {
                        host.stats_mut().decrease_total_phys_dmg_mod(rounded);
                    }
                }
                // A multiplicative stack per magic school of the mask (Vengeance: holy): a
                // negative aura (Defensive Stance's -10 %) is added as-is and removed by the
                // same value.
                for magic in MagicSchool::magic_schools_of(school) {
                    if apply {
                        host.stats_mut()
                            .increase_magic_school_damage_mod(rounded, magic);
                    } else {
                        host.stats_mut()
                            .decrease_magic_school_damage_mod(rounded, magic);
                    }
                }
            }
            // The damage the target takes from the magic schools of the mask, from everyone.
            A::ModDamagePercentTaken if on_target => {
                for magic in MagicSchool::magic_schools_of(school) {
                    if apply {
                        host.target_mut()
                            .increase_magic_school_damage_mod(rounded, magic);
                    } else {
                        host.target_mut()
                            .decrease_magic_school_damage_mod(rounded, magic);
                    }
                }
            }
            A::ModDamagePercentTaken if !on_target => {
                if physical {
                    if apply {
                        host.stats_mut().add_phys_damage_taken_mod(rounded);
                    } else {
                        host.stats_mut().remove_phys_damage_taken_mod(rounded);
                    }
                }
                if magic {
                    if apply {
                        host.stats_mut().add_spell_damage_taken_mod(rounded);
                    } else {
                        host.stats_mut().remove_spell_damage_taken_mod(rounded);
                    }
                }
            }
            A::ModThreat if !on_target => {
                if apply {
                    host.stats_mut().increase_total_threat_mod(rounded);
                } else {
                    host.stats_mut().decrease_total_threat_mod(rounded);
                }
            }
            // Wind Blessed (Skyborne) is a MOD_MELEE_RANGED_HASTE_2.
            A::ModMeleeHaste3
            | A::ModMeleeHaste
            | A::ModAttackspeed
            | A::ModMeleeRangedHaste
            | A::ModMeleeRangedHaste2
                if !on_target && rounded > 0 =>
            {
                if apply {
                    host.increase_melee_attack_speed(rounded as u32);
                } else {
                    host.decrease_melee_attack_speed(rounded as u32);
                }
                // Faster swings that each deal less: the white damage per second stays.
                if self.script_kind() == Some(ScriptKind::AttackSpeedDamagePenalty) {
                    let factor = 100.0 / (100.0 + value);
                    host.adjust_mainhand_damage_multiplier(factor, apply);
                }
            }
            A::ModCastingSpeedNotStack if !on_target => {
                adjust(
                    host.stats_mut(),
                    signed,
                    |s, v| s.increase_casting_speed_mod(v),
                    |s, v| s.decrease_casting_speed_mod(v),
                );
            }
            A::ModCritPct => {
                adjust(
                    host.stats_mut(),
                    signed_of(hundredths, apply),
                    |s, v| s.increase_melee_aura_crit(v),
                    |s, v| s.decrease_melee_aura_crit(v),
                );
                adjust(
                    host.stats_mut(),
                    signed_of(hundredths, apply),
                    |s, v| s.increase_spell_crit(v),
                    |s, v| s.decrease_spell_crit(v),
                );
            }
            A::ModWeaponCritPercent => adjust(
                host.stats_mut(),
                signed_of(hundredths, apply),
                |s, v| s.increase_melee_aura_crit(v),
                |s, v| s.decrease_melee_aura_crit(v),
            ),
            A::ModSpellCritChance => adjust(
                host.stats_mut(),
                signed_of(hundredths, apply),
                |s, v| s.increase_spell_crit(v),
                |s, v| s.decrease_spell_crit(v),
            ),
            // Hit for the off-hand attacks only (Dual Wield Specialization).
            A::ModHitChance if self.script_kind() == Some(ScriptKind::OffhandHitChance) => adjust(
                host.stats_mut(),
                signed_of(hundredths, apply),
                |s, v| s.increase_offhand_melee_hit(v),
                |s, v| s.decrease_offhand_melee_hit(v),
            ),
            A::ModHitChance => adjust(
                host.stats_mut(),
                signed_of(hundredths, apply),
                |s, v| s.increase_melee_hit(v),
                |s, v| s.decrease_melee_hit(v),
            ),
            // The target's dodge and parry chance against the character's attacks, in percent
            // (Weapon Expertise).
            A::ModExpertise if !on_target => adjust(
                host.stats_mut(),
                signed_of(hundredths, apply),
                |s, v| s.increase_expertise(v),
                |s, v| s.decrease_expertise(v),
            ),
            A::ModSpellHitChance => adjust(
                host.stats_mut(),
                signed_of(hundredths, apply),
                |s, v| s.increase_spell_hit(v),
                |s, v| s.decrease_spell_hit(v),
            ),
            A::ModOffhandDamagePct => host.adjust_offhand_damage_percent(signed),
            // All damage done against the creature types of the mask (Murder: humanoids and
            // giants; Beast Slaying).
            A::ModDamageDoneVersus if !on_target => {
                let types = CreatureTypes::from_game_mask(self.record.misc_value[0] as u32);
                let stats = host.stats_mut();
                for creature in Vec::<CreatureType>::from(types) {
                    if apply {
                        stats.increase_dmg_vs_type(creature, value / 100.0);
                        stats.increase_magic_damage_mod_vs_type(creature, rounded);
                    } else {
                        stats.decrease_dmg_vs_type(creature, value / 100.0);
                        stats.decrease_magic_damage_mod_vs_type(creature, rounded);
                    }
                }
            }
            // The maximum of a resource: Improved Energy (`MOD_INCREASE_ENERGY`), Flask of
            // Distilled Wisdom's 2000 mana (`MOD_MAX_POWER`).
            A::ModPowerRegenPercent | A::ModIncreaseEnergy | A::ModMaxPower if !on_target => {
                if let Some(resource) = ResourceType::from_power_type(self.record.power_type()) {
                    if self.record.aura == A::ModPowerRegenPercent {
                        host.adjust_power_regen_percent(resource, signed);
                    } else {
                        host.adjust_max_power(resource, signed);
                    }
                }
            }
            A::ModManaRegenInterrupt if !on_target => host.adjust_mana_regen_while_casting(signed),
            // Melee attack power against the creature types of the mask (Justice Battlegear 4:
            // +36 against Undead).
            A::ModMeleeAttackPowerVersus if !on_target => {
                let types = CreatureTypes::from_game_mask(self.record.misc_value[0] as u32);
                let amount = rounded.max(0) as u32;
                let stats = host.stats_mut();
                for creature in Vec::<CreatureType>::from(types) {
                    if apply {
                        stats.increase_ap_vs_type(creature, amount);
                    } else {
                        stats.decrease_ap_vs_type(creature, amount);
                    }
                }
            }
            // Mana every 5 s, as the gear's mp5 (Lightforge Armor 6, Freethinker's Armor 2).
            A::ModPowerRegen
                if !on_target
                    && ResourceType::from_power_type(self.record.power_type())
                        == Some(ResourceType::Mana) =>
            {
                let amount = rounded.max(0) as u32;
                if apply {
                    host.stats_mut().increase_mp5(amount);
                } else {
                    host.stats_mut().decrease_mp5(amount);
                }
            }
            // Armor ignored by the attacks with the weapon types the spell requires
            // (Weaponmaster: maces, staves).
            A::ModArmorPenetrationPct if !on_target => {
                let percent = rounded.max(0) as u32;
                for weapon_type in self.weapon_types() {
                    if apply {
                        host.stats_mut()
                            .increase_armor_penetration_for_weapon_type(weapon_type, percent);
                    } else {
                        host.stats_mut()
                            .decrease_armor_penetration_for_weapon_type(weapon_type, percent);
                    }
                }
            }
            A::ModShapeshift => {
                if let Some(stance) = Stance::from_form(self.record.shapeshift_form()) {
                    if apply {
                        host.swap_stance(stance);
                    } else {
                        host.leave_stance(stance);
                    }
                }
            }
            A::ModIgnoreShapeshift if !on_target => {
                let set = self.class_options.map_or(0, |c| c.set);
                host.spell_modifiers_mut().adjust_ignore_shapeshift(
                    set,
                    self.record.spell_class_mask,
                    apply,
                );
            }
            A::AddFlatModifier | A::AddPctModifier => {
                let modifier = self.spell_modifier(value);
                if apply {
                    host.spell_modifiers_mut().add(modifier);
                } else {
                    host.spell_modifiers_mut().remove(&modifier);
                }
            }
            // The caster's debuff raises the damage the target takes from the caster's spells
            // in its class mask (Hemorrhage: Rupture). Only the owner's spells see it, so it is
            // kept with the owner's modifiers.
            A::ModSpellDamageFromCaster if on_target => {
                let modifier = SpellModifier {
                    set: self.class_options.map_or(0, |c| c.set),
                    class_mask: self.record.spell_class_mask,
                    op: SpellModOp::HealingAndDamage,
                    pct: true,
                    amount: value,
                    source: self.spell,
                };
                if apply {
                    host.spell_modifiers_mut().add_damage_from_caster(modifier);
                } else {
                    host.spell_modifiers_mut()
                        .remove_damage_from_caster(&modifier);
                }
            }
            A::OverrideActionbarSpells => {
                host.override_actionbar_spell(
                    self.record.misc_value[0] as u32,
                    self.record.base_points as u32,
                    apply,
                );
            }
            A::Dummy => match self.script_kind() {
                Some(ScriptKind::StanceRageRetained) => host.adjust_stance_rage_retained(signed),
                // `params.value` scales the client's value (Dual Wield Specialization's
                // 20 % per rank is 10 % in the patched game).
                Some(ScriptKind::OffhandRagePercent) => {
                    let scale = self.script().and_then(|s| s.params.value).unwrap_or(1.0);
                    host.adjust_offhand_rage_percent((f64::from(signed) * scale).round() as i32);
                }
                Some(ScriptKind::OffhandCopy) => {
                    // Validated as present when the overrides were loaded.
                    if let Some(spell) = self.script().and_then(|s| s.params.spell) {
                        host.adjust_offhand_copy(spell, apply);
                    }
                }
                // Damage of the spells in the family mask below the target health threshold
                // (Quietus); kept with the owner's modifiers.
                Some(ScriptKind::DamagePercentBelowHealth) => {
                    let mask = self.script().and_then(|s| s.params.family_mask);
                    if let (Some(class_mask), Some(threshold)) = (mask, self.health_threshold) {
                        let modifier = SpellModifier {
                            set: self.class_options.map_or(0, |c| c.set),
                            class_mask,
                            op: SpellModOp::HealingAndDamage,
                            pct: true,
                            amount: value,
                            source: self.spell,
                        };
                        let modifiers = host.spell_modifiers_mut();
                        if apply {
                            modifiers.add_below_health(modifier, threshold);
                        } else {
                            modifiers.remove_below_health(&modifier, threshold);
                        }
                    }
                }
                Some(ScriptKind::SchoolDamagePercentWhileAura) => {
                    if let Some(spell) = self.script().and_then(|s| s.params.spell) {
                        host.adjust_school_damage_while_aura(spell, value, apply);
                    }
                }
                Some(ScriptKind::JudgedSealManaReturn) => {
                    if let Some(chance_effect) = self.script().and_then(|s| s.params.effect) {
                        host.adjust_judged_seal_mana_return(
                            self.spell,
                            chance_effect,
                            value,
                            apply,
                        );
                    }
                }
                Some(ScriptKind::GainResourceOnUse) => {
                    // Validated as present when the overrides were loaded.
                    let params = self.script().map(|s| &s.params);
                    let spell = params.and_then(|p| p.spell);
                    let resource = params
                        .and_then(|p| p.resource)
                        .and_then(ResourceType::from_power_type);
                    if let (Some(spell), Some(resource)) = (spell, resource) {
                        let amount = resource.from_stored_amount(value);
                        host.adjust_resource_on_use(spell, resource, amount, apply);
                    }
                }
                // Proc payloads (`TRIGGER_WITH_VALUE`), periodic gains and the talent scripts
                // without a runtime yet act through the proc / periodic systems.
                _ => {}
            },
            // Periodic and proc auras are driven by the periodic and proc systems; the rest
            // (healing taken, immunities, movement, taunts, skills, max power, ...) does not
            // affect a damage simulation and is a documented no-op.
            _ => {}
        }
    }

    /// The modifier this `ADD_*_MODIFIER` aura contributes, at `amount`.
    pub fn spell_modifier(&self, amount: f64) -> SpellModifier {
        SpellModifier::from_effect(
            &self.record,
            self.spell,
            self.class_options.map_or(0, |c| c.set),
            amount,
        )
    }
}

fn signed_of(value: i32, apply: bool) -> i32 {
    if apply { value } else { -value }
}

/// Applies `signed` to the stats: positive through `inc`, negative through `dec`.
fn adjust(
    stats: &mut CharacterStats,
    signed: i32,
    inc: impl FnOnce(&mut CharacterStats, u32),
    dec: impl FnOnce(&mut CharacterStats, u32),
) {
    if signed >= 0 {
        inc(stats, signed as u32);
    } else {
        dec(stats, signed.unsigned_abs());
    }
}

/// The attributes a `MOD_TOTAL_STAT_PERCENTAGE` aura scales: the stat bitmask in its second misc
/// value (1 strength, 2 agility, 4 stamina, 8 intellect, 16 spirit; Blessing of Kings 31, The
/// Human Spirit 16, Divine Intellect 8), or without one the first misc value (−1 all, else the
/// stat index). The first value alone is not reliable: Divine Intellect's is 0 (strength).
fn total_stat_percentage_stats(misc_value: [i32; 2]) -> Vec<ItemStat> {
    const ATTRIBUTES: [ItemStat; 5] = [
        ItemStat::Strength,
        ItemStat::Agility,
        ItemStat::Stamina,
        ItemStat::Intellect,
        ItemStat::Spirit,
    ];
    let mask = match misc_value {
        [_, mask] if mask > 0 => mask,
        [-1, _] => 31,
        [index @ 0..=4, _] => 1 << index,
        _ => 0,
    };
    ATTRIBUTES
        .into_iter()
        .enumerate()
        .filter(|&(i, _)| mask & (1 << i) != 0)
        .map(|(_, stat)| stat)
        .collect()
}

/// Adds (`apply`) or removes the percentage effect `percent` on a multiplicative stack. The
/// sign of the value cannot pick the direction as with [`adjust`]: a stack holds negative
/// effects too, and a 0 % effect must be removed like any other.
fn multiplier(
    stats: &mut CharacterStats,
    apply: bool,
    percent: i32,
    add: impl FnOnce(&mut CharacterStats, i32),
    remove: impl FnOnce(&mut CharacterStats, i32),
) {
    if apply {
        add(stats, percent);
    } else {
        remove(stats, percent);
    }
}

fn change_target_armor(host: &mut impl EffectHost, value: i32) {
    if value >= 0 {
        host.target_mut().increase_armor(value);
    } else {
        host.target_mut().decrease_armor(-value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spell::dbc::ImplicitTarget;
    use crate::spell::overrides::ScriptParams;
    use crate::spell::record::Categories;
    use std::collections::VecDeque;

    struct MockHost {
        level: u32,
        combo_points: u32,
        rage: u32,
        melee_ap: u32,
        rolls: VecDeque<PhysicalAttackResult>,
        rolled_with: Vec<IncludedOutcomes>,
        extra_crits: Vec<u32>,
        can_crits: Vec<bool>,
        stats: CharacterStats,
        target: Target,
        stance: Option<Stance>,
        attack_speed_calls: Vec<i32>,
        modifiers: SpellModifiers,
        extra_attacks: u32,
        stance_rage: i32,
        offhand_damage: i32,
        offhand_rage: i32,
        offhand_copies: Vec<(u32, bool)>,
        resources_on_use: Vec<(u32, ResourceType, u32, bool)>,
        overrides: Vec<(u32, u32, bool)>,
        two_hand: bool,
        mainhand: Option<WeaponType>,
        poisoned: bool,
        spell_rolls: VecDeque<SpellRoll>,
        spell_rolled_with: Vec<(MagicSchool, SpellResistKind, bool)>,
    }

    impl MockHost {
        fn new() -> Self {
            MockHost {
                level: 60,
                combo_points: 0,
                rage: 50,
                melee_ap: 1000,
                rolls: VecDeque::new(),
                rolled_with: Vec::new(),
                extra_crits: Vec::new(),
                can_crits: Vec::new(),
                stats: CharacterStats::new(),
                target: Target::new(63),
                stance: None,
                attack_speed_calls: Vec::new(),
                modifiers: SpellModifiers::new(),
                extra_attacks: 0,
                stance_rage: 0,
                offhand_damage: 0,
                offhand_rage: 0,
                offhand_copies: Vec::new(),
                resources_on_use: Vec::new(),
                overrides: Vec::new(),
                two_hand: false,
                mainhand: None,
                poisoned: false,
                spell_rolls: VecDeque::new(),
                spell_rolled_with: Vec::new(),
            }
        }

        fn with_spell_rolls(mut self, rolls: &[SpellRoll]) -> Self {
            self.spell_rolls = rolls.iter().copied().collect();
            self
        }

        fn with_rolls(mut self, rolls: &[PhysicalAttackResult]) -> Self {
            self.rolls = rolls.iter().copied().collect();
            self
        }
    }

    impl EffectHost for MockHost {
        fn caster_level(&self) -> u32 {
            self.level
        }
        fn combo_points(&self) -> u32 {
            self.combo_points
        }
        fn gain_combo_points(&mut self, amount: u32) {
            self.combo_points = (self.combo_points + amount).min(5);
        }
        fn spend_combo_points(&mut self) {
            self.combo_points = 0;
        }
        fn resource_level(&self, resource: ResourceType) -> u32 {
            assert_eq!(resource, ResourceType::Rage);
            self.rage
        }
        fn gain_resource(&mut self, resource: ResourceType, amount: u32) -> u32 {
            assert_eq!(resource, ResourceType::Rage);
            let before = self.rage;
            self.rage = (self.rage + amount).min(100);
            self.rage - before
        }
        fn melee_ap(&self) -> u32 {
            self.melee_ap
        }
        fn max_health(&self) -> u32 {
            4000
        }
        fn random_in_range(&mut self, min: f64, max: f64) -> f64 {
            (min + max) / 2.0
        }
        fn random_normalized_mh_dmg(&mut self) -> f64 {
            300.0
        }
        fn random_non_normalized_mh_dmg(&mut self) -> f64 {
            400.0
        }
        fn roll_melee_ability(
            &mut self,
            included: IncludedOutcomes,
            extra_crit: u32,
            can_crit: bool,
        ) -> PhysicalAttackResult {
            self.rolled_with.push(included);
            self.extra_crits.push(extra_crit);
            self.can_crits.push(can_crit);
            self.rolls.pop_front().expect("no roll queued")
        }
        fn roll_spell(
            &mut self,
            school: MagicSchool,
            kind: SpellResistKind,
            _extra_crit: u32,
            can_crit: bool,
        ) -> SpellRoll {
            self.spell_rolled_with.push((school, kind, can_crit));
            self.spell_rolls.pop_front().expect("no spell roll queued")
        }
        fn stats_mut(&mut self) -> &mut CharacterStats {
            &mut self.stats
        }
        fn target_mut(&mut self) -> &mut Target {
            &mut self.target
        }
        fn target_creature_type(&self) -> CreatureType {
            self.target.creature_type()
        }
        fn increase_melee_attack_speed(&mut self, percent: u32) {
            self.attack_speed_calls.push(percent as i32);
            self.stats.increase_melee_attack_speed(percent);
        }
        fn decrease_melee_attack_speed(&mut self, percent: u32) {
            self.attack_speed_calls.push(-(percent as i32));
            self.stats.decrease_melee_attack_speed(percent);
        }
        fn swap_stance(&mut self, stance: Stance) {
            self.stance = Some(stance);
        }
        fn spell_modifiers(&self) -> &SpellModifiers {
            &self.modifiers
        }
        fn spell_modifiers_mut(&mut self) -> &mut SpellModifiers {
            &mut self.modifiers
        }
        fn add_extra_attacks(&mut self, count: u32) {
            self.extra_attacks += count;
        }
        fn adjust_stance_rage_retained(&mut self, delta: i32) {
            self.stance_rage += delta;
        }
        fn adjust_offhand_damage_percent(&mut self, percent: i32) {
            self.offhand_damage += percent;
        }
        fn adjust_offhand_rage_percent(&mut self, percent: i32) {
            self.offhand_rage += percent;
        }
        fn adjust_offhand_copy(&mut self, spell: u32, apply: bool) {
            self.offhand_copies.push((spell, apply));
        }
        fn adjust_resource_on_use(
            &mut self,
            spell: u32,
            resource: ResourceType,
            amount: u32,
            apply: bool,
        ) {
            self.resources_on_use.push((spell, resource, amount, apply));
        }
        fn override_actionbar_spell(&mut self, replaced: u32, replacement: u32, apply: bool) {
            self.overrides.push((replaced, replacement, apply));
        }
        fn has_two_hand_weapon(&self) -> bool {
            self.two_hand
        }
        fn mainhand_weapon_type(&self) -> Option<WeaponType> {
            self.mainhand
        }
        fn target_poisoned_by_caster(&self) -> bool {
            self.poisoned
        }
    }

    fn melee_spell() -> SpellRecord {
        let mut spell = SpellRecord::new(12294, "Mortal Strike");
        spell.categories = Categories {
            defense_type: DefenseType::Melee,
            ..Categories::default()
        };
        spell.levels = Levels {
            base: 40,
            spell: 40,
            max: 0,
        };
        spell.class_options = Some(ClassOptions {
            set: 4,
            mask: [33554432, 0, 0, 0],
        });
        spell
    }

    fn effect_record(index: u32, effect: SpellEffectName, base_points: f32) -> EffectRecord {
        let mut record = EffectRecord::new(index, effect);
        record.base_points = base_points;
        record.implicit_target = [ImplicitTarget::UnitTargetEnemy, ImplicitTarget::None];
        record
    }

    fn effect(kind: SpellEffectName, value: f32) -> Effect {
        Effect::new(&effect_record(0, kind, value), &melee_spell(), None, false)
    }

    fn aura_record(index: u32, aura: AuraType, base_points: f32, misc: i32) -> EffectRecord {
        let mut record = effect_record(index, SpellEffectName::ApplyAura, base_points);
        record.aura = aura;
        record.misc_value = [misc, 0];
        record.implicit_target = [ImplicitTarget::UnitCaster, ImplicitTarget::None];
        record
    }

    fn aura(kind: AuraType, value: f32, misc: i32) -> Effect {
        Effect::new(
            &aura_record(0, kind, value, misc),
            &melee_spell(),
            None,
            false,
        )
    }

    #[test]
    fn dependency_and_included_outcomes_come_from_the_records() {
        let first = effect(SpellEffectName::SchoolDamage, 0.0);
        assert_eq!(first.dependency(), Dependency::Independent);
        assert_eq!(first.included_outcomes(), IncludedOutcomes::ALL);
        assert!(first.can_crit());
        let second = Effect::new(
            &effect_record(1, SpellEffectName::SchoolDamage, 0.0),
            &melee_spell(),
            None,
            true,
        );
        assert_eq!(second.dependency(), Dependency::PartialSuccess);
        assert!(!second.can_crit());

        let mut overpower = SpellRecord::new(7384, "Overpower");
        overpower.attributes[0] = 0x250010;
        let overpower = Effect::new(
            &effect_record(0, SpellEffectName::NormalizedWeaponDmg, 35.0),
            &overpower,
            None,
            false,
        );
        assert_eq!(
            overpower.included_outcomes(),
            IncludedOutcomes {
                dodge: false,
                parry: false,
                block: false,
                miss: true
            }
        );
        assert_eq!(overpower.kind(), SpellEffectName::NormalizedWeaponDmg);
        assert_eq!(overpower.spell(), 7384);
        assert!(!overpower.is_aura());
        assert!(aura(AuraType::ModAttackPower, 1.0, 0).is_aura());
    }

    #[test]
    fn values_scale_per_level_and_through_modifiers() {
        let mut record = effect_record(0, SpellEffectName::ApplyAura, 21.0);
        record.real_points_per_level = 0.3;
        let mut shout = SpellRecord::new(5242, "Battle Shout");
        shout.levels = Levels {
            base: 12,
            spell: 12,
            max: 21,
        };
        let scaled = Effect::new(&record, &shout, None, false);
        assert_eq!(scaled.value_at_level(12), 21.0);
        assert!((scaled.value_at_level(20) - 23.4).abs() < 1e-5);
        assert!((scaled.value_at_level(60) - 23.7).abs() < 1e-5);
        assert_eq!(scaled.value_at_level(1), 21.0);

        let mut host = MockHost::new();
        let mut ms = effect(SpellEffectName::NormalizedWeaponDmg, 85.0);
        assert_eq!(ms.effective_value(&host), 85.0);
        host.modifiers.add(SpellModifier {
            set: 4,
            class_mask: [33554432, 0, 0, 0],
            op: SpellModOp::Points,
            pct: true,
            amount: 20.0,
            source: 1,
        });
        assert_eq!(ms.effective_value(&host), 102.0);
        ms.set_value(100.0);
        assert_eq!(ms.value(), 100.0);
        assert_eq!(ms.base_value(), 85.0);
        assert_eq!(ms.effective_value(&host), 120.0);
        ms.reset_value();
        assert_eq!(ms.value(), 85.0);
        assert_eq!(ms.index(), 0);
    }

    #[test]
    fn damage_effects_roll_and_compute_damage() {
        let mut host = MockHost::new().with_rolls(&[
            PhysicalAttackResult::Hit,
            PhysicalAttackResult::Critical,
            PhysicalAttackResult::Hit,
            PhysicalAttackResult::Hit,
            PhysicalAttackResult::Hit,
        ]);

        let mut normalized = effect(SpellEffectName::NormalizedWeaponDmg, 160.0);
        let outcome = normalized.perform_independent(&mut host, 30, 0);
        assert!(outcome.success);
        assert_eq!(outcome.rolled, Some(PhysicalAttackResult::Hit));
        assert_eq!(normalized.damage_dealt, 460.0);
        assert!(normalized.was_successful());

        let mut weapon = effect(SpellEffectName::WeaponDamageNoschool, 138.0);
        assert!(weapon.perform_independent(&mut host, 15, 0).success);
        assert_eq!(weapon.damage_dealt, 538.0);
        assert_eq!(weapon.last_result, Some(PhysicalAttackResult::Critical));

        let mut percent = effect(SpellEffectName::WeaponPercentDamage, 115.0);
        assert!(percent.perform_independent(&mut host, 0, 0).success);
        assert_eq!(percent.damage_dealt, 460.0);

        let mut revenge = effect_record(0, SpellEffectName::SchoolDamage, 153.0);
        revenge.variance = 0.2;
        let mut revenge = Effect::new(&revenge, &melee_spell(), None, false);
        assert!(revenge.perform_independent(&mut host, 5, 0).success);
        assert!(
            (revenge.damage_dealt - 153.0).abs() < 1e-9,
            "midpoint of the range"
        );

        let mut hammer = effect_record(0, SpellEffectName::SchoolDamage, 100.0);
        hammer.bonus_coefficient_from_ap = 0.5;
        hammer.points_per_resource = 10.0;
        host.combo_points = 2;
        let mut hammer = Effect::new(&hammer, &melee_spell(), None, false);
        assert!(hammer.perform_independent(&mut host, 0, 0).success);
        assert_eq!(hammer.damage_dealt, 100.0 + 20.0 + 500.0);
        assert_eq!(host.rolled_with.len(), 5);
    }

    fn magic_spell(school: SpellSchoolMask, defense_type: DefenseType) -> SpellRecord {
        let mut spell = melee_spell();
        spell.categories.defense_type = defense_type;
        spell.school_mask = school;
        spell
    }

    #[test]
    fn magic_school_damage_rolls_the_spell_table() {
        let mut host = MockHost::new().with_spell_rolls(&[SpellRoll::MISS, SpellRoll::HIT]);
        let mut spell = magic_spell(SpellSchoolMask::NATURE, DefenseType::Magic);
        let record = effect_record(0, SpellEffectName::SchoolDamage, 50.0);
        spell.effects = vec![record.clone()];
        let mut bolt = Effect::new(&record, &spell, None, false);

        let missed = bolt.perform_independent(&mut host, 0, 0);
        assert!(!missed.success);
        assert_eq!(missed.spell_roll, Some(SpellRoll::MISS));
        assert_eq!(missed.rolled, None, "not the melee table");
        assert_eq!(bolt.damage_dealt, 0.0);

        let landed = bolt.perform_independent(&mut host, 0, 0);
        assert!(landed.success);
        assert_eq!(landed.spell_roll, Some(SpellRoll::HIT));
        assert_eq!(bolt.damage_dealt, 50.0);
        assert!(host.rolled_with.is_empty());
        assert_eq!(
            host.spell_rolled_with,
            [(MagicSchool::Nature, SpellResistKind::NonBinary, true); 2]
        );
    }

    #[test]
    fn health_leech_deals_a_percent_of_the_caster_max_health() {
        let mut host = MockHost::new().with_spell_rolls(&[SpellRoll::HIT]);
        let mut spell = magic_spell(SpellSchoolMask::SHADOW, DefenseType::Magic);
        let record = effect_record(0, SpellEffectName::HealthLeech, 5.0);
        spell.effects = vec![record.clone()];
        let mut drain = Effect::new(&record, &spell, None, false);
        let outcome = drain.perform_independent(&mut host, 0, 0);
        assert!(outcome.success);
        assert_eq!(outcome.spell_roll, Some(SpellRoll::HIT));
        assert_eq!(drain.damage_dealt, 200.0, "5 % of 4000");
        // A drain only deals damage: a non-binary spell, partially resisted.
        assert_eq!(
            host.spell_rolled_with,
            [(MagicSchool::Shadow, SpellResistKind::NonBinary, true)]
        );
    }

    /// The magic table is for spells whose `DefenseType` is magic, or none with a magic school;
    /// a physical spell without one (Sweeping Strikes' copy) always lands; melee and ranged
    /// spells roll the special attack table.
    #[test]
    fn which_spells_roll_the_spell_table() {
        let rolls = |school, defense_type| {
            let spell = magic_spell(school, defense_type);
            let record = effect_record(0, SpellEffectName::SchoolDamage, 50.0);
            Effect::new(&record, &spell, None, false).rolls_spell_table()
        };
        assert!(rolls(SpellSchoolMask::FIRE, DefenseType::Magic));
        assert!(rolls(SpellSchoolMask::PHYSICAL, DefenseType::Magic));
        assert!(rolls(SpellSchoolMask::FIRE, DefenseType::None));
        assert!(!rolls(SpellSchoolMask::PHYSICAL, DefenseType::None));
        assert!(!rolls(SpellSchoolMask::FIRE, DefenseType::Ranged));
        assert!(!rolls(SpellSchoolMask::FIRE, DefenseType::Melee));

        let mut host = MockHost::new();
        let spell = magic_spell(SpellSchoolMask::PHYSICAL, DefenseType::None);
        let record = effect_record(0, SpellEffectName::SchoolDamage, 50.0);
        let mut copy = Effect::new(&record, &spell, None, false);
        let outcome = copy.perform_independent(&mut host, 0, 0);
        assert!(outcome.success);
        assert_eq!((outcome.rolled, outcome.spell_roll), (None, None));
        assert_eq!(copy.damage_dealt, 50.0);
    }

    /// Royalgiraffe: a spell that only deals damage (a health leech too) is non-binary; any
    /// other effect makes it binary.
    #[test]
    fn spell_resist_kinds() {
        let mut spell = magic_spell(SpellSchoolMask::FIRE, DefenseType::Magic);
        let damage = effect_record(0, SpellEffectName::SchoolDamage, 50.0);
        let mut dot = effect_record(1, SpellEffectName::ApplyAura, 10.0);
        dot.aura = AuraType::PeriodicDamage;
        let mut dummy = effect_record(2, SpellEffectName::ApplyAura, 0.0);
        dummy.aura = AuraType::Dummy;
        let mut debuff = effect_record(2, SpellEffectName::ApplyAura, -25.0);
        debuff.aura = AuraType::ModResistance;

        spell.effects = vec![damage.clone()];
        assert_eq!(spell_resist_kind(&spell), SpellResistKind::NonBinary);
        spell.effects = vec![damage.clone(), dot.clone(), dummy];
        assert_eq!(spell_resist_kind(&spell), SpellResistKind::NonBinary);
        spell.effects = vec![dot.clone()];
        assert_eq!(spell_resist_kind(&spell), SpellResistKind::NonBinary);
        spell.effects = vec![damage, debuff];
        assert_eq!(spell_resist_kind(&spell), SpellResistKind::Binary);
        spell.effects = vec![effect_record(0, SpellEffectName::HealthLeech, 5.0)];
        assert_eq!(spell_resist_kind(&spell), SpellResistKind::NonBinary);
        let mut leech = effect_record(0, SpellEffectName::ApplyAura, 10.0);
        leech.aura = AuraType::PeriodicLeech;
        spell.effects = vec![leech];
        assert_eq!(spell_resist_kind(&spell), SpellResistKind::NonBinary);
    }

    /// A damage-over-time of a spell on the magic table rolls its hit (it cannot crit); a
    /// dependent effect reuses the chain's roll.
    #[test]
    fn spell_damage_debuffs_roll_and_reuse_the_chain_roll() {
        let mut host = MockHost::new().with_spell_rolls(&[SpellRoll::MISS]);
        let mut spell = magic_spell(SpellSchoolMask::NATURE, DefenseType::Magic);
        let mut record = effect_record(0, SpellEffectName::ApplyAura, 10.0);
        record.aura = AuraType::PeriodicDamage;
        record.implicit_target = [ImplicitTarget::UnitTargetEnemy, ImplicitTarget::None];
        spell.effects = vec![record.clone()];
        let mut dot = Effect::new(&record, &spell, None, false);
        assert!(dot.is_spell_damage_debuff());
        assert!(dot.rolls_attack());

        let outcome = dot.perform_independent(&mut host, 0, 0);
        assert!(!outcome.success);
        assert_eq!(
            host.spell_rolled_with,
            [(MagicSchool::Nature, SpellResistKind::NonBinary, false)]
        );

        dot.set_dependency(Dependency::PartialSuccess);
        let chain = ChainState {
            result: SpellResult::Success,
            previous: None,
            previous_spell: Some(SpellRoll::HIT),
            resource_cost: 0,
            extra_crit: 0,
            hit_guaranteed: false,
        };
        let reused = dot.perform(&mut host, &chain);
        assert!(reused.success);
        assert_eq!(reused.spell_roll, None, "no fresh roll");
        assert_eq!(host.spell_rolled_with.len(), 1);
    }

    #[test]
    fn failed_rolls_report_the_result_and_deal_no_damage() {
        let mut host = MockHost::new().with_rolls(&[
            PhysicalAttackResult::Dodge,
            PhysicalAttackResult::Miss,
            PhysicalAttackResult::Parry,
        ]);
        for expected in [
            PhysicalAttackResult::Dodge,
            PhysicalAttackResult::Miss,
            PhysicalAttackResult::Parry,
        ] {
            let mut effect = effect(SpellEffectName::SchoolDamage, 600.0);
            let outcome = effect.perform_independent(&mut host, 15, 0);
            assert!(!outcome.success);
            assert_eq!(outcome.rolled, Some(expected));
            assert_eq!(effect.damage_dealt, 0.0);
            assert!(!effect.was_successful());
        }
    }

    #[test]
    fn scripts_execute_and_attack_power_damage() {
        let mut host =
            MockHost::new().with_rolls(&[PhysicalAttackResult::Hit, PhysicalAttackResult::Hit]);
        host.rage = 80;
        let mut record = effect_record(0, SpellEffectName::Dummy, 600.0);
        record.chain_amplitude = 0.5;
        let script = EffectScript {
            index: 0,
            script: ScriptKind::Execute,
            params: ScriptParams::default(),
        };
        let mut execute = Effect::new(&record, &melee_spell(), Some(script), false);
        let outcome = execute.perform_independent(&mut host, 15, 0);
        assert!(outcome.success && outcome.consumes_all_resource);
        assert_eq!(execute.damage_dealt, 600.0 + 65.0 * 5.0);
        assert_eq!(execute.script_kind(), Some(ScriptKind::Execute));

        let bt = EffectScript {
            index: 1,
            script: ScriptKind::AttackPowerPercentDamage,
            params: ScriptParams::default(),
        };
        let mut bloodthirst = Effect::new(
            &effect_record(1, SpellEffectName::Dummy, 35.0),
            &melee_spell(),
            Some(bt),
            false,
        );
        assert!(bloodthirst.perform_independent(&mut host, 30, 0).success);
        assert_eq!(bloodthirst.damage_dealt, 350.0);

        let mut unscripted = effect(SpellEffectName::Dummy, 1.0);
        assert!(unscripted.perform_independent(&mut host, 0, 0).success);
        assert_eq!(unscripted.damage_dealt, 0.0);
    }

    #[test]
    fn dependent_effects_reuse_the_first_roll_and_respect_the_chain() {
        let mut host = MockHost::new().with_rolls(&[PhysicalAttackResult::Critical]);
        let mut first = effect(SpellEffectName::SchoolDamage, 600.0);
        let mut second = Effect::new(
            &effect_record(1, SpellEffectName::NormalizedWeaponDmg, 15.0),
            &melee_spell(),
            None,
            false,
        );

        let mut chain = ChainState {
            result: SpellResult::Undetermined,
            previous: None,
            previous_spell: None,
            resource_cost: 15,
            extra_crit: 2500,
            hit_guaranteed: false,
        };
        let first_outcome = first.perform(&mut host, &chain);
        assert_eq!(first_outcome.rolled, Some(PhysicalAttackResult::Critical));
        assert_eq!(host.extra_crits, vec![2500]);
        chain.result = SpellResult::from_first(first_outcome.success);
        chain.previous = first.last_result;
        let second_outcome = second.perform(&mut host, &chain);
        assert!(second_outcome.success);
        assert_eq!(second_outcome.rolled, None);
        assert_eq!(second.last_result, Some(PhysicalAttackResult::Critical));
        assert_eq!(second.damage_dealt, 315.0);
        assert_eq!(host.rolled_with.len(), 1);

        let mut third = Effect::new(
            &effect_record(1, SpellEffectName::SchoolDamage, 1.0),
            &melee_spell(),
            None,
            false,
        );
        chain.result = SpellResult::Failure;
        chain.previous = Some(PhysicalAttackResult::Miss);
        assert_eq!(third.perform(&mut host, &chain), EffectOutcome::SKIPPED);

        let mut fourth = Effect::new(
            &effect_record(1, SpellEffectName::SchoolDamage, 1.0),
            &melee_spell(),
            None,
            false,
        );
        chain.result = SpellResult::PartialSuccess;
        let outcome = fourth.perform(&mut host, &chain);
        assert!(!outcome.success);
        assert_eq!(outcome.rolled, None);
    }

    fn scripted(index: u32, script: ScriptKind, params: ScriptParams) -> Option<EffectScript> {
        Some(EffectScript {
            index,
            script,
            params,
        })
    }

    /// A hostile trigger of a melee spell lands on the attack table before it triggers (without
    /// crit); the strikes it triggers roll with nothing to avoid.
    #[test]
    fn strike_triggers_roll_and_strikes_cannot_be_avoided() {
        let mut host = MockHost::new().with_rolls(&[
            PhysicalAttackResult::Dodge,
            PhysicalAttackResult::Hit,
            PhysicalAttackResult::Critical,
        ]);
        let mut record = effect_record(1, SpellEffectName::TriggerSpell, 0.0);
        record.trigger_spell = 1310706;
        let mut strike = Effect::new(&record, &melee_spell(), None, false);
        assert!(strike.is_strike_trigger());
        assert!(strike.rolls_attack());
        let dodged = strike.perform_independent(&mut host, 0, 0);
        assert!(!dodged.success);
        assert_eq!(dodged.rolled, Some(PhysicalAttackResult::Dodge));
        assert_eq!(dodged.trigger, None);
        let landed = strike.perform_independent(&mut host, 0, 0);
        assert_eq!(landed.trigger, Some(1310706));
        assert_eq!(host.can_crits, [false, false]);

        let mut weapon = effect(SpellEffectName::NormalizedWeaponDmg, 23.0);
        let chain = ChainState {
            result: SpellResult::Undetermined,
            previous: None,
            previous_spell: None,
            resource_cost: 0,
            extra_crit: 0,
            hit_guaranteed: true,
        };
        assert!(weapon.perform(&mut host, &chain).success);
        assert_eq!(host.rolled_with[2], IncludedOutcomes::NONE);
        assert!(host.can_crits[2]);
    }

    /// Eviscerate's attack power share: 3 % per combo point, dealt with the direct damage;
    /// Rupture's per-point table.
    #[test]
    fn combo_point_attack_power_damage() {
        let mut host = MockHost::new().with_rolls(&[PhysicalAttackResult::Hit]);
        host.combo_points = 5;
        let params = ScriptParams {
            value: Some(3.0),
            ..ScriptParams::default()
        };
        let mut eviscerate = Effect::new(
            &effect_record(1, SpellEffectName::Dummy, 0.0),
            &melee_spell(),
            scripted(1, ScriptKind::ComboPointApDamage, params),
            false,
        );
        let outcome = eviscerate.perform_independent(&mut host, 35, 0);
        assert!(outcome.success);
        assert_eq!(eviscerate.damage_dealt, 150.0, "15 % of 1000 attack power");

        let rupture = ScriptParams {
            effect: Some(0),
            per_combo_point: Some([4.0, 10.0, 18.0, 21.0, 24.0]),
            ..ScriptParams::default()
        };
        assert_eq!(rupture.combo_point_ap_percent(0), 0.0);
        assert_eq!(rupture.combo_point_ap_percent(1), 4.0);
        assert_eq!(rupture.combo_point_ap_percent(5), 24.0);
        assert_eq!(params.combo_point_ap_percent(2), 6.0);
        let mut periodic = Effect::new(
            &effect_record(2, SpellEffectName::Dummy, 0.0),
            &melee_spell(),
            scripted(2, ScriptKind::ComboPointApDamage, rupture),
            false,
        );
        periodic.perform_independent(&mut host, 0, 0);
        assert_eq!(
            periodic.damage_dealt, 0.0,
            "the ticks take a periodic aura's share"
        );
    }

    #[test]
    fn cast_bonus_and_weapon_type_values_replace_the_base_points() {
        let mut host = MockHost::new();
        let mut percent = effect(SpellEffectName::WeaponPercentDamage, 125.0);
        percent.set_weapon_type_value(1 << 15, 180.0);
        assert_eq!(percent.effective_value(&host), 125.0);
        host.mainhand = Some(WeaponType::Sword);
        assert_eq!(percent.effective_value(&host), 125.0);
        host.mainhand = Some(WeaponType::Dagger);
        assert_eq!(percent.effective_value(&host), 180.0);

        let mut expose = aura(AuraType::ModResistance, 0.0, 1);
        expose.set_cast_bonus(-2250.0);
        assert_eq!(expose.cast_bonus(), -2250.0);
        assert_eq!(expose.effective_value(&host), -2250.0);
    }

    #[test]
    fn damage_percent_vs_poisoned_is_handed_to_the_strikes() {
        let mut host = MockHost::new();
        let mut mutilate = Effect::new(
            &effect_record(3, SpellEffectName::Dummy, 20.0),
            &melee_spell(),
            scripted(
                3,
                ScriptKind::DamagePercentVsPoisoned,
                ScriptParams::default(),
            ),
            false,
        );
        let outcome = mutilate.perform_independent(&mut host, 0, 0);
        assert_eq!(outcome.triggered_damage_percent, 0.0);
        host.poisoned = true;
        let outcome = mutilate.perform_independent(&mut host, 0, 0);
        assert_eq!(outcome.triggered_damage_percent, 20.0);
    }

    /// Expose Armor and Sunder Armor share the exclusive armor reduction; Hemorrhage's debuff
    /// is a modifier of the caster's own spells.
    /// The stat mask of `MOD_TOTAL_STAT_PERCENTAGE`, with the first misc value as the fallback.
    #[test]
    fn total_stat_percentage_scales_the_named_attributes() {
        use ItemStat::{Agility, Intellect, Spirit, Stamina, Strength};
        let all = vec![Strength, Agility, Stamina, Intellect, Spirit];
        assert_eq!(
            total_stat_percentage_stats([-1, 31]),
            all,
            "Blessing of Kings"
        );
        assert_eq!(
            total_stat_percentage_stats([4, 16]),
            [Spirit],
            "The Human Spirit"
        );
        assert_eq!(
            total_stat_percentage_stats([0, 8]),
            [Intellect],
            "Divine Intellect"
        );
        assert_eq!(
            total_stat_percentage_stats([0, 1]),
            [Strength],
            "Divine Strength"
        );
        assert_eq!(
            total_stat_percentage_stats([0, 4]),
            [Stamina],
            "Sacred Duty"
        );
        assert_eq!(total_stat_percentage_stats([-1, 0]), all);
        assert_eq!(total_stat_percentage_stats([1, 0]), [Agility]);
    }

    #[test]
    fn exclusive_armor_and_damage_from_caster_auras() {
        let mut host = MockHost::new();
        let base_armor = host.target.armor();
        let script = scripted(
            0,
            ScriptKind::ExclusiveArmorReduction,
            ScriptParams::default(),
        );
        let mut sunder = aura_record(0, AuraType::ModResistance, -450.0, 1);
        sunder.implicit_target = [ImplicitTarget::UnitTargetEnemy, ImplicitTarget::None];
        let sunder = Effect::new(&sunder, &melee_spell(), script, false);
        for _ in 0..5 {
            sunder.apply_aura(&mut host, true);
        }
        let mut expose = sunder.clone();
        expose.spell = 11198;
        expose.set_value(0.0);
        expose.set_cast_bonus(-1800.0);
        expose.apply_aura(&mut host, true);
        assert_eq!(host.target.armor(), base_armor - 2250);
        for _ in 0..5 {
            sunder.remove_aura(&mut host, true);
        }
        assert_eq!(host.target.armor(), base_armor - 1800);
        expose.remove_aura(&mut host, true);
        assert_eq!(host.target.armor(), base_armor);

        let mut hemorrhage = aura_record(2, AuraType::ModSpellDamageFromCaster, 15.0, 0);
        hemorrhage.spell_class_mask = [1 << 20, 0, 0, 0];
        let mut spell = melee_spell();
        spell.class_options = Some(ClassOptions {
            set: 8,
            mask: [0, 0, 0, 0],
        });
        let hemorrhage = Effect::new(&hemorrhage, &spell, None, false);
        let rupture = ClassOptions {
            set: 8,
            mask: [1 << 20, 0, 0, 0],
        };
        hemorrhage.apply_aura(&mut host, true);
        let multiplier = host.modifiers.damage_from_caster_multiplier(Some(&rupture));
        assert!((multiplier - 1.15).abs() < 1e-12);
        assert_eq!(
            host.modifiers
                .damage_from_caster_multiplier(Some(&ClassOptions {
                    set: 8,
                    mask: [1, 0, 0, 0],
                })),
            1.0
        );
        assert!(
            host.modifiers.is_empty(),
            "not a modifier of the spells' own values"
        );
        hemorrhage.remove_aura(&mut host, true);
        assert_eq!(
            host.modifiers.damage_from_caster_multiplier(Some(&rupture)),
            1.0
        );
    }

    #[test]
    fn resource_threat_trigger_and_extra_attack_effects() {
        let mut host = MockHost::new();
        host.rage = 95;
        let mut energize = effect_record(0, SpellEffectName::Energize, 100.0);
        energize.misc_value = [PowerType::Rage.id(), 0];
        let mut energize = Effect::new(&energize, &melee_spell(), None, false);
        let outcome = energize.perform_independent(&mut host, 0, 0);
        assert_eq!(outcome.resource_gained, Some((ResourceType::Rage, 5)));
        assert_eq!(
            energize
                .perform_independent(&mut host, 0, 0)
                .resource_gained,
            None
        );
        // TWO_HAND_ENERGIZE_MULTIPLIER: 1 rage, 2 with a two-hander.
        let mut uw = effect_record(0, SpellEffectName::Energize, 10.0);
        uw.misc_value = [PowerType::Rage.id(), 0];
        let script = EffectScript {
            index: 0,
            script: ScriptKind::TwoHandEnergizeMultiplier,
            params: ScriptParams {
                value: Some(2.0),
                ..ScriptParams::default()
            },
        };
        let mut uw = Effect::new(&uw, &melee_spell(), Some(script), false);
        host.rage = 0;
        let gained = |uw: &mut Effect, host: &mut MockHost| {
            uw.perform_independent(host, 0, 0).resource_gained
        };
        assert_eq!(gained(&mut uw, &mut host), Some((ResourceType::Rage, 1)));
        host.two_hand = true;
        assert_eq!(gained(&mut uw, &mut host), Some((ResourceType::Rage, 2)));
        host.rage = 95;

        let mut combo = effect_record(0, SpellEffectName::Energize, 1.0);
        combo.misc_value = [PowerType::ComboPoints.id(), 0];
        let mut combo = Effect::new(&combo, &melee_spell(), None, false);
        let outcome = combo.perform_independent(&mut host, 0, 0);
        assert!(outcome.success);
        assert_eq!(outcome.resource_gained, None);
        assert_eq!(host.combo_points, 1, "ENERGIZE of combo points grants them");

        let mut threat = effect(SpellEffectName::Threat, 1013.0);
        assert_eq!(threat.perform_independent(&mut host, 0, 0).threat, 1013.0);

        let mut trigger = effect_record(1, SpellEffectName::TriggerSpell, 0.0);
        trigger.trigger_spell = 29131;
        trigger.implicit_target = [ImplicitTarget::UnitCaster, ImplicitTarget::None];
        let mut trigger = Effect::new(&trigger, &melee_spell(), None, false);
        assert_eq!(
            trigger.perform_independent(&mut host, 0, 0).trigger,
            Some(29131)
        );

        let mut extra = effect(SpellEffectName::AddExtraAttacks, 1.0);
        extra.perform_independent(&mut host, 0, 0);
        assert_eq!(host.extra_attacks, 1);

        assert!(
            effect(SpellEffectName::Unknown(38), 1.0)
                .perform_independent(&mut host, 0, 0)
                .success
        );
        assert!(
            aura(AuraType::Unknown(118), -50.0, 127)
                .perform_independent(&mut host, 0, 0)
                .success
        );
    }

    #[test]
    fn stat_auras_apply_and_remove_symmetrically() {
        let mut host = MockHost::new();
        let effects = [
            aura(AuraType::ModDamagePercentDone, 20.0, 1),
            aura(AuraType::ModDamagePercentTaken, 10.0, 127),
            aura(AuraType::ModCritPct, 3.0, 0),
            aura(AuraType::ModWeaponCritPercent, 5.0, 0),
            aura(AuraType::ModAttackPower, 232.0, 0),
            aura(AuraType::ModMeleeHaste3, 30.0, 0),
            aura(AuraType::ModThreat, -20.0, 127),
            aura(AuraType::ModStat, 15.0, 0),
            aura(AuraType::ModHitChance, 2.0, 0),
            aura(AuraType::ModOffhandDamagePct, 25.0, 0),
            aura(AuraType::Unknown(118), -50.0, 127),
        ];
        let sunder = aura(AuraType::ModResistance, -450.0, 1);

        let base_armor = host.target.armor();
        for effect in &effects {
            effect.apply_aura(&mut host, false);
        }
        sunder.apply_aura(&mut host, true);
        assert_eq!(host.stats.get_total_threat_mod(), 0.8);
        assert_eq!(host.stats.get_physical_damage_taken_mod(), 1.1);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.3);
        assert_eq!(host.stats.aura_effects().get_melee_crit_chance(), 800);
        assert_eq!(host.stats.base_stats().get_base_melee_ap(), 232);
        assert_eq!(host.stats.base_stats().get_strength(), 15);
        assert_eq!(host.stats.base_stats().get_melee_hit_chance(), 200);
        assert_eq!(host.target.armor(), base_armor - 450);
        assert_eq!(host.attack_speed_calls, vec![30]);
        assert_eq!(host.offhand_damage, 25);

        for effect in &effects {
            effect.remove_aura(&mut host, false);
        }
        sunder.remove_aura(&mut host, true);
        assert_eq!(host.stats.get_total_threat_mod(), 1.0);
        assert_eq!(host.stats.get_physical_damage_taken_mod(), 1.0);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.0);
        assert_eq!(host.stats.aura_effects().get_melee_crit_chance(), 0);
        assert_eq!(host.stats.base_stats().get_base_melee_ap(), 0);
        assert_eq!(host.stats.base_stats().get_strength(), 0);
        assert_eq!(host.stats.base_stats().get_melee_hit_chance(), 0);
        assert_eq!(host.target.armor(), base_armor);
        assert_eq!(host.attack_speed_calls, vec![30, -30]);
        assert_eq!(host.offhand_damage, 0);
    }

    #[test]
    fn melee_ranged_haste_2_is_attack_speed() {
        // Wind Blessed (1259710): +1 % melee, ranged and casting haste.
        let mut host = MockHost::new();
        let haste = aura(AuraType::ModMeleeRangedHaste2, 1.0, 0);
        haste.apply_aura(&mut host, false);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.01);
        haste.remove_aura(&mut host, false);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.0);
        assert_eq!(host.attack_speed_calls, vec![1, -1]);
    }

    #[test]
    fn debuff_auras_only_touch_the_target_where_it_makes_sense() {
        let mut host = MockHost::new();
        // Defensive Stance passive on the character: threat +30 %; the same aura as a debuff
        // (Thunder Clap's attack speed) leaves the character alone.
        aura(AuraType::ModThreat, 30.0, 127).apply_aura(&mut host, false);
        assert_eq!(host.stats.get_total_threat_mod(), 1.3);
        aura(AuraType::ModMeleeHaste3, -20.0, 0).apply_aura(&mut host, true);
        assert!(host.attack_speed_calls.is_empty());
        aura(AuraType::ModDamagePercentTaken, 10.0, 127).apply_aura(&mut host, true);
        assert_eq!(host.stats.get_physical_damage_taken_mod(), 1.0);
    }

    /// Spell damage auras: the caster's of every magic school (126) or of the mask's schools,
    /// a physical one stays flat physical damage; the target's flat holy damage taken
    /// (Judgement of the Crusader) and percent damage taken of magic schools.
    #[test]
    fn spell_damage_auras() {
        let mut host = MockHost::new();
        let all_magic = aura(AuraType::ModDamageDone, 23.0, 126);
        all_magic.apply_aura(&mut host, false);
        let holy = aura(AuraType::ModDamageDone, 10.0, 2);
        holy.apply_aura(&mut host, false);
        let spell_damage =
            |host: &MockHost, school| host.stats.base_stats().get_spell_damage(school);
        assert_eq!(spell_damage(&host, MagicSchool::Holy), 33);
        assert_eq!(spell_damage(&host, MagicSchool::Fire), 23);
        assert_eq!(host.stats.get_flat_physical_damage_bonus(), 0);
        all_magic.remove_aura(&mut host, false);
        holy.remove_aura(&mut host, false);
        assert_eq!(spell_damage(&host, MagicSchool::Holy), 0);
        let physical = aura(AuraType::ModDamageDone, 8.0, 1);
        physical.apply_aura(&mut host, false);
        assert_eq!(host.stats.get_flat_physical_damage_bonus(), 8);
        assert_eq!(spell_damage(&host, MagicSchool::Holy), 0);

        let mut crusader = aura_record(0, AuraType::ModDamageTaken, 161.0, 2);
        crusader.implicit_target = [ImplicitTarget::UnitTargetEnemy, ImplicitTarget::None];
        let crusader = Effect::new(&crusader, &melee_spell(), None, false);
        crusader.apply_aura(&mut host, true);
        assert_eq!(host.target.spell_damage(MagicSchool::Holy), 161);
        assert_eq!(host.target.spell_damage(MagicSchool::Fire), 0);
        crusader.remove_aura(&mut host, true);
        assert_eq!(host.target.spell_damage(MagicSchool::Holy), 0);

        let curse = aura(AuraType::ModDamagePercentTaken, 10.0, 4 | 16);
        curse.apply_aura(&mut host, true);
        assert!((host.target.magic_school_damage_mod(MagicSchool::Fire) - 1.1).abs() < 1e-12);
        assert!((host.target.magic_school_damage_mod(MagicSchool::Frost) - 1.1).abs() < 1e-12);
        assert_eq!(host.target.magic_school_damage_mod(MagicSchool::Holy), 1.0);
        curse.remove_aura(&mut host, true);
        assert_eq!(host.target.magic_school_damage_mod(MagicSchool::Fire), 1.0);
    }

    #[test]
    fn stance_modifier_and_actionbar_auras() {
        let mut host = MockHost::new();
        aura(AuraType::ModShapeshift, 0.0, 19).apply_aura(&mut host, false);
        assert_eq!(host.stance, Some(Stance::Berserker));
        aura(AuraType::ModShapeshift, 0.0, 19).remove_aura(&mut host, false);
        assert_eq!(
            host.stance,
            Some(Stance::Berserker),
            "removal keeps the stance"
        );
        aura(AuraType::ModShapeshift, 0.0, 16).apply_aura(&mut host, false);
        assert_eq!(
            host.stance,
            Some(Stance::Berserker),
            "Ghost Wolf is not a stance"
        );

        let mut record = aura_record(0, AuraType::AddFlatModifier, -10.0, 14);
        record.spell_class_mask = [64, 0, 0, 0];
        let improved_hs = Effect::new(&record, &melee_spell(), None, false);
        improved_hs.apply_aura(&mut host, false);
        assert_eq!(host.modifiers.len(), 1);
        let modifier = host.modifiers.all()[0];
        assert_eq!(modifier.op, SpellModOp::PowerCost0);
        assert_eq!(modifier.amount, -10.0);
        assert_eq!(modifier.set, 4);
        assert_eq!(modifier.source, 12294);
        assert!(!modifier.pct);
        improved_hs.remove_aura(&mut host, false);
        assert!(host.modifiers.is_empty());

        let mut record = aura_record(0, AuraType::OverrideActionbarSpells, 1310197.0, 1464);
        record.implicit_target = [ImplicitTarget::UnitCaster, ImplicitTarget::None];
        let improved_slam = Effect::new(&record, &melee_spell(), None, false);
        improved_slam.apply_aura(&mut host, false);
        improved_slam.remove_aura(&mut host, false);
        assert_eq!(
            host.overrides,
            vec![(1464, 1310197, true), (1464, 1310197, false)]
        );

        let script = EffectScript {
            index: 0,
            script: ScriptKind::StanceRageRetained,
            params: ScriptParams::default(),
        };
        let tactical = Effect::new(
            &aura_record(0, AuraType::Dummy, 10.0, 0),
            &melee_spell(),
            Some(script),
            false,
        );
        tactical.apply_aura(&mut host, false);
        assert_eq!(host.stance_rage, 10);
        tactical.remove_aura(&mut host, false);
        assert_eq!(host.stance_rage, 0);
        let script = EffectScript {
            index: 1,
            script: ScriptKind::OffhandRagePercent,
            params: ScriptParams::default(),
        };
        let dw = Effect::new(
            &aura_record(1, AuraType::Dummy, 40.0, 0),
            &melee_spell(),
            Some(script),
            false,
        );
        dw.apply_aura(&mut host, false);
        assert_eq!(host.offhand_rage, 40);
        dw.remove_aura(&mut host, false);
        assert_eq!(host.offhand_rage, 0);

        // `value` scales the record's value.
        let script = EffectScript {
            index: 1,
            script: ScriptKind::OffhandRagePercent,
            params: ScriptParams {
                value: Some(0.5),
                ..ScriptParams::default()
            },
        };
        let dw = Effect::new(
            &aura_record(1, AuraType::Dummy, 40.0, 0),
            &melee_spell(),
            Some(script),
            false,
        );
        dw.apply_aura(&mut host, false);
        assert_eq!(host.offhand_rage, 20);

        // A direct effect never applies as an aura.
        effect(SpellEffectName::SchoolDamage, 100.0).apply_aura(&mut host, false);
        assert_eq!(host.stats.get_flat_physical_damage_bonus(), 0);
    }
}
