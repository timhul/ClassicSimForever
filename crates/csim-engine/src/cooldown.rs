//! Spell cooldowns. Port of `Spells/CooldownControl.*` and the cooldown registry part of
//! `Character/CharacterSpells.*` (`new_cooldown`, the canonical shared-cooldown name).
//!
//! A [`CooldownControl`] is pure bookkeeping (`base`, `last_used`); scheduling the "cooldown is
//! ready" player action goes through the engine explicitly instead of the C++ `Character*` /
//! `Engine*` back-pointers. Spells that share a cooldown (the Warrior stances) share one control,
//! looked up by [`CooldownRegistry::new_cooldown`] under the canonical name.

use std::collections::HashMap;

use crate::engine::{Engine, EventKind};
use crate::ids::{CharId, CooldownId};
use crate::spell::SpellGroupSpec;

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

/// The name under which a spell group's cooldown is registered: spells that share a cooldown
/// register it under the joined names of the whole group. Port of the naming in
/// `CharacterSpells::add_spell_group`.
pub fn canonical_cooldown_name(group: &SpellGroupSpec) -> String {
    if group.cooldown > 0.0 && !group.shared_cooldowns.is_empty() {
        group
            .shared_cooldowns
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("_")
    } else {
        group.name.clone()
    }
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

    /// Registers the cooldown for a spell group under its canonical name.
    pub fn new_cooldown_for_group(&mut self, group: &SpellGroupSpec) -> CooldownId {
        self.new_cooldown(&canonical_cooldown_name(group), group.cooldown)
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
    use crate::spell::SpellFileSpec;

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

    fn groups() -> Vec<std::sync::Arc<SpellGroupSpec>> {
        let file: SpellFileSpec = serde_yaml::from_str(
            r#"
spell_groups:
  - { name: Battle Stance, causes_gcd: stance, cooldown: 1, ranks: [{ rank: 1, resource: rage }] }
  - { name: Defensive Stance, causes_gcd: stance, cooldown: 1, ranks: [{ rank: 1, resource: rage }] }
  - { name: Whirlwind, causes_gcd: normal, cooldown: 10, ranks: [{ rank: 1, resource: rage }] }
  - { name: Heroic Strike, causes_gcd: normal, ranks: [{ rank: 1, resource: rage }] }
shared_spell_cooldowns:
  - [Battle Stance, Defensive Stance]
"#,
        )
        .unwrap();
        let mut db = crate::spell::SpellDb::new();
        db.add_file(file).unwrap();
        db.groups().to_vec()
    }

    #[test]
    fn shared_cooldowns_map_to_one_control() {
        let groups = groups();
        assert_eq!(
            canonical_cooldown_name(&groups[0]),
            "Battle Stance_Defensive Stance"
        );
        assert_eq!(canonical_cooldown_name(&groups[2]), "Whirlwind");

        let mut registry = CooldownRegistry::new();
        let battle = registry.new_cooldown_for_group(&groups[0]);
        let defensive = registry.new_cooldown_for_group(&groups[1]);
        let whirlwind = registry.new_cooldown_for_group(&groups[2]);
        let heroic = registry.new_cooldown_for_group(&groups[3]);
        assert_eq!(battle, defensive);
        assert_ne!(battle, whirlwind);
        assert_eq!(registry.len(), 3);
        assert_eq!(registry.get(whirlwind).base, 10.0);
        assert_eq!(registry.get(heroic).base, 0.0);
        assert_eq!(
            registry.id_by_name("Battle Stance_Defensive Stance"),
            Some(battle)
        );
        assert!(registry.get_by_name("Battle Stance").is_none());

        registry.get_mut(battle).start(3.0);
        assert_eq!(registry.get(defensive).remaining(3.5), 0.5);

        // Re-registering keeps the first base, as in C++.
        assert_eq!(registry.new_cooldown("Whirlwind", 99.0), whirlwind);
        assert_eq!(registry.get(whirlwind).base, 10.0);

        registry.reset_all();
        assert!(registry.get(battle).is_ready(0.0));
    }
}
