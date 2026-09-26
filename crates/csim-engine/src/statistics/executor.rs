//! Rotation executor statistics: per `cast_if` line, how often it cast, how often its
//! condition failed and how often its spell was unavailable, by status. Port of
//! `Statistics/StatisticsRotationExecutor.*`.
//!
//! The counting itself lives with the executor
//! ([`crate::rotation::ExecutorStatistics`]); this wraps a snapshot with the executor's
//! position, spell and condition for reporting.

use crate::rotation::{ExecutorStatistics, RotationExecutor};
use crate::spell::SpellStatus;

/// How an executor's attempt ended. Port of `ExecutorResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExecutorResult {
    SpellStatusFail,
    ConditionGroupFail,
    Success,
}

impl ExecutorResult {
    /// Port of `get_description_for_executor_result`.
    pub fn description(self) -> &'static str {
        match self {
            ExecutorResult::Success => "Success",
            ExecutorResult::SpellStatusFail => "SpellStatus",
            ExecutorResult::ConditionGroupFail => "ConditionGroup",
        }
    }
}

/// One line of an executor's breakdown. Port of `ExecutorOutcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorOutcome {
    pub result: ExecutorResult,
    pub count: u64,
    /// The status that made the spell unavailable for `SpellStatusFail`; `Available` otherwise.
    pub spell_status: SpellStatus,
}

impl ExecutorOutcome {
    /// Port of `get_description_for_status` on the outcome's status.
    pub fn description(&self) -> &'static str {
        match self.result {
            ExecutorResult::SpellStatusFail => self.spell_status.description(),
            other => other.description(),
        }
    }
}

/// A `cast_if` line that was not linked, so it never ran. Not in C++, which dropped them
/// silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedExecutor {
    /// 1-based position among the rotation's `cast_if` lines.
    pub line: usize,
    pub spell_name: String,
    pub reason: String,
}

impl SkippedExecutor {
    /// The skipped `executor` at `line`; an active executor has an empty reason.
    pub fn from_executor(line: usize, executor: &RotationExecutor) -> Self {
        SkippedExecutor {
            line,
            spell_name: executor.spell_name().to_string(),
            reason: executor
                .skip_reason()
                .map(ToString::to_string)
                .unwrap_or_default(),
        }
    }
}

/// The statistics of one active executor of a rotation. Port of `StatisticsRotationExecutor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationExecutorStatistics {
    /// `"(position) spell name"`, the C++ executor name.
    name: String,
    spell_name: String,
    /// The condition as written, one sentence per line; empty for an unconditional executor.
    conditions: String,
    statistics: ExecutorStatistics,
}

impl RotationExecutorStatistics {
    /// The statistics of `executor`, the `position`th (1-based) active executor of its rotation.
    pub fn from_executor(position: usize, executor: &RotationExecutor) -> Self {
        RotationExecutorStatistics {
            name: format!("({position}) {}", executor.spell_name()),
            spell_name: executor.spell_name().to_string(),
            conditions: executor.conditions_string(),
            statistics: executor.statistics().clone(),
        }
    }

    pub fn new(name: impl Into<String>, spell_name: impl Into<String>) -> Self {
        RotationExecutorStatistics {
            name: name.into(),
            spell_name: spell_name.into(),
            conditions: String::new(),
            statistics: ExecutorStatistics::default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn spell_name(&self) -> &str {
        &self.spell_name
    }

    pub fn conditions(&self) -> &str {
        &self.conditions
    }

    pub fn statistics(&self) -> &ExecutorStatistics {
        &self.statistics
    }

    pub fn successful_casts(&self) -> u64 {
        self.statistics.successful_casts
    }

    pub fn attempts(&self) -> u64 {
        self.statistics.attempts()
    }

    /// The breakdown: the successes, the condition failures, then every status that made the
    /// spell unavailable at least once. Port of `get_list_of_executor_outcomes`.
    pub fn outcomes(&self) -> Vec<ExecutorOutcome> {
        let mut outcomes = vec![
            ExecutorOutcome {
                result: ExecutorResult::Success,
                count: self.statistics.successful_casts,
                spell_status: SpellStatus::Available,
            },
            ExecutorOutcome {
                result: ExecutorResult::ConditionGroupFail,
                count: self.statistics.no_condition_group_fulfilled,
                spell_status: SpellStatus::Available,
            },
        ];
        outcomes.extend(
            self.statistics
                .spell_status
                .iter()
                .filter(|(_, count)| **count > 0)
                .map(|(status, count)| ExecutorOutcome {
                    result: ExecutorResult::SpellStatusFail,
                    count: *count,
                    spell_status: *status,
                }),
        );
        outcomes
    }

    /// Merges `other` into `self` (the same executor from another thread).
    pub fn add(&mut self, other: &RotationExecutorStatistics) {
        self.statistics.add(&other.statistics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(casts: u64, no_group: u64, statuses: &[(SpellStatus, u64)]) -> ExecutorStatistics {
        ExecutorStatistics {
            successful_casts: casts,
            no_condition_group_fulfilled: no_group,
            spell_status: statuses.iter().copied().collect(),
        }
    }

    #[test]
    fn outcomes_list_successes_condition_failures_and_statuses() {
        let mut executor = RotationExecutorStatistics::new("(1) Bloodthirst", "Bloodthirst");
        executor.statistics = stats(
            10,
            3,
            &[
                (SpellStatus::OnCooldown, 40),
                (SpellStatus::InsufficientResources, 0),
                (SpellStatus::OnGcd, 2),
            ],
        );
        let outcomes = executor.outcomes();
        assert_eq!(outcomes.len(), 4);
        assert_eq!(outcomes[0].result, ExecutorResult::Success);
        assert_eq!(outcomes[0].count, 10);
        assert_eq!(outcomes[0].description(), "Success");
        assert_eq!(outcomes[1].result, ExecutorResult::ConditionGroupFail);
        assert_eq!(outcomes[1].count, 3);
        assert_eq!(outcomes[1].description(), "ConditionGroup");
        assert_eq!(outcomes[2].spell_status, SpellStatus::OnCooldown);
        assert_eq!(outcomes[2].count, 40);
        assert_eq!(outcomes[2].description(), "FAIL: On spell cooldown");
        assert_eq!(outcomes[3].spell_status, SpellStatus::OnGcd);
        assert_eq!(executor.attempts(), 55);
        assert_eq!(executor.successful_casts(), 10);
    }

    #[test]
    fn merge_sums_every_counter() {
        let mut a = RotationExecutorStatistics::new("(2) Whirlwind", "Whirlwind");
        a.statistics = stats(1, 2, &[(SpellStatus::OnCooldown, 3)]);
        let mut b = RotationExecutorStatistics::new("(2) Whirlwind", "Whirlwind");
        b.statistics = stats(
            10,
            20,
            &[(SpellStatus::OnCooldown, 30), (SpellStatus::OnGcd, 1)],
        );
        a.add(&b);
        assert_eq!(
            a.statistics,
            stats(
                11,
                22,
                &[(SpellStatus::OnCooldown, 33), (SpellStatus::OnGcd, 1)]
            )
        );
    }
}
