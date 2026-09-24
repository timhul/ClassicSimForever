//! Port of `Test/Warrior/Procs/TestSwordSpecialization`.
//!
//! Forever has no Sword Specialization talent: its extra attack is the sword part of
//! Weaponmaster (1 - 5 % of the successful melee attacks of a sword trigger an extra attack),
//! whose `DUMMY` auras the sim does not implement yet. The tests are written against a
//! Weaponmaster proc and ignored until it exists.

use crate::proc::ProcSource;
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
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
fn name_correct() {
    let test = test();
    let proc = test.proc(TALENT);
    assert_eq!(test.character().spells().procs().get(proc).name(), TALENT);
}

#[test]
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
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
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
fn proc_sources_are_valid() {
    let test = test();
    let id = test.proc(TALENT);
    let proc = test.character().spells().procs().get(id);
    assert!(proc.procs_from_source(ProcSource::MainhandSpell));
    assert!(proc.procs_from_source(ProcSource::MainhandSwing));
    assert!(proc.procs_from_source(ProcSource::OffhandSwing));
}

#[test]
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
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
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
fn proc_conditions_not_fulfilled_if_not_using_sword_in_either_mh_or_oh() {
    let mut test = test();
    test.given_1h_axe_equipped_in_mainhand();
    test.given_1h_mace_equipped_in_offhand();
    assert!(!procs_on(&mut test, ProcSource::MainhandSpell));
    assert!(!procs_on(&mut test, ProcSource::MainhandSwing));
    assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
}

#[test]
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
fn mh_proc_conditions_fulfilled_if_using_2h_sword() {
    let mut test = test();
    test.given_2h_sword_equipped();
    assert!(procs_on(&mut test, ProcSource::MainhandSpell));
    assert!(procs_on(&mut test, ProcSource::MainhandSwing));
    assert!(!procs_on(&mut test, ProcSource::OffhandSwing));
}

#[test]
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
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
#[ignore = "Weaponmaster's sword extra attack is not implemented"]
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
