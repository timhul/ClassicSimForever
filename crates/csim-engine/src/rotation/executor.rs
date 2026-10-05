//! The rotation at runtime: executors linked to a character's spells, the precombat actions
//! and the per-executor statistics. Port of `Rotation/Rotation.*` (the runtime half) and
//! `Rotation/RotationExecutor.*`.
//!
//! A [`Rotation`] is built from its [`RotationSpec`] once ([`Rotation::new`], which parses the
//! conditions) and linked to a character whenever the character's spells may have changed
//! ([`Rotation::link`]): every executor whose spell the character has (at the requested rank,
//! enabled), whose condition names only buffs, spells and talents the character knows and
//! whose talent requirements (`talent "<talent>" greater 0`) the character meets becomes
//! *active*; the others are skipped, the way C++ `link_spells` skipped them, and keep the
//! [`SkipReason`] for the report. The linked
//! condition holds `BuffId` / `SpellId` handles, so evaluating it costs no name lookups.
//!
//! The character owns its rotation (`Character::rotation`); the context takes it out to run
//! it ([`crate::character::context::CharacterContext::perform_rotation`]) since the rotation
//! needs the whole context to check statuses and cast.
//!
//! A rotation can record its decisions ([`Rotation::enable_trace`]): every cast it makes, with
//! the executor that returned true (or the precombat action / precast it was). Off by default;
//! recording changes nothing about what is cast.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::ids::{BuffId, SpellId};
use crate::rotation::condition::{
    BuiltinVariable, Condition, ConditionContext, Measure, NextChange, Test, Watched,
};
use crate::rotation::spec::RotationSpec;
use crate::spell::SpellStatus;

/// What an executor counted since the statistics were last reset. Port of the counters in
/// `RotationExecutor` that `finish_set_of_combat_iterations` hands to
/// `StatisticsRotationExecutor`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutorStatistics {
    /// The executor's spell was available, its condition held and the spell was cast.
    pub successful_casts: u64,
    /// The spell was available but no condition group held.
    pub no_condition_group_fulfilled: u64,
    /// Per status, how often the spell was not available.
    pub spell_status: BTreeMap<SpellStatus, u64>,
}

impl ExecutorStatistics {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Merges `other` into `self` (across executors of the same spell, or across threads).
    pub fn add(&mut self, other: &Self) {
        self.successful_casts += other.successful_casts;
        self.no_condition_group_fulfilled += other.no_condition_group_fulfilled;
        for (status, count) in &other.spell_status {
            *self.spell_status.entry(*status).or_default() += count;
        }
    }

    /// Every attempt: casts, failed conditions and unavailable statuses.
    pub fn attempts(&self) -> u64 {
        self.successful_casts
            + self.no_condition_group_fulfilled
            + self.spell_status.values().sum::<u64>()
    }
}

/// An executor linked to the character's spells.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedExecutor {
    pub spell: SpellId,
    /// `None`: cast whenever the spell is available.
    pub condition: Option<Condition<BuffId, SpellId>>,
}

/// Why an executor was not linked, so it never runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The character has no spell of that name: an item that is not equipped, a racial of
    /// another race, a spell the sim does not give the character, or a typo.
    UnknownSpell,
    /// The spell is known, but not at the requested rank.
    RankNotLearned(u32),
    /// The spell comes from this talent, which the character has not taken.
    TalentNotTaken(String),
    /// The spell is known but not enabled, for another reason than a talent.
    NotEnabled,
    /// The condition names this spell, which the character does not have.
    UnknownConditionSpell(String),
    /// The condition names this talent, which is not in the character's talent tree.
    UnknownConditionTalent(String),
    /// Every condition group needs talent points the character has not spent, the first such
    /// sentence described.
    TalentConditionNotMet(String),
    /// Every condition group needs a buff the character can never have.
    ConditionNeverHolds,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkipReason::UnknownSpell => f.write_str("no spell of this name"),
            SkipReason::RankNotLearned(rank) => write!(f, "rank {rank} not learned"),
            SkipReason::TalentNotTaken(talent) => write!(f, "talent {talent} not taken"),
            SkipReason::NotEnabled => f.write_str("spell not enabled"),
            SkipReason::UnknownConditionSpell(spell) => {
                write!(f, "condition names unknown spell {spell}")
            }
            SkipReason::UnknownConditionTalent(talent) => {
                write!(f, "condition names unknown talent {talent}")
            }
            SkipReason::TalentConditionNotMet(sentence) => {
                write!(f, "condition needs {sentence}")
            }
            SkipReason::ConditionNeverHolds => {
                f.write_str("condition can never hold (it needs buffs the character cannot have)")
            }
        }
    }
}

/// One `cast_if` of the rotation. Port of `RotationExecutor`.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationExecutor {
    spell_name: String,
    spell_rank: u32,
    /// The condition as written (names), or `None` for an unconditional executor.
    condition: Option<Condition>,
    linked: Option<LinkedExecutor>,
    /// Why the last link left the executor inactive.
    skipped: Option<SkipReason>,
    statistics: ExecutorStatistics,
}

impl RotationExecutor {
    pub fn spell_name(&self) -> &str {
        &self.spell_name
    }

    /// The rank asked for; `MAX_RANK` for the highest learned rank.
    pub fn spell_rank(&self) -> u32 {
        self.spell_rank
    }

    /// The condition as written in the file, with names.
    pub fn condition(&self) -> Option<&Condition> {
        self.condition.as_ref()
    }

    /// The condition description for statistics, one sentence per line, groups separated by
    /// `OR`; empty for an unconditional executor. Port of
    /// `RotationExecutor::get_conditions_string`.
    pub fn conditions_string(&self) -> String {
        self.condition
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    /// The link to the character's spells, if the executor is active.
    pub fn linked(&self) -> Option<&LinkedExecutor> {
        self.linked.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.linked.is_some()
    }

    /// Why the executor is inactive; `None` when it is active or not linked yet.
    pub fn skip_reason(&self) -> Option<&SkipReason> {
        self.skipped.as_ref()
    }

    pub fn statistics(&self) -> &ExecutorStatistics {
        &self.statistics
    }
}

/// What made the rotation cast a spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecidedBy {
    /// The executor at this index of [`Rotation::executors`] (the `cast_if` file order):
    /// its spell was available and its condition held.
    Executor(usize),
    /// One of the precombat actions.
    Precombat,
    /// The precast.
    Precast,
    /// Not the rotation: the player's input ([`Character::queue_input`]).
    ///
    /// [`Character::queue_input`]: crate::character::Character::queue_input
    Input,
}

/// One cast the rotation made, recorded while its trace is enabled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationDecision {
    /// Sim time of the cast.
    pub time: f64,
    pub spell: SpellId,
    pub by: DecidedBy,
}

/// Where a rotation finds the character's spells and buffs when it links, and what it acts
/// on when it runs. Implemented by `CharacterContext`.
pub trait RotationHost: ConditionContext<BuffId, SpellId> {
    /// The current sim time (for the decision trace).
    fn now(&self) -> f64;
    /// The spell `name` at `rank` (`MAX_RANK` = the highest learned rank), if the character
    /// has learned it. Port of `get_spell_rank_group_by_name` + `get_spell_rank`.
    fn spell_by_name(&self, name: &str, rank: u32) -> Option<SpellId>;
    /// The buff `name` the way rotations refer to buffs. Port of `get_buff_by_name`.
    fn buff_by_name(&self, name: &str) -> Option<BuffId>;
    fn spell_is_enabled(&self, spell: SpellId) -> bool;
    /// The name of the talent that grants `spell` (any rank of it) when the character has not
    /// taken it.
    fn missing_talent(&self, spell: SpellId) -> Option<String>;
    /// The points spent in the talent `name`; `None` when the character's talent tree has no
    /// such talent.
    fn talent_rank(&self, name: &str) -> Option<u32>;
    fn spell_has_cast_time(&self, spell: SpellId) -> bool;
    /// The spell's cast time now (for the precast).
    fn spell_cast_time(&self, spell: SpellId) -> f64;
    fn spell_status(&self, spell: SpellId) -> SpellStatus;
    /// Casts the spell (statuses were checked by the caller).
    fn cast_spell(&mut self, spell: SpellId);
    /// Whether a cast with casting time is in progress, but for a run to the target (Charge),
    /// during which the spell statuses say what can be cast.
    fn is_casting(&self) -> bool;
    /// The character's global cooldown length in seconds.
    fn gcd_length(&self) -> f64;
    /// The spell's cost now, in the character's resource.
    fn spell_cost(&self, spell: SpellId) -> u32;
}

/// A rotation built from its file and linked to a character. Port of `Rotation`.
#[derive(Debug, Clone)]
pub struct Rotation {
    spec: Arc<RotationSpec>,
    executors: Vec<RotationExecutor>,
    /// Indices into `executors` of the active ones, in priority order.
    active: Vec<usize>,
    precombat_spells: Vec<SpellId>,
    precast_spell: Option<SpellId>,
    /// The spec's prerequisites the character lacks at the last link, with why.
    missing_prerequisites: Vec<(String, SkipReason)>,
    /// The decisions since the set of iterations started, when the trace is enabled.
    trace: Option<Vec<RotationDecision>>,
}

impl Rotation {
    /// Builds the executors of `spec`. The conditions were parsed when the spec was loaded, so
    /// a spec that validated builds; nothing is linked yet.
    ///
    /// # Panics
    /// Panics on a spec whose condition does not parse (`RotationSpec::validate` rejects it).
    pub fn new(spec: Arc<RotationSpec>) -> Self {
        let executors = spec
            .cast_if
            .iter()
            .map(|cast_if| RotationExecutor {
                spell_name: cast_if.name.clone(),
                spell_rank: cast_if.rank(),
                condition: cast_if.parse_condition().unwrap_or_else(|error| {
                    panic!(
                        "rotation {:?}: cast_if {}: {error}",
                        spec.name, cast_if.name
                    )
                }),
                linked: None,
                skipped: None,
                statistics: ExecutorStatistics::default(),
            })
            .collect();
        Rotation {
            spec,
            executors,
            active: Vec::new(),
            precombat_spells: Vec::new(),
            precast_spell: None,
            missing_prerequisites: Vec::new(),
            trace: None,
        }
    }

    /// Starts recording the rotation's decisions (kept across [`Rotation::link`], cleared at
    /// the start of every set of iterations).
    pub fn enable_trace(&mut self) {
        self.trace.get_or_insert_with(Vec::new);
    }

    /// The decisions recorded so far, in the order they were made; empty when the trace is
    /// not enabled.
    pub fn trace(&self) -> &[RotationDecision] {
        self.trace.as_deref().unwrap_or_default()
    }

    /// Records a decision, when the trace is enabled.
    /// Records a cast of the player's input in the trace (when it is enabled), so the trace
    /// lists every cast of a character played by input too.
    pub fn record_input(&mut self, time: f64, spell: SpellId) {
        self.record(time, spell, DecidedBy::Input);
    }

    fn record(&mut self, time: f64, spell: SpellId, by: DecidedBy) {
        if let Some(trace) = &mut self.trace {
            trace.push(RotationDecision { time, spell, by });
        }
    }

    pub fn spec(&self) -> &Arc<RotationSpec> {
        &self.spec
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }

    /// Every executor in file order, active or not.
    pub fn executors(&self) -> &[RotationExecutor] {
        &self.executors
    }

    /// The inactive executors with their 1-based position among the `cast_if` lines and why
    /// they were skipped, in file order.
    pub fn skipped_executors(&self) -> impl Iterator<Item = (usize, &RotationExecutor)> {
        self.executors
            .iter()
            .enumerate()
            .filter(|(_, executor)| executor.skipped.is_some())
            .map(|(index, executor)| (index + 1, executor))
    }

    /// The active executors in priority order.
    pub fn active_executors(&self) -> impl Iterator<Item = &RotationExecutor> {
        self.active.iter().map(|&index| &self.executors[index])
    }

    /// The linked precombat spells, in order.
    pub fn precombat_spells(&self) -> &[SpellId] {
        &self.precombat_spells
    }

    pub fn precast_spell(&self) -> Option<SpellId> {
        self.precast_spell
    }

    /// The spec's prerequisites the character lacked at the last link, with why, in file
    /// order. The rotation is not valid for the character when there is any.
    pub fn missing_prerequisites(&self) -> &[(String, SkipReason)] {
        &self.missing_prerequisites
    }

    /// Links the executors, precombat spells and the precast to the character's spells.
    /// Port of `Rotation::link_spells` (+ `add_conditionals`, `link_precombat_spells`,
    /// `link_precast_spell`). An executor is active when its spell exists at the requested
    /// rank and is enabled, every buff / spell / talent its condition names resolves and a
    /// condition group can still hold with the character's talents.
    pub fn link(&mut self, host: &impl RotationHost) {
        self.active.clear();
        for (index, executor) in self.executors.iter_mut().enumerate() {
            match Self::link_executor(executor, host) {
                Ok(linked) => {
                    executor.linked = Some(linked);
                    executor.skipped = None;
                    self.active.push(index);
                }
                Err(reason) => {
                    executor.linked = None;
                    executor.skipped = Some(reason);
                }
            }
        }

        self.precombat_spells = self
            .spec
            .precombat_actions
            .iter()
            .filter_map(|name| host.spell_by_name(name, crate::spell::MAX_RANK))
            .filter(|&spell| host.spell_is_enabled(spell))
            .collect();

        self.precast_spell = self
            .spec
            .precast
            .as_deref()
            .and_then(|name| host.spell_by_name(name, crate::spell::MAX_RANK))
            .filter(|&spell| host.spell_has_cast_time(spell));

        self.missing_prerequisites = self
            .spec
            .prerequisites
            .iter()
            .filter_map(|name| {
                Self::available_spell(name, crate::spell::MAX_RANK, host)
                    .err()
                    .map(|reason| (name.clone(), reason))
            })
            .collect();
    }

    /// The enabled spell `name` at `rank`, or why the character cannot cast it.
    fn available_spell(
        name: &str,
        rank: u32,
        host: &impl RotationHost,
    ) -> Result<SpellId, SkipReason> {
        use crate::spell::MAX_RANK;

        let Some(spell) = host.spell_by_name(name, rank) else {
            let other_rank = rank != MAX_RANK && host.spell_by_name(name, MAX_RANK).is_some();
            return Err(if other_rank {
                SkipReason::RankNotLearned(rank)
            } else {
                SkipReason::UnknownSpell
            });
        };
        if !host.spell_is_enabled(spell) {
            return Err(host
                .missing_talent(spell)
                .map_or(SkipReason::NotEnabled, SkipReason::TalentNotTaken));
        }
        Ok(spell)
    }

    fn link_executor(
        executor: &RotationExecutor,
        host: &impl RotationHost,
    ) -> Result<LinkedExecutor, SkipReason> {
        use crate::spell::MAX_RANK;

        let spell = Self::available_spell(&executor.spell_name, executor.spell_rank, host)?;
        let Some(condition) = &executor.condition else {
            return Ok(LinkedExecutor {
                spell,
                condition: None,
            });
        };
        let unknown_spell = condition
            .sentences()
            .find_map(|sentence| match &sentence.measure {
                Measure::SpellCooldown(name) if host.spell_by_name(name, MAX_RANK).is_none() => {
                    Some(name.clone())
                }
                _ => None,
            });
        if let Some(name) = unknown_spell {
            return Err(SkipReason::UnknownConditionSpell(name));
        }
        let unknown_talent = condition
            .sentences()
            .find_map(|sentence| match &sentence.measure {
                Measure::Talent(name) if host.talent_rank(name).is_none() => Some(name.clone()),
                _ => None,
            });
        if let Some(name) = unknown_talent {
            return Err(SkipReason::UnknownConditionTalent(name));
        }
        let talent_rank = |name: &str| host.talent_rank(name).unwrap_or_default();
        let Some(linked) = condition.clone().map(
            |name| host.buff_by_name(&name),
            |name| host.spell_by_name(&name, MAX_RANK),
            talent_rank,
        ) else {
            let unmet_talent =
                condition
                    .sentences()
                    .find(|sentence| match (&sentence.measure, sentence.test) {
                        (Measure::Talent(name), Test::Compare(cmp, rhs)) => {
                            !cmp.holds(f64::from(talent_rank(name)), rhs)
                        }
                        _ => false,
                    });
            return Err(
                unmet_talent.map_or(SkipReason::ConditionNeverHolds, |sentence| {
                    SkipReason::TalentConditionNotMet(sentence.to_string())
                }),
            );
        };
        Ok(LinkedExecutor {
            spell,
            condition: Some(linked),
        })
    }

    /// Seconds before the pull the iteration has to start at so the precombat actions fit:
    /// one global cooldown, or the precast's cast time when longer (a shorter precast, Charge,
    /// must not leave a precombat global cooldown running into the pull). Port of
    /// `Rotation::get_time_required_to_run_precombat`.
    pub fn time_required_to_run_precombat(&self, host: &impl RotationHost) -> f64 {
        let precast = self
            .precast_spell
            .map_or(0.0, |spell| host.spell_cast_time(spell));
        precast.max(host.gcd_length())
    }

    /// Casts the precombat spells that are available (or merely on cooldown, which the
    /// precombat time ignores). Port of `Rotation::run_precombat_actions`.
    pub fn run_precombat_actions(&mut self, host: &mut impl RotationHost) {
        for index in 0..self.precombat_spells.len() {
            let spell = self.precombat_spells[index];
            if Self::precombat_castable(host, spell) {
                self.record(host.now(), spell, DecidedBy::Precombat);
                host.cast_spell(spell);
            }
        }
    }

    /// Starts the precast if it is enabled and castable as a precombat spell (Charge needs
    /// Battle Stance). The caller starts it its cast time before the pull. Port of the precast
    /// lines of `SimControl::run_sim`.
    pub fn cast_precast(&mut self, host: &mut impl RotationHost) {
        if let Some(spell) = self.precast_spell
            && host.spell_is_enabled(spell)
            && Self::precombat_castable(host, spell)
        {
            self.record(host.now(), spell, DecidedBy::Precast);
            host.cast_spell(spell);
        }
    }

    fn precombat_castable(host: &impl RotationHost, spell: SpellId) -> bool {
        matches!(
            host.spell_status(spell),
            SpellStatus::Available | SpellStatus::OnCooldown
        )
    }

    /// Lets every active executor attempt its cast, in priority order. Nothing happens
    /// while a cast is in progress. Port of `CharacterSpells::perform_rotation` +
    /// `Rotation::perform_rotation` + `RotationExecutor::attempt_cast`.
    pub fn perform(&mut self, host: &mut impl RotationHost) {
        if host.is_casting() {
            return;
        }
        for &index in &self.active {
            let executor = &mut self.executors[index];
            let linked = executor
                .linked
                .as_ref()
                .expect("active executors are linked");
            let status = host.spell_status(linked.spell);
            if status != SpellStatus::Available {
                *executor.statistics.spell_status.entry(status).or_default() += 1;
                continue;
            }
            let fulfilled = linked
                .condition
                .as_ref()
                .is_none_or(|condition| condition.holds(host));
            if fulfilled {
                executor.statistics.successful_casts += 1;
                let spell = linked.spell;
                if let Some(trace) = &mut self.trace {
                    trace.push(RotationDecision {
                        time: host.now(),
                        spell,
                        by: DecidedBy::Executor(index),
                    });
                }
                host.cast_spell(spell);
            } else {
                executor.statistics.no_condition_group_fulfilled += 1;
            }
        }
    }

    /// When a pass could next cast something the pass now would not, without an event of the
    /// character's own (see [`NextChange`]): an executor held back by the `watched` resource
    /// becomes affordable, the execute phase starts, a condition of an available spell flips. [`NextChange::NOW`] when an executor is castable already. Rounded
    /// early, never late: a pass at the time given may still cast nothing.
    pub fn next_change(&self, host: &impl RotationHost, watched: Watched) -> NextChange {
        if host.is_casting() {
            return NextChange::NEVER;
        }
        let mut next = NextChange::NEVER;
        for executor in self.active_executors() {
            let linked = executor
                .linked
                .as_ref()
                .expect("active executors are linked");
            let change = match host.spell_status(linked.spell) {
                SpellStatus::Available => match &linked.condition {
                    Some(condition) if !condition.holds(host) => {
                        condition.next_change(host, watched)
                    }
                    _ => return NextChange::NOW,
                },
                SpellStatus::InsufficientResources => {
                    NextChange::at_level(host.spell_cost(linked.spell))
                }
                SpellStatus::NotInExecuteRange => {
                    NextChange::after(host.variable(BuiltinVariable::TimeRemainingExecute))
                }
                // The end of a global cooldown, a cooldown, a cast or a stance cooldown wakes
                // the player on its own; the rest changes through events (combo points,
                // stances, buffs).
                _ => NextChange::NEVER,
            };
            next = next.or(change);
        }
        next
    }

    /// Zeroes the executor statistics. Port of `Rotation::prepare_set_of_combat_iterations`
    /// (`crate::statistics::RotationExecutorStatistics` snapshots them for reporting).
    pub fn prepare_set_of_combat_iterations(&mut self) {
        for executor in &mut self.executors {
            executor.statistics.reset();
        }
        if let Some(trace) = &mut self.trace {
            trace.clear();
        }
    }

    /// The statistics of the active executors, merged per spell name (a spell with several
    /// executors reports once, as `ClassStatistics::get_executor_statistics` keyed by name).
    pub fn statistics_by_spell(&self) -> BTreeMap<&str, ExecutorStatistics> {
        let mut by_spell: BTreeMap<&str, ExecutorStatistics> = BTreeMap::new();
        for executor in self.active_executors() {
            by_spell
                .entry(executor.spell_name.as_str())
                .or_default()
                .add(&executor.statistics);
        }
        by_spell
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceType;
    use crate::rotation::condition::BuiltinVariable;
    use crate::rotation::spec::CastIfSpec;
    use crate::spell::MAX_RANK;
    use crate::target::CreatureType;
    use std::collections::HashMap;

    /// A host with a fixed set of spells (name → id, rank), buffs and statuses that records
    /// its casts.
    #[derive(Default)]
    struct Mock {
        spells: HashMap<(String, u32), SpellId>,
        enabled: HashMap<SpellId, bool>,
        cast_times: HashMap<SpellId, f64>,
        statuses: HashMap<SpellId, SpellStatus>,
        buffs: HashMap<String, BuffId>,
        buff_time_left: HashMap<BuffId, f64>,
        cooldowns: HashMap<SpellId, f64>,
        resources: HashMap<ResourceType, u32>,
        variables: HashMap<BuiltinVariable, f64>,
        casting: bool,
        casts: Vec<SpellId>,
        talents: HashMap<SpellId, String>,
        talent_ranks: HashMap<String, u32>,
        costs: HashMap<SpellId, u32>,
        now: f64,
    }

    impl Mock {
        fn spell(&mut self, name: &str, rank: u32, id: u32) -> SpellId {
            let id = SpellId(id);
            self.spells.insert((name.to_string(), rank), id);
            self.enabled.insert(id, true);
            id
        }
        fn buff(&mut self, name: &str, id: u32) -> BuffId {
            let id = BuffId(id);
            self.buffs.insert(name.to_string(), id);
            id
        }
    }

    impl ConditionContext<BuffId, SpellId> for Mock {
        fn buff_time_left(&self, buff: &BuffId) -> f64 {
            self.buff_time_left.get(buff).copied().unwrap_or(0.0)
        }
        fn buff_is_active(&self, buff: &BuffId) -> bool {
            self.buff_time_left(buff) > 0.0
        }
        fn buff_stacks(&self, buff: &BuffId) -> u32 {
            u32::from(self.buff_is_active(buff))
        }
        fn spell_cooldown_remaining(&self, spell: &SpellId) -> f64 {
            self.cooldowns.get(spell).copied().unwrap_or(0.0)
        }
        fn resource_level(&self, resource: ResourceType) -> u32 {
            self.resources.get(&resource).copied().unwrap_or(0)
        }
        fn variable(&self, variable: BuiltinVariable) -> f64 {
            self.variables.get(&variable).copied().unwrap_or(0.0)
        }
        fn target_creature_type(&self) -> CreatureType {
            CreatureType::Dragonkin
        }
    }

    impl RotationHost for Mock {
        fn now(&self) -> f64 {
            self.now
        }
        fn spell_by_name(&self, name: &str, rank: u32) -> Option<SpellId> {
            let requested = self.spells.get(&(name.to_string(), rank)).copied();
            if rank != MAX_RANK {
                return requested;
            }
            // MAX_RANK: the highest rank registered under the name.
            self.spells
                .iter()
                .filter(|((n, _), _)| n == name)
                .max_by_key(|((_, r), _)| *r)
                .map(|(_, id)| *id)
        }
        fn buff_by_name(&self, name: &str) -> Option<BuffId> {
            self.buffs.get(name).copied()
        }
        fn spell_is_enabled(&self, spell: SpellId) -> bool {
            self.enabled.get(&spell).copied().unwrap_or(false)
        }
        fn missing_talent(&self, spell: SpellId) -> Option<String> {
            self.talents.get(&spell).cloned()
        }
        fn talent_rank(&self, name: &str) -> Option<u32> {
            self.talent_ranks.get(name).copied()
        }
        fn spell_has_cast_time(&self, spell: SpellId) -> bool {
            self.cast_times.contains_key(&spell)
        }
        fn spell_cast_time(&self, spell: SpellId) -> f64 {
            self.cast_times.get(&spell).copied().unwrap_or(0.0)
        }
        fn spell_status(&self, spell: SpellId) -> SpellStatus {
            self.statuses
                .get(&spell)
                .copied()
                .unwrap_or(SpellStatus::Available)
        }
        fn cast_spell(&mut self, spell: SpellId) {
            self.casts.push(spell);
        }
        fn is_casting(&self) -> bool {
            self.casting
        }
        fn gcd_length(&self) -> f64 {
            1.5
        }
        fn spell_cost(&self, spell: SpellId) -> u32 {
            self.costs.get(&spell).copied().unwrap_or(0)
        }
    }

    fn spec(cast_if: Vec<CastIfSpec>) -> Arc<RotationSpec> {
        Arc::new(RotationSpec {
            class: crate::faction::PlayerClass::Warrior,
            name: "Test".to_string(),
            attack_mode: crate::attack_mode::AttackMode::MeleeAttack,
            description: String::new(),
            precombat_actions: vec!["Bloodrage".to_string(), "Battle Shout".to_string()],
            precast: None,
            cast_if,
            prerequisites: Vec::new(),
        })
    }

    fn names<'a>(executors: impl Iterator<Item = &'a RotationExecutor>) -> Vec<&'a str> {
        executors.map(RotationExecutor::spell_name).collect()
    }

    #[test]
    fn linking_keeps_the_executors_the_character_can_use() {
        let mut host = Mock::default();
        let bloodrage = host.spell("Bloodrage", 1, 1);
        host.spell("Heroic Strike", 8, 2);
        let hs9 = host.spell("Heroic Strike", 9, 3);
        let death_wish = host.spell("Death Wish", 1, 4);
        host.enabled.insert(death_wish, false);
        let overpower = host.spell("Overpower", 1, 5);
        let bloodthirst = host.spell("Bloodthirst", 1, 6);
        let dw_buff = host.buff("Death Wish", 10);
        let spearing_strike = host.spell("Spearing Strike", 1, 7);
        host.enabled.insert(spearing_strike, false);
        host.talents
            .insert(spearing_strike, "Spearing Strike".to_string());

        let mut rotation = Rotation::new(spec(vec![
            CastIfSpec::when("Bloodrage", "resource \"Rage\" less 70"),
            CastIfSpec::always("Kiss of the Spider"),
            CastIfSpec::when(
                "Death Wish",
                "variable \"time_remaining_encounter\" less 33",
            ),
            CastIfSpec::when("Whirlwind", "spell \"Bloodthirst\" greater 1.5"),
            CastIfSpec::when("Overpower", "buff_duration \"Overpower Buff\" is true"),
            CastIfSpec::when("Overpower", "buff_duration \"Death Wish\" is true"),
            CastIfSpec {
                rank: Some(8),
                ..CastIfSpec::always("Heroic Strike")
            },
            CastIfSpec {
                rank: Some(7),
                ..CastIfSpec::always("Heroic Strike")
            },
            CastIfSpec::when("Heroic Strike", "spell \"Bloodthirst\" less 1"),
            CastIfSpec::always("Spearing Strike"),
            CastIfSpec::when("Bloodrage", "spell \"Rampage\" less 1"),
        ]));
        assert_eq!(rotation.executors().len(), 11);
        assert!(rotation.active_executors().next().is_none());
        assert!(
            rotation.skipped_executors().next().is_none(),
            "not linked yet"
        );

        rotation.link(&host);
        assert_eq!(
            names(rotation.active_executors()),
            ["Bloodrage", "Overpower", "Heroic Strike", "Heroic Strike"]
        );
        let active: Vec<&LinkedExecutor> = rotation
            .active_executors()
            .map(|e| e.linked().unwrap())
            .collect();
        assert_eq!(active[0].spell, bloodrage);
        assert!(active[0].condition.is_some());
        assert_eq!(active[1].spell, overpower);
        assert_eq!(active[2].spell, SpellId(2), "rank 8 was asked for");
        assert!(active[2].condition.is_none());
        assert_eq!(active[3].spell, hs9, "MAX_RANK");
        // The unlinkable ones say why: unknown spell, disabled, unknown buff, unknown rank.
        let executors = rotation.executors();
        assert!(!executors[1].is_active(), "Kiss of the Spider is not known");
        assert!(!executors[2].is_active(), "Death Wish is disabled");
        assert!(!executors[3].is_active(), "Whirlwind is not known");
        assert!(!executors[4].is_active(), "Overpower Buff is not known");
        assert!(
            !executors[7].is_active(),
            "Heroic Strike rank 7 is not known"
        );
        let skipped: Vec<(usize, &str, Option<SkipReason>)> = rotation
            .skipped_executors()
            .map(|(line, e)| (line, e.spell_name(), e.skip_reason().cloned()))
            .collect();
        assert_eq!(
            skipped,
            [
                (2, "Kiss of the Spider", Some(SkipReason::UnknownSpell)),
                (3, "Death Wish", Some(SkipReason::NotEnabled)),
                (4, "Whirlwind", Some(SkipReason::UnknownSpell)),
                (5, "Overpower", Some(SkipReason::ConditionNeverHolds)),
                (8, "Heroic Strike", Some(SkipReason::RankNotLearned(7))),
                (
                    10,
                    "Spearing Strike",
                    Some(SkipReason::TalentNotTaken("Spearing Strike".to_string()))
                ),
                (
                    11,
                    "Bloodrage",
                    Some(SkipReason::UnknownConditionSpell("Rampage".to_string()))
                ),
            ]
        );
        assert_eq!(
            SkipReason::TalentNotTaken("Spearing Strike".to_string()).to_string(),
            "talent Spearing Strike not taken"
        );
        assert_eq!(
            SkipReason::RankNotLearned(7).to_string(),
            "rank 7 not learned"
        );
        assert_eq!(rotation.precombat_spells(), [bloodrage]);
        assert_eq!(rotation.precast_spell(), None);
        assert_eq!(
            rotation.executors()[5].conditions_string(),
            "Death Wish buff active"
        );
        let _ = (dw_buff, bloodthirst);
    }

    #[test]
    fn linking_records_the_prerequisites_the_character_lacks() {
        let mut host = Mock::default();
        host.spell("Bloodthirst", 1, 1);
        let mortal_strike = host.spell("Mortal Strike", 1, 2);
        host.enabled.insert(mortal_strike, false);
        host.talents
            .insert(mortal_strike, "Mortal Strike".to_string());
        let mut spec = Arc::unwrap_or_clone(spec(vec![CastIfSpec::always("Bloodthirst")]));
        spec.prerequisites = vec![
            "Bloodthirst".to_string(),
            "Mortal Strike".to_string(),
            "Rampage".to_string(),
        ];
        let mut rotation = Rotation::new(Arc::new(spec));
        assert!(
            rotation.missing_prerequisites().is_empty(),
            "not linked yet"
        );
        rotation.link(&host);
        assert_eq!(
            rotation.missing_prerequisites(),
            [
                (
                    "Mortal Strike".to_string(),
                    SkipReason::TalentNotTaken("Mortal Strike".to_string())
                ),
                ("Rampage".to_string(), SkipReason::UnknownSpell),
            ]
        );
        host.enabled.insert(mortal_strike, true);
        host.spell("Rampage", 1, 3);
        rotation.link(&host);
        assert!(rotation.missing_prerequisites().is_empty());
    }

    #[test]
    fn talent_conditions_link_only_with_the_points_spent() {
        let mut host = Mock::default();
        host.spell("Berserker Rage", 1, 1);
        host.talent_ranks
            .insert("Improved Berserker Rage".to_string(), 0);
        let mut rotation = Rotation::new(spec(vec![
            CastIfSpec::when(
                "Berserker Rage",
                "talent \"Improved Berserker Rage\" greater 0
                 and resource \"Rage\" less 50",
            ),
            CastIfSpec::when("Berserker Rage", "talent \"Improved Berserker Rag\" eq 0"),
        ]));
        rotation.link(&host);
        assert!(rotation.active_executors().next().is_none());
        let reasons: Vec<String> = rotation
            .executors()
            .iter()
            .map(|e| e.skip_reason().unwrap().to_string())
            .collect();
        assert_eq!(
            reasons,
            [
                "condition needs Improved Berserker Rage talent rank > 0",
                "condition names unknown talent Improved Berserker Rag",
            ]
        );

        // With a point spent the talent sentence holds and drops out of the linked condition.
        host.talent_ranks
            .insert("Improved Berserker Rage".to_string(), 1);
        rotation.link(&host);
        let linked: Vec<&LinkedExecutor> = rotation
            .active_executors()
            .map(|e| e.linked().unwrap())
            .collect();
        assert_eq!(linked.len(), 1);
        assert_eq!(
            linked[0].condition.as_ref().unwrap().groups(),
            [vec![crate::rotation::condition::Sentence {
                measure: Measure::Resource(ResourceType::Rage),
                test: Test::Compare(crate::rotation::condition::Comparator::Less, 50.0),
            }]]
        );
        assert_eq!(
            rotation.executors()[0].conditions_string(),
            "Improved Berserker Rage talent rank > 0
Rage < 50"
        );
    }

    #[test]
    fn relinking_replaces_the_previous_links() {
        let mut host = Mock::default();
        let dw = host.spell("Death Wish", 1, 4);
        host.enabled.insert(dw, false);
        let mut rotation = Rotation::new(spec(vec![CastIfSpec::always("Death Wish")]));
        rotation.link(&host);
        assert!(rotation.active_executors().next().is_none());
        host.enabled.insert(dw, true);
        rotation.link(&host);
        assert_eq!(names(rotation.active_executors()), ["Death Wish"]);
        host.enabled.insert(dw, false);
        rotation.link(&host);
        assert!(rotation.active_executors().next().is_none());
        assert!(!rotation.executors()[0].is_active());
    }

    #[test]
    fn next_change_is_what_the_blocked_executors_wait_for() {
        let watched = Watched {
            resource: ResourceType::Energy,
            max: 100,
            encounter_length: 300.0,
        };
        let mut host = Mock::default();
        let eviscerate = host.spell("Eviscerate", 1, 1);
        let slice = host.spell("Slice and Dice", 1, 2);
        let strike = host.spell("Sinister Strike", 1, 3);
        let slice_buff = host.buff("Slice and Dice", 10);
        host.resources.insert(ResourceType::Energy, 20);
        host.statuses
            .insert(eviscerate, SpellStatus::InsufficientComboPoints);
        host.statuses
            .insert(strike, SpellStatus::InsufficientResources);
        host.costs.insert(strike, 45);
        host.buff_time_left.insert(slice_buff, 6.0);
        let mut rotation = Rotation::new(spec(vec![
            CastIfSpec::always("Eviscerate"),
            CastIfSpec::when("Slice and Dice", "buff_duration \"Slice and Dice\" less 2"),
            CastIfSpec::always("Sinister Strike"),
        ]));
        rotation.link(&host);
        // Combo points come with events; Slice and Dice runs low in 4 s; Sinister Strike needs
        // 45 energy.
        let next = rotation.next_change(&host, watched);
        assert!((next.delay - 4.0).abs() < 0.001, "{next:?}");
        assert_eq!(next.level, Some(45));
        // Castable already.
        host.statuses.insert(strike, SpellStatus::Available);
        assert_eq!(rotation.next_change(&host, watched), NextChange::NOW);
        // The end of a global cooldown or a cast wakes the player on its own.
        for status in [SpellStatus::OnGcd, SpellStatus::OnCooldown] {
            host.statuses.insert(strike, status);
            host.statuses.insert(slice, status);
            assert_eq!(rotation.next_change(&host, watched), NextChange::NEVER);
        }
        host.statuses.insert(strike, SpellStatus::Available);
        host.casting = true;
        assert_eq!(rotation.next_change(&host, watched), NextChange::NEVER);
    }

    #[test]
    fn performing_casts_available_spells_whose_condition_holds() {
        let mut host = Mock::default();
        let bloodrage = host.spell("Bloodrage", 1, 1);
        let bloodthirst = host.spell("Bloodthirst", 1, 2);
        let whirlwind = host.spell("Whirlwind", 1, 3);
        let overpower = host.spell("Overpower", 1, 4);
        host.statuses
            .insert(overpower, SpellStatus::InBerserkerStance);
        host.resources.insert(ResourceType::Rage, 80);
        host.cooldowns.insert(bloodthirst, 0.0);

        let mut rotation = Rotation::new(spec(vec![
            CastIfSpec::when("Bloodrage", "resource \"Rage\" less 70"),
            CastIfSpec::always("Bloodthirst"),
            CastIfSpec::when("Whirlwind", "spell \"Bloodthirst\" greater 1.5"),
            CastIfSpec::always("Overpower"),
        ]));
        rotation.link(&host);
        rotation.perform(&mut host);
        assert_eq!(host.casts, [bloodthirst]);

        // Bloodthirst went on cooldown, rage dropped: Bloodrage and Whirlwind go off.
        host.statuses.insert(bloodthirst, SpellStatus::OnCooldown);
        host.cooldowns.insert(bloodthirst, 5.0);
        host.resources.insert(ResourceType::Rage, 20);
        host.casts.clear();
        rotation.perform(&mut host);
        assert_eq!(host.casts, [bloodrage, whirlwind]);

        let stats = rotation.statistics_by_spell();
        assert_eq!(stats["Bloodrage"].successful_casts, 1);
        assert_eq!(stats["Bloodrage"].no_condition_group_fulfilled, 1);
        assert_eq!(stats["Bloodthirst"].successful_casts, 1);
        assert_eq!(
            stats["Bloodthirst"].spell_status[&SpellStatus::OnCooldown],
            1
        );
        assert_eq!(stats["Whirlwind"].successful_casts, 1);
        assert_eq!(stats["Whirlwind"].no_condition_group_fulfilled, 1);
        assert_eq!(stats["Overpower"].successful_casts, 0);
        assert_eq!(
            stats["Overpower"].spell_status[&SpellStatus::InBerserkerStance],
            2
        );
        assert_eq!(stats["Overpower"].attempts(), 2);

        rotation.prepare_set_of_combat_iterations();
        assert!(
            rotation
                .statistics_by_spell()
                .values()
                .all(|s| *s == ExecutorStatistics::default())
        );
    }

    #[test]
    fn nothing_happens_while_a_cast_is_in_progress() {
        let mut host = Mock::default();
        host.spell("Bloodthirst", 1, 2);
        host.casting = true;
        let mut rotation = Rotation::new(spec(vec![CastIfSpec::always("Bloodthirst")]));
        rotation.link(&host);
        rotation.perform(&mut host);
        assert!(host.casts.is_empty());
        assert_eq!(rotation.statistics_by_spell()["Bloodthirst"].attempts(), 0);
    }

    #[test]
    fn two_executors_of_one_spell_report_together() {
        let mut host = Mock::default();
        let hs = host.spell("Heroic Strike", 9, 3);
        host.resources.insert(ResourceType::Rage, 60);
        let mut rotation = Rotation::new(spec(vec![
            CastIfSpec::when("Heroic Strike", "resource \"Rage\" greater 70"),
            CastIfSpec::when("Heroic Strike", "resource \"Rage\" greater 50"),
        ]));
        rotation.link(&host);
        rotation.perform(&mut host);
        assert_eq!(host.casts, [hs]);
        let stats = rotation.statistics_by_spell();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats["Heroic Strike"].successful_casts, 1);
        assert_eq!(stats["Heroic Strike"].no_condition_group_fulfilled, 1);
    }

    #[test]
    fn precombat_casts_available_and_cooling_spells_then_the_precast() {
        let mut host = Mock::default();
        let bloodrage = host.spell("Bloodrage", 1, 1);
        let shout = host.spell("Battle Shout", 7, 2);
        let aimed = host.spell("Aimed Shot", 6, 3);
        host.cast_times.insert(aimed, 3.0);
        host.statuses.insert(bloodrage, SpellStatus::OnCooldown);
        let mut spec = (*spec(Vec::new())).clone();
        spec.precast = Some("Aimed Shot".to_string());
        let mut rotation = Rotation::new(Arc::new(spec));
        rotation.link(&host);
        assert_eq!(rotation.precombat_spells(), [bloodrage, shout]);
        assert_eq!(rotation.precast_spell(), Some(aimed));
        assert_eq!(rotation.time_required_to_run_precombat(&host), 3.0);

        rotation.run_precombat_actions(&mut host);
        rotation.cast_precast(&mut host);
        assert_eq!(host.casts, [bloodrage, shout, aimed]);

        // Not available (GCD) precombat spells and a disabled precast are skipped.
        host.casts.clear();
        host.statuses.insert(shout, SpellStatus::OnGcd);
        host.enabled.insert(aimed, false);
        rotation.run_precombat_actions(&mut host);
        rotation.cast_precast(&mut host);
        assert_eq!(host.casts, [bloodrage]);

        // So is a precast that cannot be cast (Charge outside Battle Stance).
        host.casts.clear();
        host.enabled.insert(aimed, true);
        host.statuses.insert(aimed, SpellStatus::InBerserkerStance);
        rotation.cast_precast(&mut host);
        assert!(host.casts.is_empty());

        // A precast shorter than the global cooldown leaves the precombat time at one GCD.
        host.cast_times.insert(aimed, 1.0);
        assert_eq!(rotation.time_required_to_run_precombat(&host), 1.5);

        // A precast without a cast time is not a precast; the precombat time is one GCD.
        host.cast_times.clear();
        rotation.link(&host);
        assert_eq!(rotation.precast_spell(), None);
        assert_eq!(rotation.time_required_to_run_precombat(&host), 1.5);
    }

    #[test]
    fn the_trace_records_which_executor_cast() {
        let mut host = Mock::default();
        let bloodrage = host.spell("Bloodrage", 1, 1);
        let shout = host.spell("Battle Shout", 7, 2);
        let bloodthirst = host.spell("Bloodthirst", 1, 3);
        let hs = host.spell("Heroic Strike", 9, 4);
        host.resources.insert(ResourceType::Rage, 60);
        let mut rotation = Rotation::new(spec(vec![
            // Skipped (no such spell): the indices stay those of the file.
            CastIfSpec::always("Kiss of the Spider"),
            CastIfSpec::when("Bloodrage", "resource \"Rage\" less 50"),
            CastIfSpec::always("Bloodthirst"),
            CastIfSpec::when("Heroic Strike", "resource \"Rage\" greater 70"),
            CastIfSpec::when("Heroic Strike", "resource \"Rage\" greater 50"),
        ]));
        rotation.link(&host);

        // Off by default.
        host.now = -1.5;
        rotation.run_precombat_actions(&mut host);
        rotation.perform(&mut host);
        assert!(rotation.trace().is_empty());

        rotation.enable_trace();
        rotation.run_precombat_actions(&mut host);
        host.now = 2.25;
        rotation.perform(&mut host);
        let decision = |time, spell, by| RotationDecision { time, spell, by };
        assert_eq!(
            rotation.trace(),
            [
                decision(-1.5, bloodrage, DecidedBy::Precombat),
                decision(-1.5, shout, DecidedBy::Precombat),
                decision(2.25, bloodthirst, DecidedBy::Executor(2)),
                decision(2.25, hs, DecidedBy::Executor(4)),
            ]
        );

        // Kept across a relink, cleared for a new set of iterations.
        rotation.link(&host);
        assert_eq!(rotation.trace().len(), 4);
        rotation.prepare_set_of_combat_iterations();
        assert!(rotation.trace().is_empty());
        rotation.perform(&mut host);
        assert_eq!(rotation.trace().len(), 2, "still enabled");
    }

    #[test]
    fn statistics_merge() {
        let mut a = ExecutorStatistics {
            successful_casts: 2,
            no_condition_group_fulfilled: 1,
            spell_status: [(SpellStatus::OnGcd, 3)].into_iter().collect(),
        };
        let b = ExecutorStatistics {
            successful_casts: 1,
            no_condition_group_fulfilled: 0,
            spell_status: [(SpellStatus::OnGcd, 1), (SpellStatus::OnCooldown, 4)]
                .into_iter()
                .collect(),
        };
        a.add(&b);
        assert_eq!(a.successful_casts, 3);
        assert_eq!(a.no_condition_group_fulfilled, 1);
        assert_eq!(a.spell_status[&SpellStatus::OnGcd], 4);
        assert_eq!(a.spell_status[&SpellStatus::OnCooldown], 4);
        assert_eq!(a.attempts(), 12);
    }
}
