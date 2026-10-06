//! Proc statistics: attempts and successes. Port of `Statistics/StatisticsProc.*`.
//!
//! The C++ object carried its own `time_in_combat`; here the effective PPM takes it as a
//! parameter ([`crate::statistics::ClassStatistics::time_in_combat`]) so merged statistics
//! report against the merged time.

use serde::{Deserialize, Serialize};

/// Attempts and successes of one proc. Port of `StatisticsProc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcStatistics {
    name: String,
    attempts: u64,
    procs: u64,
}

impl ProcStatistics {
    pub fn new(name: impl Into<String>) -> Self {
        ProcStatistics {
            name: name.into(),
            attempts: 0,
            procs: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
        self.procs = 0;
    }

    pub fn increment_attempt(&mut self) {
        self.attempts += 1;
    }

    pub fn increment_proc(&mut self) {
        self.procs += 1;
    }

    /// Replaces the counters with the proc's own (the runtime counts, the statistics report).
    pub fn set_counts(&mut self, attempts: u64, procs: u64) {
        self.attempts = attempts;
        self.procs = procs;
    }

    pub fn attempts(&self) -> u64 {
        self.attempts
    }

    pub fn procs(&self) -> u64 {
        self.procs
    }

    /// Successes per attempt, 0 without attempts.
    pub fn avg_proc_rate(&self) -> f64 {
        if self.attempts == 0 {
            0.0
        } else {
            self.procs as f64 / self.attempts as f64
        }
    }

    /// Procs per minute over `time_in_combat` seconds, 0 for no time.
    pub fn effective_ppm(&self, time_in_combat: f64) -> f64 {
        if time_in_combat <= 0.0 {
            0.0
        } else {
            self.procs as f64 / time_in_combat * 60.0
        }
    }

    /// Merges `other` into `self` (the same proc from another thread).
    pub fn add(&mut self, other: &ProcStatistics) {
        self.attempts += other.attempts;
        self.procs += other.procs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_ppm() {
        let mut stats = ProcStatistics::new("Windfury Totem");
        assert_eq!(stats.avg_proc_rate(), 0.0);
        assert_eq!(stats.effective_ppm(0.0), 0.0);
        for _ in 0..10 {
            stats.increment_attempt();
        }
        stats.increment_proc();
        stats.increment_proc();
        assert_eq!(stats.attempts(), 10);
        assert_eq!(stats.procs(), 2);
        assert!((stats.avg_proc_rate() - 0.2).abs() < 1e-12);
        // 2 procs in 30 s = 4 per minute.
        assert!((stats.effective_ppm(30.0) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn set_counts_and_merge() {
        let mut a = ProcStatistics::new("Sword Specialization");
        a.set_counts(100, 5);
        let mut b = ProcStatistics::new("Sword Specialization");
        b.set_counts(50, 3);
        a.add(&b);
        assert_eq!(a.attempts(), 150);
        assert_eq!(a.procs(), 8);
        a.reset();
        assert_eq!(a, ProcStatistics::new("Sword Specialization"));
    }
}
