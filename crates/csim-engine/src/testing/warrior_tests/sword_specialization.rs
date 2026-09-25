//! Port of `Test/Warrior/Procs/TestSwordSpecialization`.
//!
//! Forever has no Sword Specialization talent: its extra attack is the sword part of
//! Weaponmaster (1 - 5 % of the successful melee attacks of a sword trigger an extra attack).
//! The talent's third `DUMMY` aura enables the hidden aura 12281 "Weaponmaster" (`ENABLE_PROC`),
//! which carries the proc: swords only, 200 ms internal cooldown, extra attack 1257049.

use crate::proc::ProcSource;
use crate::spell::Hand;
use crate::testing::warrior::WarriorTest;
use crate::testing::SpellTest;

const TALENT: &str = "Weaponmaster";

fn test() -> WarriorTest {
    let mut test = WarriorTest::new("Sword Specialization");
    test.given_arms_talent_with_rank(TALENT, 1);
    test
}

fn procs_on(test: &mut WarriorTest, source: ProcSource) -> bool {
    test.proc_conditions_fulfilled(TALENT, source)
}

#[test]
fn name_correct() {
    let test = test();
    let proc = test.proc(TALENT);
    assert_eq!(test.character().spells().procs().get(proc).name(), TALENT);
}

#[test]
fn proc_range_for_sword_spec() {
    for rank in 1..=5 {
        let mut test = WarriorTest::new("Sword Specialization");
        test.given_arms_talent_with_rank(TALENT, rank);
        test.given_1h_sword_equipped_in_mainhand();
        assert_eq!(
            test.proc_range(TALENT, ProcSource::MainhandSwing),
            100 * rank,
            "{rank} of 5"
        );
    }
}

#[test]
fn proc_sources_are_valid() {
    let test = test();
    let id = test.proc(TALENT);
    let proc = test.character().spells().procs().get(id);
    assert!(proc.procs_from_source(ProcSource::MainhandSpell));
    assert!(proc.procs_from_source(ProcSource::MainhandSwing));
    assert!(proc.procs_from_source(ProcSource::OffhandSwing));
}

#[test]
fn mh_proc_conditions_fulfilled_if_using_sword_in_mh() {
    let mut test = test();
    test.given_1h_sword_equipped_in_mainhand();
    assert!(procs_on(&mut test, ProcSource::MainhandSpell));
    assert!(procs_on(&mut test, ProcSource::MainhandSwing));

    test.given_no_offhand();
    assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
    for given in [
        SpellTest::given_1h_axe_equipped_in_offhand as fn(&mut SpellTest),
        SpellTest::given_1h_mace_equipped_in_offhand,
        SpellTest::given_fist_weapon_equipped_in_offhand,
        SpellTest::given_dagger_equipped_in_offhand,
    ] {
        given(&mut test);
        assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
    }
}

#[test]
fn proc_conditions_not_fulfilled_if_not_using_sword_in_either_mh_or_oh() {
    let mut test = test();
    test.given_1h_axe_equipped_in_mainhand();
    test.given_1h_mace_equipped_in_offhand();
    assert!(!procs_on(&mut test, ProcSource::MainhandSpell));
    assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
    assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
}

#[test]
fn mh_proc_conditions_fulfilled_if_using_2h_sword() {
    let mut test = test();
    test.given_2h_sword_equipped();
    assert!(procs_on(&mut test, ProcSource::MainhandSpell));
    assert!(procs_on(&mut test, ProcSource::MainhandSwing));
    assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
}

#[test]
fn mh_proc_conditions_not_fulfilled_if_using_other_types_of_2h() {
    let mut test = test();
    for given in [
        SpellTest::given_2h_axe_equipped as fn(&mut SpellTest),
        SpellTest::given_2h_mace_equipped,
        SpellTest::given_polearm_equipped,
        SpellTest::given_staff_equipped,
    ] {
        given(&mut test);
        assert!(!procs_on(&mut test, ProcSource::MainhandSpell));
        assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
        assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
    }
}

#[test]
fn oh_proc_conditions_fulfilled_if_using_sword_in_oh() {
    let mut test = test();
    test.given_1h_sword_equipped_in_offhand();
    assert!(procs_on(&mut test, ProcSource::OffhandSwing));

    test.given_no_mainhand();
    assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
    for given in [
        SpellTest::given_1h_axe_equipped_in_mainhand as fn(&mut SpellTest),
        SpellTest::given_1h_mace_equipped_in_mainhand,
        SpellTest::given_fist_weapon_equipped_in_mainhand,
        SpellTest::given_dagger_equipped_in_mainhand,
    ] {
        given(&mut test);
        assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
    }
}

#[test]
fn proc_is_enabled_by_the_talent() {
    let mut test = WarriorTest::new("Sword Specialization");
    let proc = test.proc(TALENT);
    let enabled = |test: &WarriorTest| test.character().spells().procs().is_enabled(proc);
    assert!(!enabled(&test));
    test.given_arms_talent_with_rank(TALENT, 1);
    assert!(enabled(&test));
    test.with_ctx(|ctx| ctx.clear_talents());
    assert!(!enabled(&test));
}

#[test]
fn proc_grants_one_extra_attack_and_starts_its_internal_cooldown() {
    let mut test = test();
    test.given_1h_sword_equipped_in_mainhand();
    test.given_a_guaranteed_white_hit();
    let proc = test.proc(TALENT);
    test.with_ctx(|ctx| ctx.perform_proc(proc));
    assert_eq!(test.character().pending_extra_attacks(), 1);
    let ready = |test: &mut WarriorTest| {
        let proc = test.character().spells().procs().get(proc).clone();
        test.with_ctx(|ctx| proc.is_ready(ctx))
    };
    assert!(!ready(&mut test));
    let swings = test.with_ctx(|ctx| ctx.perform_extra_attacks());
    assert_eq!(swings.len(), 1);
    assert_eq!(swings[0].hand, Hand::Mainhand);
    assert_eq!(test.character().pending_extra_attacks(), 0);
    test.given_engine_priority_pushed_forward(0.199);
    assert!(!ready(&mut test));
    test.given_engine_priority_pushed_forward(0.001);
    assert!(ready(&mut test));
}
