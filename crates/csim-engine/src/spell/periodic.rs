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

use crate::combat_roll::MagicResistResult;
use crate::effect::{Effect, EffectHost};
use crate::engine::EventKind;
use crate::ids::SpellId;
use crate::resource::ResourceType;
use crate::spell::SpellHost;
use crate::spell::dbc::AuraType;
use crate::spell::overrides::{EffectScript, ScriptKind};
use crate::spell::record::EffectRecord;

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
    /// `PERIODIC_DAMAGE`: `per_tick` damage per stack of the aura (times the spell's periodic
    /// damage modifier) on each of `ticks` ticks; a refresh re-arms the full count (Rend) and
    /// may add a stack (Deadly Poison).
    Damage { per_tick: f64, ticks: u32 },
    /// `DEEP_WOUNDS_BLEED`: `percent` % of the average base main-hand weapon damage, without
    /// attack power (an off-hand crit's too), as of the crit, per application, dealt in
    /// `ticks_per_application` ticks, times the damage done modifiers of each tick. An
    /// application while the bleed runs adds its damage to what the bleed has left and spreads
    /// the pool over a fresh `ticks_per_application` ticks
    /// (on the running tick chain); the rounding remainder is carried between ticks. Port of
    /// `DeepWounds`, which instead kept the per-tick damage and a stack of ticks per application.
    WeaponDamage {
        percent: f64,
        ticks_per_application: u32,
    },
    /// `PERIODIC_TRIGGER_SPELL`: casts `spell` on every tick. `at_expiry`: the tick due as the
    /// aura expires casts it too (an area's damage: Consecration's 8 ticks over 8 s); a table
    /// trigger's does not.
    TriggerSpell { spell: u32, at_expiry: bool },
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
                    at_expiry: false,
                }
            }
            // A server-side periodic trigger: the spell a `TRIGGER_SPELL` script names
            // (Consecration's ticks of its damage spell).
            AuraType::PeriodicDummy => match effect.script() {
                Some(script) if script.script == ScriptKind::TriggerSpell => {
                    PeriodicKind::TriggerSpell {
                        spell: script.params.spell?,
                        at_expiry: true,
                    }
                }
                _ => return None,
            },
            _ => return None,
        };
        Some((kind, period))
    }

    /// The Deep Wounds bleed: `percent` of the average base main-hand weapon damage over
    /// `duration` seconds in ticks every `period` seconds.
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

/// The crit roll of a damage tick that can crit (`PERIODIC_CAN_CRIT`: Rend).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeriodicCrit {
    /// Crit chance added to the roll, hundredths of a percent (`CRIT_CHANCE` modifiers).
    pub extra: u32,
    /// The damage multiplier of a crit.
    pub multiplier: f64,
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
    /// Whether the damage tick crit (`PERIODIC_CAN_CRIT`: Rend).
    pub crit: bool,
    /// Whether the tick dealt damage of a magic school (and so rolled a partial resist).
    pub magic: bool,
    /// The partial resist of a damage tick of a magic school.
    pub resist: MagicResistResult,
    /// The damage the partial resist took away (not in `damage`).
    pub resisted: u32,
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
    /// The stacks of the aura as of its last application (0 before the first): a damage tick
    /// deals its value once per stack. Kept here because the tick due as the aura expires
    /// runs after the buff dropped its stacks.
    aura_stacks: u32,
    // Deep-Wounds-style state (ticks left in `ticks_left`).
    /// The damage the bleed has left to deal, before the damage done modifiers of its ticks.
    pool: f64,
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
            aura_stacks: 0,
            pool: 0.0,
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

    /// The damage a bleed has left to deal, before the damage done modifiers of its ticks.
    pub fn pool(&self) -> f64 {
        self.pool
    }

    /// Records the aura's stack count after an application or refresh.
    pub fn set_aura_stacks(&mut self, stacks: u32) {
        self.aura_stacks = stacks;
    }

    /// The aura's stack count a damage tick multiplies its value by (at least 1).
    pub fn aura_stacks(&self) -> u32 {
        self.aura_stacks.max(1)
    }

    /// The buff was applied: starts a new tick chain. Port of `SpellPeriodic::start_ticking` +
    /// `new_application_effect`.
    pub fn start(&mut self, spell: SpellId, host: &mut impl SpellHost, kind: &PeriodicKind) {
        self.application_id += 1;
        self.reset_state();
        self.arm(host, kind);
        self.schedule_tick(spell, host);
    }

    /// The buff was refreshed while active: re-arms the effect without restarting the tick chain.
    /// Port of `refresh_effect`.
    pub fn refresh(&mut self, host: &impl SpellHost, kind: &PeriodicKind) {
        self.arm(host, kind);
    }

    fn arm(&mut self, host: &impl SpellHost, kind: &PeriodicKind) {
        match *kind {
            PeriodicKind::ResourceGain { .. } | PeriodicKind::TriggerSpell { .. } => {}
            PeriodicKind::Damage { ticks, .. } => self.ticks_left = ticks,
            PeriodicKind::WeaponDamage {
                percent,
                ticks_per_application,
            } => {
                let weapon = host.avg_mh_weapon_damage();
                self.add_bleed(weapon * percent / 100.0, ticks_per_application);
            }
        }
    }

    /// Adds `damage` to what the bleed has left and spreads it over `ticks` fresh ticks.
    fn add_bleed(&mut self, damage: f64, ticks: u32) {
        self.pool += damage;
        self.ticks_left = ticks;
    }

    /// Handles a `DotTick` event. Returns `None` for stale ticks or after the buff is gone
    /// (which also clears the state). `damage_mod` is every multiplier on the tick's damage
    /// (Improved Rend, Death Wish). `crit` is, for a damage aura whose ticks can crit (Rend),
    /// the extra crit chance of the roll (hundredths of a percent) and the crit damage
    /// multiplier; other ticks never crit. `resource_cost` its cost in displayed units. Port of
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
        crit: Option<PeriodicCrit>,
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
            crit: false,
            magic: false,
            resist: MagicResistResult::NoResist,
            resisted: 0,
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
            // The tick due as the buff expires (queued after its removal) casts nothing: what
            // it would apply outlives the aura that should end it (Jom Gabbar's stacks).
            PeriodicKind::TriggerSpell {
                at_expiry: false, ..
            } if !buff_active => None,
            // The last tick of an area as the aura expires: no tick follows it.
            PeriodicKind::TriggerSpell { spell: trigger, .. } => {
                if buff_active {
                    self.schedule_tick(spell, host);
                }
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
                let stacks = f64::from(self.aura_stacks());
                let mut damage = per_tick * stacks * damage_mod;
                let crit = match crit {
                    Some(c) if damage > 0.0 && host.roll_periodic_crit(c.extra) => {
                        damage *= c.multiplier;
                        true
                    }
                    _ => false,
                };
                let damage = damage.round().max(0.0) as u32;
                Some(TickReport {
                    damage,
                    crit,
                    threat: f64::from(damage) * host.total_threat_mod(),
                    resource_cost: f64::from(resource_cost) / f64::from(ticks),
                    execution_time: host.global_cooldown() / f64::from(ticks),
                    ..quiet
                })
            }
            PeriodicKind::WeaponDamage { .. } => {
                if self.ticks_left == 0 {
                    return None;
                }
                let share = self.pool / f64::from(self.ticks_left);
                self.pool -= share;
                self.ticks_left -= 1;
                let mut damage = share * damage_mod;
                damage += self.previous_tick_rest;
                self.previous_tick_rest = damage - damage.round();
                if self.ticks_left == 0 {
                    self.pool = 0.0;
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
            PeriodicKind::WeaponDamage { .. } => self.ticks_left == 0,
            _ => false,
        }
    }

    /// Clears the tick state (buff gone, iteration reset). Port of `reset_effect`.
    pub fn reset_state(&mut self) {
        self.ticks_left = 0;
        self.aura_stacks = 0;
        self.pool = 0.0;
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
        periodic.add_bleed(60.0, 4);
        periodic.add_bleed(30.0, 4);
        assert_eq!(periodic.pool(), 90.0);
        assert_eq!(periodic.ticks_left(), 4);
        assert!(!periodic.is_exhausted(&kind));
        periodic.reset_state();
        assert_eq!(periodic.pool(), 0.0);
        assert!(periodic.is_exhausted(&kind));
    }

    #[test]
    #[should_panic(expected = "positive")]
    fn zero_tick_rate_panics() {
        let _ = Periodic::new(None, 0.0);
    }
}
