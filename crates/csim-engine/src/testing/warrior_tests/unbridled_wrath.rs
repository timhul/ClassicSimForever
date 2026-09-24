//! Port of `Test/Warrior/Procs/TestUnbridledWrath`.

use crate::proc::ProcSource;
use crate::testing::warrior::WarriorTest;

const PROC: &str = "Unbridled Wrath";

fn test() -> WarriorTest {
    WarriorTest::new(PROC)
}

fn given_unbridled_wrath(test: &mut WarriorTest, rank: u32) {
    test.given_fury_talent_with_rank(PROC, rank);
}

#[test]
fn name_correct() {
    let mut test = test();
    given_unbridled_wrath(&mut test, 1);
    let proc = test.proc(PROC);
    assert_eq!(test.character().spells().procs().get(proc).name(), PROC);
}

#[test]
fn proc_range_for_unbridled_wrath() {
    // Forever: 12 % per rank (the C++ 8 %: 800 - 4000).
    for rank in 1..=5 {
        let mut test = test();
        given_unbridled_wrath(&mut test, rank);
        test.given_a_mainhand_weapon_with_100_min_max_dmg();
        assert_eq!(
            test.proc_range(PROC, ProcSource::MainhandSwing),
            1200 * rank,
            "{rank} of 5"
        );
    }
}

#[test]
fn proc_sources_are_valid() {
    let mut test = test();
    given_unbridled_wrath(&mut test, 1);
    let id = test.proc(PROC);
    let proc = test.character().spells().procs().get(id);
    assert!(proc.procs_from_source(ProcSource::MainhandSwing));
    assert!(proc.procs_from_source(ProcSource::OffhandSwing));
    assert!(!proc.procs_from_source(ProcSource::MainhandSpell));
    assert!(!proc.procs_from_source(ProcSource::MagicSpell));
    assert!(!proc.procs_from_source(ProcSource::RangedSpell));
    assert!(!proc.procs_from_source(ProcSource::RangedAutoShot));
}
