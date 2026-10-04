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
//! time shown, as the game's spell queue window does. The player pulls: the iteration starts
//! [`MANUAL_PRE_PULL`] before the encounter, and the first offensive spell cast (one hitting the
//! enemy: Charge, not Battle Shout) moves the pull to when it lands. The session then rebuilds
//! the iteration with its pull there and replays the key presses before it; the sim being
//! deterministic, the pre-pull is the same at times shifted so that the pull is at 0.

use std::sync::Arc;

use csim_engine::buff::Buff;
use csim_engine::character::context::REGENERATION;
use csim_engine::character_loader::{CharacterSetup, TargetSetup};
use csim_engine::character_spells::CharacterSpells;
use csim_engine::combat_log::{CombatLogEvent, Damage, LogUnit, MissType};
use csim_engine::data_bundle::DataBundle;
use csim_engine::engine::{Event, EventKind};
use csim_engine::faction::PlayerClass;
use csim_engine::ids::{CharId, SpellId};
use csim_engine::item::{EquipmentSlot, ItemSpec};
use csim_engine::proc::Proc;
use csim_engine::raid::RaidControl;
use csim_engine::resource::ResourceType;
use csim_engine::rotation::{DecidedBy, RotationHost};
use csim_engine::sim_control::IterationStepper;
use csim_engine::sim_settings::SimSettings;
use csim_engine::spell::record::SpellRecord;
use csim_engine::spell::{Hand, SpellStatus};
use csim_engine::stance::Stance;
use csim_engine::statistics::report::{
    BuffRow, ProcRow, ResourceRow, ResourceTotal, SpellRow, buff_rows_so_far, proc_rows_so_far,
    resource_rows_so_far, resource_totals, spell_rows,
};
use serde::Serialize;

use crate::keybinds::Keybind;
use crate::sheet::{StatSummary, WornItem, worn_gear};

/// The watched character: a setup builds a raid of one.
pub(crate) const PLAYER: CharId = CharId(0);

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
    /// The named settings that are not the default (`--setting`), as `name:value,...`.
    pub settings: Option<String>,
    /// When the iteration starts: before the pull (0), for the precombat actions.
    pub start_at: f64,
    /// When this iteration's encounter ends (the drawn length).
    pub end_at: f64,
    /// The target's health as a fraction at the pull; it falls linearly to 0 at `end_at`
    /// (`target_start_health_percent`, or the ruleset's).
    pub target_start_health: f64,
    /// The rotation's `cast_if` entries, in file order (empty without a rotation).
    pub cast_if: Vec<RotationEntry>,
    /// The rotation's precombat actions the character can cast, in order (none when played
    /// from the keyboard).
    pub precombat: Vec<String>,
    /// Played from the keyboard: the rotation does not run.
    pub manual: bool,
    /// The bound spells, in the keybinds file's order (none without keybinds).
    pub keybinds: Vec<KeybindInfo>,
    /// The spells a keybind can name (see [`bindable_spells`]), played from the keyboard or
    /// not.
    pub bindable: Vec<BindableSpell>,
    /// The gear worn, in slot order.
    pub equipment: Vec<WornItem>,
    /// The character's stats before the iteration: gear, enchants, talents and the setup's
    /// buffs; not what the iteration casts (precombat stance and shouts, cooldowns, procs).
    pub stats: StatSummary,
}

/// A spell the character can cast from a key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BindableSpell {
    /// The name a keybind gives it (without rank).
    pub name: String,
    pub icon: Option<Icon>,
    /// Whether it triggers the global cooldown.
    pub gcd: bool,
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
    pub icon: Option<Icon>,
}

/// How long a press waits for its spell to become castable (the game's spell queue window).
pub const INPUT_QUEUE_WINDOW: f64 = 0.4;

/// Played from the keyboard: how long before the encounter the iteration starts, the time the
/// player has to pull (with an offensive spell) before the encounter starts by itself.
pub const MANUAL_PRE_PULL: f64 = 600.0;

/// One `cast_if` entry of the rotation: an executor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RotationEntry {
    /// 1-based position in the rotation file's `cast_if` list.
    pub position: usize,
    pub spell: String,
    /// The rank asked for; `None` for the highest learned.
    pub rank: Option<u32>,
    pub icon: Option<Icon>,
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
    pub icon: Option<Icon>,
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
    /// The procs tried so far, with their rate and procs per minute, as `csim run` reports them
    /// (empty before the pull).
    pub procs: Vec<ProcCount>,
    /// The resource gained so far per source, as `csim run` reports it (empty before the
    /// pull).
    pub resources: Vec<ResourceGain>,
    /// The sums over `resources`, one per resource, with the regeneration lost at the cap.
    pub resource_totals: Vec<ResourceTotal>,
    /// From the keyboard: the player pulled since the previous frame, which moved every time
    /// by this many seconds (the pull, before the encounter, is the new 0). The iteration was
    /// rebuilt: the frame holds everything since its start again.
    pub rebased_by: Option<f64>,
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
    /// The icon: the spell's, or the weapon's for a swing.
    pub icon: Option<Icon>,
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

/// A proc's count so far, with its icon.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProcCount {
    #[serde(flatten)]
    pub row: ProcRow,
    /// The icon of the proc's spell, or of a spell it casts.
    pub icon: Option<Icon>,
}

/// A source's resource gain so far, with its icon.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceGain {
    #[serde(flatten)]
    pub row: ResourceRow,
    /// The icon of the spell or proc gaining it, or of the weapon for a swing.
    pub icon: Option<Icon>,
}

/// A buff's uptime so far, with its icon.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuffUptime {
    #[serde(flatten)]
    pub row: BuffRow,
    /// The icon of the spell applying it; `None` for a buff made in code.
    pub icon: Option<Icon>,
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
    pub icon: Option<Icon>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CooldownState {
    pub name: String,
    /// When the spell is off cooldown (in the past when it is ready); `None` when it was not
    /// used yet.
    pub ready_at: Option<f64>,
    /// The cooldown length in seconds.
    pub duration: f64,
    pub icon: Option<Icon>,
    /// The spell waits for the global cooldown too.
    pub on_gcd: bool,
    /// The character has the resource it costs.
    pub affordable: bool,
    /// Its demands beyond time and resource hold (stance, Overpower's dodge, execute range, ...).
    pub usable: bool,
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
    /// The iteration starts at least this many seconds before the pull.
    pre_pull: f64,
    /// From the keyboard, the key presses before the pull: keybind index and time.
    presses: Vec<(usize, f64)>,
    /// Whether the pull is decided (always when the rotation plays).
    pulled: bool,
    /// The decisions already looked at for the pull.
    pull_scan: usize,
    /// The time shift of a pull not shown in a frame yet.
    rebased_by: Option<f64>,
    /// The stats before the iteration (the same for every iteration of the setup).
    stats: StatSummary,
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
        let manual = !keybinds.is_empty();
        let pre_pull = initial_pre_pull(manual);
        let stats = StatSummary::of_setup(&data, &setup, &settings)?;
        let stepper = start(&settings, seed, &mut raid, manual, pre_pull);
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
            pre_pull,
            presses: Vec::new(),
            pulled: !manual,
            pull_scan: 0,
            rebased_by: None,
            stats,
        })
    }

    /// Starts the iteration of `seed` over.
    ///
    /// # Panics
    /// Panics if the setup no longer builds (it built when the session was created).
    pub fn restart(&mut self, seed: u64) {
        self.pre_pull = initial_pre_pull(self.manual());
        self.presses.clear();
        self.rebased_by = None;
        self.rebuild(seed);
        self.pulled = !self.manual();
    }

    /// Builds the iteration of `seed` again, starting `pre_pull` before the pull.
    fn rebuild(&mut self, seed: u64) {
        let mut raid = self
            .setup
            .build_raid(&self.data, &self.settings)
            .expect("the setup built before");
        self.bound = bound_spells(&raid, &self.keybinds).expect("they were bound before");
        self.stepper = start(
            &self.settings,
            seed,
            &mut raid,
            self.manual(),
            self.pre_pull,
        );
        self.proc_counts = proc_counts(&raid);
        self.proc_marks.clear();
        self.raid = raid;
        self.seed = seed;
        self.read = 0;
        self.decisions_read = 0;
        self.pull_scan = 0;
        self.total_damage = 0;
        self.time = self.stepper.start_at();
    }

    /// The target the setup fights.
    pub fn target(&self) -> &TargetSetup {
        &self.setup.target
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
            settings: self.settings.named_settings_text(),
            start_at: self.stepper.start_at(),
            end_at: character.sim().combat_length,
            target_start_health: character.sim().target_start_health,
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
                    icon: Icon::of_spell(
                        character
                            .spells()
                            .spell(main_spell(character.spells(), ids))
                            .record(),
                    ),
                })
                .collect(),
            bindable: bindable_spells(character.spells()),
            equipment: worn_gear(&self.data, character),
            stats: self.stats.clone(),
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
        // A pull while running up to the press moves the press with every other time.
        let at = self.run_until(at.max(self.time));
        if self.done() {
            return Ok(self.advance(at));
        }
        let at = at.max(self.raid.engine().current_time());
        if !self.pulled {
            self.presses.push((index, at));
        }
        self.press(index, at);
        Ok(self.advance(at))
    }

    /// Queues the spell or macro of keybind `index` pressed at `at` and wakes the character.
    fn press(&mut self, index: usize, at: f64) {
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
    }

    /// From the keyboard, before the pull: when the player's last cast was offensive, the
    /// pull is when it lands. A pull before the encounter's start rebuilds the iteration with
    /// its pull there (see [`Session::rebase`]).
    fn check_pull(&mut self) {
        if self.pulled {
            return;
        }
        if self.raid.engine().current_time() >= 0.0 {
            self.pulled = true;
            return;
        }
        let character = self.raid.character(PLAYER);
        let Some(rotation) = character.rotation() else {
            return;
        };
        let spells = character.spells();
        let trace = rotation.trace();
        let offensive = trace[self.pull_scan..]
            .iter()
            .find(|decision| {
                matches!(decision.by, DecidedBy::Input)
                    && spells.spell(decision.spell).record().is_offensive()
            })
            .map(|decision| (decision.time, decision.spell));
        self.pull_scan = trace.len();
        let Some((time, spell)) = offensive else {
            return;
        };
        self.pulled = true;
        let pull = time + self.raid.context(PLAYER).spell_cast_time(spell);
        if pull < 0.0 {
            self.rebase(pull);
        }
    }

    /// Rebuilds the iteration with its pull at `pull` (the present iteration's time, before
    /// its encounter starts) and replays the key presses before it: every time moves by
    /// `-pull`, so the pull is at 0.
    fn rebase(&mut self, pull: f64) {
        let shown = self.time - pull;
        let now = self.raid.engine().current_time() - pull;
        let presses = std::mem::take(&mut self.presses);
        self.pre_pull += pull;
        self.rebuild(self.seed);
        self.pulled = true;
        for (index, at) in presses {
            let at = at - pull;
            self.run_until(at);
            self.press(index, at);
        }
        self.run_until(now);
        self.time = shown;
        self.rebased_by = Some(self.rebased_by.unwrap_or(0.0) + pull);
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
                    .and_then(|linked| Icon::of_spell(spells.spell(linked.spell).record())),
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
        let time = self.run_until(time);
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

    /// Runs every event up to `time`, one at a time for the procs each fires. Returns `time`,
    /// moved with every other time by a pull on the way.
    fn run_until(&mut self, time: f64) -> f64 {
        let mut time = time;
        while self
            .stepper
            .next_event_time(&self.raid)
            .is_some_and(|next| next <= time)
        {
            let before = self.rebased_by.unwrap_or(0.0);
            self.step();
            time -= self.rebased_by.unwrap_or(0.0) - before;
        }
        time
    }

    fn step(&mut self) -> Option<Event> {
        let start = self.log().len();
        let event = self.stepper.step(&mut self.raid)?;
        self.time = event.time;
        self.mark_procs(start, event.time);
        self.check_pull();
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
                        icon: icon.clone(),
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
        let (resources, resource_totals) = self.resources_so_far();
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
            procs: self.proc_counts_so_far(),
            resources,
            resource_totals,
            rebased_by: self.rebased_by.take(),
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

    /// The character's procs' counts over the combat so far.
    fn proc_counts_so_far(&self) -> Vec<ProcCount> {
        if self.time <= 0.0 {
            return Vec::new();
        }
        let procs = self.raid.character(PLAYER).spells().procs().procs();
        proc_rows_so_far(procs, self.time)
            .into_iter()
            .map(|row| {
                let icon = procs
                    .iter()
                    .filter(|proc| proc.name() == row.name)
                    .flat_map(|proc| std::iter::once(proc.game_id()).chain(proc.payload_spells()))
                    .find_map(|id| self.spell_icon(id));
                ProcCount { row, icon }
            })
            .collect()
    }

    /// The character's resource gains over the combat so far, with their totals. The energy
    /// regenerated since the pull is read from the energy: the statistics learn it at the end.
    fn resources_so_far(&self) -> (Vec<ResourceGain>, Vec<ResourceTotal>) {
        if self.time <= 0.0 {
            return (Vec::new(), Vec::new());
        }
        let character = self.raid.character(PLAYER);
        let shown_at = self.time.max(self.raid.engine().current_time());
        let (regenerated, lost) = character
            .resource()
            .as_energy()
            .map_or((0, 0), |energy| energy.regen_counters(shown_at));
        let statistics = character.statistics();
        let regeneration = (
            REGENERATION.to_string(),
            ResourceType::Energy,
            regenerated as f64,
        );
        let rows = resource_rows_so_far(statistics, [regeneration], self.time);
        let lost_at_cap = |kind| {
            let lost = if kind == ResourceType::Energy {
                lost as f64
            } else {
                0.0
            };
            statistics.lost_at_cap(kind) + lost
        };
        let totals = resource_totals(&rows, lost_at_cap, 1, self.time);
        let rows = rows
            .into_iter()
            .map(|row| {
                let icon = self.source_icon(&row.source);
                ResourceGain { row, icon }
            })
            .collect();
        (rows, totals)
    }

    /// The icon of a resource source (`" (rank N)"` appended above rank 1): a swing's weapon,
    /// a spell's or a proc's.
    fn source_icon(&self, source: &str) -> Option<Icon> {
        let name = source
            .rsplit_once(" (rank ")
            .map_or(source, |(name, _)| name);
        let spells = self.raid.character(PLAYER).spells();
        if name == spells.mh_attack().name() {
            return self.weapon_icon(Hand::Mainhand);
        }
        if name == spells.oh_attack().name() {
            return self.weapon_icon(Hand::Offhand);
        }
        let spell = spells
            .spell_ids()
            .map(|id| spells.spell(id))
            .filter(|spell| spell.name() == name)
            .map(|spell| spell.game_id());
        let procs = spells
            .procs()
            .procs()
            .iter()
            .filter(|proc| proc.name() == name)
            .flat_map(|proc| std::iter::once(proc.game_id()).chain(proc.payload_spells()));
        spell.chain(procs).find_map(|id| self.spell_icon(id))
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
                    icon: Icon::of_spell(spell.record()),
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
    fn spell_icon(&self, id: u32) -> Option<Icon> {
        self.data
            .spells
            .get(id)
            .and_then(|record| Icon::of_spell(record))
    }

    /// The icon of the weapon in `hand`.
    fn weapon_icon(&self, hand: Hand) -> Option<Icon> {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        let equipment = self.raid.character(PLAYER).equipment();
        equipment
            .item(slot)
            .and_then(|item| Icon::of_item(item.spec()))
    }

    /// The spells bar: from the keyboard the keybinds, each named as bound with its main
    /// spell; else the spells the rotation can cast.
    fn bar_spells(&self) -> Vec<(String, SpellId)> {
        let character = self.raid.character(PLAYER);
        let spells = character.spells();
        if self.manual() {
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
        }
    }

    fn character_state(&mut self) -> CharacterState {
        let bar = self.bar_spells();
        let usable: Vec<bool> = {
            let context = self.raid.context(PLAYER);
            bar.iter()
                .map(|&(_, id)| context.spell_requirements(id).is_available())
                .collect()
        };
        let now = self.raid.engine().current_time();
        // The resources at the time shown: energy regenerates without events, and no event
        // between the last one run and the time shown changes it.
        let shown_at = self.time.max(now);
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

        let mut rotation_spells: Vec<CooldownState> = Vec::new();
        for ((name, id), usable) in bar.into_iter().zip(usable) {
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
                    .filter(|cooldown| cooldown.was_used())
                    .map(|cooldown| cooldown.next_use()),
                duration: longest.map_or(0.0, |cooldown| cooldown.base),
                icon: Icon::of_spell(spell.record()),
                on_gcd: spell.triggers_gcd(),
                affordable: spell.resource_type().is_none_or(|resource| {
                    character.resource_level(resource, shown_at)
                        >= spell.resource_cost_with(character.spell_modifiers())
                }),
                usable,
            });
        }

        let resource = character.resource();
        CharacterState {
            resource: ResourceState {
                kind: resource.resource_type().name(),
                current: resource.current(shown_at),
                max: resource.max(),
            },
            combo_points: (character.class_kind() == PlayerClass::Rogue)
                .then(|| character.combo_points(shown_at)),
            stance: Some(character.stance())
                .filter(|&stance| stance != Stance::Caster)
                .map(Stance::name),
            // Never started: the start of the iteration (JSON has no infinity).
            gcd_end: character.next_gcd().max(self.stepper.start_at()),
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
/// The pre-pull a session starts with: the precombat actions' own, or the player's to pull.
fn initial_pre_pull(manual: bool) -> f64 {
    if manual { MANUAL_PRE_PULL } else { 0.0 }
}

fn start(
    settings: &SimSettings,
    seed: u64,
    raid: &mut RaidControl,
    manual: bool,
    pre_pull: f64,
) -> IterationStepper {
    let character = raid.character_mut(PLAYER);
    if manual {
        character.enable_manual_input();
    }
    if let Some(rotation) = character.rotation_mut() {
        rotation.enable_trace();
    }
    IterationStepper::with_pre_pull(settings, seed, raid, pre_pull)
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

/// The spells a keybind can name, by name: the ones a name reaches (rank groups, as
/// `bound_spells` resolves names; spellbook spells and item uses) whose highest learned rank
/// is enabled and cast, not a passive. Sorted by name.
fn bindable_spells(spells: &CharacterSpells) -> Vec<BindableSpell> {
    let mut bindable: Vec<BindableSpell> = spells
        .rank_groups()
        .filter_map(|group| {
            let id = group.get_max_available_spell_rank(|id| spells.spell(id).is_enabled())?;
            let spell = spells.spell(id);
            (!spell.is_passive() && !spell.is_ignored()).then(|| BindableSpell {
                name: group.name().to_owned(),
                icon: Icon::of_spell(spell.record()),
                gcd: spell.triggers_gcd(),
            })
        })
        .collect();
    bindable.sort_by(|a, b| a.name.cmp(&b.name));
    bindable
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

/// An icon of the data: its texture's `FileDataID`, by which the native server serves its local
/// copy (`icons/<id>.png`, from `tools/fetch_icons.py`), and its name, by which Wowhead's CDN
/// serves it (`None` when the community listfile has no name for it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Icon {
    pub id: u32,
    pub name: Option<String>,
}

impl Icon {
    /// The icon `id` named `name`; `None` for 0 (no icon).
    pub fn new(id: u32, name: Option<&str>) -> Option<Icon> {
        (id != 0).then(|| Icon {
            id,
            name: name.map(str::to_owned),
        })
    }

    /// The icon of a spell.
    pub fn of_spell(record: &SpellRecord) -> Option<Icon> {
        Icon::new(record.icon, record.icon_name.as_deref())
    }

    /// The icon of an item.
    pub fn of_item(spec: &ItemSpec) -> Option<Icon> {
        Icon::new(spec.icon, spec.icon_name.as_deref())
    }
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
