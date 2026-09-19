//! Periodic (tick-based) spell behaviour. Port of `Spells/SpellPeriodic.*`,
//! `Class/Common/Spells/PeriodicResourceGainSpell.*`, `Class/Warrior/Spells/Rend.*` and
//! `Class/Warrior/Spells/DeepWounds.*`.
//!
//! In C++ every periodic spell was a `SpellPeriodic` subclass with hand-written
//! `new_application_effect` / `refresh_effect` / `tick_effect`. Here the periodic behaviour is
//! read from the marker buff's periodic aura effect ([`PeriodicKind`], re-read on every use so
//! talent changes to the effect apply) and driven by one [`Periodic`] state machine owned by the
//! spell: applying the buff starts a `DotTick` chain tagged with an application id, refreshing
//! it re-arms the effect, and ticks stop when the buff is gone (stale ticks are ignored by their
//! application id).

use crate::effect::Effect;
use crate::engine::EventKind;
use crate::ids::SpellId;
use crate::resource::ResourceType;
use crate::spell::{SpellEffect, SpellHost};

/// Tolerance for "the buff expired at this very moment". Port of `almost_equal`.
const TIME_EPSILON: f64 = 0.0001;

/// What a periodic aura does on every tick.
#[derive(Debug, Clone, PartialEq)]
pub enum PeriodicKind {
    /// `APPLY_AURA_PERIODIC_RESOURCE_GAIN_*`: gains `amount` of `resource` per tick for as long
    /// as the buff lasts. Port of `PeriodicResourceGainSpell`.
    ResourceGain { resource: ResourceType, amount: u32 },
    /// `APPLY_AURA_PERIODIC_DAMAGE_FROM_WEAPON`: `base + avg_mh_damage × duration ×
    /// weapon_coeff` (times the spell's damage modifier) spread evenly over `ticks` ticks; a
    /// refresh re-arms the full amount. Port of `Rend`.
    DamageFromWeapon {
        base: f64,
        weapon_coeff: f64,
        ticks: u32,
    },
    /// `APPLY_AURA_PERIODIC_WEAPON_DAMAGE`: `percent`% of average mainhand damage per
    /// application, dealt in `ticks_per_application` equal ticks; every application adds an
    /// independent stack of ticks and the rounding remainder is carried between ticks. Port of
    /// `DeepWounds`.
    WeaponDamage {
        percent: f64,
        ticks_per_application: u32,
    },
}

impl PeriodicKind {
    /// The periodic behaviour described by a buff effect, if it is a periodic aura.
    /// `buff_duration` supplies the tick count where the effect gives only a period.
    pub fn from_effect(effect: &Effect, buff_duration: Option<f64>) -> Option<(PeriodicKind, f64)> {
        let spec = &effect.spec;
        let period = spec.period.or(spec.tick_rate);
        match effect.kind() {
            SpellEffect::ApplyAuraPeriodicResourceGainRage => Some((
                PeriodicKind::ResourceGain {
                    resource: ResourceType::Rage,
                    amount: spec.value.round().max(0.0) as u32,
                },
                period.expect("periodic resource gain needs tick_rate (validated at load)"),
            )),
            SpellEffect::ApplyAuraPeriodicDamageFromWeapon => {
                let period = period.expect("periodic damage needs period (validated at load)");
                let ticks = spec
                    .ticks
                    .or_else(|| buff_duration.map(|d| (d / period).round() as u32))
                    .expect("periodic damage needs ticks or a buff duration (validated at load)");
                Some((
                    PeriodicKind::DamageFromWeapon {
                        base: spec.value,
                        weapon_coeff: spec.weapon_coeff.unwrap_or(0.0),
                        ticks,
                    },
                    period,
                ))
            }
            SpellEffect::ApplyAuraPeriodicWeaponDamage => {
                let period = period.expect("periodic damage needs period (validated at load)");
                let ticks = spec
                    .ticks
                    .or_else(|| buff_duration.map(|d| (d / period).round() as u32))
                    .expect("periodic damage needs ticks or a buff duration (validated at load)");
                Some((
                    PeriodicKind::WeaponDamage {
                        percent: spec.value,
                        ticks_per_application: ticks,
                    },
                    period,
                ))
            }
            _ => None,
        }
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
}

/// The tick state of one spell's periodic aura. Port of `SpellPeriodic`'s bookkeeping plus the
/// per-subclass state.
#[derive(Debug, Clone, PartialEq)]
pub struct Periodic {
    /// Index of the periodic aura effect in the marker buff's effect list.
    effect_index: usize,
    tick_rate: f64,
    application_id: u32,
    // Rend-style state.
    damage_remaining: f64,
    ticks_left: u32,
    // Deep-Wounds-style state.
    stacks: Vec<u32>,
    previous_tick_rest: f64,
}

impl Periodic {
    pub fn new(effect_index: usize, tick_rate: f64) -> Self {
        assert!(tick_rate > 0.0, "periodic tick rate must be positive");
        Periodic {
            effect_index,
            tick_rate,
            application_id: 0,
            damage_remaining: 0.0,
            ticks_left: 0,
            stacks: Vec::new(),
            previous_tick_rest: 0.0,
        }
    }

    pub fn effect_index(&self) -> usize {
        self.effect_index
    }

    pub fn tick_rate(&self) -> f64 {
        self.tick_rate
    }

    /// The id the current tick chain carries; ticks with another id are stale.
    pub fn application_id(&self) -> u32 {
        self.application_id
    }

    pub fn damage_remaining(&self) -> f64 {
        self.damage_remaining
    }

    pub fn ticks_left(&self) -> u32 {
        self.ticks_left
    }

    pub fn stacks(&self) -> &[u32] {
        &self.stacks
    }

    /// The buff was applied: starts a new tick chain. Port of `SpellPeriodic::start_ticking` +
    /// `new_application_effect`.
    pub fn start(
        &mut self,
        spell: SpellId,
        host: &mut impl SpellHost,
        kind: &PeriodicKind,
        damage_mod: f64,
    ) {
        self.application_id += 1;
        self.reset_state();
        self.arm(host, kind, damage_mod);
        self.schedule_tick(spell, host);
    }

    /// The buff was refreshed while active: re-arms the effect without restarting the tick chain.
    /// Port of `refresh_effect`.
    pub fn refresh(&mut self, host: &mut impl SpellHost, kind: &PeriodicKind, damage_mod: f64) {
        self.arm(host, kind, damage_mod);
    }

    fn arm(&mut self, host: &mut impl SpellHost, kind: &PeriodicKind, damage_mod: f64) {
        match *kind {
            PeriodicKind::ResourceGain { .. } => {}
            PeriodicKind::DamageFromWeapon {
                base,
                weapon_coeff,
                ticks,
            } => {
                let duration = self.tick_rate * f64::from(ticks);
                self.damage_remaining =
                    (base + host.avg_mh_damage() * duration * weapon_coeff) * damage_mod;
                self.ticks_left = ticks;
            }
            PeriodicKind::WeaponDamage {
                ticks_per_application,
                ..
            } => self.stacks.push(ticks_per_application),
        }
    }

    /// Handles a `DotTick` event. Returns `None` for stale ticks, ticks of a disabled spell or
    /// after the buff is gone (which also clears the state). Port of
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

        match *kind {
            PeriodicKind::ResourceGain { resource, amount } => {
                let gained = host.gain_resource(resource, amount);
                self.schedule_tick(spell, host);
                Some(TickReport {
                    damage: 0,
                    threat: 0.0,
                    resource_cost: 0.0,
                    execution_time: 0.0,
                    resource_gained: (gained > 0).then_some((resource, gained)),
                })
            }
            PeriodicKind::DamageFromWeapon { ticks, .. } => {
                if self.ticks_left == 0 {
                    return None;
                }
                let damage = (self.damage_remaining / f64::from(self.ticks_left)).round();
                self.damage_remaining -= damage;
                self.ticks_left -= 1;
                if self.ticks_left > 0 {
                    self.schedule_tick(spell, host);
                }
                let damage = damage.max(0.0) as u32;
                Some(TickReport {
                    damage,
                    threat: f64::from(damage) * host.total_threat_mod(),
                    resource_cost: f64::from(resource_cost) / f64::from(ticks),
                    execution_time: host.global_cooldown() / f64::from(ticks),
                    resource_gained: None,
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
                    resource_cost: 0.0,
                    execution_time: 0.0,
                    resource_gained: None,
                })
            }
        }
    }

    /// Clears the tick state (buff gone, iteration reset). Port of `reset_effect`.
    pub fn reset_state(&mut self) {
        self.damage_remaining = 0.0;
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
    #[should_panic(expected = "positive")]
    fn zero_tick_rate_panics() {
        let _ = Periodic::new(0, 0.0);
    }
}
