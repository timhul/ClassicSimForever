//! One watched iteration: the character's raid, its [`IterationStepper`] and what the page has
//! been shown of it.
//!
//! The raid is built and seeded exactly as `csim run --combat-log` builds and seeds it, so with
//! the same seed, length and variance the session runs the iteration that command logs. Each
//! call moves the iteration forward and returns a [`Frame`]: the damage dealt since the last
//! frame (read from the combat log, which a step only appends to) with the procs that fired
//! (the log has no proc lines: each proc's count is compared after every event), the rotation's decisions
//! since the last frame (its decision trace, enabled for the session) and the character's
//! state. Every time in a frame is absolute sim time, so the page can animate between frames.
//!
//! With keybinds the character is played from the keyboard instead of its rotation (manual
//! input, see `Character::enable_manual_input`): [`Session::cast`] queues a bound spell at the
//! time shown, as the game's spell queue window does.

use std::sync::Arc;

use csim_engine::buff::Buff;
use csim_engine::character_loader::CharacterSetup;
use csim_engine::character_spells::CharacterSpells;
use csim_engine::combat_log::{CombatLogEvent, Damage, LogUnit, MissType};
use csim_engine::data_bundle::DataBundle;
use csim_engine::engine::{Event, EventKind};
use csim_engine::faction::PlayerClass;
use csim_engine::ids::{CharId, SpellId};
use csim_engine::item::EquipmentSlot;
use csim_engine::proc::Proc;
use csim_engine::raid::RaidControl;
use csim_engine::rotation::DecidedBy;
use csim_engine::sim_control::IterationStepper;
use csim_engine::sim_settings::SimSettings;
use csim_engine::spell::{Hand, SpellStatus};
use csim_engine::stance::Stance;
use csim_engine::statistics::report::{BuffRow, SpellRow, buff_rows_so_far, spell_rows};
use serde::Serialize;

use crate::keybinds::Keybind;

/// The watched character: a setup builds a raid of one.
const PLAYER: CharId = CharId(0);

/// What stays the same for a whole iteration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Info {
    pub name: String,
    pub class: &'static str,
    pub race: &'static str,
    pub rotation: String,
    /// A string in JSON: seeds do not fit a JavaScript number.
    #[serde(serialize_with = "as_string")]
    pub seed: u64,
    /// The encounter length asked for, in seconds.
    pub combat_length: u32,
    /// The length variance in percent.
    pub length_variance: f64,
    /// When the iteration starts: before the pull (0), for the precombat actions.
    pub start_at: f64,
    /// When this iteration's encounter ends (the drawn length).
    pub end_at: f64,
    /// The rotation's `cast_if` entries, in file order (empty without a rotation).
    pub cast_if: Vec<RotationEntry>,
    /// The rotation's precombat actions the character can cast, in order (none when played
    /// from the keyboard).
    pub precombat: Vec<String>,
    /// Played from the keyboard: the rotation does not run.
    pub manual: bool,
    /// The bound spells, in the keybinds file's order (none without keybinds).
    pub keybinds: Vec<KeybindInfo>,
}

/// A spell or a macro bound to a key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeybindInfo {
    /// The spell's name, or the macro's.
    pub name: String,
    /// `Ctrl+Shift+Alt+KEY` (see `keybinds`).
    pub binding: String,
    /// The spells it casts: the one, or the macro's entries in order.
    pub spells: Vec<String>,
    #[serde(rename = "macro")]
    pub is_macro: bool,
    /// The icon of its main spell (see [`main_spell`]).
    pub icon: Option<u32>,
}

/// How long a press waits for its spell to become castable (the game's spell queue window).
pub const INPUT_QUEUE_WINDOW: f64 = 0.4;

/// One `cast_if` entry of the rotation: an executor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RotationEntry {
    /// 1-based position in the rotation file's `cast_if` list.
    pub position: usize,
    pub spell: String,
    /// The rank asked for; `None` for the highest learned.
    pub rank: Option<u32>,
    pub icon: Option<u32>,
    /// The condition as written in the rotation file; `None` without one.
    pub condition: Option<String>,
    /// Why the entry never casts; `None` when it is active.
    pub skipped: Option<String>,
}

/// One cast the rotation decided.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub time: f64,
    pub spell: String,
    pub icon: Option<u32>,
    /// `entry` (a `cast_if` entry returned true), `precombat`, `precast` or `input` (the
    /// player's, in manual mode).
    pub by: &'static str,
    /// The 1-based position of the `cast_if` entry, for `by: entry`.
    pub entry: Option<usize>,
}

/// The iteration at one point in time.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frame {
    /// The sim time shown.
    pub time: f64,
    /// Every event of the iteration ran.
    pub done: bool,
    /// The type of the last event run by a single step, for the step buttons.
    pub event: Option<&'static str>,
    /// The damage dealt since the previous frame, in the order it was dealt.
    pub damage: Vec<DamageNumber>,
    /// The rotation's decisions since the previous frame, in the order they were made.
    pub decisions: Vec<Decision>,
    /// The damage dealt so far.
    pub total_damage: u64,
    /// `total_damage` per second of combat so far (0 before the pull).
    pub dps: f64,
    /// From the keyboard: the last key press since the previous frame that was dropped uncast.
    pub input_error: Option<InputError>,
    pub state: CharacterState,
    /// The damage so far per spell, by outcome, as `csim run` reports it (one iteration of the
    /// combat time so far; empty before the pull).
    pub breakdown: Vec<SpellRow>,
    /// The buffs' (and debuffs') uptimes so far, as `csim run` reports them (empty before the
    /// pull).
    pub buff_uptimes: Vec<BuffUptime>,
}

/// A key press that could not cast its spell, with why, as the game says it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputError {
    pub spell: String,
    pub reason: String,
}

/// One hit of the damage feed, one attack the target avoided, or one proc that fired.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DamageNumber {
    pub time: f64,
    /// 0 for an avoided attack.
    pub amount: u32,
    /// How the attack was avoided ("Miss", "Dodge", "Parry", "Block" or "Resist"); `None` for a
    /// hit.
    pub miss: Option<&'static str>,
    /// A proc that fired (`name` is the proc's; no damage): not damage, but it shows what
    /// happened in between (a Windfury Totem extra swing, a Flurry).
    pub proc: bool,
    pub critical: bool,
    pub glancing: bool,
    /// A white swing (an auto attack); else a spell, its periodic damage included.
    pub auto: bool,
    /// The spell, or the hand of a swing.
    pub name: String,
    /// The icon (a texture `FileDataID`, see [`icon`]): the spell's, or the weapon's for a swing.
    pub icon: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CharacterState {
    pub resource: ResourceState,
    /// Rogues only.
    pub combo_points: Option<u32>,
    /// `None` in caster form (no stance).
    pub stance: Option<&'static str>,
    /// When the global cooldown ends (in the past when it is not running).
    pub gcd_end: f64,
    /// The length of the global cooldown.
    pub gcd: f64,
    pub mainhand: SwingState,
    /// `None` without a weapon in the off hand.
    pub offhand: Option<SwingState>,
    /// The visible buffs on the character.
    pub buffs: Vec<BuffState>,
    /// The visible debuffs on the target: the character's own (Deep Wound), the raid's shared
    /// ones, then the setup's external debuffs (Sunder Armor, Faerie Fire), one per name.
    pub debuffs: Vec<BuffState>,
    /// Every spell the rotation can cast, in rotation order (from the keyboard: the bound
    /// spells, in the keybinds' order), with its cooldown (`duration` 0 without one).
    pub rotation_spells: Vec<CooldownState>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceState {
    pub kind: &'static str,
    pub current: u32,
    pub max: u32,
}

/// The swing timer of one hand: the next swing lands at `next`, the previous one at `last`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SwingState {
    pub last: f64,
    pub next: f64,
}

/// A buff's uptime so far, with its icon.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuffUptime {
    #[serde(flatten)]
    pub row: BuffRow,
    /// The icon of the spell applying it; `None` for a buff made in code.
    pub icon: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuffState {
    pub name: String,
    pub stacks: u32,
    /// The charges left, 0 for a buff without charges.
    pub charges: u32,
    /// `None` for a buff without a duration.
    pub expires_at: Option<f64>,
    /// The length of the application in seconds; `None` without a duration.
    pub duration: Option<f64>,
    /// The icon of the spell applying it; `None` for a buff made in code.
    pub icon: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CooldownState {
    pub name: String,
    /// When the spell is off cooldown (in the past when it is ready); `None` when it was not
    /// used yet.
    pub ready_at: Option<f64>,
    /// The cooldown length in seconds.
    pub duration: f64,
    pub icon: Option<u32>,
    /// The spell waits for the global cooldown too.
    pub on_gcd: bool,
    /// The character has the resource it costs.
    pub affordable: bool,
}

/// One iteration being watched. See the module documentation.
pub struct Session {
    data: Arc<DataBundle>,
    setup: CharacterSetup,
    settings: SimSettings,
    seed: u64,
    /// The keybinds of the file; empty when the rotation plays.
    keybinds: Vec<Keybind>,
    /// The spells of each keybind, in `keybinds` order.
    bound: Vec<Vec<SpellId>>,
    raid: RaidControl,
    stepper: IterationStepper,
    /// The combat log entries already read into frames.
    read: usize,
    /// The rotation decisions already read into frames.
    decisions_read: usize,
    /// Each of the character's procs' firings so far, by proc index.
    proc_counts: Vec<u32>,
    /// The procs fired and not read into a frame yet.
    proc_marks: Vec<ProcMark>,
    total_damage: u64,
    /// The latest time shown.
    time: f64,
}

impl Session {
    /// Builds the setup under `settings` and starts its iteration of `seed`, played by its
    /// rotation, or from the keyboard with `keybinds` (when there are any).
    ///
    /// # Errors
    /// The setup does not build against the data, the settings are invalid, or a bound spell
    /// is not one the character has learned.
    pub fn new(
        data: Arc<DataBundle>,
        setup: CharacterSetup,
        settings: SimSettings,
        seed: u64,
        keybinds: Vec<Keybind>,
    ) -> Result<Session, String> {
        settings.validate().map_err(|error| error.to_string())?;
        let mut raid = setup
            .build_raid(&data, &settings)
            .map_err(|error| error.to_string())?;
        let bound = bound_spells(&raid, &keybinds)?;
        let stepper = start(&settings, seed, &mut raid, !keybinds.is_empty());
        let time = stepper.start_at();
        let proc_counts = proc_counts(&raid);
        Ok(Session {
            data,
            setup,
            settings,
            seed,
            keybinds,
            bound,
            raid,
            stepper,
            read: 0,
            decisions_read: 0,
            proc_counts,
            proc_marks: Vec::new(),
            total_damage: 0,
            time,
        })
    }

    /// Starts the iteration of `seed` over.
    ///
    /// # Panics
    /// Panics if the setup no longer builds (it built when the session was created).
    pub fn restart(&mut self, seed: u64) {
        let mut raid = self
            .setup
            .build_raid(&self.data, &self.settings)
            .expect("the setup built before");
        self.bound = bound_spells(&raid, &self.keybinds).expect("they were bound before");
        self.stepper = start(&self.settings, seed, &mut raid, self.manual());
        self.proc_counts = proc_counts(&raid);
        self.proc_marks.clear();
        self.raid = raid;
        self.seed = seed;
        self.read = 0;
        self.decisions_read = 0;
        self.total_damage = 0;
        self.time = self.stepper.start_at();
    }

    pub fn info(&self) -> Info {
        let character = self.raid.character(PLAYER);
        Info {
            name: self.setup.name.clone(),
            class: character.class_kind().name(),
            race: character.race().name(),
            rotation: character.rotation_name().to_owned(),
            seed: self.seed,
            combat_length: self.settings.combat_length,
            length_variance: self.settings.length_variance,
            start_at: self.stepper.start_at(),
            end_at: character.sim().combat_length,
            cast_if: self.rotation_entries(),
            precombat: match character.rotation() {
                Some(rotation) if !self.manual() => {
                    let spells = character.spells();
                    rotation
                        .precombat_spells()
                        .iter()
                        .map(|&id| spells.spell(id).name().to_owned())
                        .collect()
                }
                _ => Vec::new(),
            },
            manual: self.manual(),
            keybinds: self
                .keybinds
                .iter()
                .zip(&self.bound)
                .map(|(keybind, ids)| KeybindInfo {
                    name: keybind.name.clone(),
                    binding: keybind.binding.clone(),
                    spells: keybind.spells.clone(),
                    is_macro: keybind.is_macro,
                    icon: icon(
                        character
                            .spells()
                            .spell(main_spell(character.spells(), ids))
                            .record()
                            .icon,
                    ),
                })
                .collect(),
        }
    }

    /// Whether the character is played from the keyboard.
    pub fn manual(&self) -> bool {
        !self.keybinds.is_empty()
    }

    /// A key press of the keybind `name` (a spell or a macro) at `at` (the time shown, at the
    /// earliest): runs the events up to then, queues its spell or macro for
    /// [`INPUT_QUEUE_WINDOW`] and wakes the character, which casts it now or as soon as it can
    /// within the window.
    ///
    /// # Errors
    /// The character is played by its rotation, or `name` is not bound.
    pub fn cast(&mut self, name: &str, at: f64) -> Result<Frame, String> {
        if !self.manual() {
            return Err("played by the rotation: no keybinds".to_owned());
        }
        let index = self
            .keybinds
            .iter()
            .position(|keybind| keybind.name == name)
            .ok_or_else(|| format!("{name} is not bound"))?;
        let at = at.max(self.time);
        let frame = self.advance(at);
        if frame.done {
            return Ok(frame);
        }
        let at = at.max(self.raid.engine().current_time());
        let character = self.raid.character_mut(PLAYER);
        let spells = self.bound[index].clone();
        if self.keybinds[index].is_macro {
            character.queue_macro(spells, at + INPUT_QUEUE_WINDOW);
        } else {
            character.queue_input(spells[0], at + INPUT_QUEUE_WINDOW);
        }
        // Woken at the press, and just after its window: a press still waiting is dropped then.
        for wake in [at, at + INPUT_QUEUE_WINDOW + 1e-6] {
            let wake = Event::new(wake, EventKind::PlayerAction { character: PLAYER });
            self.raid.engine_mut().add_event(wake);
        }
        Ok(self.advance(at))
    }

    fn rotation_entries(&self) -> Vec<RotationEntry> {
        let character = self.raid.character(PLAYER);
        let Some(rotation) = character.rotation() else {
            return Vec::new();
        };
        let spells = character.spells();
        rotation
            .executors()
            .iter()
            .zip(&rotation.spec().cast_if)
            .enumerate()
            .map(|(index, (executor, written))| RotationEntry {
                position: index + 1,
                spell: written.name.clone(),
                rank: written.rank,
                icon: executor
                    .linked()
                    .and_then(|linked| icon(spells.spell(linked.spell).record().icon)),
                condition: written
                    .condition
                    .as_deref()
                    .map(|text| text.trim().to_owned()),
                skipped: executor.skip_reason().map(ToString::to_string),
            })
            .collect()
    }

    /// Runs every event up to `time` and shows `time` (the end of the encounter once every
    /// event ran). A time before the one shown runs nothing.
    pub fn advance(&mut self, time: f64) -> Frame {
        // One event at a time, for the procs each fires.
        while self
            .stepper
            .next_event_time(&self.raid)
            .is_some_and(|next| next <= time)
        {
            self.step();
        }
        let now = self.raid.engine().current_time();
        self.time = if self.done() {
            now
        } else {
            self.time.max(time)
        };
        self.frame(None)
    }

    /// Runs the next event.
    pub fn step_event(&mut self) -> Frame {
        let event = self.step();
        self.frame(event.map(|event| event.kind.event_type().name()))
    }

    /// Runs events until the character casts a spell (or the iteration is over).
    pub fn step_cast(&mut self) -> Frame {
        let mut last = None;
        while let Some(event) = self.step() {
            last = Some(event);
            let log = self.log();
            if log[self.read..].iter().any(|entry| {
                entry.source == LogUnit::Character(PLAYER)
                    && matches!(entry.event, CombatLogEvent::SpellCastSuccess { .. })
            }) {
                break;
            }
        }
        self.frame(last.map(|event| event.kind.event_type().name()))
    }

    fn step(&mut self) -> Option<Event> {
        let start = self.log().len();
        let event = self.stepper.step(&mut self.raid)?;
        self.time = event.time;
        self.mark_procs(start, event.time);
        Some(event)
    }

    /// Marks the procs the event that logged from `start` on fired: before its first line of
    /// the proc's spell or of a spell it casts, else after its lines. Later events only append
    /// to the log, so the place stays.
    fn mark_procs(&mut self, start: usize, time: f64) {
        let procs = self.raid.character(PLAYER).spells().procs().procs();
        let log = self.log();
        let mut marks = Vec::new();
        for (index, proc) in procs.iter().enumerate() {
            let before = self.proc_counts.get(index).copied().unwrap_or(0);
            if proc.procs() <= before {
                continue;
            }
            let mut spells = proc.payload_spells();
            spells.insert(0, proc.game_id());
            let at = log[start..]
                .iter()
                .position(|entry| {
                    entry.source == LogUnit::Character(PLAYER)
                        && logged_spell(&entry.event).is_some_and(|id| spells.contains(&id))
                })
                .map_or(log.len(), |offset| start + offset);
            let icon = spells.iter().find_map(|&id| self.spell_icon(id));
            for _ in before..proc.procs() {
                marks.push(ProcMark {
                    at,
                    number: DamageNumber {
                        time,
                        amount: 0,
                        miss: None,
                        proc: true,
                        critical: false,
                        glancing: false,
                        auto: false,
                        name: proc.name().to_owned(),
                        icon,
                    },
                });
            }
        }
        self.proc_counts = procs.iter().map(Proc::procs).collect();
        self.proc_marks.extend(marks);
    }

    fn done(&self) -> bool {
        self.stepper.is_done(&self.raid)
    }

    fn log(&self) -> &[csim_engine::combat_log::CombatLogEntry] {
        self.raid
            .engine()
            .combat_log()
            .expect("the stepper enables the log")
            .entries()
    }

    /// The frame at the time shown, with the damage logged since the last frame.
    fn frame(&mut self, event: Option<&'static str>) -> Frame {
        let mut marks = std::mem::take(&mut self.proc_marks);
        marks.sort_by_key(|mark| mark.at);
        let mut marks = marks.into_iter().peekable();
        let mut damage = Vec::new();
        for (index, entry) in self.log().iter().enumerate().skip(self.read) {
            while let Some(mark) = marks.next_if(|mark| mark.at <= index) {
                damage.push(mark.number);
            }
            if entry.source == LogUnit::Character(PLAYER)
                && let Some(number) = self.damage_number(entry.time, &entry.event)
            {
                damage.push(number);
            }
        }
        damage.extend(marks.map(|mark| mark.number));
        self.read = self.log().len();
        let decisions = self.decisions();
        self.total_damage += damage.iter().map(|hit| u64::from(hit.amount)).sum::<u64>();
        let dps = if self.time > 0.0 {
            self.total_damage as f64 / self.time
        } else {
            0.0
        };
        Frame {
            time: self.time,
            done: self.done(),
            event,
            damage,
            decisions,
            total_damage: self.total_damage,
            dps,
            input_error: self.input_error(),
            state: self.character_state(),
            breakdown: self.breakdown(),
            buff_uptimes: self.buff_uptimes(),
        }
    }

    /// The character's buffs' uptimes over the combat so far.
    fn buff_uptimes(&self) -> Vec<BuffUptime> {
        if self.time <= 0.0 {
            return Vec::new();
        }
        let buffs = self.buffs();
        let statistics = self.raid.character(PLAYER).statistics();
        buff_rows_so_far(statistics, buffs.iter().copied(), self.time)
            .into_iter()
            .map(|row| {
                let icon = buffs
                    .iter()
                    .find(|buff| buff.statistics_name() == row.name)
                    .and_then(|buff| self.spell_icon(buff.spell()));
                BuffUptime { row, icon }
            })
            .collect()
    }

    /// Every buff of the character: its own and the ones it shares.
    fn buffs(&self) -> Vec<&Buff> {
        let spells = self.raid.character(PLAYER).spells();
        spells
            .buff_ids()
            .filter_map(|id| match spells.buff_slot(id) {
                csim_engine::character_spells::BuffSlot::Owned(buff) => Some(&**buff),
                csim_engine::character_spells::BuffSlot::Shared(shared) => {
                    self.raid.shared_buffs().buffs().get(shared.index())
                }
            })
            .collect()
    }

    /// The character's spell rows over the combat so far.
    fn breakdown(&self) -> Vec<SpellRow> {
        if self.time > 0.0 {
            spell_rows(self.raid.character(PLAYER).statistics(), 1, self.time)
        } else {
            Vec::new()
        }
    }

    /// The key press dropped uncast since the last frame, if any.
    fn input_error(&mut self) -> Option<InputError> {
        let character = self.raid.character_mut(PLAYER);
        let (spell, status) = character.take_input_failure()?;
        let resource = character.resource().resource_type().name().to_lowercase();
        Some(InputError {
            spell: character.spells().spell(spell).name().to_owned(),
            reason: input_reason(status, &resource),
        })
    }

    /// The rotation's decisions not read into a frame yet.
    fn decisions(&mut self) -> Vec<Decision> {
        let character = self.raid.character(PLAYER);
        let Some(rotation) = character.rotation() else {
            return Vec::new();
        };
        let spells = character.spells();
        let trace = rotation.trace();
        let decisions = trace[self.decisions_read..]
            .iter()
            .map(|decision| {
                let spell = spells.spell(decision.spell);
                let (by, entry) = match decision.by {
                    DecidedBy::Executor(index) => ("entry", Some(index + 1)),
                    DecidedBy::Precombat => ("precombat", None),
                    DecidedBy::Precast => ("precast", None),
                    DecidedBy::Input => ("input", None),
                };
                Decision {
                    time: decision.time,
                    spell: spell.name().to_owned(),
                    icon: icon(spell.record().icon),
                    by,
                    entry,
                }
            })
            .collect();
        self.decisions_read = trace.len();
        decisions
    }

    /// The damage number of a damage or missed event; `None` for every other event.
    pub(crate) fn damage_number(&self, time: f64, event: &CombatLogEvent) -> Option<DamageNumber> {
        let (damage, miss, auto, name, icon) = match event {
            CombatLogEvent::SwingDamage { hand, damage, .. } => (
                *damage,
                None,
                true,
                hand_name(*hand),
                self.weapon_icon(*hand),
            ),
            CombatLogEvent::SwingMissed { hand, miss } => (
                Damage::default(),
                Some(miss_name(*miss)),
                true,
                hand_name(*hand),
                self.weapon_icon(*hand),
            ),
            CombatLogEvent::SpellDamage { spell, damage, .. }
            | CombatLogEvent::SpellPeriodicDamage { spell, damage, .. } => (
                *damage,
                None,
                false,
                spell.name.clone(),
                self.spell_icon(spell.id),
            ),
            CombatLogEvent::SpellMissed { spell, miss, .. } => (
                Damage::default(),
                Some(miss_name(*miss)),
                false,
                spell.name.clone(),
                self.spell_icon(spell.id),
            ),
            _ => return None,
        };
        Some(DamageNumber {
            time,
            amount: damage.amount,
            miss,
            proc: false,
            critical: damage.critical,
            glancing: damage.glancing,
            auto,
            name,
            icon,
        })
    }

    /// The icon of the game spell `id` (0 for spells made in code: none).
    fn spell_icon(&self, id: u32) -> Option<u32> {
        self.data
            .spells
            .get(id)
            .and_then(|record| icon(record.icon))
    }

    /// The icon of the weapon in `hand`.
    fn weapon_icon(&self, hand: Hand) -> Option<u32> {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        let equipment = self.raid.character(PLAYER).equipment();
        equipment.item(slot).and_then(|item| icon(item.spec().icon))
    }

    fn character_state(&self) -> CharacterState {
        let now = self.raid.engine().current_time();
        let character = self.raid.character(PLAYER);
        let spells = character.spells();
        let swing = |hand: Hand| {
            let attack = spells.auto_attack(hand);
            SwingState {
                last: attack.last_used(),
                next: attack.next_expected_use(now),
            }
        };
        let shown: Vec<&Buff> = self
            .buffs()
            .into_iter()
            .filter(|buff| buff.is_active() && !buff.is_hidden())
            .collect();
        let state = |buff: &Buff| BuffState {
            name: buff.name().to_owned(),
            stacks: buff.stacks(),
            charges: buff.charges(),
            expires_at: buff.duration().map(|_| now + buff.time_left(now)),
            duration: buff.duration(),
            icon: self.spell_icon(buff.spell()),
        };
        let buffs = shown
            .iter()
            .filter(|buff| !buff.is_debuff())
            .map(|buff| state(buff))
            .collect();
        let mut debuffs: Vec<BuffState> = Vec::new();
        let shared = self.raid.shared_buffs().buffs().iter();
        for buff in shown.iter().copied().chain(shared) {
            if buff.is_debuff()
                && buff.is_active()
                && !buff.is_hidden()
                && !debuffs.iter().any(|known| known.name == buff.name())
            {
                debuffs.push(state(buff));
            }
        }
        // The setup's external debuffs (hidden buffs of the character), after the sim's own.
        for entry in character.general_buffs().entries() {
            let Some(buff) = spells.owned_buff(entry.buff) else {
                continue;
            };
            if entry.debuff
                && buff.is_active()
                && !debuffs.iter().any(|known| known.name == buff.name())
            {
                debuffs.push(state(buff));
            }
        }

        // From the keyboard: the keybinds, each named as bound with its main spell's cooldown.
        // Else the spells the rotation can cast.
        let ids: Vec<(String, SpellId)> = if self.manual() {
            self.keybinds
                .iter()
                .zip(&self.bound)
                .map(|(keybind, ids)| (keybind.name.clone(), main_spell(spells, ids)))
                .collect()
        } else {
            character
                .rotation()
                .into_iter()
                .flat_map(|rotation| rotation.active_executors())
                .filter_map(|executor| executor.linked())
                .map(|linked| (spells.spell(linked.spell).name().to_owned(), linked.spell))
                .collect()
        };
        let mut rotation_spells: Vec<CooldownState> = Vec::new();
        for (name, id) in ids {
            let spell = spells.spell(id);
            if rotation_spells.iter().any(|known| known.name == name) {
                continue;
            }
            // The longest of its cooldowns (its own or its category's); none for most.
            let longest = spell
                .cooldown_ids()
                .map(|id| spells.cooldowns().get(id))
                .max_by(|a, b| a.next_use().total_cmp(&b.next_use()));
            rotation_spells.push(CooldownState {
                name,
                ready_at: longest
                    .filter(|cooldown| cooldown.last_used != -cooldown.base)
                    .map(|cooldown| cooldown.next_use()),
                duration: longest.map_or(0.0, |cooldown| cooldown.base),
                icon: icon(spell.record().icon),
                on_gcd: spell.triggers_gcd(),
                affordable: spell.resource_type().is_none_or(|resource| {
                    character.resource_level(resource, now)
                        >= spell.resource_cost_with(character.spell_modifiers())
                }),
            });
        }

        let resource = character.resource();
        CharacterState {
            resource: ResourceState {
                kind: resource.resource_type().name(),
                current: resource.current(now),
                max: resource.max(),
            },
            combo_points: (character.class_kind() == PlayerClass::Rogue)
                .then(|| character.combo_points(now)),
            stance: Some(character.stance())
                .filter(|&stance| stance != Stance::Caster)
                .map(Stance::name),
            gcd_end: character.next_gcd(),
            gcd: character.global_cooldown(),
            mainhand: swing(Hand::Mainhand),
            offhand: character.is_dual_wielding().then(|| swing(Hand::Offhand)),
            buffs,
            debuffs,
            rotation_spells,
        }
    }
}

fn as_string<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

/// Enables the character's decision trace (the precombat actions are decided when the
/// iteration starts) and starts the iteration of `seed`.
fn start(
    settings: &SimSettings,
    seed: u64,
    raid: &mut RaidControl,
    manual: bool,
) -> IterationStepper {
    let character = raid.character_mut(PLAYER);
    if manual {
        character.enable_manual_input();
    }
    if let Some(rotation) = character.rotation_mut() {
        rotation.enable_trace();
    }
    IterationStepper::new(settings, seed, raid)
}

/// The spells of each keybind, each a rank group's highest learned rank, else an enabled
/// spell of that name.
///
/// # Errors
/// A bound spell the character has not learned.
fn bound_spells(raid: &RaidControl, keybinds: &[Keybind]) -> Result<Vec<Vec<SpellId>>, String> {
    let spells = raid.character(PLAYER).spells();
    let learned = |name: &str| {
        let ranked = spells.rank_group(name).and_then(|group| {
            group.get_max_available_spell_rank(|id| spells.spell(id).is_enabled())
        });
        ranked.or_else(|| {
            spells.spell_ids().find(|&id| {
                let spell = spells.spell(id);
                spell.is_enabled() && spell.name() == name
            })
        })
    };
    keybinds
        .iter()
        .map(|keybind| {
            keybind
                .spells
                .iter()
                .map(|spell| {
                    learned(spell).ok_or_else(|| {
                        format!(
                            "{} ({}): {spell} is not a learned spell",
                            keybind.name, keybind.binding
                        )
                    })
                })
                .collect()
        })
        .collect()
}

/// The spell a keybind is shown as: a macro's first entry that triggers the GCD (what it is
/// for), else its first.
fn main_spell(spells: &CharacterSpells, ids: &[SpellId]) -> SpellId {
    ids.iter()
        .copied()
        .find(|&id| spells.spell(id).triggers_gcd())
        .unwrap_or(ids[0])
}

/// Why a key press could not cast its spell, as the game's error text says it.
fn input_reason(status: SpellStatus, resource: &str) -> String {
    match status {
        SpellStatus::OnGcd
        | SpellStatus::OnCooldown
        | SpellStatus::OnStanceCooldown
        | SpellStatus::OnTrinketCooldown
        | SpellStatus::CastInProgress => "Ability is not ready yet".to_owned(),
        SpellStatus::InsufficientResources => format!("Not enough {resource}"),
        SpellStatus::OvercapResource => format!("Too much {resource}"),
        SpellStatus::InsufficientComboPoints => "That ability requires combo points".to_owned(),
        SpellStatus::NotInExecuteRange => "Not in execute range".to_owned(),
        SpellStatus::BuffInactive => "Can't do that yet".to_owned(),
        SpellStatus::InCombat => "Can't do that while in combat".to_owned(),
        SpellStatus::IncorrectWeaponType => "Requires a different weapon".to_owned(),
        SpellStatus::NotBehindTarget => "You must be behind your target".to_owned(),
        SpellStatus::NotEnabled => "Not learned".to_owned(),
        SpellStatus::NotSupported => "Not modelled by the simulator".to_owned(),
        _ => match status.stance() {
            Some(stance) => format!("Can't do that in {}", stance.name()),
            None => status.description().to_owned(),
        },
    }
}

/// An icon `FileDataID` of the data, `None` for 0 (no icon).
pub fn icon(file_data_id: u32) -> Option<u32> {
    Some(file_data_id).filter(|&id| id != 0)
}

/// A proc that fired, to show before the log line `at`.
#[derive(Debug)]
struct ProcMark {
    at: usize,
    number: DamageNumber,
}

/// Each of the character's procs' firings so far.
fn proc_counts(raid: &RaidControl) -> Vec<u32> {
    let procs = raid.character(PLAYER).spells().procs().procs();
    procs.iter().map(Proc::procs).collect()
}

/// The game spell of a spell event.
fn logged_spell(event: &CombatLogEvent) -> Option<u32> {
    match event {
        CombatLogEvent::SpellCastSuccess { spell, .. }
        | CombatLogEvent::SpellDamage { spell, .. }
        | CombatLogEvent::SpellMissed { spell, .. }
        | CombatLogEvent::SpellPeriodicDamage { spell, .. }
        | CombatLogEvent::SpellEnergize { spell, .. }
        | CombatLogEvent::SpellAura { spell, .. } => Some(spell.id),
        CombatLogEvent::SwingDamage { .. } | CombatLogEvent::SwingMissed { .. } => None,
    }
}

fn miss_name(miss: MissType) -> &'static str {
    match miss {
        MissType::Miss => "Miss",
        MissType::Dodge => "Dodge",
        MissType::Parry => "Parry",
        MissType::Block => "Block",
        MissType::Resist => "Resist",
    }
}

fn hand_name(hand: Hand) -> String {
    match hand {
        Hand::Mainhand => "Main hand",
        Hand::Offhand => "Off hand",
    }
    .to_owned()
}

#[cfg(test)]
pub(crate) mod tests;
