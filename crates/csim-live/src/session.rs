//! One watched iteration: the character's raid, its [`IterationStepper`] and what the page has
//! been shown of it.
//!
//! The raid is built and seeded exactly as `csim run --combat-log` builds and seeds it, so with
//! the same seed, length and variance the session runs the iteration that command logs. Each
//! call moves the iteration forward and returns a [`Frame`]: the damage dealt since the last
//! frame (read from the combat log, which a step only appends to), the rotation's decisions
//! since the last frame (its decision trace, enabled for the session) and the character's
//! state. Every time in a frame is absolute sim time, so the page can animate between frames.

use std::sync::Arc;

use csim_engine::character_loader::CharacterSetup;
use csim_engine::combat_log::{CombatLogEvent, LogUnit};
use csim_engine::data_bundle::DataBundle;
use csim_engine::engine::Event;
use csim_engine::faction::PlayerClass;
use csim_engine::ids::CharId;
use csim_engine::item::EquipmentSlot;
use csim_engine::raid::RaidControl;
use csim_engine::rotation::DecidedBy;
use csim_engine::sim_control::IterationStepper;
use csim_engine::sim_settings::SimSettings;
use csim_engine::spell::Hand;
use csim_engine::stance::Stance;
use serde::Serialize;

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
    /// The rotation's precombat actions the character can cast, in order.
    pub precombat: Vec<String>,
}

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
    /// `entry` (a `cast_if` entry returned true), `precombat` or `precast`.
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
    pub state: CharacterState,
}

/// One hit of the damage feed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DamageNumber {
    pub time: f64,
    pub amount: u32,
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
    /// The cooldowns of the rotation's spells, in rotation order.
    pub cooldowns: Vec<CooldownState>,
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
}

/// One iteration being watched. See the module documentation.
pub struct Session {
    data: Arc<DataBundle>,
    setup: CharacterSetup,
    settings: SimSettings,
    seed: u64,
    raid: RaidControl,
    stepper: IterationStepper,
    /// The combat log entries already read into frames.
    read: usize,
    /// The rotation decisions already read into frames.
    decisions_read: usize,
    total_damage: u64,
    /// The latest time shown.
    time: f64,
}

impl Session {
    /// Builds the setup under `settings` and starts its iteration of `seed`.
    ///
    /// # Errors
    /// The setup does not build against the data, or the settings are invalid.
    pub fn new(
        data: Arc<DataBundle>,
        setup: CharacterSetup,
        settings: SimSettings,
        seed: u64,
    ) -> Result<Session, String> {
        settings.validate().map_err(|error| error.to_string())?;
        let mut raid = setup
            .build_raid(&data, &settings)
            .map_err(|error| error.to_string())?;
        let stepper = start(&settings, seed, &mut raid);
        let time = stepper.start_at();
        Ok(Session {
            data,
            setup,
            settings,
            seed,
            raid,
            stepper,
            read: 0,
            decisions_read: 0,
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
        self.stepper = start(&self.settings, seed, &mut raid);
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
            precombat: character.rotation().map_or_else(Vec::new, |rotation| {
                let spells = character.spells();
                rotation
                    .precombat_spells()
                    .iter()
                    .map(|&id| spells.spell(id).name().to_owned())
                    .collect()
            }),
        }
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
        self.stepper.step_until(&mut self.raid, time);
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
        let event = self.stepper.step(&mut self.raid)?;
        self.time = event.time;
        Some(event)
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
        let damage: Vec<DamageNumber> = self.log()[self.read..]
            .iter()
            .filter(|entry| entry.source == LogUnit::Character(PLAYER))
            .filter_map(|entry| self.damage_number(entry.time, &entry.event))
            .collect();
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
            state: self.character_state(),
        }
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

    /// The damage number of a damage event; `None` for every other event.
    pub(crate) fn damage_number(&self, time: f64, event: &CombatLogEvent) -> Option<DamageNumber> {
        let (damage, auto, name, icon) = match event {
            CombatLogEvent::SwingDamage { hand, damage, .. } => {
                (damage, true, hand_name(*hand), self.weapon_icon(*hand))
            }
            CombatLogEvent::SpellDamage { spell, damage, .. }
            | CombatLogEvent::SpellPeriodicDamage { spell, damage, .. } => {
                (damage, false, spell.name.clone(), self.spell_icon(spell.id))
            }
            _ => return None,
        };
        Some(DamageNumber {
            time,
            amount: damage.amount,
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
        let buffs = spells
            .buff_ids()
            .filter_map(|id| match spells.buff_slot(id) {
                csim_engine::character_spells::BuffSlot::Owned(buff) => Some(&**buff),
                csim_engine::character_spells::BuffSlot::Shared(shared) => {
                    self.raid.shared_buffs().buffs().get(shared.index())
                }
            })
            .filter(|buff| buff.is_active() && !buff.is_hidden() && !buff.is_debuff())
            .map(|buff| BuffState {
                name: buff.name().to_owned(),
                stacks: buff.stacks(),
                charges: buff.charges(),
                expires_at: buff.duration().map(|_| now + buff.time_left(now)),
                duration: buff.duration(),
                icon: self.spell_icon(buff.spell()),
            })
            .collect();

        let mut cooldowns: Vec<CooldownState> = Vec::new();
        let linked = character
            .rotation()
            .into_iter()
            .flat_map(|rotation| rotation.active_executors())
            .filter_map(|executor| executor.linked());
        for executor in linked {
            let spell = spells.spell(executor.spell);
            let longest = spell
                .cooldown_ids()
                .map(|id| spells.cooldowns().get(id))
                .max_by(|a, b| a.next_use().total_cmp(&b.next_use()));
            let Some(cooldown) = longest else { continue };
            // Stance swaps and the like are no longer than the global cooldown.
            if cooldown.base <= character.global_cooldown()
                || cooldowns.iter().any(|known| known.name == spell.name())
            {
                continue;
            }
            cooldowns.push(CooldownState {
                name: spell.name().to_owned(),
                ready_at: (cooldown.last_used != -cooldown.base).then(|| cooldown.next_use()),
                duration: cooldown.base,
                icon: icon(spell.record().icon),
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
            cooldowns,
        }
    }
}

fn as_string<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

/// Enables the character's decision trace (the precombat actions are decided when the
/// iteration starts) and starts the iteration of `seed`.
fn start(settings: &SimSettings, seed: u64, raid: &mut RaidControl) -> IterationStepper {
    if let Some(rotation) = raid.character_mut(PLAYER).rotation_mut() {
        rotation.enable_trace();
    }
    IterationStepper::new(settings, seed, raid)
}

/// An icon `FileDataID` of the data, `None` for 0 (no icon).
pub fn icon(file_data_id: u32) -> Option<u32> {
    Some(file_data_id).filter(|&id| id != 0)
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
