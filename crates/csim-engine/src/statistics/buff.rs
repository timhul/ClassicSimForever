//! Buff uptime statistics. Port of `Statistics/StatisticsBuff.*`.

/// Uptime of one buff (or debuff): the shortest and longest single application in seconds,
/// and the mean share of the encounter it was active. Port of `StatisticsBuff`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuffStatistics {
    name: String,
    debuff: bool,
    /// Shortest single application, seconds.
    min_uptime: Option<f64>,
    /// Longest single application, seconds.
    max_uptime: Option<f64>,
    /// Mean uptime per encounter, as a fraction of the encounter length.
    avg_uptime: f64,
    /// Encounters averaged into `avg_uptime`.
    encounters: u64,
}

impl BuffStatistics {
    pub fn new(name: impl Into<String>, debuff: bool) -> Self {
        BuffStatistics {
            name: name.into(),
            debuff,
            min_uptime: None,
            max_uptime: None,
            avg_uptime: 0.0,
            encounters: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_debuff(&self) -> bool {
        self.debuff
    }

    /// Clears every counter.
    pub fn reset(&mut self) {
        *self = BuffStatistics::new(std::mem::take(&mut self.name), self.debuff);
    }

    /// Records one application that lasted `seconds`. Port of `add_uptime`.
    pub fn add_uptime(&mut self, seconds: f64) {
        self.min_uptime = Some(self.min_uptime.map_or(seconds, |min| min.min(seconds)));
        self.max_uptime = Some(self.max_uptime.map_or(seconds, |max| max.max(seconds)));
    }

    /// Records the share of one encounter the buff was active. Port of
    /// `add_uptime_for_encounter`.
    pub fn add_uptime_for_encounter(&mut self, fraction: f64) {
        self.encounters += 1;
        self.avg_uptime += (fraction - self.avg_uptime) / self.encounters as f64;
    }

    /// Shortest single application in seconds, 0 without any.
    pub fn min_uptime(&self) -> f64 {
        self.min_uptime.unwrap_or(0.0)
    }

    /// Longest single application in seconds, 0 without any.
    pub fn max_uptime(&self) -> f64 {
        self.max_uptime.unwrap_or(0.0)
    }

    /// Mean share of the encounter the buff was active.
    pub fn avg_uptime(&self) -> f64 {
        self.avg_uptime
    }

    pub fn encounters(&self) -> u64 {
        self.encounters
    }

    /// Merges `other` into `self` (the same buff from another thread).
    pub fn add(&mut self, other: &BuffStatistics) {
        if let Some(uptime) = other.min_uptime {
            self.min_uptime = Some(self.min_uptime.map_or(uptime, |min| min.min(uptime)));
        }
        if let Some(uptime) = other.max_uptime {
            self.max_uptime = Some(self.max_uptime.map_or(uptime, |max| max.max(uptime)));
        }
        let total = self.encounters + other.encounters;
        if total > 0 {
            self.avg_uptime = self.avg_uptime * (self.encounters as f64 / total as f64)
                + other.avg_uptime * (other.encounters as f64 / total as f64);
        }
        self.encounters = total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_shortest_and_longest_application() {
        let mut stats = BuffStatistics::new("Flurry", false);
        assert_eq!(stats.min_uptime(), 0.0);
        assert_eq!(stats.max_uptime(), 0.0);
        stats.add_uptime(3.0);
        stats.add_uptime(1.5);
        stats.add_uptime(12.0);
        assert_eq!(stats.min_uptime(), 1.5);
        assert_eq!(stats.max_uptime(), 12.0);
    }

    #[test]
    fn averages_the_uptime_over_encounters() {
        let mut stats = BuffStatistics::new("Sunder Armor", true);
        assert!(stats.is_debuff());
        stats.add_uptime_for_encounter(0.5);
        stats.add_uptime_for_encounter(1.0);
        stats.add_uptime_for_encounter(0.75);
        assert_eq!(stats.encounters(), 3);
        assert!((stats.avg_uptime() - 0.75).abs() < 1e-12);
    }

    #[test]
    fn merge_weights_averages_by_encounters() {
        let mut a = BuffStatistics::new("Battle Shout", false);
        a.add_uptime(10.0);
        a.add_uptime_for_encounter(0.8);
        let mut b = BuffStatistics::new("Battle Shout", false);
        b.add_uptime(4.0);
        b.add_uptime(20.0);
        b.add_uptime_for_encounter(0.9);
        b.add_uptime_for_encounter(1.0);
        b.add_uptime_for_encounter(0.5);

        a.add(&b);
        assert_eq!(a.min_uptime(), 4.0);
        assert_eq!(a.max_uptime(), 20.0);
        assert_eq!(a.encounters(), 4);
        assert!((a.avg_uptime() - 0.8).abs() < 1e-12);

        let mut empty = BuffStatistics::new("Battle Shout", false);
        empty.add(&a);
        assert_eq!(empty, a);
    }
}
