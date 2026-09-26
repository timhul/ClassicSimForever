//! The combat log of an iteration: what happened, when, from whom to whom, recorded as
//! structured entries while the simulation runs. Off by default (see
//! [`Engine::enable_combat_log`](crate::engine::Engine::enable_combat_log)).
//!
//! The entries are recorded where the statistics are (`character/context.rs`), so the log shows
//! the same swings, casts, ticks and auras the statistics count.
//!
//! [`CombatLog::render`] writes the entries as the lines of a `WoWCombatLog.txt` with advanced
//! combat logging, in the field order of the `classic-warrior` wiki's `Combat-log-format.md`:
//! `M/D HH:MM:SS.mmm  EVENT,<source and destination>,<event fields>`. The pull (sim time 0)
//! is at `1/1 12:00:00.000`, so the precombat actions come just before noon. What the sim does
//! not model is logged like the client logs an unknown value: health, position, map and facing
//! as zeros, overkill as `-1`, a missing flag as `nil`.

use std::fmt::Write;

use crate::ids::CharId;
use crate::raid::RaidControl;
use crate::resource::ResourceType;
use crate::spell::Hand;

/// Milliseconds from midnight to the pull.
const PULL_MS: i64 = 12 * 3600 * 1000;
/// The unit flags of the character whose log it is: a player, controlled by a player,
/// friendly, mine.
const FLAGS_MINE: u32 = 0x511;
/// The unit flags of the other raid members: affiliation raid.
const FLAGS_RAID: u32 = 0x514;
/// The unit flags of the target: a hostile NPC outsider.
const FLAGS_TARGET: u32 = 0x10a48;
/// The GUID of no unit (an owner).
const NO_GUID: &str = "0000000000000000";

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

/// The names of the units of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitNames {
    /// By [`CharId`] index.
    pub characters: Vec<String>,
    pub target: String,
}

impl UnitNames {
    /// The raid's characters by their player names and the target.
    pub fn of(raid: &RaidControl) -> Self {
        UnitNames {
            characters: raid
                .characters()
                .iter()
                .map(|c| c.player_name().to_string())
                .collect(),
            target: "Target".to_string(),
        }
    }
}

impl CombatLog {
    /// The log as combat log lines, the `COMBAT_LOG_VERSION` header first (see the module
    /// documentation).
    pub fn render(&self, names: &UnitNames) -> String {
        let start = self.entries.first().map_or(0.0, |entry| entry.time);
        let mut out = format!(
            "{}  COMBAT_LOG_VERSION,9,ADVANCED_LOG_ENABLED,1\n",
            timestamp(start)
        );
        for entry in &self.entries {
            out.push_str(&entry.render(names));
            out.push('\n');
        }
        out
    }
}

impl CombatLogEntry {
    /// The entry as one combat log line, without the line break.
    pub fn render(&self, names: &UnitNames) -> String {
        let mut line = format!("{}  {}", timestamp(self.time), self.event.name());
        for unit in [self.source, self.dest] {
            let (name, flags) = match unit {
                LogUnit::Character(id) => (
                    names.characters[id.index()].as_str(),
                    if id.index() == 0 {
                        FLAGS_MINE
                    } else {
                        FLAGS_RAID
                    },
                ),
                LogUnit::Target => (names.target.as_str(), FLAGS_TARGET),
            };
            let _ = write!(line, ",{},\"{name}\",0x{flags:x},0x0", guid(unit));
        }
        let info_unit = match &self.event {
            CombatLogEvent::SwingDamage { .. } | CombatLogEvent::SpellCastSuccess { .. } => {
                self.source
            }
            _ => self.dest,
        };
        match &self.event {
            CombatLogEvent::SwingDamage { damage, info, .. } => {
                push_info(&mut line, info_unit, info);
                push_damage(&mut line, damage, 1);
            }
            CombatLogEvent::SwingMissed { hand, miss } => {
                let _ = write!(line, ",{},{}", miss.name(), flag(*hand == Hand::Offhand));
            }
            CombatLogEvent::SpellCastSuccess { spell, info } => {
                push_spell(&mut line, spell);
                push_info(&mut line, info_unit, info);
            }
            CombatLogEvent::SpellDamage {
                spell,
                damage,
                info,
            }
            | CombatLogEvent::SpellPeriodicDamage {
                spell,
                damage,
                info,
            } => {
                push_spell(&mut line, spell);
                push_info(&mut line, info_unit, info);
                push_damage(&mut line, damage, spell.school);
            }
            CombatLogEvent::SpellMissed {
                spell,
                miss,
                offhand,
            } => {
                push_spell(&mut line, spell);
                let _ = write!(line, ",{},{}", miss.name(), flag(*offhand));
            }
            CombatLogEvent::SpellEnergize {
                spell,
                power,
                amount,
                info,
                ..
            } => {
                push_spell(&mut line, spell);
                push_info(&mut line, info_unit, info);
                let _ = write!(
                    line,
                    ",{amount},0,{},{}",
                    power_type(Some(*power)),
                    info.max_power
                );
            }
            CombatLogEvent::SpellAura {
                spell,
                change,
                debuff,
            } => {
                push_spell(&mut line, spell);
                let _ = write!(line, ",{}", if *debuff { "DEBUFF" } else { "BUFF" });
                if let AuraChange::AppliedDose(stacks) = change {
                    let _ = write!(line, ",{stacks}");
                }
            }
        }
        line
    }
}

impl CombatLogEvent {
    /// The client's event name.
    pub fn name(&self) -> &'static str {
        match self {
            CombatLogEvent::SwingDamage { .. } => "SWING_DAMAGE",
            CombatLogEvent::SwingMissed { .. } => "SWING_MISSED",
            CombatLogEvent::SpellCastSuccess { .. } => "SPELL_CAST_SUCCESS",
            CombatLogEvent::SpellDamage { .. } => "SPELL_DAMAGE",
            CombatLogEvent::SpellMissed { .. } => "SPELL_MISSED",
            CombatLogEvent::SpellPeriodicDamage { .. } => "SPELL_PERIODIC_DAMAGE",
            CombatLogEvent::SpellEnergize {
                periodic: false, ..
            } => "SPELL_ENERGIZE",
            CombatLogEvent::SpellEnergize { periodic: true, .. } => "SPELL_PERIODIC_ENERGIZE",
            CombatLogEvent::SpellAura { change, .. } => match change {
                AuraChange::Applied => "SPELL_AURA_APPLIED",
                AuraChange::Refresh => "SPELL_AURA_REFRESH",
                AuraChange::AppliedDose(_) => "SPELL_AURA_APPLIED_DOSE",
                AuraChange::Removed => "SPELL_AURA_REMOVED",
            },
        }
    }
}

impl MissType {
    /// The client's miss type.
    pub fn name(self) -> &'static str {
        match self {
            MissType::Miss => "MISS",
            MissType::Dodge => "DODGE",
            MissType::Parry => "PARRY",
            MissType::Block => "BLOCK",
        }
    }
}

/// `M/D HH:MM:SS.mmm` of sim time `time` (the pull at `1/1 12:00:00.000`).
fn timestamp(time: f64) -> String {
    let ms = PULL_MS + (time * 1000.0).round() as i64;
    let (hours, rest) = (ms / 3_600_000, ms % 3_600_000);
    let (minutes, rest) = (rest / 60_000, rest % 60_000);
    let (seconds, millis) = (rest / 1000, rest % 1000);
    format!("1/1 {hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

fn guid(unit: LogUnit) -> String {
    match unit {
        LogUnit::Character(id) => format!("Player-0-{:08X}", id.index() + 1),
        LogUnit::Target => "Creature-0-0-0-0-0-0000000000".to_string(),
    }
}

/// `1` or `nil`.
fn flag(set: bool) -> &'static str {
    if set { "1" } else { "nil" }
}

/// The client's `Enum.PowerType` (0 for a unit without power).
fn power_type(power: Option<ResourceType>) -> u32 {
    match power {
        None | Some(ResourceType::Mana) => 0,
        Some(ResourceType::Rage) => 1,
        Some(ResourceType::Focus) => 2,
        Some(ResourceType::Energy) => 3,
    }
}

/// `spellID,"spellName",spellSchool`.
fn push_spell(line: &mut String, spell: &LogSpell) {
    let _ = write!(
        line,
        ",{},\"{}\",0x{:x}",
        spell.id, spell.name, spell.school
    );
}

/// The advanced combat logging fields of `unit`.
fn push_info(line: &mut String, unit: LogUnit, info: &UnitInfo) {
    let _ = write!(
        line,
        ",{},{NO_GUID},0,0,{},0,{},{},{},{},0,0.00,0.00,0,0.0000,{}",
        guid(unit),
        info.attack_power,
        info.armor,
        power_type(info.power),
        info.current_power,
        info.max_power,
        info.level
    );
}

/// `amount,rawAmount,overkill,school,resisted,blocked,absorbed,critical,glancing,crushing`.
fn push_damage(line: &mut String, damage: &Damage, school: u32) {
    let _ = write!(
        line,
        ",{0},{0},-1,{school},0,0,0,{1},{2},nil",
        damage.amount,
        flag(damage.critical),
        flag(damage.glancing)
    );
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
    fn nothing_is_logged_from_the_end_of_the_encounter_to_the_next_iteration() {
        let mut engine = Engine::new();
        engine.enable_combat_log();
        let me = LogUnit::Character(CharId(0));
        engine.end_combat();
        assert!(!engine.is_logging());
        engine.log(me, LogUnit::Target, cast(1));
        engine.log_at(0, me, LogUnit::Target, cast(2));
        engine.prepare_iteration(0.0);
        assert!(engine.is_logging());
        engine.log(me, LogUnit::Target, cast(3));
        assert_eq!(engine.take_combat_log().unwrap().len(), 1);
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

#[cfg(test)]
mod render_tests {
    use super::*;

    fn names() -> UnitNames {
        UnitNames {
            characters: vec!["Grom".into(), "Thrall".into()],
            target: "Target".into(),
        }
    }

    fn line(time: f64, source: LogUnit, dest: LogUnit, event: CombatLogEvent) -> String {
        CombatLogEntry {
            time,
            source,
            dest,
            event,
        }
        .render(&names())
    }

    const ME: LogUnit = LogUnit::Character(CharId(0));
    const HEAD: &str =
        "Player-0-00000001,\"Grom\",0x511,0x0,Creature-0-0-0-0-0-0000000000,\"Target\",0x10a48,0x0";
    const SELF: &str = "Player-0-00000001,\"Grom\",0x511,0x0,Player-0-00000001,\"Grom\",0x511,0x0";

    fn warrior() -> UnitInfo {
        UnitInfo {
            attack_power: 1500,
            armor: 3000,
            power: Some(ResourceType::Rage),
            current_power: 35,
            max_power: 100,
            level: 60,
        }
    }

    fn target() -> UnitInfo {
        UnitInfo {
            armor: 3731,
            level: 63,
            ..UnitInfo::default()
        }
    }

    fn bloodthirst() -> LogSpell {
        LogSpell {
            id: 23894,
            name: "Bloodthirst".into(),
            school: 1,
        }
    }

    #[test]
    fn timestamps_count_from_noon_at_the_pull() {
        assert_eq!(timestamp(0.0), "1/1 12:00:00.000");
        assert_eq!(timestamp(83.2517), "1/1 12:01:23.252");
        assert_eq!(timestamp(-1.5), "1/1 11:59:58.500");
    }

    #[test]
    fn swing_damage() {
        let event = CombatLogEvent::SwingDamage {
            hand: Hand::Mainhand,
            damage: Damage {
                amount: 512,
                critical: true,
                glancing: false,
            },
            info: warrior(),
        };
        assert_eq!(
            line(1.25, ME, LogUnit::Target, event),
            format!(
                "1/1 12:00:01.250  SWING_DAMAGE,{HEAD},Player-0-00000001,0000000000000000,0,0,\
                 1500,0,3000,1,35,100,0,0.00,0.00,0,0.0000,60,512,512,-1,1,0,0,0,1,nil,nil"
            )
        );
    }

    #[test]
    fn swing_missed() {
        let event = CombatLogEvent::SwingMissed {
            hand: Hand::Offhand,
            miss: MissType::Dodge,
        };
        assert_eq!(
            line(2.0, ME, LogUnit::Target, event),
            format!("1/1 12:00:02.000  SWING_MISSED,{HEAD},DODGE,1")
        );
    }

    #[test]
    fn spell_cast_success() {
        let event = CombatLogEvent::SpellCastSuccess {
            spell: bloodthirst(),
            info: warrior(),
        };
        assert_eq!(
            line(3.0, ME, LogUnit::Target, event),
            format!(
                "1/1 12:00:03.000  SPELL_CAST_SUCCESS,{HEAD},23894,\"Bloodthirst\",0x1,\
                 Player-0-00000001,0000000000000000,0,0,1500,0,3000,1,35,100,0,0.00,0.00,0,0.0000,60"
            )
        );
    }

    #[test]
    fn spell_damage_has_the_victims_info() {
        let event = CombatLogEvent::SpellDamage {
            spell: bloodthirst(),
            damage: Damage {
                amount: 700,
                critical: false,
                glancing: false,
            },
            info: target(),
        };
        assert_eq!(
            line(3.0, ME, LogUnit::Target, event),
            format!(
                "1/1 12:00:03.000  SPELL_DAMAGE,{HEAD},23894,\"Bloodthirst\",0x1,\
                 Creature-0-0-0-0-0-0000000000,0000000000000000,0,0,0,0,3731,0,0,0,0,0.00,0.00,0,\
                 0.0000,63,700,700,-1,1,0,0,0,nil,nil,nil"
            )
        );
    }

    #[test]
    fn spell_periodic_damage_of_another_school() {
        let event = CombatLogEvent::SpellPeriodicDamage {
            spell: LogSpell {
                id: 1,
                name: "Burn".into(),
                school: 4,
            },
            damage: Damage {
                amount: 30,
                ..Damage::default()
            },
            info: target(),
        };
        let line = line(4.0, ME, LogUnit::Target, event);
        assert!(line.starts_with(&format!(
            "1/1 12:00:04.000  SPELL_PERIODIC_DAMAGE,{HEAD},1,\"Burn\",0x4,"
        )));
        assert!(line.ends_with(",63,30,30,-1,4,0,0,0,nil,nil,nil"), "{line}");
    }

    #[test]
    fn spell_missed() {
        let event = CombatLogEvent::SpellMissed {
            spell: bloodthirst(),
            miss: MissType::Parry,
            offhand: false,
        };
        assert_eq!(
            line(3.0, ME, LogUnit::Target, event),
            format!("1/1 12:00:03.000  SPELL_MISSED,{HEAD},23894,\"Bloodthirst\",0x1,PARRY,nil")
        );
    }

    #[test]
    fn spell_energize() {
        let event = CombatLogEvent::SpellEnergize {
            spell: LogSpell {
                id: 2687,
                name: "Bloodrage".into(),
                school: 1,
            },
            power: ResourceType::Rage,
            amount: 10,
            periodic: false,
            info: warrior(),
        };
        assert_eq!(
            line(-1.5, ME, ME, event),
            format!(
                "1/1 11:59:58.500  SPELL_ENERGIZE,{SELF},2687,\"Bloodrage\",0x1,\
                 Player-0-00000001,0000000000000000,0,0,1500,0,3000,1,35,100,0,0.00,0.00,0,0.0000,60,\
                 10,0,1,100"
            )
        );
    }

    #[test]
    fn periodic_energize_has_its_own_name() {
        let event = CombatLogEvent::SpellEnergize {
            spell: bloodthirst(),
            power: ResourceType::Rage,
            amount: 1,
            periodic: true,
            info: warrior(),
        };
        assert!(line(0.0, ME, ME, event).contains("  SPELL_PERIODIC_ENERGIZE,"));
    }

    #[test]
    fn auras() {
        let aura = |change, debuff| CombatLogEvent::SpellAura {
            spell: LogSpell {
                id: 11597,
                name: "Sunder Armor".into(),
                school: 1,
            },
            change,
            debuff,
        };
        let sunder = "11597,\"Sunder Armor\",0x1";
        let cases = [
            (
                AuraChange::Applied,
                format!("SPELL_AURA_APPLIED,{HEAD},{sunder},DEBUFF"),
            ),
            (
                AuraChange::Refresh,
                format!("SPELL_AURA_REFRESH,{HEAD},{sunder},DEBUFF"),
            ),
            (
                AuraChange::AppliedDose(3),
                format!("SPELL_AURA_APPLIED_DOSE,{HEAD},{sunder},DEBUFF,3"),
            ),
            (
                AuraChange::Removed,
                format!("SPELL_AURA_REMOVED,{HEAD},{sunder},DEBUFF"),
            ),
        ];
        for (change, expected) in cases {
            assert_eq!(
                line(0.0, ME, LogUnit::Target, aura(change, true)),
                format!("1/1 12:00:00.000  {expected}")
            );
        }
        assert!(line(0.0, ME, ME, aura(AuraChange::Applied, false)).ends_with(",BUFF"));
    }

    #[test]
    fn other_raid_members_are_raid_affiliated() {
        let event = CombatLogEvent::SwingMissed {
            hand: Hand::Mainhand,
            miss: MissType::Miss,
        };
        let line = line(0.0, LogUnit::Character(CharId(1)), LogUnit::Target, event);
        assert!(
            line.contains("SWING_MISSED,Player-0-00000002,\"Thrall\",0x514,0x0,"),
            "{line}"
        );
        assert!(line.ends_with(",MISS,nil"));
    }

    #[test]
    fn the_log_starts_with_the_version_header() {
        let mut log = CombatLog::new();
        log.push(CombatLogEntry {
            time: -2.0,
            source: ME,
            dest: LogUnit::Target,
            event: CombatLogEvent::SwingMissed {
                hand: Hand::Mainhand,
                miss: MissType::Miss,
            },
        });
        let text = log.render(&names());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "1/1 11:59:58.000  COMBAT_LOG_VERSION,9,ADVANCED_LOG_ENABLED,1"
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with("1/1 11:59:58.000  SWING_MISSED,"));
    }
}
