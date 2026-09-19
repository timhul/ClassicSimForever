//! Spell cooldowns. Port of `Spells/CooldownControl.*` and the cooldown registry part of
//! `Character/CharacterSpells.*` (`new_cooldown`, the canonical shared-cooldown name).
//!
//! A [`CooldownControl`] is pure bookkeeping (`base`, `last_used`); scheduling the "cooldown is
//! ready" player action goes through the engine explicitly instead of the C++ `Character*` /
//! `Engine*` back-pointers. A spell has its own control (`SpellCooldowns.RecoveryTime`) and,
//! when it belongs to a `SpellCategories.Category` with a `CategoryRecoveryTime`, shares the
//! category's control with every spell of that category (Mortal Strike / Bloodthirst / Shield
//! Slam, the stances); both are looked up by name in the [`CooldownRegistry`].

use std::collections::HashMap;

use crate::engine::{Engine, EventKind};
use crate::ids::{CharId, CooldownId};

/// One (possibly shared) spell cooldown.
#[derive(Debug, Clone, PartialEq)]
pub struct CooldownControl {
    /// Cooldown length in seconds.
    pub base: f64,
    /// Time the cooldown was last started; `-base` when never used so it is ready at time 0.
    pub last_used: f64,
}

impl CooldownControl {
    pub fn new(base: f64) -> Self {
        CooldownControl {
            base,
            last_used: -base,
        }
    }

    /// Time at which the cooldown is over.
    pub fn next_use(&self) -> f64 {
        self.last_used + self.base
    }

    /// Seconds left at `now`, never negative.
    pub fn remaining(&self, now: f64) -> f64 {
        (self.next_use() - now).max(0.0)
    }

    pub fn is_ready(&self, now: f64) -> bool {
        self.next_use() <= now
    }

    /// Marks the cooldown as started at `now`.
    pub fn start(&mut self, now: f64) {
        self.last_used = now;
    }

    /// Marks the cooldown as started at `now` with a length of `duration` instead of `base`
    /// (cooldown modifiers: Improved Intercept). The control's `base` is left alone.
    pub fn start_for(&mut self, now: f64, duration: f64) {
        self.last_used = now + duration - self.base;
    }

    /// Makes the cooldown ready as if it had never been used.
    pub fn reset(&mut self) {
        self.last_used = -self.base;
    }

    /// Schedules a player action for `character` when the cooldown is over. Port of
    /// `CooldownControl::add_spell_cd_event`.
    pub fn add_spell_cd_event(&self, engine: &mut Engine, character: CharId) {
        engine.add_event_in(self.base, EventKind::PlayerAction { character });
    }
}

/// Schedules a player action for `character` when a global cooldown of `gcd` seconds started now
/// is over. Port of `CooldownControl::add_gcd_event`.
///
/// Returns `false` without scheduling anything while the encounter has not started (negative
/// time: precombat casts do not trigger a global cooldown); the caller starts the character's
/// global cooldown only when `true` is returned.
pub fn add_gcd_event(engine: &mut Engine, character: CharId, gcd: f64) -> bool {
    if engine.current_time() < 0.0 {
        return false;
    }
    engine.add_event_in(gcd, EventKind::PlayerAction { character });
    true
}

/// The registry name of a spell's own cooldown.
pub fn spell_cooldown_name(spell: u32) -> String {
    format!("spell:{spell}")
}

/// The registry name of a shared category cooldown (`SpellCategory.ID`).
pub fn category_cooldown_name(category: u32) -> String {
    format!("category:{category}")
}

/// All cooldown controls of one character, addressed by [`CooldownId`]. Port of
/// `CharacterSpells::cooldowns` / `new_cooldown`.
#[derive(Debug, Clone, Default)]
pub struct CooldownRegistry {
    controls: Vec<CooldownControl>,
    by_name: HashMap<String, CooldownId>,
}

impl CooldownRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the control registered under `name`, creating it with `base` if it is new. The
    /// base of an existing control is not changed (the first registration wins, as in C++).
    pub fn new_cooldown(&mut self, name: &str, base: f64) -> CooldownId {
        if let Some(&id) = self.by_name.get(name) {
            return id;
        }
        let id = CooldownId(u32::try_from(self.controls.len()).expect("too many cooldowns"));
        self.controls.push(CooldownControl::new(base));
        self.by_name.insert(name.to_string(), id);
        id
    }

    /// Registers the own cooldown of `spell` with `base` seconds.
    pub fn new_spell_cooldown(&mut self, spell: u32, base: f64) -> CooldownId {
        self.new_cooldown(&spell_cooldown_name(spell), base)
    }

    /// Registers (or finds) the shared cooldown of `category` with `base` seconds.
    pub fn new_category_cooldown(&mut self, category: u32, base: f64) -> CooldownId {
        self.new_cooldown(&category_cooldown_name(category), base)
    }

    pub fn get(&self, id: CooldownId) -> &CooldownControl {
        &self.controls[id.index()]
    }

    pub fn get_mut(&mut self, id: CooldownId) -> &mut CooldownControl {
        &mut self.controls[id.index()]
    }

    pub fn get_by_name(&self, name: &str) -> Option<&CooldownControl> {
        self.by_name.get(name).map(|&id| self.get(id))
    }

    pub fn id_by_name(&self, name: &str) -> Option<CooldownId> {
        self.by_name.get(name).copied()
    }

    /// Resets every cooldown (start of a combat iteration).
    pub fn reset_all(&mut self) {
        for control in &mut self.controls {
            control.reset();
        }
    }

    pub fn len(&self) -> usize {
        self.controls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EventType;

    #[test]
    fn cooldown_is_ready_at_start_and_tracks_use() {
        let mut cd = CooldownControl::new(30.0);
        assert_eq!(cd.last_used, -30.0);
        assert_eq!(cd.next_use(), 0.0);
        assert!(cd.is_ready(0.0));
        assert!(!cd.is_ready(-1.0));
        assert_eq!(cd.remaining(-1.0), 1.0);
        assert_eq!(cd.remaining(5.0), 0.0);

        cd.start(10.0);
        assert_eq!(cd.next_use(), 40.0);
        assert!(!cd.is_ready(39.9));
        assert!(cd.is_ready(40.0));
        assert_eq!(cd.remaining(25.0), 15.0);
        assert_eq!(cd.remaining(45.0), 0.0);

        cd.reset();
        assert_eq!(cd, CooldownControl::new(30.0));
    }

    #[test]
    fn cooldown_event_is_scheduled_at_ready_time() {
        let mut engine = Engine::new();
        engine.prepare_iteration(12.0);

        let cd = CooldownControl::new(6.0);
        cd.add_spell_cd_event(&mut engine, CharId(2));
        let next = engine.peek().unwrap();
        assert_eq!(next.time, 18.0);
        assert_eq!(
            next.kind,
            EventKind::PlayerAction {
                character: CharId(2)
            }
        );
    }

    #[test]
    fn gcd_event_is_skipped_before_the_encounter_starts() {
        let mut engine = Engine::new();
        engine.prepare_iteration(-2.0);
        assert!(!add_gcd_event(&mut engine, CharId(0), 1.5));
        assert!(engine.peek().is_none());

        engine.prepare_iteration(0.0);
        assert!(add_gcd_event(&mut engine, CharId(0), 1.5));
        let next = engine.peek().unwrap();
        assert_eq!(next.time, 1.5);
        assert_eq!(next.kind.event_type(), EventType::PlayerAction);
    }

    #[test]
    fn spells_and_categories_map_to_controls() {
        let mut registry = CooldownRegistry::new();
        let stances = registry.new_category_cooldown(47, 1.0);
        let stances_again = registry.new_category_cooldown(47, 1.0);
        let whirlwind = registry.new_spell_cooldown(1680, 0.0);
        let whirlwind_category = registry.new_category_cooldown(891, 10.0);
        let heroic = registry.new_spell_cooldown(78, 0.0);
        assert_eq!(stances, stances_again);
        assert_ne!(stances, whirlwind);
        assert_eq!(registry.len(), 4);
        assert_eq!(registry.get(whirlwind_category).base, 10.0);
        assert_eq!(registry.get(heroic).base, 0.0);
        assert_eq!(registry.id_by_name("category:47"), Some(stances));
        assert_eq!(registry.id_by_name("spell:78"), Some(heroic));
        assert!(registry.get_by_name("Battle Stance").is_none());

        registry.get_mut(stances).start(3.0);
        assert_eq!(registry.get(stances_again).remaining(3.5), 0.5);

        registry.get_mut(whirlwind_category).start_for(3.0, 8.0);
        assert_eq!(registry.get(whirlwind_category).next_use(), 11.0);
        assert_eq!(registry.get(whirlwind_category).base, 10.0);

        // Re-registering keeps the first base, as in C++.
        assert_eq!(registry.new_cooldown("spell:1680", 99.0), whirlwind);
        assert_eq!(registry.get(whirlwind).base, 0.0);

        registry.reset_all();
        assert!(registry.get(stances).is_ready(0.0));
        assert!(registry.get(whirlwind_category).is_ready(0.0));
    }
}
