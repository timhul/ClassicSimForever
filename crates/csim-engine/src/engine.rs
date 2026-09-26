//! Discrete-event engine.
//!
//! An [`Event`] is plain data: a timestamp and an [`EventKind`] that names the objects involved by
//! handle. The engine only orders and hands out events; the simulation owner pops them with
//! [`Engine::next_event`] and dispatches on the kind.
//!
//! Events with equal timestamps are ordered by insertion (a sequence number).
//!
//! Many events have an identifier approach where instead of removing stale events from the queue
//! (expensive operation) the staleness of an event is evaluated at event dispatch-time using the
//! identifier (if the identifier does not match immediately discard event and continue).

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::Instant;

use crate::combat_log::{CombatLog, CombatLogEntry, CombatLogEvent, LogUnit};
use crate::ids::{BuffId, CharId, SpellId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    BuffRemoval {
        character: CharId,
        buff: BuffId,
        iteration: u32,
    },
    CastComplete {
        character: CharId,
        spell: SpellId,
        cast_id: u32,
    },
    DotTick {
        character: CharId,
        spell: SpellId,
        application_id: u32,
    },
    EncounterEnd,
    EncounterStart {
        character: CharId,
    },
    IncomingDamage {
        character: CharId,
    },
    MainhandMeleeHit {
        character: CharId,
        iteration: u32,
    },
    OffhandMeleeHit {
        character: CharId,
        iteration: u32,
    },
    PeriodicRefreshBuff {
        character: CharId,
        buff: BuffId,
    },
    PlayerAction {
        character: CharId,
    },
}

impl EventKind {
    /// The character the event is addressed to; `None` for the raid-wide `EncounterEnd`.
    pub fn character(&self) -> Option<CharId> {
        match *self {
            EventKind::BuffRemoval { character, .. }
            | EventKind::CastComplete { character, .. }
            | EventKind::DotTick { character, .. }
            | EventKind::EncounterStart { character }
            | EventKind::IncomingDamage { character }
            | EventKind::MainhandMeleeHit { character, .. }
            | EventKind::OffhandMeleeHit { character, .. }
            | EventKind::PeriodicRefreshBuff { character, .. }
            | EventKind::PlayerAction { character } => Some(character),
            EventKind::EncounterEnd => None,
        }
    }

    /// The field-less type of this event, used for statistics.
    pub fn event_type(&self) -> EventType {
        match self {
            EventKind::BuffRemoval { .. } => EventType::BuffRemoval,
            EventKind::CastComplete { .. } => EventType::CastComplete,
            EventKind::DotTick { .. } => EventType::DotTick,
            EventKind::EncounterEnd => EventType::EncounterEnd,
            EventKind::EncounterStart { .. } => EventType::EncounterStart,
            EventKind::IncomingDamage { .. } => EventType::IncomingDamage,
            EventKind::MainhandMeleeHit { .. } => EventType::MainhandMeleeHit,
            EventKind::OffhandMeleeHit { .. } => EventType::OffhandMeleeHit,
            EventKind::PeriodicRefreshBuff { .. } => EventType::PeriodicRefreshBuff,
            EventKind::PlayerAction { .. } => EventType::PlayerAction,
        }
    }
}

/// Field-less event type, mirroring the C++ `EventType` enum. Used for event statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EventType {
    BuffRemoval,
    CastComplete,
    DotTick,
    EncounterEnd,
    EncounterStart,
    IncomingDamage,
    MainhandMeleeHit,
    OffhandMeleeHit,
    PeriodicRefreshBuff,
    PlayerAction,
}

impl EventType {
    pub const ALL: [EventType; 10] = [
        EventType::BuffRemoval,
        EventType::CastComplete,
        EventType::DotTick,
        EventType::EncounterEnd,
        EventType::EncounterStart,
        EventType::IncomingDamage,
        EventType::MainhandMeleeHit,
        EventType::OffhandMeleeHit,
        EventType::PeriodicRefreshBuff,
        EventType::PlayerAction,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            EventType::BuffRemoval => "Buff removal",
            EventType::CastComplete => "Cast complete",
            EventType::DotTick => "DoT tick",
            EventType::EncounterEnd => "Encounter end",
            EventType::EncounterStart => "Encounter start",
            EventType::IncomingDamage => "Incoming damage",
            EventType::MainhandMeleeHit => "Mainhand melee hit",
            EventType::OffhandMeleeHit => "Offhand melee hit",
            EventType::PeriodicRefreshBuff => "Periodic buff refresh",
            EventType::PlayerAction => "Player action",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Simulation time in seconds. Negative times are used for the precombat phase.
    pub time: f64,
    pub kind: EventKind,
}

impl Event {
    pub fn new(time: f64, kind: EventKind) -> Self {
        Self { time, kind }
    }
}

/// Heap entry: orders by ascending time, then ascending insertion sequence.
#[derive(Debug, Clone, Copy)]
struct QueuedEvent {
    event: Event,
    seq: u64,
}

impl PartialEq for QueuedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for QueuedEvent {}

impl PartialOrd for QueuedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedEvent {
    /// Reversed so that the earliest event is the maximum of the max-heap.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .event
            .time
            .total_cmp(&self.event.time)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

/// Priority queue of events ordered by time, then by insertion order.
#[derive(Debug, Clone, Default)]
pub struct EventQueue {
    heap: BinaryHeap<QueuedEvent>,
    next_seq: u64,
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues given event.
    pub fn push(&mut self, event: Event) {
        debug_assert!(!event.time.is_nan(), "event time must not be NaN");
        let seq = self.next_seq;
        self.next_seq += 1;
        self.heap.push(QueuedEvent { event, seq });
    }

    /// Removes and returns the earliest event.
    pub fn pop(&mut self) -> Option<Event> {
        self.heap.pop().map(|queued| queued.event)
    }

    /// Returns the earliest event without removing it.
    pub fn peek(&self) -> Option<&Event> {
        self.heap.peek().map(|queued| &queued.event)
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Drops every queued event and restarts the insertion sequence.
    pub fn clear(&mut self) {
        self.heap.clear();
        self.next_seq = 0;
    }
}

/// Per-event-type counters for a set of combat iterations.
/// Only used for statistic purposes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventCounts {
    counts: [u64; EventType::ALL.len()],
}

impl EventCounts {
    pub fn get(&self, event_type: EventType) -> u64 {
        self.counts[event_type.index()]
    }

    pub fn increment(&mut self, event_type: EventType) {
        self.counts[event_type.index()] += 1;
    }

    /// Sum of all counters.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// Adds the counters of another set of iterations (used when merging thread results).
    pub fn add(&mut self, other: &EventCounts) {
        for (mine, theirs) in self.counts.iter_mut().zip(other.counts.iter()) {
            *mine += theirs;
        }
    }

    /// `(event type, count)` pairs for every type with a non-zero count.
    pub fn non_zero(&self) -> impl Iterator<Item = (EventType, u64)> + '_ {
        EventType::ALL
            .iter()
            .map(|&event_type| (event_type, self.get(event_type)))
            .filter(|&(_, count)| count > 0)
    }
}

/// The event engine itself. It contains the event queue, keeps track of the current simulation
/// time, and event + engine performance statistics.
#[derive(Debug)]
pub struct Engine {
    queue: EventQueue,
    current_time: f64,
    event_counts: EventCounts,
    started_at: Instant,
    /// The combat log being recorded, if enabled.
    combat_log: Option<CombatLog>,
    /// The encounter has ended: nothing is logged until the next iteration starts (the reset
    /// of the characters is not part of the fight).
    combat_over: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            queue: EventQueue::new(),
            current_time: 0.0,
            event_counts: EventCounts::default(),
            started_at: Instant::now(),
            combat_log: None,
            combat_over: false,
        }
    }

    /// Current simulation time in seconds.
    pub fn current_time(&self) -> f64 {
        self.current_time
    }

    /// Schedules an event.
    pub fn add_event(&mut self, event: Event) {
        self.queue.push(event);
    }

    /// Schedules an event of `kind` at current simulation time + `delay`.
    pub fn add_event_in(&mut self, delay: f64, kind: EventKind) {
        self.queue.push(Event::new(self.current_time + delay, kind));
    }

    /// Pops the next event, advances the simulation time to it and counts it.
    ///
    /// Returns `None` when the queue is empty, i.e. when the iteration is over.
    ///
    /// # Panics
    /// Panics if the event lies in the past, which indicates a scheduling bug.
    pub fn next_event(&mut self) -> Option<Event> {
        let event = self.queue.pop()?;
        self.set_current_time(&event);
        self.event_counts.increment(event.kind.event_type());
        Some(event)
    }

    /// Starts recording a combat log (kept across iterations until taken).
    pub fn enable_combat_log(&mut self) {
        self.combat_log.get_or_insert_with(CombatLog::new);
    }

    /// Whether a combat log is being recorded now (not between the end of an encounter and the
    /// next iteration).
    pub fn is_logging(&self) -> bool {
        self.combat_log.is_some() && !self.combat_over
    }

    /// The combat log recorded so far, if enabled.
    pub fn combat_log(&self) -> Option<&CombatLog> {
        self.combat_log.as_ref()
    }

    /// Stops recording and returns the combat log, if it was enabled.
    pub fn take_combat_log(&mut self) -> Option<CombatLog> {
        self.combat_log.take()
    }

    /// Logs `event` at the current time, if the combat log is enabled.
    pub fn log(&mut self, source: LogUnit, dest: LogUnit, event: CombatLogEvent) {
        let time = self.current_time;
        if self.combat_over {
            return;
        }
        if let Some(log) = &mut self.combat_log {
            log.push(CombatLogEntry {
                time,
                source,
                dest,
                event,
            });
        }
    }

    /// Logs `event` at the current time before the entries from `index` on.
    pub fn log_at(&mut self, index: usize, source: LogUnit, dest: LogUnit, event: CombatLogEvent) {
        let time = self.current_time;
        if self.combat_over {
            return;
        }
        if let Some(log) = &mut self.combat_log {
            log.insert(
                index,
                CombatLogEntry {
                    time,
                    source,
                    dest,
                    event,
                },
            );
        }
    }

    /// Returns the next event without popping it.
    pub fn peek(&self) -> Option<&Event> {
        self.queue.peek()
    }

    pub fn queue(&self) -> &EventQueue {
        &self.queue
    }

    pub fn event_counts(&self) -> &EventCounts {
        &self.event_counts
    }

    /// Wall-clock time spent since [`Engine::prepare_set_of_iterations`].
    /// Used for engine statistics.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    /// Prepares for a new set of combat iterations: clears statistics, the queue and the clock.
    /// This should only be done once per thread before any iteration starts.
    pub fn prepare_set_of_iterations(&mut self) {
        self.event_counts = EventCounts::default();
        self.current_time = 0.0;
        self.queue.clear();
        self.started_at = Instant::now();
    }

    /// Prepares for a single combat iteration starting at `start_at` (usually negative, leaving
    /// room for precombat actions before the encounter starts at T=0).
    pub fn prepare_iteration(&mut self, start_at: f64) {
        self.queue.clear();
        self.current_time = start_at;
        self.combat_over = false;
    }

    /// Ends combat by dropping every pending event; the combat log pauses until the next
    /// iteration.
    pub fn end_combat(&mut self) {
        self.queue.clear();
        self.combat_over = true;
    }

    /// Moves the clock forward to `time`, leaving the queue alone. For tests that start an
    /// action at a given time (the C++ tests' `Engine::set_current_priority`).
    ///
    /// # Panics
    /// Panics if `time` lies in the past.
    #[cfg(test)]
    pub(crate) fn advance_time_to(&mut self, time: f64) {
        assert!(
            time >= self.current_time,
            "Engine is at '{}' and cannot go back to '{time}'",
            self.current_time
        );
        self.current_time = time;
    }

    /// Sets current simulation time in a given iteration. Only allows for monotonic values.
    ///
    /// # Panics
    /// Panics if the event lies in the past, which indicates a scheduling bug.
    fn set_current_time(&mut self, event: &Event) {
        assert!(
            event.time >= self.current_time,
            "Engine is at '{}' and got event at '{}'",
            self.current_time,
            event.time
        );
        self.current_time = event.time;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(character: u8) -> EventKind {
        EventKind::PlayerAction {
            character: CharId(character),
        }
    }

    #[test]
    fn queue_pops_in_time_order() {
        let mut queue = EventQueue::new();
        queue.push(Event::new(3.0, action(0)));
        queue.push(Event::new(1.0, action(1)));
        queue.push(Event::new(2.0, action(2)));
        queue.push(Event::new(-1.5, action(3)));

        let order: Vec<f64> = std::iter::from_fn(|| queue.pop()).map(|e| e.time).collect();
        assert_eq!(order, vec![-1.5, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn queue_breaks_ties_by_insertion_order() {
        let mut queue = EventQueue::new();
        for character in 0..10 {
            queue.push(Event::new(1.5, action(character)));
        }
        queue.push(Event::new(1.0, action(99)));

        let order: Vec<EventKind> = std::iter::from_fn(|| queue.pop()).map(|e| e.kind).collect();
        let expected: Vec<EventKind> = std::iter::once(action(99))
            .chain((0..10).map(action))
            .collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn queue_peek_len_and_clear() {
        let mut queue = EventQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.peek(), None);

        queue.push(Event::new(2.0, action(0)));
        queue.push(Event::new(1.0, action(1)));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek().map(|e| e.time), Some(1.0));

        queue.clear();
        assert!(queue.is_empty());
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn engine_advances_time_and_counts_events() {
        let mut engine = Engine::new();
        engine.prepare_set_of_iterations();
        engine.prepare_iteration(-1.5);
        assert_eq!(engine.current_time(), -1.5);

        engine.add_event(Event::new(
            0.0,
            EventKind::EncounterStart {
                character: CharId(0),
            },
        ));
        engine.add_event_in(2.0, action(0));
        engine.add_event(Event::new(-1.0, action(0)));
        engine.add_event(Event::new(60.0, EventKind::EncounterEnd));

        let event = engine.next_event().unwrap();
        assert_eq!(event.time, -1.0);
        assert_eq!(engine.current_time(), -1.0);

        let event = engine.next_event().unwrap();
        assert_eq!(
            event.kind,
            EventKind::EncounterStart {
                character: CharId(0)
            }
        );
        assert_eq!(engine.current_time(), 0.0);

        let event = engine.next_event().unwrap();
        assert_eq!(event.time, 0.5);

        let event = engine.next_event().unwrap();
        assert_eq!(event.kind, EventKind::EncounterEnd);
        assert_eq!(engine.next_event(), None);

        let counts = engine.event_counts();
        assert_eq!(counts.get(EventType::PlayerAction), 2);
        assert_eq!(counts.get(EventType::EncounterStart), 1);
        assert_eq!(counts.get(EventType::EncounterEnd), 1);
        assert_eq!(counts.get(EventType::DotTick), 0);
        assert_eq!(counts.total(), 4);
    }

    #[test]
    fn end_combat_drops_pending_events() {
        let mut engine = Engine::new();
        engine.add_event(Event::new(1.0, action(0)));
        engine.add_event(Event::new(2.0, action(0)));
        assert_eq!(engine.next_event().map(|e| e.time), Some(1.0));

        engine.end_combat();
        assert_eq!(engine.next_event(), None);
        assert_eq!(engine.current_time(), 1.0);
    }

    #[test]
    fn prepare_iteration_clears_queue_but_keeps_counts() {
        let mut engine = Engine::new();
        engine.prepare_set_of_iterations();
        engine.add_event(Event::new(1.0, action(0)));
        engine.next_event();

        engine.prepare_iteration(-2.0);
        engine.add_event(Event::new(0.0, action(0)));
        engine.next_event();

        let non_zero: Vec<_> = engine.event_counts().non_zero().collect();
        assert_eq!(non_zero, vec![(EventType::PlayerAction, 2)]);

        engine.prepare_set_of_iterations();
        assert_eq!(engine.event_counts().total(), 0);
        assert_eq!(engine.current_time(), 0.0);
        assert!(engine.queue().is_empty());
    }

    #[test]
    #[should_panic(expected = "Engine is at '5' and got event at '4'")]
    fn event_in_the_past_panics() {
        let mut engine = Engine::new();
        engine.prepare_iteration(5.0);
        engine.add_event(Event::new(4.0, action(0)));
        engine.next_event();
    }

    #[test]
    fn event_counts_merge() {
        let mut a = EventCounts::default();
        a.increment(EventType::DotTick);
        a.increment(EventType::DotTick);
        let mut b = EventCounts::default();
        b.increment(EventType::DotTick);
        b.increment(EventType::BuffRemoval);

        a.add(&b);
        assert_eq!(a.get(EventType::DotTick), 3);
        assert_eq!(a.get(EventType::BuffRemoval), 1);
        let non_zero: Vec<_> = a.non_zero().collect();
        assert_eq!(
            non_zero,
            vec![(EventType::BuffRemoval, 1), (EventType::DotTick, 3)]
        );
    }
}
