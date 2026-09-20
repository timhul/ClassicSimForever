//! The rotation at runtime: executors linked to a character's spells, the precombat actions
//! and the per-executor statistics. Port of `Rotation/Rotation.*` (the runtime half) and
//! `Rotation/RotationExecutor.*`.
//!
//! A [`Rotation`] is built from its [`RotationSpec`] once ([`Rotation::new`], which parses the
//! conditions) and linked to a character whenever the character's spells may have changed
//! ([`Rotation::link`]): every executor whose spell the character has (at the requested rank,
//! enabled) and whose condition names only buffs and spells the character knows becomes
//! *active*; the others are skipped, the way C++ `link_spells` skipped them. The linked
//! condition holds `BuffId` / `SpellId` handles, so evaluating it costs no name lookups.
//!
//! The character owns its rotation (`Character::rotation`); the context takes it out to run
//! it ([`crate::character::context::CharacterContext::perform_rotation`]) since the rotation
//! needs the whole context to check statuses and cast.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::ids::{BuffId, SpellId};
use crate::rotation::condition::{Condition, ConditionContext};
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

/// One `cast_if` of the rotation. Port of `RotationExecutor`.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationExecutor {
    spell_name: String,
    spell_rank: u32,
    /// The condition as written (names), or `None` for an unconditional executor.
    condition: Option<Condition>,
    linked: Option<LinkedExecutor>,
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

    pub fn statistics(&self) -> &ExecutorStatistics {
        &self.statistics
    }
}

/// Where a rotation finds the character's spells and buffs when it links, and what it acts
/// on when it runs. Implemented by `CharacterContext`.
pub trait RotationHost: ConditionContext<BuffId, SpellId> {
    /// The spell `name` at `rank` (`MAX_RANK` = the highest learned rank), if the character
    /// has learned it. Port of `get_spell_rank_group_by_name` + `get_spell_rank`.
    fn spell_by_name(&self, name: &str, rank: u32) -> Option<SpellId>;
    /// The buff `name` the way rotations refer to buffs. Port of `get_buff_by_name`.
    fn buff_by_name(&self, name: &str) -> Option<BuffId>;
    fn spell_is_enabled(&self, spell: SpellId) -> bool;
    fn spell_has_cast_time(&self, spell: SpellId) -> bool;
    /// The spell's cast time now (for the precast).
    fn spell_cast_time(&self, spell: SpellId) -> f64;
    fn spell_status(&self, spell: SpellId) -> SpellStatus;
    /// Casts the spell (statuses were checked by the caller).
    fn cast_spell(&mut self, spell: SpellId);
    /// Whether a cast with casting time is in progress.
    fn is_casting(&self) -> bool;
    /// The character's global cooldown length in seconds.
    fn gcd_length(&self) -> f64;
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
                statistics: ExecutorStatistics::default(),
            })
            .collect();
        Rotation {
            spec,
            executors,
            active: Vec::new(),
            precombat_spells: Vec::new(),
            precast_spell: None,
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

    /// Links the executors, precombat spells and the precast to the character's spells.
    /// Port of `Rotation::link_spells` (+ `add_conditionals`, `link_precombat_spells`,
    /// `link_precast_spell`). An executor is active when its spell exists at the requested
    /// rank and is enabled and every buff / spell its condition names resolves.
    pub fn link(&mut self, host: &impl RotationHost) {
        self.active.clear();
        for (index, executor) in self.executors.iter_mut().enumerate() {
            executor.linked = None;
            let Some(spell) = host.spell_by_name(&executor.spell_name, executor.spell_rank) else {
                continue;
            };
            if !host.spell_is_enabled(spell) {
                continue;
            }
            let condition = match &executor.condition {
                None => None,
                Some(condition) => {
                    let mapped = condition.clone().map(
                        |name| host.buff_by_name(&name),
                        |name| host.spell_by_name(&name, crate::spell::MAX_RANK),
                    );
                    match mapped {
                        Some(mapped) => Some(mapped),
                        None => continue,
                    }
                }
            };
            executor.linked = Some(LinkedExecutor { spell, condition });
            self.active.push(index);
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
    }

    /// Seconds before the pull the iteration has to start at so the precombat actions fit:
    /// the precast's cast time, else one global cooldown. Port of
    /// `Rotation::get_time_required_to_run_precombat`.
    pub fn time_required_to_run_precombat(&self, host: &impl RotationHost) -> f64 {
        match self.precast_spell {
            Some(spell) => host.spell_cast_time(spell),
            None => host.gcd_length(),
        }
    }

    /// Casts the precombat spells that are available (or merely on cooldown, which the
    /// precombat time ignores), then starts the precast if it is enabled. Port of
    /// `Rotation::run_precombat_actions` and the precast lines of `SimControl::run_sim`.
    pub fn run_precombat_actions(&self, host: &mut impl RotationHost) {
        for &spell in &self.precombat_spells {
            if matches!(
                host.spell_status(spell),
                SpellStatus::Available | SpellStatus::OnCooldown
            ) {
                host.cast_spell(spell);
            }
        }
        if let Some(spell) = self.precast_spell {
            if host.spell_is_enabled(spell) {
                host.cast_spell(spell);
            }
        }
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
                host.cast_spell(linked.spell);
            } else {
                executor.statistics.no_condition_group_fulfilled += 1;
            }
        }
    }

    /// Zeroes the executor statistics. Port of `Rotation::prepare_set_of_combat_iterations`
    /// (the statistics objects themselves arrive in Phase 5.5).
    pub fn prepare_set_of_combat_iterations(&mut self) {
        for executor in &mut self.executors {
            executor.statistics.reset();
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
    }

    impl RotationHost for Mock {
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
        ]));
        assert_eq!(rotation.executors().len(), 9);
        assert!(rotation.active_executors().next().is_none());

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
        assert_eq!(rotation.precombat_spells(), [bloodrage]);
        assert_eq!(rotation.precast_spell(), None);
        assert_eq!(
            rotation.executors()[5].conditions_string(),
            "Death Wish buff active"
        );
        let _ = (dw_buff, bloodthirst);
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
        assert!(rotation
            .statistics_by_spell()
            .values()
            .all(|s| *s == ExecutorStatistics::default()));
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
        assert_eq!(host.casts, [bloodrage, shout, aimed]);

        // Not available (GCD) precombat spells and a disabled precast are skipped.
        host.casts.clear();
        host.statuses.insert(shout, SpellStatus::OnGcd);
        host.enabled.insert(aimed, false);
        rotation.run_precombat_actions(&mut host);
        assert_eq!(host.casts, [bloodrage]);

        // A precast without a cast time is not a precast; the precombat time is one GCD.
        host.cast_times.clear();
        rotation.link(&host);
        assert_eq!(rotation.precast_spell(), None);
        assert_eq!(rotation.time_required_to_run_precombat(&host), 1.5);
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
