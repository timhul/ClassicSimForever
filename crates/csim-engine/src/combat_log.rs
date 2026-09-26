//! The combat log of an iteration: what happened, when, from whom to whom, recorded as
//! structured entries while the simulation runs. Off by default (see
//! [`Engine::enable_combat_log`](crate::engine::Engine::enable_combat_log)).
//!
//! The entries are recorded where the statistics are (`character/context.rs`), so the log shows
//! the same swings, casts, ticks and auras the statistics count.

use crate::ids::CharId;
use crate::resource::ResourceType;
use crate::spell::Hand;

/// A unit taking part in the fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogUnit {
    Character(CharId),
    Target,
}

/// The spell an event is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSpell {
    /// The game's spell id (0 for buffs made in code).
    pub id: u32,
    pub name: String,
    /// The spell school mask (1 = physical).
    pub school: u32,
}

/// The "advanced combat logging" snapshot of the info unit of a damage event. The sim does not
/// model health, position, map or facing; they are logged as zeros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitInfo {
    pub attack_power: u32,
    pub armor: i32,
    /// The power type, `None` for a unit without one.
    pub power: Option<ResourceType>,
    pub current_power: u32,
    pub max_power: u32,
    pub level: u32,
}

/// How an attack that dealt no damage was avoided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissType {
    Miss,
    Dodge,
    Parry,
    Block,
}

/// The damage of one hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Damage {
    /// Damage dealt.
    pub amount: u32,
    pub critical: bool,
    pub glancing: bool,
}

/// What happened to an aura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraChange {
    Applied,
    Refresh,
    /// Another stack; the new stack count.
    AppliedDose(u32),
    Removed,
}

/// One combat log event, by the client's event name.
#[derive(Debug, Clone, PartialEq)]
pub enum CombatLogEvent {
    SwingDamage {
        hand: Hand,
        damage: Damage,
        /// The attacker.
        info: UnitInfo,
    },
    SwingMissed {
        hand: Hand,
        miss: MissType,
    },
    SpellCastSuccess {
        spell: LogSpell,
        /// The caster.
        info: UnitInfo,
    },
    SpellDamage {
        spell: LogSpell,
        damage: Damage,
        /// The victim.
        info: UnitInfo,
    },
    SpellMissed {
        spell: LogSpell,
        miss: MissType,
        offhand: bool,
    },
    SpellPeriodicDamage {
        spell: LogSpell,
        damage: Damage,
        info: UnitInfo,
    },
    SpellEnergize {
        spell: LogSpell,
        power: ResourceType,
        amount: u32,
        periodic: bool,
        /// The unit gaining the power.
        info: UnitInfo,
    },
    SpellAura {
        spell: LogSpell,
        change: AuraChange,
        debuff: bool,
    },
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatLogEntry {
    /// Simulation time in seconds (negative before the pull).
    pub time: f64,
    pub source: LogUnit,
    pub dest: LogUnit,
    pub event: CombatLogEvent,
}

/// The entries of one or more iterations, in the order they happened.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CombatLog {
    entries: Vec<CombatLogEntry>,
}

impl CombatLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[CombatLogEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, entry: CombatLogEntry) {
        self.entries.push(entry);
    }

    /// Inserts `entry` at `index`: a cast is logged before the effects it caused, which were
    /// recorded while it was performed.
    ///
    /// # Panics
    /// Panics if `index` is past the end.
    pub fn insert(&mut self, index: usize, entry: CombatLogEntry) {
        self.entries.insert(index, entry);
    }

    /// The damage of every damage event.
    pub fn total_damage(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| match &entry.event {
                CombatLogEvent::SwingDamage { damage, .. }
                | CombatLogEvent::SpellDamage { damage, .. }
                | CombatLogEvent::SpellPeriodicDamage { damage, .. } => u64::from(damage.amount),
                _ => 0,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Engine, EventKind};

    fn cast(id: u32) -> CombatLogEvent {
        CombatLogEvent::SpellCastSuccess {
            spell: LogSpell {
                id,
                name: format!("Spell {id}"),
                school: 1,
            },
            info: UnitInfo::default(),
        }
    }

    #[test]
    fn the_engine_logs_nothing_unless_enabled() {
        let mut engine = Engine::new();
        engine.log(LogUnit::Target, LogUnit::Target, cast(1));
        assert!(!engine.is_logging());
        assert!(engine.take_combat_log().is_none());
    }

    #[test]
    fn entries_carry_the_time_and_insert_before_later_entries() {
        let mut engine = Engine::new();
        engine.enable_combat_log();
        engine.prepare_iteration(-1.5);
        let me = LogUnit::Character(CharId(0));
        engine.log(me, LogUnit::Target, cast(1));
        engine.add_event_in(2.0, EventKind::EncounterEnd);
        engine.next_event();
        let mark = engine.combat_log().unwrap().len();
        engine.log(me, LogUnit::Target, cast(3));
        engine.log_at(mark, me, LogUnit::Target, cast(2));

        let log = engine.take_combat_log().unwrap();
        let times: Vec<f64> = log.entries().iter().map(|e| e.time).collect();
        assert_eq!(times, [-1.5, 0.5, 0.5]);
        let ids: Vec<u32> = log
            .entries()
            .iter()
            .map(|e| match &e.event {
                CombatLogEvent::SpellCastSuccess { spell, .. } => spell.id,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(ids, [1, 2, 3]);
        assert!(!engine.is_logging());
    }

    #[test]
    fn total_damage_sums_the_damage_events() {
        let mut log = CombatLog::new();
        let damage = |amount| Damage {
            amount,
            ..Damage::default()
        };
        let entry = |event| CombatLogEntry {
            time: 0.0,
            source: LogUnit::Character(CharId(0)),
            dest: LogUnit::Target,
            event,
        };
        log.push(entry(CombatLogEvent::SwingDamage {
            hand: Hand::Mainhand,
            damage: damage(100),
            info: UnitInfo::default(),
        }));
        log.push(entry(cast(1)));
        log.push(entry(CombatLogEvent::SpellPeriodicDamage {
            spell: LogSpell {
                id: 2,
                name: "Rend".into(),
                school: 1,
            },
            damage: damage(20),
            info: UnitInfo::default(),
        }));
        assert_eq!(log.total_damage(), 120);
    }
}
