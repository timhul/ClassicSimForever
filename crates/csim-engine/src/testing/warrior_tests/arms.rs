//! Port of `Test/Warrior/Talents/TestArms`, walked through Forever's Arms tree: the same
//! rules as the C++ walk (a tier opens at 5 points per tier above it, a point cannot come out
//! of a tier that a higher tier leans on, prerequisites), with Forever's talents and tiers.

use super::talent_tree::TalentTreeTest;

fn tree() -> TalentTreeTest {
    TalentTreeTest::new("Arms")
}

#[test]
fn spending_talent_points() {
    let mut t = tree();
    assert!(!t.dec("Deflection"));
    assert!(t.increment("Deflection", 5));
    // 5 points.
    assert!(!t.inc("Deflection"));
    assert!(t.dec("Deflection"));
    assert!(t.inc("Deflection"));

    // Spend a point in tier 2 and check that tier 1 cannot drop below 5 points.
    assert!(t.inc("Improved Tactical Mastery"));
    assert!(!t.dec("Deflection"));
    assert!(t.inc("Improved Rend"));
    assert!(t.dec("Deflection"));
    assert!(!t.dec("Deflection"));
    assert!(!t.dec("Improved Rend"));
    assert!(t.inc("Deflection"));
    assert!(t.dec("Improved Rend"));

    // 5 Deflection, 5 Improved Tactical Mastery.
    assert!(t.increment("Improved Tactical Mastery", 4));
    // Deep Wounds needs 3 of 3 Improved Rend.
    for _ in 0..2 {
        assert!(!t.inc("Deep Wounds"));
        assert!(t.inc("Improved Rend"));
    }
    assert!(!t.inc("Deep Wounds"));
    assert!(t.inc("Improved Rend"));
    // Improved Rend cannot lose points once Deep Wounds has some.
    for _ in 0..3 {
        assert!(t.inc("Deep Wounds"));
        assert!(!t.dec("Improved Rend"));
    }
    assert!(t.increment("Improved Overpower", 2));
    assert!(t.increment("Impale", 2));
    // Forever's Impale does not need Deep Wounds (the C++ refused this).
    assert!(t.dec("Deep Wounds"));
    assert!(t.inc("Deep Wounds"));
    assert_eq!(t.tree_points(), 20);
    assert!(t.inc("Sweeping Strikes"));

    // Tier 5 leans on the 20 points below it.
    assert!(!t.dec("Impale"));
    assert!(!t.dec("Deep Wounds"));
    assert!(!t.dec("Improved Tactical Mastery"));
    assert!(!t.dec("Deflection"));

    // Shifting points in tier 1.
    assert!(t.inc("Improved Heroic Strike"));
    assert!(t.dec("Deflection"));
    assert!(!t.dec("Deflection"));
    assert!(!t.dec("Improved Heroic Strike"));
    assert!(t.inc("Deflection"));
    assert!(t.dec("Improved Heroic Strike"));

    // Shifting points in tier 2.
    assert!(t.inc("Improved Charge"));
    assert!(t.dec("Improved Tactical Mastery"));
    assert!(!t.dec("Improved Charge"));
    assert!(!t.dec("Improved Tactical Mastery"));
    assert!(t.inc("Improved Tactical Mastery"));
    assert!(t.dec("Improved Charge"));

    // Shifting points in tier 3 (Anger Management needs 5 of 5 Improved Tactical Mastery).
    assert!(t.inc("Anger Management"));
    assert!(t.dec("Deep Wounds"));
    assert!(!t.dec("Anger Management"));
    assert!(!t.dec("Deep Wounds"));
    assert!(t.inc("Deep Wounds"));
    assert!(t.dec("Anger Management"));

    // Shifting points in tier 4.
    assert!(t.inc("Two-Handed Weapon Specialization"));
    assert!(t.dec("Impale"));
    assert!(!t.dec("Two-Handed Weapon Specialization"));
    assert!(!t.dec("Two-Handed Weapon Specialization"));
    assert!(t.inc("Impale"));
    assert!(t.dec("Two-Handed Weapon Specialization"));

    // Mortal Strike needs 30 points and Sweeping Strikes.
    assert!(t.increment("Weaponmaster", 5));
    for _ in 0..3 {
        assert!(!t.inc("Mortal Strike"));
        assert!(t.inc("Two-Handed Weapon Specialization"));
    }
    assert!(!t.inc("Mortal Strike"));
    assert!(t.inc("Improved Slam"));
    assert_eq!(t.tree_points(), 30);
    assert!(t.inc("Mortal Strike"));
    // The parent cannot go while the child is active, although the points allow it.
    assert!(!t.dec("Sweeping Strikes"));
    // The lower tiers cannot be decremented.
    assert!(!t.dec("Weaponmaster"));
    assert!(!t.dec("Impale"));
    assert!(!t.dec("Deep Wounds"));
    assert!(!t.dec("Improved Tactical Mastery"));
    assert!(!t.dec("Deflection"));
    assert!(t.dec("Mortal Strike"));
}

#[test]
fn clearing_tree_after_filling() {
    let mut t = tree();
    assert!(t.increment("Deflection", 5));
    assert!(t.increment("Improved Rend", 3));
    assert!(t.increment("Improved Tactical Mastery", 5));
    assert!(t.increment("Deep Wounds", 3));
    assert!(t.inc("Anger Management"));
    assert!(t.increment("Impale", 2));
    assert!(t.increment("Two-Handed Weapon Specialization", 3));
    assert!(t.increment("Weaponmaster", 5));
    assert!(t.inc("Sweeping Strikes"));
    assert!(t.increment("Improved Slam", 2));
    assert!(t.inc("Mortal Strike"));
    assert!(!t.dec("Two-Handed Weapon Specialization"));
    t.clear_tree();
}

/// A 31 point Mortal Strike spec.
fn spec_ms(t: &mut TalentTreeTest) {
    assert!(t.increment("Improved Rend", 3));
    assert!(t.increment("Deflection", 3));
    assert!(t.increment("Improved Tactical Mastery", 5));
    assert!(t.increment("Improved Overpower", 2));
    assert!(t.inc("Anger Management"));
    assert!(t.increment("Deep Wounds", 3));
    assert!(t.increment("Impale", 2));
    assert!(t.increment("Two-Handed Weapon Specialization", 3));
    assert!(t.inc("Sweeping Strikes"));
    assert!(t.increment("Weaponmaster", 5));
    assert!(t.increment("Improved Slam", 2));
    assert!(t.inc("Mortal Strike"));
}

#[test]
fn refilling_tree_after_switching_talent_setup() {
    let mut t = tree();
    assert_eq!(t.tree_points(), 0);
    spec_ms(&mut t);
    assert_eq!(t.tree_points(), 31);
    for setup in [1, 2] {
        t.switch_to_setup(setup);
        assert_eq!(t.tree_points(), 0);
        spec_ms(&mut t);
        assert_eq!(t.tree_points(), 31);
    }
}
