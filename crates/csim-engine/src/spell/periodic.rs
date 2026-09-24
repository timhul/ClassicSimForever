//! Periodic (tick-based) spell behaviour. Port of `Spells/SpellPeriodic.*`,
//! `Class/Common/Spells/PeriodicResourceGainSpell.*`, `Class/Warrior/Spells/Rend.*` and
//! `Class/Warrior/Spells/DeepWounds.*`.
//!
//! In C++ every periodic spell was a `SpellPeriodic` subclass with hand-written
//! `new_application_effect` / `refresh_effect` / `tick_effect`. Here the behaviour is read from
//! the table data: a `PERIODIC_DAMAGE` / `PERIODIC_ENERGIZE` / `PERIODIC_TRIGGER_SPELL` aura on
//! the spell's buff with its `EffectAuraPeriod` ([`PeriodicKind::from_effect`]), or the
//! `DEEP_WOUNDS_BLEED` script of a payload spell ([`PeriodicKind::weapon_damage`]). One
//! [`Periodic`] state machine owned by the spell drives it: applying the buff starts a `DotTick`
//! chain tagged with an application id, refreshing it re-arms the effect, and ticks stop when the
//! buff is gone (stale ticks are ignored by their application id).

use crate::effect::{Effect, EffectHost};
use crate::engine::EventKind;
use crate::ids::SpellId;
use crate::resource::ResourceType;
use crate::spell::dbc::AuraType;
use crate::spell::overrides::{EffectScript, ScriptKind};
use crate::spell::record::EffectRecord;
use crate::spell::SpellHost;

/// The tick period of an aura effect in milliseconds, if it ticks: a periodic aura's
/// `EffectAuraPeriod`, or the `period_ms` of a `PERIODIC_RESOURCE_GAIN` script on a `DUMMY`
/// aura (Anger Management).
pub fn period_ms(record: &EffectRecord, script: Option<&EffectScript>) -> Option<u32> {
    if !record.is_apply_aura() {
        return None;
    }
    if record.is_periodic() && record.aura_period_ms > 0 {
        return Some(record.aura_period_ms);
    }
    match script {
        Some(script) if script.script == ScriptKind::PeriodicResourceGain => {
            script.params.period_ms.filter(|ms| *ms > 0)
        }
        _ => None,
    }
}

/// Tolerance for "the buff expired at this very moment". Port of `almost_equal`.
const TIME_EPSILON: f64 = 0.0001;

/// What a periodic aura does on every tick.
#[derive(Debug, Clone, PartialEq)]
pub enum PeriodicKind {
    /// `PERIODIC_ENERGIZE`: gains `amount` of `resource` per tick for as long as the buff
    /// lasts (Bloodrage's 1 rage per second). Port of `PeriodicResourceGainSpell`.
    ResourceGain { resource: ResourceType, amount: u32 },
    /// `PERIODIC_DAMAGE`: `per_tick` damage (times the spell's periodic damage modifier) on
    /// each of `ticks` ticks; a refresh re-arms the full count (Rend).
    Damage { per_tick: f64, ticks: u32 },
    /// `DEEP_WOUNDS_BLEED`: `percent` % of the average main-hand damage per application, dealt
    /// in `ticks_per_application` equal ticks; every application adds an independent stack of
    /// ticks and the rounding remainder is carried between ticks. Port of `DeepWounds`.
    WeaponDamage {
        percent: f64,
        ticks_per_application: u32,
    },
    /// `PERIODIC_TRIGGER_SPELL`: casts `spell` on every tick.
    TriggerSpell { spell: u32 },
}

impl PeriodicKind {
    /// The periodic behaviour described by an aura effect, with its tick rate in seconds, if it
    /// is a periodic aura. `duration` is the buff's duration (the tick count of a damage aura).
    pub fn from_effect(
        effect: &Effect,
        duration: Option<f64>,
        host: &impl EffectHost,
    ) -> Option<(PeriodicKind, f64)> {
        let record = effect.record();
        let period = f64::from(period_ms(record, effect.script())?) / 1000.0;
        let ticks = duration.map_or(1, |d| (d / period).round().max(1.0) as u32);
        let kind = match effect.aura() {
            AuraType::Dummy => {
                let params = &effect.script()?.params;
                let resource = ResourceType::from_power_type(params.resource?)?;
                PeriodicKind::ResourceGain {
                    resource,
                    amount: effect.effective_value(host).round().max(0.0) as u32,
                }
            }
            AuraType::PeriodicEnergize => {
                let resource = ResourceType::from_power_type(record.power_type())?;
                PeriodicKind::ResourceGain {
                    resource,
                    amount: effect.resource_amount(host, resource),
                }
            }
            AuraType::PeriodicDamage => PeriodicKind::Damage {
                per_tick: effect.effective_value(host),
                ticks,
            },
            AuraType::PeriodicTriggerSpell if record.trigger_spell != 0 => {
                PeriodicKind::TriggerSpell {
                    spell: record.trigger_spell,
                }
            }
            _ => return None,
        };
        Some((kind, period))
    }

    /// The Deep Wounds bleed: `percent` of the average main-hand damage over `duration` seconds
    /// in ticks every `period` seconds.
    pub fn weapon_damage(percent: f64, duration: f64, period: f64) -> (PeriodicKind, f64) {
        let ticks = (duration / period).round().max(1.0) as u32;
        (
            PeriodicKind::WeaponDamage {
                percent,
                ticks_per_application: ticks,
            },
            period,
        )
    }
}

/// What one tick did, for the statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct TickReport {
    /// Damage dealt by the tick (0 for resource ticks).
    pub damage: u32,
    pub threat: f64,
    /// Share of the spell's resource cost attributed to this tick.
    pub resource_cost: f64,
    /// Share of the spell's execution time attributed to this tick.
    pub execution_time: f64,
    pub resource_gained: Option<(ResourceType, u32)>,
    /// A spell to cast on this tick (`PERIODIC_TRIGGER_SPELL`).
    pub trigger: Option<u32>,
}

/// The tick state of one spell's periodic aura. Port of `SpellPeriodic`'s bookkeeping plus the
/// per-subclass state.
#[derive(Debug, Clone, PartialEq)]
pub struct Periodic {
    /// Indices of the periodic aura effects in the buff's effect list: one, or one
    /// `PERIODIC_ENERGIZE` per power type (empty for a scripted bleed, whose kind is fixed at
    /// construction).
    effect_indices: Vec<usize>,
    tick_rate: f64,
    application_id: u32,
    // Rend-style state.
    ticks_left: u32,
    // Deep-Wounds-style state.
    stacks: Vec<u32>,
    previous_tick_rest: f64,
}

impl Periodic {
    pub fn new(effect_index: Option<usize>, tick_rate: f64) -> Self {
        Self::with_effects(effect_index.into_iter().collect(), tick_rate)
    }

    /// A periodic driven by several aura effects of which one applies (see
    /// `effect_indices`).
    pub fn with_effects(effect_indices: Vec<usize>, tick_rate: f64) -> Self {
        assert!(tick_rate > 0.0, "periodic tick rate must be positive");
        Periodic {
            effect_indices,
            tick_rate,
            application_id: 0,
            ticks_left: 0,
            stacks: Vec::new(),
            previous_tick_rest: 0.0,
        }
    }

    /// The first periodic aura effect's index.
    pub fn effect_index(&self) -> Option<usize> {
        self.effect_indices.first().copied()
    }

    pub fn effect_indices(&self) -> &[usize] {
        &self.effect_indices
    }

    pub fn tick_rate(&self) -> f64 {
        self.tick_rate
    }

    /// The id the current tick chain carries; ticks with another id are stale.
    pub fn application_id(&self) -> u32 {
        self.application_id
    }

    pub fn ticks_left(&self) -> u32 {
        self.ticks_left
    }

    pub fn stacks(&self) -> &[u32] {
        &self.stacks
    }

    /// The buff was applied: starts a new tick chain. Port of `SpellPeriodic::start_ticking` +
    /// `new_application_effect`.
    pub fn start(&mut self, spell: SpellId, host: &mut impl SpellHost, kind: &PeriodicKind) {
        self.application_id += 1;
        self.reset_state();
        self.arm(kind);
        self.schedule_tick(spell, host);
    }

    /// The buff was refreshed while active: re-arms the effect without restarting the tick chain.
    /// Port of `refresh_effect`.
    pub fn refresh(&mut self, kind: &PeriodicKind) {
        self.arm(kind);
    }

    fn arm(&mut self, kind: &PeriodicKind) {
        match *kind {
            PeriodicKind::ResourceGain { .. } | PeriodicKind::TriggerSpell { .. } => {}
            PeriodicKind::Damage { ticks, .. } => self.ticks_left = ticks,
            PeriodicKind::WeaponDamage {
                ticks_per_application,
                ..
            } => self.stacks.push(ticks_per_application),
        }
    }

    /// Handles a `DotTick` event. Returns `None` for stale ticks or after the buff is gone
    /// (which also clears the state). `damage_mod` is the spell's periodic damage multiplier
    /// (Improved Rend), `resource_cost` its cost in displayed units. Port of
    /// `SpellPeriodic::perform_periodic` + the `tick_effect` overrides.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        application_id: u32,
        spell: SpellId,
        host: &mut impl SpellHost,
        kind: &PeriodicKind,
        buff_active: bool,
        buff_expired_at: f64,
        damage_mod: f64,
        resource_cost: u32,
    ) -> Option<TickReport> {
        let now = host.engine().current_time();
        if !buff_active && (buff_expired_at - now).abs() >= TIME_EPSILON {
            self.reset_state();
            return None;
        }
        if application_id != self.application_id {
            return None;
        }
        let quiet = TickReport {
            damage: 0,
            threat: 0.0,
            resource_cost: 0.0,
            execution_time: 0.0,
            resource_gained: None,
            trigger: None,
        };

        match *kind {
            PeriodicKind::ResourceGain { resource, amount } => {
                let gained = host.gain_resource(resource, amount);
                self.schedule_tick(spell, host);
                Some(TickReport {
                    resource_gained: (gained > 0).then_some((resource, gained)),
                    ..quiet
                })
            }
            PeriodicKind::TriggerSpell { spell: trigger } => {
                self.schedule_tick(spell, host);
                Some(TickReport {
                    trigger: Some(trigger),
                    ..quiet
                })
            }
            PeriodicKind::Damage { per_tick, ticks } => {
                if self.ticks_left == 0 {
                    return None;
                }
                self.ticks_left -= 1;
                if self.ticks_left > 0 {
                    self.schedule_tick(spell, host);
                }
                let damage = (per_tick * damage_mod).round().max(0.0) as u32;
                Some(TickReport {
                    damage,
                    threat: f64::from(damage) * host.total_threat_mod(),
                    resource_cost: f64::from(resource_cost) / f64::from(ticks),
                    execution_time: host.global_cooldown() / f64::from(ticks),
                    ..quiet
                })
            }
            PeriodicKind::WeaponDamage {
                percent,
                ticks_per_application,
            } => {
                if self.stacks.is_empty() {
                    return None;
                }
                let mut damage = host.avg_mh_damage() * percent / 100.0 * damage_mod
                    / f64::from(ticks_per_application);
                damage += self.previous_tick_rest;
                self.previous_tick_rest = damage - damage.round();
                for stack in &mut self.stacks {
                    *stack -= 1;
                }
                self.stacks.retain(|stack| *stack > 0);
                if self.stacks.is_empty() {
                    self.previous_tick_rest = 0.0;
                } else {
                    self.schedule_tick(spell, host);
                }
                let damage = damage.round().max(0.0) as u32;
                Some(TickReport {
                    damage,
                    threat: f64::from(damage) * host.total_threat_mod(),
                    ..quiet
                })
            }
        }
    }

    /// Whether the periodic still has ticks to deliver (a bleed that ran out of stacks lets its
    /// buff be cancelled).
    pub fn is_exhausted(&self, kind: &PeriodicKind) -> bool {
        match kind {
            PeriodicKind::Damage { .. } => self.ticks_left == 0,
            PeriodicKind::WeaponDamage { .. } => self.stacks.is_empty(),
            _ => false,
        }
    }

    /// Clears the tick state (buff gone, iteration reset). Port of `reset_effect`.
    pub fn reset_state(&mut self) {
        self.ticks_left = 0;
        self.stacks.clear();
        self.previous_tick_rest = 0.0;
    }

    fn schedule_tick(&self, spell: SpellId, host: &mut impl SpellHost) {
        let character = host.character_id();
        host.engine_mut().add_event_in(
            self.tick_rate,
            EventKind::DotTick {
                character,
                spell,
                application_id: self.application_id,
            },
        );
    }
}

/// Spell power coefficient of a pure damage-over-time spell. Port of
/// `SpellPeriodic::get_spell_coefficient_from_duration`.
pub fn spell_coefficient_from_duration(duration: f64) -> f64 {
    (duration / 15.0).clamp(0.0, 1.0)
}

/// Port of `get_spell_coefficient_for_dot_portion_of_hybrid_spell`.
pub fn spell_coefficient_for_dot_portion(duration: f64, cast_time: f64) -> f64 {
    ((duration / 15.0).powi(2) / (cast_time / 3.5 + duration / 15.0)).clamp(0.0, 1.0)
}

/// Port of `get_spell_coefficient_for_instant_portion_of_hybrid_spell`.
pub fn spell_coefficient_for_instant_portion(duration: f64, cast_time: f64) -> f64 {
    ((cast_time / 3.5).powi(2) / (cast_time / 3.5 + duration / 15.0)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn spell_coefficients() {
        assert_relative_eq!(spell_coefficient_from_duration(15.0), 1.0);
        assert_relative_eq!(spell_coefficient_from_duration(7.5), 0.5);
        assert_relative_eq!(spell_coefficient_from_duration(30.0), 1.0);
        assert_relative_eq!(spell_coefficient_from_duration(-1.0), 0.0);
        // Immolate: 1.5s cast, 15s dot.
        assert_relative_eq!(
            spell_coefficient_for_dot_portion(15.0, 1.5),
            1.0 / (1.5 / 3.5 + 1.0)
        );
        assert_relative_eq!(
            spell_coefficient_for_instant_portion(15.0, 1.5),
            (1.5f64 / 3.5).powi(2) / (1.5 / 3.5 + 1.0)
        );
    }

    #[test]
    fn weapon_damage_kind_counts_ticks_from_the_duration() {
        let (kind, rate) = PeriodicKind::weapon_damage(60.0, 12.0, 3.0);
        assert_eq!(rate, 3.0);
        assert_eq!(
            kind,
            PeriodicKind::WeaponDamage {
                percent: 60.0,
                ticks_per_application: 4
            }
        );
        let mut periodic = Periodic::new(None, rate);
        assert!(periodic.is_exhausted(&kind));
        periodic.refresh(&kind);
        periodic.refresh(&kind);
        assert_eq!(periodic.stacks(), [4, 4]);
        assert!(!periodic.is_exhausted(&kind));
        periodic.reset_state();
        assert!(periodic.stacks().is_empty());
    }

    #[test]
    #[should_panic(expected = "positive")]
    fn zero_tick_rate_panics() {
        let _ = Periodic::new(None, 0.0);
    }
}
