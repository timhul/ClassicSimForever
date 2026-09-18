//! The effect interpreter. Port of `Spells/Effect.*`.
//!
//! An [`Effect`] is one [`SpellEffectSpec`] plus its runtime state (last roll, damage dealt,
//! talent that added it). It interprets the effect kind against an [`EffectHost`]: the view of
//! the world a casting spell has (its character, the engine, the target). The host trait is
//! implemented by the spell runtime's context (3.6 / Phase 4); tests use a mock.
//!
//! Compared to the C++ `Effect`, results are reported back instead of being pushed into the
//! spell: whether the effect rolled, which [`PhysicalAttackResult`] it got and how much resource
//! it gained come back in an [`EffectOutcome`], so the spell can update statistics and collect
//! proc sources without the effect holding a `Spell*`.

use crate::combat_roll::{IncludedOutcomes, PhysicalAttackResult};
use crate::resource::ResourceType;
use crate::spell::{SpellEffect, SpellEffectSpec, SpellFlag, SpellResult};
use crate::stance::Stance;
use crate::stats::CharacterStats;
use crate::target::Target;

/// What an effect needs from the world. Port of the `Character` / `CombatRoll` / `Spell` calls
/// made by `Effect.cpp`.
pub trait EffectHost {
    fn combo_points(&self) -> u32;
    fn gain_combo_points(&mut self, amount: u32);
    fn spend_combo_points(&mut self);

    fn resource_level(&self, resource: ResourceType) -> u32;
    /// Gains `amount` of `resource`, returning how much was actually gained (caps).
    fn gain_resource(&mut self, resource: ResourceType, amount: u32) -> u32;

    fn melee_ap(&self) -> u32;
    /// A uniformly random value in `[min, max]`.
    fn random_in_range(&mut self, min: f64, max: f64) -> f64;
    /// Random mainhand damage normalized to the weapon type's standard speed.
    fn random_normalized_mh_dmg(&mut self) -> f64;
    /// Random mainhand damage including the attack power contribution of the weapon speed.
    fn random_non_normalized_mh_dmg(&mut self) -> f64;
    /// Rolls a mainhand melee ability on the special attack table, with `extra_crit` (hundredths
    /// of a percent) added to the character's crit chance for this roll.
    fn roll_melee_ability(
        &mut self,
        included: IncludedOutcomes,
        extra_crit: u32,
    ) -> PhysicalAttackResult;

    fn stats_mut(&mut self) -> &mut CharacterStats;
    fn target_mut(&mut self) -> &mut Target;
    /// Attack speed changes go through the character so pending swings are re-timed.
    fn increase_melee_attack_speed(&mut self, percent: u32);
    fn decrease_melee_attack_speed(&mut self, percent: u32);
    fn swap_stance(&mut self, stance: Stance);
    /// Uses one charge of the named buff of the casting character (`AURA_CONSUME_CHARGE`).
    fn use_buff_charge(&mut self, buff: &str);
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
    /// The spell's resource cost (`SCHOOL_DAMAGE_CONVERT_RAGE`).
    pub resource_cost: u32,
    /// Crit chance added by the spell's talents, hundredths of a percent.
    pub extra_crit: u32,
}

/// What the spell needs to know after an effect was performed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectOutcome {
    /// Whether the effect did its thing (a failed hit check makes it `false`).
    pub success: bool,
    /// The attack table result if this effect made a fresh roll (`None` when it was skipped,
    /// reused the previous result or needs no roll). The spell counts misses, dodges and parries
    /// and collects proc sources from it.
    pub rolled: Option<PhysicalAttackResult>,
    /// Resource gained by the effect, for the resource statistics.
    pub resource_gained: Option<(ResourceType, u32)>,
}

impl EffectOutcome {
    const SKIPPED: EffectOutcome = EffectOutcome {
        success: false,
        rolled: None,
        resource_gained: None,
    };

    fn plain(success: bool) -> Self {
        EffectOutcome {
            success,
            rolled: None,
            resource_gained: None,
        }
    }
}

/// One runtime effect of a spell or buff. Port of `Effect`.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    /// The effect definition; `spec.value` is what talents modify.
    pub spec: SpellEffectSpec,
    /// `spec.value` before any talent modification.
    pub base_value: f64,
    /// The talent whose `add_effect` created this effect, if any.
    pub talent_as_source: Option<String>,
    dependency: Dependency,
    included: IncludedOutcomes,
    /// Result of the last hit check (own roll or inherited).
    pub last_result: Option<PhysicalAttackResult>,
    /// Damage produced by the last perform; the spell collects and zeroes it.
    pub damage_dealt: f64,
    reroll_result: bool,
    effect_success: bool,
}

impl Effect {
    /// Builds the effect at position `index` of its list, for a spell with `flags`.
    pub fn new(spec: SpellEffectSpec, index: usize, flags: &[SpellFlag]) -> Self {
        let dependency = if spec.is_independent(index) {
            Dependency::Independent
        } else {
            Dependency::PartialSuccess
        };
        Effect {
            base_value: spec.value,
            spec,
            talent_as_source: None,
            dependency,
            included: IncludedOutcomes {
                dodge: !flags.contains(&SpellFlag::CannotBeDodged),
                parry: !flags.contains(&SpellFlag::CannotBeParried),
                block: !flags.contains(&SpellFlag::CannotBeBlocked),
                miss: !flags.contains(&SpellFlag::CannotMiss),
            },
            last_result: None,
            damage_dealt: 0.0,
            reroll_result: true,
            effect_success: false,
        }
    }

    /// Builds an effect added by a talent's `add_effect`.
    pub fn from_talent(
        spec: SpellEffectSpec,
        index: usize,
        flags: &[SpellFlag],
        talent: &str,
    ) -> Self {
        let mut effect = Effect::new(spec, index, flags);
        effect.talent_as_source = Some(talent.to_string());
        effect
    }

    pub fn kind(&self) -> SpellEffect {
        self.spec.name
    }

    pub fn value(&self) -> f64 {
        self.spec.value
    }

    pub fn dependency(&self) -> Dependency {
        self.dependency
    }

    pub fn included_outcomes(&self) -> IncludedOutcomes {
        self.included
    }

    pub fn was_successful(&self) -> bool {
        self.effect_success
    }

    /// Performs the effect as part of a chain. `chain.result` is the spell's result so far and
    /// `chain.previous` the first effect's roll, which dependent effects reuse.
    /// Port of `Effect::perform_effect(int)`.
    pub fn perform(&mut self, host: &mut impl EffectHost, chain: &ChainState) -> EffectOutcome {
        let (chain_result, previous) = (chain.result, chain.previous);
        match self.dependency {
            Dependency::Independent => self.reroll_result = true,
            Dependency::FullSuccess => {
                if chain_result != SpellResult::Success {
                    return EffectOutcome::SKIPPED;
                }
                self.reroll_result = false;
                self.last_result = previous;
            }
            Dependency::PartialSuccess => {
                if chain_result == SpellResult::Failure {
                    return EffectOutcome::SKIPPED;
                }
                self.reroll_result = false;
                self.last_result = previous;
            }
        }
        let outcome = self.perform_internal(host, chain.resource_cost, chain.extra_crit);
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
        let value = self.spec.value;
        match self.kind() {
            SpellEffect::AddComboPoints => {
                host.gain_combo_points(value.round() as u32);
                EffectOutcome::plain(true)
            }
            SpellEffect::AuraConsumeCharge => {
                let buff = self
                    .spec
                    .buff
                    .as_deref()
                    .expect("AURA_CONSUME_CHARGE without buff is rejected at load time");
                host.use_buff_charge(buff);
                EffectOutcome::plain(true)
            }
            SpellEffect::ConsumeComboPoints => {
                assert!(
                    host.combo_points() > 0,
                    "Effect attempts to consume zero combo points"
                );
                host.spend_combo_points();
                EffectOutcome::plain(true)
            }
            SpellEffect::GainResourceRage
            | SpellEffect::GainResourceMana
            | SpellEffect::GainResourceEnergy
            | SpellEffect::GainResourceFocus => {
                let resource = match self.kind() {
                    SpellEffect::GainResourceMana => ResourceType::Mana,
                    SpellEffect::GainResourceEnergy => ResourceType::Energy,
                    SpellEffect::GainResourceFocus => ResourceType::Focus,
                    _ => ResourceType::Rage,
                };
                let gained = host.gain_resource(resource, value.round() as u32);
                EffectOutcome {
                    success: true,
                    rolled: None,
                    resource_gained: (gained > 0).then_some((resource, gained)),
                }
            }
            SpellEffect::NormalizedWeaponDamage => {
                let (hit, rolled) = self.roll_mh_melee_ability(host, extra_crit);
                if hit {
                    self.damage_dealt = host.random_normalized_mh_dmg() + value;
                }
                EffectOutcome {
                    success: hit,
                    rolled,
                    resource_gained: None,
                }
            }
            SpellEffect::WeaponDamage => {
                let (hit, rolled) = self.roll_mh_melee_ability(host, extra_crit);
                if hit {
                    self.damage_dealt = host.random_non_normalized_mh_dmg() + value;
                }
                EffectOutcome {
                    success: hit,
                    rolled,
                    resource_gained: None,
                }
            }
            SpellEffect::SchoolDamagePhysical => {
                let (hit, rolled) = self.roll_mh_melee_ability(host, extra_crit);
                if hit {
                    let flat = match (self.spec.min, self.spec.max) {
                        (Some(min), Some(max)) => host.random_in_range(min, max),
                        _ => value,
                    };
                    self.damage_dealt = flat + self.spec.ap_dmg_mod * f64::from(host.melee_ap());
                }
                EffectOutcome {
                    success: hit,
                    rolled,
                    resource_gained: None,
                }
            }
            SpellEffect::SchoolDamageConvertRage => {
                let (hit, rolled) = self.roll_mh_melee_ability(host, extra_crit);
                if hit {
                    let rage = host.resource_level(ResourceType::Rage);
                    self.damage_dealt = f64::from(rage.saturating_sub(resource_cost)) * value;
                }
                EffectOutcome {
                    success: hit,
                    rolled,
                    resource_gained: None,
                }
            }
            // Aura effects do nothing when performed; they act through the buff they belong to.
            SpellEffect::ApplyAuraArmorPenetration
            | SpellEffect::ApplyAuraGenericStat
            | SpellEffect::ApplyAuraMeleeAttackPower
            | SpellEffect::ApplyAuraMeleeAuraCritChance
            | SpellEffect::ApplyAuraModArmor
            | SpellEffect::ApplyAuraModDamageDonePhysical
            | SpellEffect::ApplyAuraModDamageTaken
            | SpellEffect::ApplyAuraModMeleeAttackSpeed
            | SpellEffect::ApplyAuraModResistance
            | SpellEffect::ApplyAuraModThreat
            | SpellEffect::ApplyAuraPeriodicDamageFromWeapon
            | SpellEffect::ApplyAuraPeriodicResourceGainRage
            | SpellEffect::ApplyAuraPeriodicWeaponDamage
            | SpellEffect::ApplyAuraShapeshiftBattleStance
            | SpellEffect::ApplyAuraShapeshiftBerserkerStance
            | SpellEffect::ApplyAuraShapeshiftDefensiveStance
            | SpellEffect::NoEffect => EffectOutcome::plain(true),
            // Not interpreted by the C++ `Effect` either; they succeed without doing anything
            // until the systems they need exist (magic rolls, extra attacks, periodic spells,
            // marker buffs, trinket cooldowns).
            SpellEffect::ApplyMarkerBuff
            | SpellEffect::ExtraAttackInstant
            | SpellEffect::ExtraAttackOnNextSwing
            | SpellEffect::NextBatchResourceLossRage
            | SpellEffect::SchoolDamageArcane
            | SpellEffect::SchoolDamageBlockValue
            | SpellEffect::SchoolDamageFire
            | SpellEffect::SchoolDamageFrost
            | SpellEffect::SchoolDamageHoly
            | SpellEffect::SchoolDamageNature
            | SpellEffect::SchoolDamageShadow
            | SpellEffect::UseTrinket
            | SpellEffect::WindfuryExtraAttack => EffectOutcome::plain(true),
        }
    }

    /// Rolls (or reuses) the mainhand ability result. Returns whether the attack landed and the
    /// fresh roll if one was made. Port of `Effect::roll_mh_melee_ability`.
    fn roll_mh_melee_ability(
        &mut self,
        host: &mut impl EffectHost,
        extra_crit: u32,
    ) -> (bool, Option<PhysicalAttackResult>) {
        if !self.reroll_result {
            let hit = self
                .last_result
                .is_some_and(PhysicalAttackResult::is_success);
            return (hit, None);
        }
        let result = host.roll_melee_ability(self.included, extra_crit);
        self.last_result = Some(result);
        (result.is_success(), Some(result))
    }

    /// Applies the effect as an aura (when its buff is applied). Port of `apply_aura_effect`.
    pub fn apply_aura(&self, host: &mut impl EffectHost) {
        let value = self.spec.value.round() as i32;
        match self.kind() {
            SpellEffect::AddComboPoints => host.gain_combo_points(value.max(0) as u32),
            SpellEffect::ApplyAuraModDamageDonePhysical => {
                host.stats_mut().increase_total_phys_dmg_mod(value);
            }
            SpellEffect::ApplyAuraModDamageTaken => {
                host.stats_mut().add_phys_damage_taken_mod(value)
            }
            SpellEffect::ApplyAuraMeleeAuraCritChance => {
                host.stats_mut()
                    .increase_melee_aura_crit(value.max(0) as u32);
            }
            SpellEffect::ApplyAuraMeleeAttackPower => {
                host.stats_mut().increase_melee_ap(value.max(0) as u32);
            }
            SpellEffect::ApplyAuraGenericStat => {
                let stat = self
                    .spec
                    .stat
                    .expect("APPLY_AURA_GENERIC_STAT without stat is rejected at load time");
                host.stats_mut().increase_stat(stat, value.max(0) as u32);
            }
            SpellEffect::ApplyAuraModMeleeAttackSpeed => {
                host.increase_melee_attack_speed(value.max(0) as u32);
            }
            SpellEffect::ApplyAuraModArmor => change_target_armor(host, value),
            SpellEffect::ApplyAuraModThreat => host.stats_mut().increase_total_threat_mod(value),
            SpellEffect::ApplyAuraShapeshiftBattleStance => host.swap_stance(Stance::Battle),
            SpellEffect::ApplyAuraShapeshiftBerserkerStance => host.swap_stance(Stance::Berserker),
            SpellEffect::ApplyAuraShapeshiftDefensiveStance => host.swap_stance(Stance::Defensive),
            // Not implemented in C++ either ("TODO: Missing implementation for mod resistance").
            SpellEffect::ApplyAuraModResistance => {}
            // Handled by the systems owning them (procs, periodic spells) or not auras at all.
            _ => {}
        }
    }

    /// Reverts [`Effect::apply_aura`]. Port of `remove_aura_effect`.
    pub fn remove_aura(&self, host: &mut impl EffectHost) {
        let value = self.spec.value.round() as i32;
        match self.kind() {
            // Combo points granted by a buff are not taken back when it ends.
            SpellEffect::AddComboPoints => {}
            SpellEffect::ApplyAuraModDamageDonePhysical => {
                host.stats_mut().decrease_total_phys_dmg_mod(value);
            }
            SpellEffect::ApplyAuraModDamageTaken => {
                host.stats_mut().remove_phys_damage_taken_mod(value);
            }
            SpellEffect::ApplyAuraMeleeAuraCritChance => {
                host.stats_mut()
                    .decrease_melee_aura_crit(value.max(0) as u32);
            }
            SpellEffect::ApplyAuraMeleeAttackPower => {
                host.stats_mut().decrease_melee_ap(value.max(0) as u32);
            }
            SpellEffect::ApplyAuraGenericStat => {
                let stat = self
                    .spec
                    .stat
                    .expect("APPLY_AURA_GENERIC_STAT without stat is rejected at load time");
                host.stats_mut().decrease_stat(stat, value.max(0) as u32);
            }
            SpellEffect::ApplyAuraModMeleeAttackSpeed => {
                host.decrease_melee_attack_speed(value.max(0) as u32);
            }
            SpellEffect::ApplyAuraModArmor => change_target_armor(host, -value),
            SpellEffect::ApplyAuraModThreat => host.stats_mut().decrease_total_threat_mod(value),
            // Swapping sets the new stance; the previous one needs no explicit removal.
            SpellEffect::ApplyAuraShapeshiftBattleStance
            | SpellEffect::ApplyAuraShapeshiftBerserkerStance
            | SpellEffect::ApplyAuraShapeshiftDefensiveStance => {}
            SpellEffect::ApplyAuraModResistance => {}
            _ => {}
        }
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
    use crate::spell::SpellEffect as E;
    use std::collections::VecDeque;

    struct MockHost {
        combo_points: u32,
        rage: u32,
        melee_ap: u32,
        rolls: VecDeque<PhysicalAttackResult>,
        rolled_with: Vec<IncludedOutcomes>,
        extra_crits: Vec<u32>,
        stats: CharacterStats,
        target: Target,
        stance: Option<Stance>,
        attack_speed_calls: Vec<i32>,
        charges_used: Vec<String>,
    }

    impl MockHost {
        fn new() -> Self {
            MockHost {
                combo_points: 0,
                rage: 50,
                melee_ap: 1000,
                rolls: VecDeque::new(),
                rolled_with: Vec::new(),
                extra_crits: Vec::new(),
                stats: CharacterStats::new(),
                target: Target::new(63),
                stance: None,
                attack_speed_calls: Vec::new(),
                charges_used: Vec::new(),
            }
        }

        fn with_rolls(mut self, rolls: &[PhysicalAttackResult]) -> Self {
            self.rolls = rolls.iter().copied().collect();
            self
        }
    }

    impl EffectHost for MockHost {
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
        ) -> PhysicalAttackResult {
            self.rolled_with.push(included);
            self.extra_crits.push(extra_crit);
            self.rolls.pop_front().expect("no roll queued")
        }
        fn stats_mut(&mut self) -> &mut CharacterStats {
            &mut self.stats
        }
        fn target_mut(&mut self) -> &mut Target {
            &mut self.target
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
        fn use_buff_charge(&mut self, buff: &str) {
            self.charges_used.push(buff.to_string());
        }
    }

    fn spec(name: E, value: f64) -> SpellEffectSpec {
        let mut spec = SpellEffectSpec::new(name);
        spec.value = value;
        spec
    }

    fn effect(name: E, value: f64) -> Effect {
        Effect::new(spec(name, value), 0, &[])
    }

    #[test]
    fn dependency_and_included_outcomes_come_from_spec_and_flags() {
        let first = Effect::new(spec(E::NoEffect, 0.0), 0, &[]);
        assert_eq!(first.dependency(), Dependency::Independent);
        assert_eq!(first.included_outcomes(), IncludedOutcomes::ALL);

        let second = Effect::new(spec(E::NoEffect, 0.0), 1, &[]);
        assert_eq!(second.dependency(), Dependency::PartialSuccess);

        let mut explicit = spec(E::NoEffect, 0.0);
        explicit.independent = Some(true);
        assert_eq!(
            Effect::new(explicit, 3, &[]).dependency(),
            Dependency::Independent
        );

        let overpower = Effect::new(
            spec(E::NormalizedWeaponDamage, 35.0),
            0,
            &[
                SpellFlag::CannotBeDodged,
                SpellFlag::CannotBeParried,
                SpellFlag::CannotBeBlocked,
            ],
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

        let talent = Effect::from_talent(spec(E::GainResourceRage, 5.0), 1, &[], "Improved");
        assert_eq!(talent.talent_as_source.as_deref(), Some("Improved"));
        assert_eq!(talent.base_value, 5.0);
    }

    #[test]
    fn damage_effects_roll_and_compute_damage() {
        let mut host = MockHost::new().with_rolls(&[
            PhysicalAttackResult::Hit,
            PhysicalAttackResult::Critical,
            PhysicalAttackResult::Hit,
            PhysicalAttackResult::Hit,
        ]);

        let mut normalized = effect(E::NormalizedWeaponDamage, 160.0);
        let outcome = normalized.perform_independent(&mut host, 30, 0);
        assert_eq!(
            outcome,
            EffectOutcome {
                success: true,
                rolled: Some(PhysicalAttackResult::Hit),
                resource_gained: None
            }
        );
        assert_eq!(normalized.damage_dealt, 460.0);
        assert_eq!(normalized.last_result, Some(PhysicalAttackResult::Hit));
        assert!(normalized.was_successful());

        let mut weapon = effect(E::WeaponDamage, 138.0);
        assert!(weapon.perform_independent(&mut host, 15, 0).success);
        assert_eq!(weapon.damage_dealt, 538.0);
        assert_eq!(weapon.last_result, Some(PhysicalAttackResult::Critical));

        let mut bloodthirst = spec(E::SchoolDamagePhysical, 0.0);
        bloodthirst.ap_dmg_mod = 0.45;
        let mut bloodthirst = Effect::new(bloodthirst, 0, &[]);
        assert!(bloodthirst.perform_independent(&mut host, 30, 0).success);
        assert_eq!(bloodthirst.damage_dealt, 450.0);

        let mut revenge = spec(E::SchoolDamagePhysical, 0.0);
        revenge.min = Some(64.0);
        revenge.max = Some(78.0);
        let mut revenge = Effect::new(revenge, 0, &[]);
        assert!(revenge.perform_independent(&mut host, 5, 0).success);
        assert_eq!(revenge.damage_dealt, 71.0);
        assert_eq!(host.rolled_with.len(), 4);
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
            let mut effect = effect(E::SchoolDamagePhysical, 600.0);
            let outcome = effect.perform_independent(&mut host, 15, 0);
            assert!(!outcome.success);
            assert_eq!(outcome.rolled, Some(expected));
            assert_eq!(effect.damage_dealt, 0.0);
            assert!(!effect.was_successful());
        }
    }

    #[test]
    fn convert_rage_uses_rage_above_the_cost() {
        let mut host = MockHost::new().with_rolls(&[PhysicalAttackResult::Hit]);
        host.rage = 80;
        let mut execute = effect(E::SchoolDamageConvertRage, 15.0);
        assert!(execute.perform_independent(&mut host, 15, 0).success);
        assert_eq!(execute.damage_dealt, 975.0);
    }

    #[test]
    fn dependent_effects_reuse_the_first_roll_and_respect_the_chain() {
        let mut host = MockHost::new().with_rolls(&[PhysicalAttackResult::Critical]);
        let mut first = Effect::new(spec(E::SchoolDamagePhysical, 600.0), 0, &[]);
        let mut second = Effect::new(spec(E::SchoolDamageConvertRage, 15.0), 1, &[]);

        let mut chain = ChainState {
            result: SpellResult::Undetermined,
            previous: None,
            resource_cost: 15,
            extra_crit: 2500,
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
        assert_eq!(second.damage_dealt, 525.0);
        assert_eq!(host.rolled_with.len(), 1);

        // A failed chain skips dependent effects entirely.
        let mut third = Effect::new(spec(E::SchoolDamagePhysical, 1.0), 1, &[]);
        chain.result = SpellResult::Failure;
        chain.previous = Some(PhysicalAttackResult::Miss);
        assert_eq!(third.perform(&mut host, &chain), EffectOutcome::SKIPPED);
        assert!(!third.was_successful());

        // A dependent effect inheriting a miss fails without rolling.
        let mut fourth = Effect::new(spec(E::SchoolDamagePhysical, 1.0), 1, &[]);
        chain.result = SpellResult::PartialSuccess;
        let outcome = fourth.perform(&mut host, &chain);
        assert!(!outcome.success);
        assert_eq!(outcome.rolled, None);
        assert_eq!(host.rolled_with.len(), 1);
    }

    #[test]
    fn resource_and_combo_point_effects() {
        let mut host = MockHost::new();
        host.rage = 95;
        let mut gain = effect(E::GainResourceRage, 10.0);
        assert_eq!(
            gain.perform_independent(&mut host, 0, 0),
            EffectOutcome {
                success: true,
                rolled: None,
                resource_gained: Some((ResourceType::Rage, 5))
            }
        );
        assert_eq!(
            gain.perform_independent(&mut host, 0, 0).resource_gained,
            None
        );

        let mut add = effect(E::AddComboPoints, 1.0);
        add.perform_independent(&mut host, 0, 0);
        assert_eq!(host.combo_points, 1);
        let mut consume = effect(E::ConsumeComboPoints, 0.0);
        assert!(consume.perform_independent(&mut host, 0, 0).success);
        assert_eq!(host.combo_points, 0);

        let mut flurry = spec(E::AuraConsumeCharge, 0.0);
        flurry.buff = Some("Flurry".to_string());
        Effect::new(flurry, 0, &[]).perform_independent(&mut host, 0, 0);
        assert_eq!(host.charges_used, vec!["Flurry"]);

        assert!(
            effect(E::NoEffect, 0.0)
                .perform_independent(&mut host, 0, 0)
                .success
        );
    }

    #[test]
    #[should_panic(expected = "zero combo points")]
    fn consuming_without_combo_points_panics() {
        let mut host = MockHost::new();
        effect(E::ConsumeComboPoints, 0.0).perform_independent(&mut host, 0, 0);
    }

    #[test]
    fn aura_effects_apply_and_remove_symmetrically() {
        let mut host = MockHost::new();
        let effects = [
            effect(E::ApplyAuraModDamageDonePhysical, 20.0),
            effect(E::ApplyAuraModDamageTaken, 10.0),
            effect(E::ApplyAuraMeleeAuraCritChance, 300.0),
            effect(E::ApplyAuraMeleeAttackPower, 232.0),
            effect(E::ApplyAuraModMeleeAttackSpeed, 30.0),
            effect(E::ApplyAuraModThreat, -20.0),
            effect(E::ApplyAuraModArmor, -450.0),
            effect(E::ApplyAuraModResistance, -20.0),
        ];
        let mut generic = spec(E::ApplyAuraGenericStat, 15.0);
        generic.stat = Some(crate::item::ItemStat::Strength);
        let generic = Effect::new(generic, 0, &[]);

        let base_armor = host.target.armor();
        for effect in effects.iter().chain(std::iter::once(&generic)) {
            effect.apply_aura(&mut host);
        }
        assert_eq!(host.stats.get_total_threat_mod(), 0.8);
        assert_eq!(host.stats.get_physical_damage_taken_mod(), 1.1);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.3);
        assert_eq!(host.stats.aura_effects().get_melee_crit_chance(), 300);
        assert_eq!(host.stats.base_stats().get_base_melee_ap(), 232);
        assert_eq!(host.stats.base_stats().get_strength(), 15);
        assert_eq!(host.target.armor(), base_armor - 450);
        assert_eq!(host.attack_speed_calls, vec![30]);

        for effect in effects.iter().chain(std::iter::once(&generic)) {
            effect.remove_aura(&mut host);
        }
        assert_eq!(host.stats.get_total_threat_mod(), 1.0);
        assert_eq!(host.stats.get_physical_damage_taken_mod(), 1.0);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.0);
        assert_eq!(host.stats.aura_effects().get_melee_crit_chance(), 0);
        assert_eq!(host.stats.base_stats().get_base_melee_ap(), 0);
        assert_eq!(host.stats.base_stats().get_strength(), 0);
        assert_eq!(host.target.armor(), base_armor);
        assert_eq!(host.attack_speed_calls, vec![30, -30]);
    }

    #[test]
    fn stance_auras_swap_stance_and_combo_point_auras_are_one_way() {
        let mut host = MockHost::new();
        effect(E::ApplyAuraShapeshiftBerserkerStance, 0.0).apply_aura(&mut host);
        assert_eq!(host.stance, Some(Stance::Berserker));
        effect(E::ApplyAuraShapeshiftBerserkerStance, 0.0).remove_aura(&mut host);
        assert_eq!(host.stance, Some(Stance::Berserker));
        effect(E::ApplyAuraShapeshiftDefensiveStance, 0.0).apply_aura(&mut host);
        assert_eq!(host.stance, Some(Stance::Defensive));
        effect(E::ApplyAuraShapeshiftBattleStance, 0.0).apply_aura(&mut host);
        assert_eq!(host.stance, Some(Stance::Battle));

        let overpower = effect(E::AddComboPoints, 1.0);
        overpower.apply_aura(&mut host);
        assert_eq!(host.combo_points, 1);
        overpower.remove_aura(&mut host);
        assert_eq!(host.combo_points, 1);
    }

    #[test]
    fn talent_value_changes_keep_the_base_value() {
        let mut flurry = effect(E::ApplyAuraModMeleeAttackSpeed, 0.0);
        flurry.spec.value += 25.0;
        assert_eq!(flurry.value(), 25.0);
        assert_eq!(flurry.base_value, 0.0);
        let mut host = MockHost::new();
        flurry.apply_aura(&mut host);
        assert_eq!(host.stats.get_melee_attack_speed_mod(), 1.25);
    }
}
