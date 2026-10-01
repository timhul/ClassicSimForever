//! Bloodthrill (Forever Arms talent 1289682): main-hand attacks against a target afflicted by
//! your Rend have a 4-20 % chance (the rank value, `chance_effect: 0`) to allow Overpower. The
//! payload 1282733 gives the combo point Overpower costs, the same window a dodge opens.

use crate::engine::EventType;
use crate::proc::ProcSource;
use crate::spell::{Hand, SpellStatus};
use crate::testing::warrior::WarriorTest;

const TALENT: &str = "Bloodthrill";

/// Battle Stance, full rage, the test sword and Bloodthrill at `rank`.
fn test(rank: u32) -> WarriorTest {
    let mut test = WarriorTest::new(TALENT);
    test.given_warrior_in_battle_stance();
    test.given_warrior_has_rage(100);
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.given_an_offhand_weapon_with_100_min_max_dmg();
    test.given_arms_talent_with_rank(TALENT, rank);
    test
}

fn given_rend_on_the_target(test: &mut WarriorTest) {
    test.given_a_guaranteed_melee_ability_hit();
    test.cast("Rend");
    test.given_warrior_has_rage(100);
}

fn procs_on(test: &mut WarriorTest, source: ProcSource) -> bool {
    test.proc_conditions_fulfilled(TALENT, source)
}

#[test]
fn proc_chance_is_the_rank_value() {
    for rank in 1..=5 {
        let mut test = test(rank);
        assert_eq!(
            test.proc_range(TALENT, ProcSource::MainhandSwing),
            400 * rank,
            "{rank} of 5"
        );
    }
}

#[test]
fn main_hand_swings_and_abilities_only() {
    let test = test(5);
    let proc = test.proc(TALENT);
    let proc = test.character().spells().procs().get(proc);
    assert!(proc.procs_from_source(ProcSource::MainhandSwing));
    assert!(proc.procs_from_source(ProcSource::MainhandSpell));
    assert!(!proc.procs_from_source(ProcSource::OffhandSwing));
    assert!(!proc.procs_from_source(ProcSource::OffhandSpell));
    assert!(!proc.procs_from_source(ProcSource::MeleeDodge));
}

#[test]
fn needs_rend_on_the_target() {
    let mut test = test(5);
    assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
    assert!(!procs_on(&mut test, ProcSource::MainhandSpell));

    given_rend_on_the_target(&mut test);
    assert!(procs_on(&mut test, ProcSource::MainhandSwing));
    assert!(procs_on(&mut test, ProcSource::MainhandSpell));

    // Rend lasts 21 s.
    test.when_running_queued_events_until(20.99);
    assert!(procs_on(&mut test, ProcSource::MainhandSwing));
    assert_eq!(test.when_running_until_event(EventType::BuffRemoval), 21.0);
    assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
}

#[test]
fn a_dodged_rend_does_not_enable_it() {
    let mut test = test(5);
    test.given_a_guaranteed_melee_ability_dodge();
    test.cast("Rend");
    assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
}

#[test]
fn proc_allows_overpower() {
    let mut test = test(5);
    given_rend_on_the_target(&mut test);
    // The Rend cast may have proc'd it already: its debuff is up when its proc check runs.
    test.with_ctx(|ctx| ctx.character.spend_combo_points());
    let gcd = test.character().global_cooldown();
    test.given_engine_priority_pushed_forward(gcd);
    assert_eq!(test.character().combo_points(test.now()), 0);
    test.then_status_is("Overpower", SpellStatus::InsufficientComboPoints);

    let proc = test.proc(TALENT);
    test.with_ctx(|ctx| ctx.perform_proc(proc));
    test.then_overpower_is_active();
    test.then_status_is("Overpower", SpellStatus::Available);

    // The same 6 s window a dodge opens.
    test.given_engine_priority_pushed_forward(5.99);
    test.then_overpower_is_active();
    test.given_engine_priority_pushed_forward(0.02);
    test.then_status_is("Overpower", SpellStatus::InsufficientComboPoints);
}

#[test]
fn main_hand_swings_proc_at_the_rank_value() {
    const SWINGS: u32 = 2000;
    for (rend, expected) in [(false, 0..1), (true, 340..460)] {
        let mut test = test(5);
        if rend {
            given_rend_on_the_target(&mut test);
        }
        test.given_a_guaranteed_white_hit();
        let proc = test.proc(TALENT);
        for _ in 0..SWINGS {
            test.when_swing_is_performed(Hand::Mainhand);
        }
        let procs = test.character().spells().procs().get(proc).procs();
        assert!(
            expected.contains(&procs),
            "{procs} procs in {SWINGS} swings, Rend {rend}"
        );
        if rend {
            test.then_overpower_is_active();
        }
    }
}
