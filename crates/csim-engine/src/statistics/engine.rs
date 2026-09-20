//! Engine statistics: event counts and wall-clock time. Port of
//! `Statistics/StatisticsEngine.*`.
//!
//! The engine itself counts its events ([`Engine::event_counts`]) and times the set of
//! iterations ([`Engine::elapsed`]); this is the snapshot the character's statistics keep and
//! merge across threads.

use std::time::Duration;

use crate::engine::{Engine, EventCounts, EventType};

/// Event counts and elapsed wall-clock time of a set of combat iterations. Port of
/// `StatisticsEngine`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineStatistics {
    events: EventCounts,
    elapsed: Duration,
}

impl EngineStatistics {
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot of `engine`'s counters and elapsed time.
    pub fn from_engine(engine: &Engine) -> Self {
        EngineStatistics {
            events: engine.event_counts().clone(),
            elapsed: engine.elapsed(),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn increment_event(&mut self, event_type: EventType) {
        self.events.increment(event_type);
    }

    pub fn set_elapsed(&mut self, elapsed: Duration) {
        self.elapsed = elapsed;
    }

    pub fn events(&self) -> &EventCounts {
        &self.events
    }

    pub fn event_count(&self, event_type: EventType) -> u64 {
        self.events.get(event_type)
    }

    pub fn total_events(&self) -> u64 {
        self.events.total()
    }

    /// Wall-clock time spent (summed over threads after a merge).
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// `(event type, count)` pairs for every type with a non-zero count. Port of
    /// `get_list_of_event_pairs`.
    pub fn non_zero(&self) -> Vec<(EventType, u64)> {
        self.events.non_zero().collect()
    }

    /// Merges `other` into `self` (another thread's engine).
    pub fn add(&mut self, other: &EngineStatistics) {
        self.events.add(&other.events);
        self.elapsed += other.elapsed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_events_and_merges() {
        let mut a = EngineStatistics::new();
        a.increment_event(EventType::PlayerAction);
        a.increment_event(EventType::PlayerAction);
        a.set_elapsed(Duration::from_millis(40));
        let mut b = EngineStatistics::new();
        b.increment_event(EventType::PlayerAction);
        b.increment_event(EventType::DotTick);
        b.set_elapsed(Duration::from_millis(60));

        a.add(&b);
        assert_eq!(a.event_count(EventType::PlayerAction), 3);
        assert_eq!(a.event_count(EventType::DotTick), 1);
        assert_eq!(a.total_events(), 4);
        assert_eq!(a.elapsed(), Duration::from_millis(100));
        assert_eq!(
            a.non_zero(),
            vec![(EventType::DotTick, 1), (EventType::PlayerAction, 3)]
        );
        a.reset();
        assert_eq!(a, EngineStatistics::new());
    }
}
