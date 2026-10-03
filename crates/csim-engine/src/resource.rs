//! Character resources. Port of `Resource/*`.
//!
//! [`Rage`] is the non-regenerating resource. [`Energy`] regenerates lazily on a 0.1 s tick
//! grid: no tick is an event, reads compute the energy at a given time (see [`Energy`]).
//! [`Mana`] and [`Focus`] keep the C++ per-tick amounts of `RegeneratingResource` until a class
//! that uses them is ported; nothing ticks them yet. The C++ virtual hierarchy is a closed
//! [`Resource`] enum.

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

/// Seconds between mana regeneration ticks.
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

/// Seconds between two energy ticks at the base rate: 10 Hz, one energy per tick (the C++
/// ticked 20 energy every 2 s).
pub const ENERGY_TICK_INTERVAL: f64 = 0.1;

/// Tolerance, in ticks, of the tick counting: `k × interval` carries float noise, and a read at
/// the time of a tick must see it.
const TICK_EPSILON: f64 = 1e-9;
/// Tolerance, in seconds, of the reaction time comparisons.
const TIME_EPSILON: f64 = 1e-9;
/// How many of the latest gaining ticks are remembered for their pending reactions: enough for
/// every tick of the last reaction delay up to +300 % regeneration.
const RECENT_GAINS: usize = 4;

/// Energy: 100 (+ `MOD_INCREASE_ENERGY`), one point per tick on a 0.1 s grid that runs from the
/// pull, faster under `MOD_POWER_REGEN_PERCENT` (Adrenaline Rush).
///
/// Ticks are not events: the energy is evaluated lazily on the tick grid.
/// `current(now) = min(max, settled + ticks since the last settle)`; every change first
/// *settles* (moves the elapsed whole ticks into `settled`), so the phase of the grid and the
/// progress of the running tick survive spending and gaining. Ticks at the cap are lost, as in
/// game, and counted. A rate change settles at the old rate and keeps the fraction of the
/// running tick.
#[derive(Debug, Clone)]
pub struct Energy {
    settled: u32,
    /// Origin of the grid at the current rate: ticks fall at `epoch + k × interval`.
    epoch: f64,
    /// The grid index of the last tick moved into `settled`; `None` while the energy has been
    /// full and untouched since the reset (it then reads as the maximum, whatever it becomes).
    ticks_done: Option<i64>,
    interval: f64,
    /// Σ `MOD_POWER_REGEN_PERCENT` of the energy.
    regen_percent: i32,
    /// Σ `MOD_INCREASE_ENERGY`.
    max_bonus: i32,
    /// Energy gained from ticks since the counters were last taken.
    regenerated: u64,
    /// Ticks lost at the cap since the counters were last taken.
    lost_at_cap: u64,
    /// Times of the latest ticks that gained energy, oldest first (`NEG_INFINITY` when unused):
    /// their reactions may still be ahead after they were settled.
    recent_gains: [f64; RECENT_GAINS],
}

impl Default for Energy {
    fn default() -> Self {
        Self {
            settled: Self::BASE_MAX,
            epoch: 0.0,
            ticks_done: None,
            interval: ENERGY_TICK_INTERVAL,
            regen_percent: 0,
            max_bonus: 0,
            regenerated: 0,
            lost_at_cap: 0,
            recent_gains: [f64::NEG_INFINITY; RECENT_GAINS],
        }
    }
}

impl Energy {
    pub const BASE_MAX: u32 = 100;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn max(&self) -> u32 {
        (Self::BASE_MAX as i32 + self.max_bonus).max(0) as u32
    }

    /// Seconds between two ticks at the current rate.
    pub fn interval(&self) -> f64 {
        self.interval
    }

    pub fn regen_percent(&self) -> i32 {
        self.regen_percent
    }

    /// The time of grid tick `index`.
    fn tick_time(&self, index: i64) -> f64 {
        self.epoch + index as f64 * self.interval
    }

    /// The grid index of the last tick at or before `now`.
    fn elapsed(&self, now: f64) -> i64 {
        ((now - self.epoch) / self.interval + TICK_EPSILON).floor() as i64
    }

    /// Ticks since the last settle, not capped.
    fn pending(&self, now: f64) -> i64 {
        self.ticks_done
            .map_or(0, |done| (self.elapsed(now) - done).max(0))
    }

    /// The energy at `now`.
    pub fn current(&self, now: f64) -> u32 {
        match self.ticks_done {
            None => self.max(),
            Some(_) => {
                let pending = u32::try_from(self.pending(now)).unwrap_or(u32::MAX);
                self.settled.saturating_add(pending).min(self.max())
            }
        }
    }

    /// Moves the ticks up to `now` into the settled energy; ticks beyond the cap are lost.
    fn settle(&mut self, now: f64) {
        let elapsed = self.elapsed(now);
        let Some(done) = self.ticks_done else {
            self.settled = self.max();
            self.ticks_done = Some(elapsed);
            return;
        };
        let pending = elapsed - done;
        if pending <= 0 {
            return;
        }
        let room = i64::from(self.max().saturating_sub(self.settled));
        let gained = pending.min(room);
        // The gaining ticks are the first `gained` ones; remember the latest of them.
        for index in (done + 1 + (gained - RECENT_GAINS as i64).max(0))..=(done + gained) {
            self.recent_gains.rotate_left(1);
            self.recent_gains[RECENT_GAINS - 1] = self.tick_time(index);
        }
        self.settled += gained as u32;
        self.regenerated += gained as u64;
        self.lost_at_cap += (pending - gained) as u64;
        self.ticks_done = Some(elapsed);
    }

    /// Adds energy at `now`, capped at the maximum; returns the energy actually gained.
    pub fn gain(&mut self, amount: u32, now: f64) -> u32 {
        if self.ticks_done.is_none() {
            return 0;
        }
        self.settle(now);
        let gained = amount.min(self.max().saturating_sub(self.settled));
        self.settled += gained;
        gained
    }

    /// Spends energy at `now`.
    ///
    /// # Panics
    /// Panics on underflow.
    pub fn lose(&mut self, amount: u32, now: f64) {
        self.settle(now);
        assert!(self.settled >= amount, "{UNDERFLOW}");
        self.settled -= amount;
    }

    /// Full energy for a new iteration, the grid back on the pull (t = 0). The rate and the
    /// maximum are aura-driven and stay.
    pub fn reset(&mut self) {
        self.settled = self.max();
        self.epoch = 0.0;
        self.ticks_done = None;
        self.recent_gains = [f64::NEG_INFINITY; RECENT_GAINS];
    }

    /// Adds `percent` to the regeneration rate at `now` (`MOD_POWER_REGEN_PERCENT`, Adrenaline
    /// Rush +100 %): the ticks so far count at the old rate and the running tick keeps its
    /// progress.
    pub fn adjust_regen_percent(&mut self, percent: i32, now: f64) {
        if self.ticks_done.is_some() {
            self.settle(now);
        }
        let done = self.ticks_done.unwrap_or_else(|| self.elapsed(now));
        let progress = ((now - self.tick_time(done)) / self.interval).clamp(0.0, 1.0);
        self.regen_percent += percent;
        self.interval = ENERGY_TICK_INTERVAL / (1.0 + f64::from(self.regen_percent) / 100.0);
        self.epoch = now - progress * self.interval;
        if self.ticks_done.is_some() {
            self.ticks_done = Some(0);
        }
    }

    /// Adds `amount` to the maximum at `now` (`MOD_INCREASE_ENERGY`, Vigor); the energy is
    /// clamped when the maximum drops.
    pub fn adjust_max_bonus(&mut self, amount: i32, now: f64) {
        if self.ticks_done.is_some() {
            self.settle(now);
        }
        self.max_bonus += amount;
        self.settled = self.settled.min(self.max());
    }

    /// Seconds from `now` until the energy reaches `amount`: 0 when it already has, infinite
    /// when `amount` is above the maximum.
    pub fn time_until(&self, amount: u32, now: f64) -> f64 {
        if amount <= self.current(now) {
            return 0.0;
        }
        match self.ticks_done {
            Some(done) if amount <= self.max() => {
                let index = done + i64::from(amount - self.settled);
                (self.tick_time(index) - now).max(0.0)
            }
            _ => f64::INFINITY,
        }
    }

    /// The first reaction to a regeneration tick from `now` on (after `now` when `after_now`)
    /// and not before `not_before`: a tick that gains energy is followed by a reaction
    /// `reaction_delay` later (the player notices the energy), a tick at the cap by none.
    /// Covers the settled ticks whose reaction is still ahead and the ticks to come, assuming
    /// nothing else changes the energy.
    pub fn next_reaction(
        &self,
        now: f64,
        after_now: bool,
        not_before: f64,
        reaction_delay: f64,
    ) -> Option<f64> {
        let accepts = |reaction: f64| {
            (if after_now {
                reaction > now
            } else {
                reaction >= now
            }) && reaction >= not_before - TIME_EPSILON
        };
        let recent = self
            .recent_gains
            .iter()
            .map(|tick| tick + reaction_delay)
            .find(|&reaction| accepts(reaction));
        let upcoming = self.ticks_done.and_then(|done| {
            let room = i64::from(self.max().saturating_sub(self.settled));
            if room == 0 {
                return None;
            }
            let lower = now.max(not_before) - reaction_delay;
            let estimate = ((lower - self.epoch) / self.interval - 1e-6).ceil();
            let mut index = (estimate as i64).max(done + 1);
            while !accepts(self.tick_time(index) + reaction_delay) {
                index += 1;
            }
            (index <= done + room).then(|| self.tick_time(index) + reaction_delay)
        });
        match (recent, upcoming) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The counters since they were last taken, settled up to `now`, without taking them.
    pub fn regen_counters(&self, now: f64) -> (u64, u64) {
        self.clone().take_regen_counters(now)
    }

    /// Takes the counters since they were last taken, settled up to `now`: the energy
    /// regenerated and the ticks lost at the cap.
    pub fn take_regen_counters(&mut self, now: f64) -> (u64, u64) {
        if self.ticks_done.is_some() {
            self.settle(now);
        }
        let counters = (self.regenerated, self.lost_at_cap);
        self.regenerated = 0;
        self.lost_at_cap = 0;
        counters
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
///
/// Reads and changes take the engine time: energy regenerates lazily on its tick grid, mana
/// reads it for the five-second rule, rage and focus ignore it.
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

    /// The amount at engine time `now`.
    pub fn current(&self, now: f64) -> u32 {
        match self {
            Resource::Rage(r) => r.current(),
            Resource::Mana(r) => r.current(),
            Resource::Energy(r) => r.current(now),
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

    pub fn is_full(&self, now: f64) -> bool {
        self.current(now) == self.max()
    }

    /// Whether the resource regenerates on a timer (everything but rage).
    pub fn regenerates(&self) -> bool {
        !matches!(self, Resource::Rage(_))
    }

    /// Adds resource at `now`, capped at the maximum; returns the amount actually gained.
    pub fn gain(&mut self, amount: u32, now: f64) -> u32 {
        match self {
            Resource::Rage(r) => r.gain(amount),
            Resource::Mana(r) => r.gain(amount),
            Resource::Energy(r) => r.gain(amount, now),
            Resource::Focus(r) => r.gain(amount),
        }
    }

    /// Gives back a fractional `amount` of a cost already paid (a refund on miss): rage keeps
    /// the tenths, the other resources round to whole points. Returns the amount actually given
    /// back (after the cap).
    pub fn refund(&mut self, amount: f64, now: f64) -> f64 {
        match self {
            Resource::Rage(r) => {
                f64::from(r.gain_tenths(amount * f64::from(Rage::TENTHS))) / f64::from(Rage::TENTHS)
            }
            Resource::Mana(r) => f64::from(r.gain(amount.round() as u32)),
            Resource::Energy(r) => f64::from(r.gain(amount.round() as u32, now)),
            Resource::Focus(r) => f64::from(r.gain(amount.round() as u32)),
        }
    }

    /// Spends resource at engine time `now` (mana starts the five-second rule, energy settles
    /// its ticks).
    ///
    /// # Panics
    /// Panics on underflow.
    pub fn lose(&mut self, amount: u32, now: f64) {
        match self {
            Resource::Rage(r) => r.lose(amount),
            Resource::Mana(r) => r.lose(amount, now),
            Resource::Energy(r) => r.lose(amount, now),
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

    pub fn as_energy(&self) -> Option<&Energy> {
        match self {
            Resource::Energy(r) => Some(r),
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

    /// Energy after a spend of everything at `at`, read at `now`.
    fn drained_at(at: f64) -> Energy {
        let mut energy = Energy::new();
        energy.lose(100, at);
        energy
    }

    #[test]
    fn energy_ticks_ten_per_second_on_the_pull_grid() {
        let energy = drained_at(0.0);
        assert_eq!(energy.current(0.0), 0);
        assert_eq!(energy.current(0.099), 0);
        assert_eq!(
            energy.current(0.1),
            1,
            "spent at 0.0: the first tick is at 0.1"
        );
        assert_eq!(
            energy.current(0.3),
            3,
            "0.3 / 0.1 is 2.999...: the tick still counts"
        );
        assert_eq!(energy.current(1.0), 10);
        assert_eq!(energy.current(4.55), 45);
        assert_eq!(energy.time_until(45, 0.0), 4.5);
        assert_eq!(energy.time_until(0, 0.0), 0.0);
        assert!((energy.time_until(45, 0.25) - 4.25).abs() < 1e-9);

        // Spent in the middle of a tick: the grid does not move.
        let energy = drained_at(0.05);
        assert_eq!(energy.current(0.099), 0);
        assert_eq!(
            energy.current(0.1),
            1,
            "spent at 0.05: the next tick is still at 0.1"
        );
        assert!((energy.time_until(1, 0.05) - 0.05).abs() < 1e-9);

        // Before the pull too: a precombat spend regenerates until 0.
        let energy = drained_at(-1.0);
        assert_eq!(energy.current(-0.95), 0);
        assert_eq!(energy.current(0.0), 10);
    }

    #[test]
    fn energy_spend_and_gain_keep_the_phase() {
        let mut energy = drained_at(0.0);
        // 3 energy at 0.35 (ticks at 0.1, 0.2, 0.3); a spend there does not restart the tick.
        energy.lose(2, 0.35);
        assert_eq!(energy.current(0.35), 1);
        assert_eq!(energy.current(0.399), 1);
        assert_eq!(energy.current(0.4), 2);
        assert_eq!(energy.gain(25, 0.45), 25);
        assert_eq!(energy.current(0.45), 27);
        assert_eq!(energy.current(0.5), 28);
        assert_eq!(energy.take_regen_counters(0.5), (5, 0));
    }

    #[test]
    fn energy_ticks_at_the_cap_are_lost() {
        let mut energy = drained_at(0.0);
        assert_eq!(energy.current(10.0), 100);
        assert_eq!(energy.current(12.0), 100);
        assert_eq!(energy.take_regen_counters(12.0), (100, 20));
        assert_eq!(energy.gain(10, 12.0), 0);
        assert!(energy.time_until(101, 12.0).is_infinite());
        // Full and untouched since the reset: nothing is counted, nothing ticks.
        let mut energy = Energy::new();
        assert_eq!(energy.current(-3.0), 100);
        assert_eq!(energy.take_regen_counters(50.0), (0, 0));
    }

    #[test]
    fn energy_rate_change_keeps_the_running_tick() {
        let mut energy = drained_at(0.0);
        // Adrenaline Rush at 1.04: 10 energy, 40 % into the tick due at 1.1.
        energy.adjust_regen_percent(100, 1.04);
        assert_eq!(energy.interval(), 0.05);
        assert_eq!(energy.current(1.04), 10);
        // The rest of the running tick at the new rate: 60 % of 0.05 s.
        assert_eq!(energy.current(1.069), 10);
        assert_eq!(energy.current(1.07), 11);
        // 20 energy per second from there.
        assert_eq!(energy.current(2.07), 31);
        assert!((energy.time_until(31, 1.04) - 1.03).abs() < 1e-9);
        // Back to 10 per second, 50 % into the tick due at 2.12.
        energy.adjust_regen_percent(-100, 2.095);
        assert_eq!(energy.current(2.095), 31);
        assert_eq!(energy.current(2.144), 31);
        assert_eq!(energy.current(2.145), 32);
        assert_eq!(energy.current(3.145), 42);
        assert_eq!(energy.take_regen_counters(3.145), (42, 0));
    }

    #[test]
    fn energy_max_bonus() {
        let mut energy = Energy::new();
        energy.adjust_max_bonus(10, -1.0);
        assert_eq!(energy.max(), 110);
        assert_eq!(
            energy.current(-1.0),
            110,
            "full at the reset whatever the maximum"
        );
        energy.lose(110, 0.0);
        assert_eq!(energy.current(11.0), 110);
        assert_eq!(energy.current(20.0), 110);
        energy.adjust_max_bonus(-10, 20.0);
        assert_eq!(energy.current(20.0), 100);
        energy.reset();
        assert_eq!(energy.current(0.0), 100);
    }

    #[test]
    fn energy_reactions_follow_the_gaining_ticks() {
        let delay = 0.1;
        let energy = drained_at(0.0);
        let at = |now, not_before| energy.next_reaction(now, true, not_before, delay).unwrap();
        // The tick at 0.1 is noticed at 0.2.
        assert!((at(0.0, 0.0) - 0.2).abs() < 1e-9);
        // At 0.2 itself, the next one is the reaction to the tick at 0.2.
        assert!((at(0.2, 0.0) - 0.3).abs() < 1e-9);
        assert!((at(0.0, 4.55) - 4.6).abs() < 1e-9);
        // The tick that fills the energy (at 10.0) is the last one with a reaction.
        assert!((at(0.0, 10.1) - 10.1).abs() < 1e-9);
        assert_eq!(energy.next_reaction(0.0, true, 10.15, delay), None);

        // A settled tick's reaction is still ahead: the spend at 0.15 settled the tick at 0.1.
        let mut energy = drained_at(0.0);
        energy.lose(1, 0.15);
        assert!((energy.next_reaction(0.15, true, 0.0, delay).unwrap() - 0.2).abs() < 1e-9);
        // A tick at the cap has none.
        let mut energy = Energy::new();
        energy.lose(1, 0.0);
        assert!((energy.next_reaction(0.0, true, 0.0, delay).unwrap() - 0.2).abs() < 1e-9);
        assert_eq!(energy.next_reaction(0.2, true, 0.0, delay), None);
        assert_eq!(Energy::new().next_reaction(0.0, true, 0.0, delay), None);
    }

    #[test]
    fn focus_ticks() {
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
                assert_eq!(resource.current(0.0), 0);
                assert!(!resource.is_full(0.0));
                assert_eq!(resource.gain(40, 0.0), 40);
                assert_eq!(resource.refund(1.55, 0.0), 1.5);
            } else {
                assert!(resource.is_full(0.0));
                assert_eq!(resource.gain(1, 0.0), 0);
                resource.lose(20, 1.0);
                assert_eq!(resource.current(1.0), resource.max() - 20);
                assert_eq!(resource.refund(4.4, 1.0), 4.0);
                assert_eq!(resource.gain(30, 1.0), 16);
                assert!(resource.is_full(1.0));
            }
        }
        let energy = Resource::new(ResourceType::Energy);
        assert!(energy.as_energy().is_some());
    }
}
