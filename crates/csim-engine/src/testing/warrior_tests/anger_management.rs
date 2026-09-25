//! Anger Management (no C++ test class: ClassicSim's `AngerManagement` was a plain
//! `PeriodicResourceGainSpell` without tests of its own).

use crate::engine::EventType;
use crate::testing::warrior::WarriorTest;

const TALENT: &str = "Anger Management";

/// A Warrior at 0 rage in combat, with Anger Management (behind its prerequisite, 5 of 5
/// Improved Tactical Mastery) if `talented`.
fn in_combat(talented: bool) -> WarriorTest {
    let mut test = WarriorTest::unprepared(TALENT);
    if talented {
        test.given_tactical_mastery(5);
        test.given_arms_talent_with_rank(TALENT, 1);
    }
    test.prepare_set_of_combat_iterations();
    test.given_warrior_has_rage(0);
    test.with_ctx(|ctx| ctx.encounter_start());
    test
}

/// Runs the queued events up to and including those at `time`. The passive already ticks
/// from learning the talent; combat start restarts it, and the ticks of that first
/// application are ignored.
fn when_running_until(test: &mut WarriorTest, time: f64) {
    test.when_running_queued_events_until(time + 0.01);
}

#[test]
fn gives_1_rage_every_3_seconds_in_combat() {
    let mut test = in_combat(true);
    when_running_until(&mut test, 2.9);
    test.then_warrior_has_rage(0);
    for tick in 1..=5 {
        when_running_until(&mut test, 3.0 * f64::from(tick));
        test.then_warrior_has_rage(tick);
    }
}

#[test]
fn restarts_ticking_at_every_combat_start() {
    let mut test = in_combat(true);
    when_running_until(&mut test, 4.5);
    test.then_warrior_has_rage(1);

    test.prepare_set_of_combat_iterations();
    test.given_engine_priority_at(0.0);
    test.given_warrior_has_rage(0);
    test.with_ctx(|ctx| ctx.encounter_start());
    when_running_until(&mut test, 2.9);
    test.then_warrior_has_rage(0);
    when_running_until(&mut test, 3.0);
    test.then_warrior_has_rage(1);
}

#[test]
fn gives_no_rage_without_the_talent() {
    let mut test = in_combat(false);
    // Runs every queued tick; with the talent the ticks would chain forever.
    test.when_running_only(EventType::DotTick);
    test.then_warrior_has_rage(0);
}
