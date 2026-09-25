//! Simulation statistics. Port of `Statistics/*` minus the GUI models and the stat-weight
//! cruncher, which is [`NumberCruncher`].
//!
//! A [`ClassStatistics`] belongs to one character and collects, for one set of combat
//! iterations: the [`SpellStatistics`] of every spell, auto attack and proc that dealt
//! damage, the [`BuffStatistics`] of every non-hidden buff, the [`ResourceStatistics`] of
//! every resource source, the [`ProcStatistics`] of every proc, the
//! [`RotationExecutorStatistics`] of the rotation, the [`EngineStatistics`] of the engine and
//! the DPS of each iteration. The character's context records into it as the reports of its
//! casts, swings and ticks come back (the C++ pushed from inside `Spell`), and
//! [`ClassStatistics::add`] merges the results of the threads.
//!
//! Spells and resource sources are keyed by name and rank ([`SpellKey`]) — the C++ appended
//! `" (rank N)"` to the name for ranks above one ([`SpellKey::display_name`]). The number of
//! iterations is counted from [`ClassStatistics::finish_combat_iteration`] instead of read
//! from the settings, so a partial run reports against the iterations it actually ran.

pub mod buff;
pub mod engine;
pub mod executor;
pub mod number_cruncher;
pub mod proc;
pub mod resource;
pub mod spell;

use std::collections::BTreeMap;

pub use buff::BuffStatistics;
pub use engine::EngineStatistics;
pub use executor::{ExecutorOutcome, ExecutorResult, RotationExecutorStatistics};
pub use number_cruncher::{NumberCruncher, ScaleResult};
pub use proc::ProcStatistics;
pub use resource::ResourceStatistics;
pub use spell::{Outcome, Ratio, SpellStatistics, Tally};

/// A spell (or auto attack, or proc) as the statistics identify it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpellKey {
    pub name: String,
    pub rank: u32,
}

impl SpellKey {
    pub fn new(name: impl Into<String>, rank: u32) -> Self {
        SpellKey {
            name: name.into(),
            rank,
        }
    }

    /// The name with the rank appended for ranks above one, as the C++ recorded it.
    pub fn display_name(&self) -> String {
        if self.rank > 1 {
            format!("{} (rank {})", self.name, self.rank)
        } else {
            self.name.clone()
        }
    }
}

/// One raid member's result. Port of `RaidMemberResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerResult {
    pub player_name: String,
    pub dps: f64,
    pub tps: f64,
    pub iterations: u64,
}

/// The statistics of one character for a set of combat iterations. Port of `ClassStatistics`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassStatistics {
    player_name: String,
    combat_length: f64,
    spells: BTreeMap<SpellKey, SpellStatistics>,
    buffs: BTreeMap<String, BuffStatistics>,
    resources: BTreeMap<SpellKey, ResourceStatistics>,
    procs: BTreeMap<String, ProcStatistics>,
    executors: Vec<RotationExecutorStatistics>,
    engine: EngineStatistics,
    dps_per_iteration: Vec<f64>,
    damage_previous_iterations: u64,
    player_results: Vec<PlayerResult>,
}

impl ClassStatistics {
    /// Empty statistics for `player_name` and encounters of `combat_length` seconds.
    pub fn new(player_name: impl Into<String>, combat_length: f64) -> Self {
        ClassStatistics {
            player_name: player_name.into(),
            combat_length,
            spells: BTreeMap::new(),
            buffs: BTreeMap::new(),
            resources: BTreeMap::new(),
            procs: BTreeMap::new(),
            executors: Vec::new(),
            engine: EngineStatistics::new(),
            dps_per_iteration: Vec::new(),
            damage_previous_iterations: 0,
            player_results: Vec::new(),
        }
    }

    /// Clears everything for a new set of iterations of `combat_length` seconds. Port of
    /// `prepare_statistics`.
    pub fn prepare(&mut self, combat_length: f64) {
        *self = ClassStatistics::new(std::mem::take(&mut self.player_name), combat_length);
    }

    pub fn player_name(&self) -> &str {
        &self.player_name
    }

    pub fn combat_length(&self) -> f64 {
        self.combat_length
    }

    // ---------------------------------------------------------------- recording

    /// The statistics of spell `name` at `rank`, created on first use. Port of
    /// `get_spell_statistics`.
    pub fn spell(&mut self, name: &str, rank: u32) -> &mut SpellStatistics {
        self.spells
            .entry(SpellKey::new(name, rank))
            .or_insert_with(|| SpellStatistics::new(name, rank))
    }

    /// The statistics of buff `name`, created on first use. Port of `get_buff_statistics`.
    pub fn buff(&mut self, name: &str, debuff: bool) -> &mut BuffStatistics {
        self.buffs
            .entry(name.to_string())
            .or_insert_with(|| BuffStatistics::new(name, debuff))
    }

    /// The resource statistics of source `name` at `rank`, created on first use. Port of
    /// `get_resource_statistics`.
    pub fn resource(&mut self, name: &str, rank: u32) -> &mut ResourceStatistics {
        self.resources
            .entry(SpellKey::new(name, rank))
            .or_insert_with(|| ResourceStatistics::new(name, rank))
    }

    /// The statistics of proc `name`, created on first use. Port of `get_proc_statistics`.
    pub fn proc(&mut self, name: &str) -> &mut ProcStatistics {
        self.procs
            .entry(name.to_string())
            .or_insert_with(|| ProcStatistics::new(name))
    }

    /// Replaces the rotation executor statistics with a fresh snapshot.
    pub fn set_executors(&mut self, executors: Vec<RotationExecutorStatistics>) {
        self.executors = executors;
    }

    /// Replaces the engine statistics with a fresh snapshot.
    pub fn set_engine(&mut self, engine: EngineStatistics) {
        self.engine = engine;
    }

    pub fn engine_mut(&mut self) -> &mut EngineStatistics {
        &mut self.engine
    }

    /// Closes an iteration: records its DPS. Port of `finish_combat_iteration`.
    pub fn finish_combat_iteration(&mut self) {
        let total = self.total_damage();
        let this_iteration = total - self.damage_previous_iterations;
        self.dps_per_iteration
            .push(this_iteration as f64 / self.combat_length);
        self.damage_previous_iterations = total;
    }

    /// Adds another raid member's result (for the raid DPS). Port of `add_player_result`.
    pub fn add_player_result(&mut self, result: PlayerResult) {
        self.player_results.push(result);
    }

    // ---------------------------------------------------------------- reading

    /// Every spell's statistics, by key.
    pub fn spells(&self) -> impl Iterator<Item = (&SpellKey, &SpellStatistics)> {
        self.spells.iter()
    }

    pub fn spell_statistics(&self, name: &str, rank: u32) -> Option<&SpellStatistics> {
        self.spells.get(&SpellKey::new(name, rank))
    }

    pub fn buffs(&self) -> impl Iterator<Item = &BuffStatistics> {
        self.buffs.values()
    }

    pub fn buff_statistics(&self, name: &str) -> Option<&BuffStatistics> {
        self.buffs.get(name)
    }

    pub fn resources(&self) -> impl Iterator<Item = (&SpellKey, &ResourceStatistics)> {
        self.resources.iter()
    }

    pub fn resource_statistics(&self, name: &str, rank: u32) -> Option<&ResourceStatistics> {
        self.resources.get(&SpellKey::new(name, rank))
    }

    pub fn procs(&self) -> impl Iterator<Item = &ProcStatistics> {
        self.procs.values()
    }

    pub fn proc_statistics(&self, name: &str) -> Option<&ProcStatistics> {
        self.procs.get(name)
    }

    pub fn executors(&self) -> &[RotationExecutorStatistics] {
        &self.executors
    }

    pub fn engine(&self) -> &EngineStatistics {
        &self.engine
    }

    /// Damage dealt by every spell over the set of iterations. Port of
    /// `get_total_personal_damage_dealt`.
    pub fn total_damage(&self) -> u64 {
        self.spells
            .values()
            .map(SpellStatistics::total_damage)
            .sum()
    }

    /// Threat dealt by every spell over the set of iterations. Port of
    /// `get_total_personal_threat_dealt`.
    pub fn total_threat(&self) -> u64 {
        self.spells
            .values()
            .map(SpellStatistics::total_threat)
            .sum()
    }

    /// Total damage of spell `name` at `rank`, 0 if unknown. Port of
    /// `get_total_damage_for_spell`.
    pub fn damage_for_spell(&self, name: &str, rank: u32) -> u64 {
        self.spell_statistics(name, rank)
            .map_or(0, SpellStatistics::total_damage)
    }

    /// Total threat of spell `name` at `rank`, 0 if unknown. Port of
    /// `get_total_threat_for_spell`.
    pub fn threat_for_spell(&self, name: &str, rank: u32) -> u64 {
        self.spell_statistics(name, rank)
            .map_or(0, SpellStatistics::total_threat)
    }

    /// Iterations finished so far.
    pub fn iterations(&self) -> u64 {
        self.dps_per_iteration.len() as u64
    }

    /// Seconds of combat simulated: iterations × encounter length.
    pub fn time_in_combat(&self) -> f64 {
        self.iterations() as f64 * self.combat_length
    }

    /// The DPS of each finished iteration, in order.
    pub fn dps_per_iteration(&self) -> &[f64] {
        &self.dps_per_iteration
    }

    /// Mean damage per second over the finished iterations, 0 without any.
    pub fn personal_dps(&self) -> f64 {
        let time = self.time_in_combat();
        if time <= 0.0 {
            0.0
        } else {
            self.total_damage() as f64 / time
        }
    }

    /// Mean threat per second over the finished iterations, 0 without any.
    pub fn personal_tps(&self) -> f64 {
        let time = self.time_in_combat();
        if time <= 0.0 {
            0.0
        } else {
            self.total_threat() as f64 / time
        }
    }

    /// Port of `get_personal_result`.
    pub fn personal_result(&self) -> PlayerResult {
        PlayerResult {
            player_name: self.player_name.clone(),
            dps: self.personal_dps(),
            tps: self.personal_tps(),
            iterations: self.iterations(),
        }
    }

    pub fn player_results(&self) -> &[PlayerResult] {
        &self.player_results
    }

    /// Sum of the added raid members' DPS. Port of `get_raid_dps`.
    pub fn raid_dps(&self) -> f64 {
        self.player_results.iter().map(|r| r.dps).sum()
    }

    // ---------------------------------------------------------------- merging

    /// Merges `other` (the same character's statistics from another thread) into `self`.
    ///
    /// # Panics
    /// Panics if the two were collected with different rotations (executor lists of different
    /// lengths), the C++ `check`.
    pub fn add(&mut self, other: &ClassStatistics) {
        for (key, stats) in &other.spells {
            self.spell(&key.name, key.rank).add(stats);
        }
        for (name, stats) in &other.buffs {
            self.buff(name, stats.is_debuff()).add(stats);
        }
        for (key, stats) in &other.resources {
            self.resource(&key.name, key.rank).add(stats);
        }
        for (name, stats) in &other.procs {
            self.proc(name).add(stats);
        }
        if self.executors.is_empty() {
            self.executors = other.executors.clone();
        } else if !other.executors.is_empty() {
            assert_eq!(
                self.executors.len(),
                other.executors.len(),
                "Mismatch ClassStatistics rotation executor size"
            );
            for (mine, theirs) in self.executors.iter_mut().zip(&other.executors) {
                mine.add(theirs);
            }
        }
        self.engine.add(&other.engine);
        self.dps_per_iteration
            .extend_from_slice(&other.dps_per_iteration);
        self.damage_previous_iterations += other.damage_previous_iterations;
        self.player_results
            .extend(other.player_results.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_roll::PhysicalAttackResult;
    use crate::engine::EventType;
    use crate::resource::ResourceType;
    use crate::spell::AttackOutcome;

    fn hit(damage: u32) -> AttackOutcome {
        AttackOutcome {
            result: PhysicalAttackResult::Hit,
            damage,
            threat: f64::from(damage) * 2.0,
            execution_time: 1.5,
        }
    }

    #[test]
    fn entries_are_created_on_first_use_and_keyed_by_rank() {
        let mut stats = ClassStatistics::new("You", 100.0);
        stats
            .spell("Heroic Strike", 9)
            .record_attack(&hit(100), 15.0);
        stats
            .spell("Heroic Strike", 9)
            .record_attack(&hit(200), 15.0);
        stats
            .spell("Heroic Strike", 8)
            .record_attack(&hit(50), 15.0);
        stats
            .spell("Mainhand Attack", 1)
            .record_attack(&hit(80), 0.0);

        assert_eq!(stats.spells().count(), 3);
        assert_eq!(stats.damage_for_spell("Heroic Strike", 9), 300);
        assert_eq!(stats.damage_for_spell("Heroic Strike", 8), 50);
        assert_eq!(stats.damage_for_spell("Heroic Strike", 1), 0);
        assert_eq!(stats.threat_for_spell("Mainhand Attack", 1), 160);
        assert_eq!(stats.total_damage(), 430);
        assert_eq!(stats.total_threat(), 860);
        assert_eq!(
            stats
                .spells()
                .map(|(k, _)| k.display_name())
                .collect::<Vec<_>>(),
            vec![
                "Heroic Strike (rank 8)",
                "Heroic Strike (rank 9)",
                "Mainhand Attack"
            ]
        );

        stats.buff("Flurry", false).add_uptime(3.0);
        stats.buff("Flurry", false).add_uptime(5.0);
        assert_eq!(stats.buffs().count(), 1);
        assert_eq!(stats.buff_statistics("Flurry").unwrap().max_uptime(), 5.0);

        stats
            .resource("Bloodrage", 1)
            .add_gain(ResourceType::Rage, 10);
        stats
            .resource("Bloodrage", 1)
            .add_gain(ResourceType::Rage, 1);
        assert_eq!(
            stats
                .resource_statistics("Bloodrage", 1)
                .unwrap()
                .gain(ResourceType::Rage),
            11.0
        );

        stats.proc("Unbridled Wrath").set_counts(20, 8);
        assert_eq!(stats.proc_statistics("Unbridled Wrath").unwrap().procs(), 8);
    }

    #[test]
    fn iterations_are_counted_and_dps_is_per_iteration() {
        let mut stats = ClassStatistics::new("You", 100.0);
        assert_eq!(stats.iterations(), 0);
        assert_eq!(stats.personal_dps(), 0.0);

        stats
            .spell("Bloodthirst", 4)
            .record_attack(&hit(1000), 30.0);
        stats.finish_combat_iteration();
        stats
            .spell("Bloodthirst", 4)
            .record_attack(&hit(3000), 30.0);
        stats.finish_combat_iteration();
        stats.finish_combat_iteration();

        assert_eq!(stats.iterations(), 3);
        assert_eq!(stats.time_in_combat(), 300.0);
        assert_eq!(stats.dps_per_iteration(), &[10.0, 30.0, 0.0]);
        assert!((stats.personal_dps() - 4000.0 / 300.0).abs() < 1e-9);
        assert!((stats.personal_tps() - 8000.0 / 300.0).abs() < 1e-9);
        let result = stats.personal_result();
        assert_eq!(result.player_name, "You");
        assert_eq!(result.iterations, 3);
        assert!((result.dps - stats.personal_dps()).abs() < 1e-12);

        stats.add_player_result(PlayerResult {
            player_name: "P1M2".into(),
            dps: 500.0,
            tps: 0.0,
            iterations: 3,
        });
        stats.add_player_result(result);
        assert!((stats.raid_dps() - (500.0 + 4000.0 / 300.0)).abs() < 1e-9);
    }

    #[test]
    fn prepare_clears_everything_but_the_name() {
        let mut stats = ClassStatistics::new("You", 100.0);
        stats
            .spell("Bloodthirst", 4)
            .record_attack(&hit(1000), 30.0);
        stats.finish_combat_iteration();
        stats.prepare(200.0);
        assert_eq!(stats, ClassStatistics::new("You", 200.0));
    }

    #[test]
    fn merge_combines_every_table() {
        let mut a = ClassStatistics::new("You", 100.0);
        a.spell("Bloodthirst", 4).record_attack(&hit(1000), 30.0);
        a.buff("Flurry", false).add_uptime_for_encounter(0.5);
        a.resource("Mainhand Attack", 1)
            .add_gain(ResourceType::Rage, 100);
        a.proc("Unbridled Wrath").set_counts(10, 2);
        a.set_executors(vec![RotationExecutorStatistics::new(
            "(1) Bloodthirst",
            "Bloodthirst",
        )]);
        a.engine_mut().increment_event(EventType::PlayerAction);
        a.finish_combat_iteration();

        let mut b = ClassStatistics::new("You", 100.0);
        b.spell("Bloodthirst", 4).record_attack(&hit(2000), 30.0);
        b.spell("Whirlwind", 1).record_attack(&hit(500), 25.0);
        b.buff("Flurry", false).add_uptime_for_encounter(1.0);
        b.buff("Sunder Armor", true).add_uptime(5.0);
        b.resource("Mainhand Attack", 1)
            .add_gain(ResourceType::Rage, 50);
        b.proc("Unbridled Wrath").set_counts(5, 1);
        b.set_executors(vec![RotationExecutorStatistics::new(
            "(1) Bloodthirst",
            "Bloodthirst",
        )]);
        b.engine_mut().increment_event(EventType::PlayerAction);
        b.engine_mut().increment_event(EventType::DotTick);
        b.finish_combat_iteration();
        b.finish_combat_iteration();

        a.add(&b);
        assert_eq!(a.iterations(), 3);
        assert_eq!(a.dps_per_iteration(), &[10.0, 25.0, 0.0]);
        assert_eq!(a.total_damage(), 3500);
        assert_eq!(a.damage_for_spell("Bloodthirst", 4), 3000);
        assert_eq!(a.spell_statistics("Bloodthirst", 4).unwrap().hits(), 2);
        assert_eq!(a.damage_for_spell("Whirlwind", 1), 500);
        assert!((a.buff_statistics("Flurry").unwrap().avg_uptime() - 0.75).abs() < 1e-12);
        assert!(a.buff_statistics("Sunder Armor").unwrap().is_debuff());
        assert_eq!(
            a.resource_statistics("Mainhand Attack", 1)
                .unwrap()
                .gain(ResourceType::Rage),
            150.0
        );
        assert_eq!(a.proc_statistics("Unbridled Wrath").unwrap().attempts(), 15);
        assert_eq!(a.executors().len(), 1);
        assert_eq!(a.engine().event_count(EventType::PlayerAction), 2);
        assert_eq!(a.engine().event_count(EventType::DotTick), 1);
        assert!((a.personal_dps() - 3500.0 / 300.0).abs() < 1e-9);

        // An empty base takes the other's executors.
        let mut empty = ClassStatistics::new("You", 100.0);
        empty.add(&a);
        assert_eq!(empty.executors(), a.executors());
    }

    #[test]
    #[should_panic(expected = "rotation executor size")]
    fn merge_rejects_different_rotations() {
        let mut a = ClassStatistics::new("You", 100.0);
        a.set_executors(vec![RotationExecutorStatistics::new("(1) A", "A")]);
        let mut b = ClassStatistics::new("You", 100.0);
        b.set_executors(vec![
            RotationExecutorStatistics::new("(1) A", "A"),
            RotationExecutorStatistics::new("(2) B", "B"),
        ]);
        a.add(&b);
    }
}
