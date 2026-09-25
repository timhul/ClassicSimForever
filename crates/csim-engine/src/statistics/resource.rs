//! Resource gain statistics. Port of `Statistics/StatisticsResource.*`.
//!
//! As with the proc statistics, the gain per 5 seconds takes the time in combat as a parameter
//! instead of storing it.

use crate::resource::ResourceType;

/// Resource gained by one spell, proc or auto attack. Port of `StatisticsResource`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceStatistics {
    name: String,
    rank: u32,
    /// Gains in displayed units; fractional, as swing rage is measured in tenths.
    gains: [f64; ResourceType::ALL.len()],
}

fn index(resource: ResourceType) -> usize {
    ResourceType::ALL
        .iter()
        .position(|r| *r == resource)
        .expect("every resource type is in ALL")
}

impl ResourceStatistics {
    pub fn new(name: impl Into<String>, rank: u32) -> Self {
        ResourceStatistics {
            name: name.into(),
            rank,
            gains: [0.0; ResourceType::ALL.len()],
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rank(&self) -> u32 {
        self.rank
    }

    /// The name with the rank appended for ranks above one, as the C++ recorded it.
    pub fn display_name(&self) -> String {
        if self.rank > 1 {
            format!("{} (rank {})", self.name, self.rank)
        } else {
            self.name.clone()
        }
    }

    pub fn reset(&mut self) {
        self.gains = [0.0; ResourceType::ALL.len()];
    }

    pub fn add_gain(&mut self, resource: ResourceType, amount: u32) {
        self.add_fractional_gain(resource, f64::from(amount));
    }

    /// Adds a fractional gain (swing rage in tenths, divided by ten).
    pub fn add_fractional_gain(&mut self, resource: ResourceType, amount: f64) {
        self.gains[index(resource)] += amount;
    }

    /// Total gained of `resource`.
    pub fn gain(&self, resource: ResourceType) -> f64 {
        self.gains[index(resource)]
    }

    /// Whether anything was gained at all.
    pub fn is_empty(&self) -> bool {
        self.gains.iter().all(|g| *g == 0.0)
    }

    /// Gain of `resource` per 5 seconds over `time_in_combat` seconds, 0 for no time.
    pub fn gain_per_5(&self, resource: ResourceType, time_in_combat: f64) -> f64 {
        if time_in_combat <= 0.0 {
            0.0
        } else {
            self.gain(resource) / time_in_combat * 5.0
        }
    }

    /// Merges `other` into `self` (the same source from another thread).
    pub fn add(&mut self, other: &ResourceStatistics) {
        for (mine, theirs) in self.gains.iter_mut().zip(other.gains.iter()) {
            *mine += theirs;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gains_per_resource_and_per_5() {
        let mut stats = ResourceStatistics::new("Bloodrage", 1);
        assert!(stats.is_empty());
        stats.add_gain(ResourceType::Rage, 10);
        stats.add_gain(ResourceType::Rage, 1);
        stats.add_gain(ResourceType::Mana, 300);
        assert!(!stats.is_empty());
        assert_eq!(stats.gain(ResourceType::Rage), 11.0);
        assert_eq!(stats.gain(ResourceType::Mana), 300.0);
        assert_eq!(stats.gain(ResourceType::Energy), 0.0);
        // 11 rage over 55 s = 1 per 5 s.
        assert!((stats.gain_per_5(ResourceType::Rage, 55.0) - 1.0).abs() < 1e-12);
        assert_eq!(stats.gain_per_5(ResourceType::Rage, 0.0), 0.0);
        stats.add_fractional_gain(ResourceType::Rage, 5.5);
        assert_eq!(stats.gain(ResourceType::Rage), 16.5);
    }

    #[test]
    fn merge_and_reset() {
        let mut a = ResourceStatistics::new("Mainhand Attack", 1);
        a.add_gain(ResourceType::Rage, 100);
        let mut b = ResourceStatistics::new("Mainhand Attack", 1);
        b.add_gain(ResourceType::Rage, 50);
        b.add_gain(ResourceType::Energy, 5);
        a.add(&b);
        assert_eq!(a.gain(ResourceType::Rage), 150.0);
        assert_eq!(a.gain(ResourceType::Energy), 5.0);
        a.reset();
        assert!(a.is_empty());
        assert_eq!(
            ResourceStatistics::new("Battle Shout", 7).display_name(),
            "Battle Shout (rank 7)"
        );
    }
}
