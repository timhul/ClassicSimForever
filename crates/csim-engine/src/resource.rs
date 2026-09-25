//! Character resources. Port of `Resource/*`.
//!
//! [`Rage`] is the non-regenerating resource; [`Mana`], [`Energy`] and [`Focus`] port
//! `RegeneratingResource` without the engine coupling: they compute one tick's worth of resource
//! and the character (which owns the `ResourceTick` event) decides when to call [`Resource::tick`].
//! The C++ virtual hierarchy is a closed [`Resource`] enum.

use serde::{Deserialize, Serialize};

use crate::spell::dbc::PowerType;
use crate::stats::MultiplicativeStack;

/// The resource a spell costs / a character uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    Mana,
    Rage,
    Energy,
    Focus,
}

impl ResourceType {
    pub const ALL: [ResourceType; 4] = [
        ResourceType::Mana,
        ResourceType::Rage,
        ResourceType::Energy,
        ResourceType::Focus,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ResourceType::Mana => "Mana",
            ResourceType::Rage => "Rage",
            ResourceType::Energy => "Energy",
            ResourceType::Focus => "Focus",
        }
    }

    /// The resource behind a table power type; `None` for health, combo points and the power
    /// types no supported class uses.
    pub fn from_power_type(power: PowerType) -> Option<ResourceType> {
        match power {
            PowerType::Mana => Some(ResourceType::Mana),
            PowerType::Rage => Some(ResourceType::Rage),
            PowerType::Focus => Some(ResourceType::Focus),
            PowerType::Energy => Some(ResourceType::Energy),
            _ => None,
        }
    }

    pub fn power_type(self) -> PowerType {
        match self {
            ResourceType::Mana => PowerType::Mana,
            ResourceType::Rage => PowerType::Rage,
            ResourceType::Energy => PowerType::Energy,
            ResourceType::Focus => PowerType::Focus,
        }
    }

    /// Converts a stored table amount (rage in tenths) to the displayed amount, rounded.
    pub fn from_stored_amount(self, stored: f64) -> u32 {
        (stored / f64::from(self.power_type().display_modifier()))
            .round()
            .max(0.0) as u32
    }
}

const UNDERFLOW: &str = "Underflow decrease RegeneratingResource::lose_resource()";

/// Rage: 0–100, gained from damage dealt and taken, never regenerates. Port of `Resource/Rage.*`.
///
/// Stored in tenths, as the client does, so a swing can add fractional rage (5.54 for a 1.6
/// one-hander). The whole-rage API (`current`, `gain`, `lose`) is what costs and conditions
/// see; `current` floors, so 14.9 rage cannot pay a 15 rage cost.
#[derive(Debug, Clone, Default)]
pub struct Rage {
    tenths: u32,
    /// Sub-tenth remainder of fractional gains, carried to the next gain so the average stays
    /// exact (55.36 tenths per swing lands as 55, 55, 56, 55, 55, 56, ...).
    carry: f64,
    max_mod: MultiplicativeStack,
}

impl Rage {
    pub const BASE_MAX: u32 = 100;
    /// Tenths per displayed rage point.
    pub const TENTHS: u32 = 10;

    pub fn new() -> Self {
        Self::default()
    }

    /// Whole rage, rounded down.
    pub fn current(&self) -> u32 {
        self.tenths / Self::TENTHS
    }

    /// Current rage in tenths.
    pub fn current_tenths(&self) -> u32 {
        self.tenths
    }

    pub fn max(&self) -> u32 {
        (self.max_mod.modifier() * f64::from(Self::BASE_MAX)).round() as u32
    }

    fn max_tenths(&self) -> u32 {
        self.max() * Self::TENTHS
    }

    /// Adds whole rage, capped at the maximum; returns the whole rage actually gained.
    pub fn gain(&mut self, amount: u32) -> u32 {
        let before = self.current();
        self.tenths = (self.tenths + amount * Self::TENTHS).min(self.max_tenths());
        self.current() - before
    }

    /// Adds `tenths` (fractional) of rage, capped at the maximum; the sub-tenth part carries to
    /// the next gain. Returns the tenths actually gained.
    pub fn gain_tenths(&mut self, tenths: f64) -> u32 {
        // The epsilon keeps products like 3.2 × 45 = 143.99999... at 144.
        let total = tenths.max(0.0) + self.carry;
        let whole = (total + 1e-9).floor();
        self.carry = (total - whole).max(0.0);
        let before = self.tenths;
        self.tenths = (self.tenths + whole as u32).min(self.max_tenths());
        self.tenths - before
    }

    /// Spends whole rage; the fraction below one point is kept.
    ///
    /// # Panics
    /// Panics if `amount` exceeds the current rage (the C++ `check`).
    pub fn lose(&mut self, amount: u32) {
        assert!(
            self.current() >= amount,
            "Underflow decrease Rage::lose_resource()"
        );
        self.tenths -= amount * Self::TENTHS;
    }

    pub fn reset(&mut self) {
        self.tenths = 0;
        self.carry = 0.0;
    }

    /// Drops rage above `amount` (a stance change keeps only the Tactical Mastery remainder).
    /// Port of `Warrior::new_stance_effect`.
    pub fn retain_at_most(&mut self, amount: u32) {
        self.tenths = self.tenths.min(amount * Self::TENTHS);
    }

    /// Adds a maximum rage percentage modifier (`MOD_MAX_POWER_PCT`, e.g. Expansive Mind +5).
    pub fn increase_max_mod(&mut self, percent: i32) {
        self.max_mod.add(percent);
        self.tenths = self.tenths.min(self.max_tenths());
    }

    pub fn decrease_max_mod(&mut self, percent: i32) {
        self.max_mod.remove(percent);
        self.tenths = self.tenths.min(self.max_tenths());
    }
}

/// Seconds between regeneration ticks of mana, energy and focus.
pub const REGEN_TICK_RATE: f64 = 2.0;

/// Mana: max from base mana and intellect, regenerates from mp5 and spirit under the five-second
/// rule. Port of `Resource/Mana.*`.
#[derive(Debug, Clone)]
pub struct Mana {
    current: u32,
    max: u32,
    base_mana: u32,
    intellect: u32,
    max_mod: MultiplicativeStack,
    /// Fractional mana carried between ticks.
    remainder: f64,
    /// Engine time of the last mana spend; `-5.0` after a reset so regen starts unhindered.
    last_use: f64,
    /// Fraction of spirit regen that continues inside the five-second rule (talents).
    mp5_from_spirit_within_5sr_modifier: f64,
    ignore_5sr: bool,
    bonus_regen_modifier: f64,
}

impl Default for Mana {
    fn default() -> Self {
        Self {
            current: 0,
            max: 0,
            base_mana: 0,
            intellect: 0,
            max_mod: MultiplicativeStack::default(),
            remainder: 0.0,
            last_use: -5.0,
            mp5_from_spirit_within_5sr_modifier: 0.0,
            ignore_5sr: false,
            bonus_regen_modifier: 1.0,
        }
    }
}

impl Mana {
    pub const MANA_PER_INTELLECT: u32 = 15;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    /// Sets the class base mana and refills (the C++ `set_base_mana` resets).
    pub fn set_base_mana(&mut self, base_mana: u32) {
        self.base_mana = base_mana;
        self.reset();
    }

    /// Recomputes the maximum from the current intellect; the current mana is clamped.
    pub fn update_max(&mut self, intellect: u32) {
        self.intellect = intellect;
        self.max = (self.max_mod.modifier()
            * f64::from(self.base_mana + intellect * Self::MANA_PER_INTELLECT))
        .round() as u32;
        self.current = self.current.min(self.max);
    }

    pub fn gain(&mut self, amount: u32) -> u32 {
        let before = self.current;
        self.current = (self.current + amount).min(self.max);
        self.current - before
    }

    /// Spends mana at engine time `now`, starting the five-second rule.
    ///
    /// # Panics
    /// Panics on underflow.
    pub fn lose(&mut self, amount: u32, now: f64) {
        assert!(self.current >= amount, "{UNDERFLOW}");
        self.current -= amount;
        self.last_use = now;
    }

    pub fn reset(&mut self) {
        self.update_max(self.intellect);
        self.current = self.max;
        self.last_use = -5.0;
        self.remainder = 0.0;
    }

    /// Whether the five-second rule suppresses spirit regeneration at `now`.
    pub fn within_5sr(&self, now: f64) -> bool {
        !self.ignore_5sr && now - self.last_use < 5.0
    }

    /// Mana gained by one tick at engine time `now`: `mp5` from gear plus spirit regen (reduced
    /// inside the five-second rule), scaled to two seconds, carrying the fraction to the next tick.
    pub fn regen_per_tick(&mut self, mp5: f64, mp5_from_spirit: f64, now: f64) -> u32 {
        let spirit_modifier = if self.within_5sr(now) {
            self.mp5_from_spirit_within_5sr_modifier
        } else {
            1.0
        };
        let total_mp5 = mp5 + mp5_from_spirit * spirit_modifier * self.bonus_regen_modifier;
        let mp2 = total_mp5 / 5.0 * REGEN_TICK_RATE + self.remainder;
        self.remainder = mp2 - mp2.floor();
        mp2.floor() as u32
    }

    pub fn increase_max_mod(&mut self, percent: i32) {
        self.max_mod.add(percent);
        self.update_max(self.intellect);
    }

    pub fn decrease_max_mod(&mut self, percent: i32) {
        self.max_mod.remove(percent);
        self.update_max(self.intellect);
    }

    pub fn set_mp5_from_spirit_within_5sr_modifier(&mut self, modifier: f64) {
        self.mp5_from_spirit_within_5sr_modifier = modifier;
    }

    pub fn set_ignore_5sr(&mut self, ignore: bool) {
        self.ignore_5sr = ignore;
    }

    pub fn set_bonus_regen_modifier(&mut self, modifier: f64) {
        self.bonus_regen_modifier = modifier;
    }
}

/// Energy: 100 (+ set bonuses), 20 per tick, 40 under Adrenaline Rush. Port of `Resource/Energy.*`.
#[derive(Debug, Clone)]
pub struct Energy {
    current: u32,
    per_tick: u32,
    max_bonus: u32,
}

impl Default for Energy {
    fn default() -> Self {
        Self {
            current: Self::BASE_MAX,
            per_tick: Self::BASE_PER_TICK,
            max_bonus: 0,
        }
    }
}

impl Energy {
    pub const BASE_MAX: u32 = 100;
    pub const BASE_PER_TICK: u32 = 20;
    pub const DOUBLED_PER_TICK: u32 = 40;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn max(&self) -> u32 {
        Self::BASE_MAX + self.max_bonus
    }

    pub fn per_tick(&self) -> u32 {
        self.per_tick
    }

    pub fn gain(&mut self, amount: u32) -> u32 {
        let before = self.current;
        self.current = (self.current + amount).min(self.max());
        self.current - before
    }

    /// # Panics
    /// Panics on underflow.
    pub fn lose(&mut self, amount: u32) {
        assert!(self.current >= amount, "{UNDERFLOW}");
        self.current -= amount;
    }

    pub fn reset(&mut self) {
        self.current = self.max();
        self.per_tick = Self::BASE_PER_TICK;
    }

    pub fn increase_energy_per_tick(&mut self) {
        self.per_tick = Self::DOUBLED_PER_TICK;
    }

    pub fn decrease_energy_per_tick(&mut self) {
        self.per_tick = Self::BASE_PER_TICK;
    }

    pub fn increase_max_bonus(&mut self, bonus: u32) {
        self.max_bonus += bonus;
    }

    pub fn decrease_max_bonus(&mut self, bonus: u32) {
        self.max_bonus -= bonus;
        self.current = self.current.min(self.max());
    }
}

/// Focus: 100, 20–24 per tick. Port of `Resource/Focus.*`.
#[derive(Debug, Clone)]
pub struct Focus {
    current: u32,
    per_tick: u32,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            current: Self::MAX,
            per_tick: Self::MIN_PER_TICK,
        }
    }
}

impl Focus {
    pub const MAX: u32 = 100;
    pub const MIN_PER_TICK: u32 = 20;
    pub const MAX_PER_TICK: u32 = 24;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    pub fn max(&self) -> u32 {
        Self::MAX
    }

    pub fn per_tick(&self) -> u32 {
        self.per_tick
    }

    pub fn gain(&mut self, amount: u32) -> u32 {
        let before = self.current;
        self.current = (self.current + amount).min(Self::MAX);
        self.current - before
    }

    /// # Panics
    /// Panics on underflow.
    pub fn lose(&mut self, amount: u32) {
        assert!(self.current >= amount, "{UNDERFLOW}");
        self.current -= amount;
    }

    pub fn reset(&mut self) {
        self.current = Self::MAX;
    }

    /// # Panics
    /// Panics beyond the 24 cap.
    pub fn increase_focus_gain(&mut self) {
        self.per_tick += 2;
        assert!(
            self.per_tick <= Self::MAX_PER_TICK,
            "Focus gain increased beyond cap"
        );
    }

    /// # Panics
    /// Panics below the 20 floor.
    pub fn decrease_focus_gain(&mut self) {
        assert!(
            self.per_tick > Self::MIN_PER_TICK,
            "Focus gain reduced below min"
        );
        self.per_tick -= 2;
    }
}

/// A character's resource. Replaces the C++ `Resource` virtual base.
#[derive(Debug, Clone)]
pub enum Resource {
    Rage(Rage),
    Mana(Mana),
    Energy(Energy),
    Focus(Focus),
}

impl Resource {
    pub fn new(resource_type: ResourceType) -> Self {
        match resource_type {
            ResourceType::Mana => Resource::Mana(Mana::new()),
            ResourceType::Rage => Resource::Rage(Rage::new()),
            ResourceType::Energy => Resource::Energy(Energy::new()),
            ResourceType::Focus => Resource::Focus(Focus::new()),
        }
    }

    pub fn resource_type(&self) -> ResourceType {
        match self {
            Resource::Rage(_) => ResourceType::Rage,
            Resource::Mana(_) => ResourceType::Mana,
            Resource::Energy(_) => ResourceType::Energy,
            Resource::Focus(_) => ResourceType::Focus,
        }
    }

    pub fn current(&self) -> u32 {
        match self {
            Resource::Rage(r) => r.current(),
            Resource::Mana(r) => r.current(),
            Resource::Energy(r) => r.current(),
            Resource::Focus(r) => r.current(),
        }
    }

    pub fn max(&self) -> u32 {
        match self {
            Resource::Rage(r) => r.max(),
            Resource::Mana(r) => r.max(),
            Resource::Energy(r) => r.max(),
            Resource::Focus(r) => r.max(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.current() == self.max()
    }

    /// Whether the resource regenerates on a timer (everything but rage).
    pub fn regenerates(&self) -> bool {
        !matches!(self, Resource::Rage(_))
    }

    /// Adds resource, capped at the maximum; returns the amount actually gained.
    pub fn gain(&mut self, amount: u32) -> u32 {
        match self {
            Resource::Rage(r) => r.gain(amount),
            Resource::Mana(r) => r.gain(amount),
            Resource::Energy(r) => r.gain(amount),
            Resource::Focus(r) => r.gain(amount),
        }
    }

    /// Spends resource at engine time `now` (only mana reads the time, for the five-second rule).
    ///
    /// # Panics
    /// Panics on underflow.
    pub fn lose(&mut self, amount: u32, now: f64) {
        match self {
            Resource::Rage(r) => r.lose(amount),
            Resource::Mana(r) => r.lose(amount, now),
            Resource::Energy(r) => r.lose(amount),
            Resource::Focus(r) => r.lose(amount),
        }
    }

    /// Rage empties, the regenerating resources fill.
    pub fn reset(&mut self) {
        match self {
            Resource::Rage(r) => r.reset(),
            Resource::Mana(r) => r.reset(),
            Resource::Energy(r) => r.reset(),
            Resource::Focus(r) => r.reset(),
        }
    }

    /// One regeneration tick at engine time `now`; returns the amount gained. Mana needs the
    /// character's `mp5` and spirit-based mp5, which the other resources ignore. Rage never ticks.
    ///
    /// # Panics
    /// Panics for rage (the C++ `check`).
    pub fn tick(&mut self, now: f64, mp5: f64, mp5_from_spirit: f64) -> u32 {
        match self {
            Resource::Rage(_) => panic!("Rage is not a regenerating resource"),
            Resource::Mana(r) => {
                let amount = r.regen_per_tick(mp5, mp5_from_spirit, now);
                r.gain(amount)
            }
            Resource::Energy(r) => r.gain(r.per_tick()),
            Resource::Focus(r) => r.gain(r.per_tick()),
        }
    }

    pub fn as_rage_mut(&mut self) -> Option<&mut Rage> {
        match self {
            Resource::Rage(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_mana_mut(&mut self) -> Option<&mut Mana> {
        match self {
            Resource::Mana(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_energy_mut(&mut self) -> Option<&mut Energy> {
        match self {
            Resource::Energy(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_focus_mut(&mut self) -> Option<&mut Focus> {
        match self {
            Resource::Focus(r) => Some(r),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_lowercase_names() {
        assert_eq!(
            serde_yaml::from_str::<ResourceType>("rage").unwrap(),
            ResourceType::Rage
        );
        assert!(serde_yaml::from_str::<ResourceType>("Rage").is_err());
    }

    #[test]
    fn power_types_map_to_resources() {
        assert_eq!(
            ResourceType::from_power_type(PowerType::Rage),
            Some(ResourceType::Rage)
        );
        assert_eq!(ResourceType::from_power_type(PowerType::ComboPoints), None);
        assert_eq!(ResourceType::from_power_type(PowerType::Health), None);
        assert_eq!(ResourceType::Rage.from_stored_amount(300.0), 30);
        assert_eq!(ResourceType::Rage.from_stored_amount(15.0), 2);
        assert_eq!(ResourceType::Mana.from_stored_amount(300.0), 300);
        for resource in ResourceType::ALL {
            assert_eq!(
                ResourceType::from_power_type(resource.power_type()),
                Some(resource)
            );
        }
    }

    #[test]
    fn rage_gains_are_capped_and_reported() {
        let mut rage = Rage::new();
        assert_eq!(rage.current(), 0);
        assert_eq!(rage.max(), 100);
        assert_eq!(rage.gain(30), 30);
        assert_eq!(rage.gain(80), 70);
        assert_eq!(rage.current(), 100);
        assert_eq!(rage.gain(1), 0);
        rage.lose(45);
        assert_eq!(rage.current(), 55);
        rage.reset();
        assert_eq!(rage.current(), 0);
    }

    #[test]
    fn fractional_rage_gains_carry_and_floor() {
        let mut rage = Rage::new();
        // 55.36 tenths per gain: the cumulative floor of 55.36 × n.
        let gains: Vec<u32> = (0..5).map(|_| rage.gain_tenths(55.36)).collect();
        assert_eq!(gains, [55, 55, 56, 55, 55]);
        assert_eq!(rage.current_tenths(), 276);
        assert_eq!(rage.current(), 27, "whole rage floors");

        // 14.4 × 10 is 143.99999... in floating point; it still lands as 144.
        let mut rage = Rage::new();
        assert_eq!(rage.gain_tenths(3.2 * 4.5 * 10.0), 144);

        // Spending whole rage keeps the fraction.
        let mut rage = Rage::new();
        rage.gain_tenths(155.0);
        rage.lose(15);
        assert_eq!(rage.current_tenths(), 5);
        assert_eq!(rage.current(), 0);
        rage.gain(1);
        assert_eq!(rage.current_tenths(), 15);

        // Capped at the maximum, in tenths.
        let mut rage = Rage::new();
        rage.gain(99);
        assert_eq!(rage.gain_tenths(55.0), 10);
        assert_eq!(rage.current(), 100);
        rage.retain_at_most(5);
        assert_eq!(rage.current_tenths(), 50);
        rage.reset();
        assert_eq!(rage.current_tenths(), 0);

        // A reset clears the carry.
        let mut rage = Rage::new();
        rage.gain_tenths(0.6);
        rage.reset();
        assert_eq!(rage.gain_tenths(0.6), 0);
    }

    #[test]
    #[should_panic(expected = "Underflow decrease Rage::lose_resource()")]
    fn rage_underflow_panics() {
        let mut rage = Rage::new();
        rage.gain(10);
        rage.lose(11);
    }

    #[test]
    fn rage_max_modifiers() {
        let mut rage = Rage::new();
        rage.increase_max_mod(5);
        assert_eq!(rage.max(), 105);
        assert_eq!(rage.gain(200), 105);
        rage.decrease_max_mod(5);
        assert_eq!(rage.max(), 100);
        assert_eq!(rage.current(), 100, "clamped when the maximum drops");
    }

    #[test]
    fn mana_max_and_regen() {
        let mut mana = Mana::new();
        mana.set_base_mana(1000);
        mana.update_max(100);
        assert_eq!(mana.max(), 2500);
        mana.reset();
        assert_eq!(mana.current(), 2500);

        // Fresh after reset: outside the 5sr, spirit regen counts in full.
        assert!(!mana.within_5sr(0.0));
        assert_eq!(mana.regen_per_tick(25.0, 50.0, 0.0), 30);

        mana.lose(500, 10.0);
        assert_eq!(mana.current(), 2000);
        assert!(mana.within_5sr(14.9));
        assert!(!mana.within_5sr(15.0));
        // Inside the 5sr only gear mp5 ticks (10 per 2 s).
        assert_eq!(mana.regen_per_tick(25.0, 50.0, 11.0), 10);

        // Fractions carry over: 7 mp5 = 2.8 per tick -> 2, 5, 8 cumulative.
        let mut mana = Mana::new();
        mana.set_base_mana(100);
        assert_eq!(mana.regen_per_tick(7.0, 0.0, 0.0), 2);
        assert_eq!(mana.regen_per_tick(7.0, 0.0, 2.0), 3);
        assert_eq!(mana.regen_per_tick(7.0, 0.0, 4.0), 3);

        // Talent modifiers.
        let mut mana = Mana::new();
        mana.set_base_mana(100);
        mana.lose(0, 0.0);
        mana.set_mp5_from_spirit_within_5sr_modifier(0.3);
        assert_eq!(mana.regen_per_tick(0.0, 100.0, 1.0), 12);
        mana.set_ignore_5sr(true);
        assert_eq!(mana.regen_per_tick(0.0, 100.0, 1.0), 40);
        mana.set_bonus_regen_modifier(1.5);
        assert_eq!(mana.regen_per_tick(0.0, 100.0, 1.0), 60);

        // Max modifiers clamp the current amount.
        let mut mana = Mana::new();
        mana.set_base_mana(1000);
        mana.increase_max_mod(10);
        assert_eq!(mana.max(), 1100);
        assert_eq!(mana.current(), 1000);
        mana.reset();
        assert_eq!(mana.current(), 1100);
        mana.decrease_max_mod(10);
        assert_eq!(mana.current(), 1000);
    }

    #[test]
    fn energy_and_focus_ticks() {
        let mut energy = Energy::new();
        assert_eq!(energy.current(), 100);
        energy.lose(60);
        assert_eq!(energy.gain(energy.per_tick()), 20);
        energy.increase_energy_per_tick();
        assert_eq!(energy.gain(energy.per_tick()), 40);
        assert_eq!(energy.gain(energy.per_tick()), 0);
        energy.increase_max_bonus(10);
        assert_eq!(energy.max(), 110);
        assert_eq!(energy.gain(40), 10);
        energy.decrease_max_bonus(10);
        assert_eq!(energy.current(), 100);
        energy.reset();
        assert_eq!(energy.per_tick(), 20);

        let mut focus = Focus::new();
        assert_eq!(focus.current(), 100);
        focus.lose(100);
        focus.increase_focus_gain();
        focus.increase_focus_gain();
        assert_eq!(focus.per_tick(), 24);
        assert_eq!(focus.gain(focus.per_tick()), 24);
        focus.decrease_focus_gain();
        focus.decrease_focus_gain();
        assert_eq!(focus.per_tick(), 20);
        focus.reset();
        assert_eq!(focus.current(), 100);
    }

    #[test]
    #[should_panic(expected = "Focus gain increased beyond cap")]
    fn focus_gain_cap() {
        let mut focus = Focus::new();
        focus.increase_focus_gain();
        focus.increase_focus_gain();
        focus.increase_focus_gain();
    }

    #[test]
    fn resource_enum_dispatch() {
        for resource_type in ResourceType::ALL {
            let mut resource = Resource::new(resource_type);
            assert_eq!(resource.resource_type(), resource_type);
            assert_eq!(resource.regenerates(), resource_type != ResourceType::Rage);
            if let Some(mana) = resource.as_mana_mut() {
                mana.set_base_mana(200);
            }
            resource.reset();
            if resource_type == ResourceType::Rage {
                assert_eq!(resource.current(), 0);
                assert!(!resource.is_full());
                assert_eq!(resource.gain(40), 40);
            } else {
                assert!(resource.is_full());
                assert_eq!(resource.gain(1), 0);
                resource.lose(20, 1.0);
                assert_eq!(resource.current(), resource.max() - 20);
                let gained = resource.tick(3.0, 50.0, 0.0);
                assert_eq!(gained, 20);
                assert!(resource.is_full());
            }
        }
    }

    #[test]
    #[should_panic(expected = "Rage is not a regenerating resource")]
    fn rage_does_not_tick() {
        Resource::new(ResourceType::Rage).tick(0.0, 0.0, 0.0);
    }
}
