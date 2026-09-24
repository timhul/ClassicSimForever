//! Port of `Test/Warrior/Talents/TestFury`, walked through Forever's Fury tree (see
//! [`super::arms`]). Forever has no Improved Battle Shout or Improved Demoralizing Shout; the
//! walk uses Blood Craze, Boundless Rage and Iron Will in their tiers.

use super::talent_tree::TalentTreeTest;

fn tree() -> TalentTreeTest {
    TalentTreeTest::new("Fury")
}

#[test]
fn spending_talent_points() {
    let mut t = tree();
    assert!(!t.dec("Cruelty"));
    assert!(t.increment("Cruelty", 5));
    // 5 points.
    assert!(!t.inc("Cruelty"));
    assert!(t.dec("Cruelty"));
    assert!(t.inc("Cruelty"));

    // Spend a point in tier 2 and check that tier 1 cannot drop below 5 points.
    assert!(t.inc("Unbridled Wrath"));
    assert!(!t.dec("Cruelty"));
    assert!(t.inc("Booming Voice"));
    assert!(t.dec("Cruelty"));
    assert!(!t.dec("Cruelty"));
    assert!(!t.dec("Booming Voice"));
    assert!(t.inc("Cruelty"));
    assert!(t.dec("Booming Voice"));

    // 5 Cruelty, 5 Unbridled Wrath.
    assert!(t.increment("Unbridled Wrath", 4));
    assert!(t.increment("Blood Craze", 3));
    assert!(t.increment("Boundless Rage", 2));
    assert!(t.increment("Enrage", 5));
    assert!(t.inc("Death Wish"));
    // Tier 5 leans on the 20 points below it.
    assert!(!t.dec("Blood Craze"));
    assert!(!t.dec("Enrage"));
    assert!(!t.dec("Unbridled Wrath"));
    assert!(!t.dec("Cruelty"));

    // Shifting points in tier 1.
    assert!(t.inc("Booming Voice"));
    assert!(t.dec("Cruelty"));
    assert!(!t.dec("Cruelty"));
    assert!(!t.dec("Booming Voice"));
    assert!(t.inc("Cruelty"));
    assert!(t.dec("Booming Voice"));

    // Shifting points in tier 2.
    assert!(t.inc("Iron Will"));
    assert!(t.dec("Unbridled Wrath"));
    assert!(!t.dec("Unbridled Wrath"));
    assert!(!t.dec("Iron Will"));
    assert!(t.inc("Unbridled Wrath"));
    assert!(t.dec("Iron Will"));

    // Shifting points in tier 3.
    assert!(t.inc("Improved Cleave"));
    assert!(t.dec("Blood Craze"));
    assert!(!t.dec("Improved Cleave"));
    assert!(!t.dec("Blood Craze"));
    assert!(t.inc("Blood Craze"));
    assert!(t.dec("Improved Cleave"));

    // Shifting points in tier 4.
    assert!(t.inc("Improved Execute"));
    assert!(t.dec("Enrage"));
    assert!(!t.dec("Improved Execute"));
    assert!(!t.dec("Enrage"));
    assert!(t.inc("Enrage"));
    assert!(t.dec("Improved Execute"));

    assert!(t.increment("Dual Wield Specialization", 5));
    assert!(t.increment("Flurry", 5));
    assert!(t.increment("Improved Execute", 2));
    assert!(t.inc("Bloodthirst"));
    assert_eq!(t.tree_points(), 34);
    // The parent cannot go while the child is active, although the points allow it.
    assert!(!t.dec("Death Wish"));
    // The lower tiers with only 5 points each cannot be decremented.
    assert!(!t.dec("Blood Craze"));
    assert!(!t.dec("Unbridled Wrath"));
    assert!(!t.dec("Cruelty"));
    // Tier 4 can come down to the 30 points Bloodthirst needs below it, not lower.
    assert!(t.decrement("Dual Wield Specialization", 3));
    assert!(!t.dec("Dual Wield Specialization"));
    assert!(t.dec("Bloodthirst"));
}

#[test]
fn clearing_tree_after_filling() {
    let mut t = tree();
    assert!(t.increment("Booming Voice", 5));
    assert!(t.increment("Cruelty", 5));
    assert!(t.increment("Unbridled Wrath", 5));
    assert!(t.increment("Blood Craze", 3));
    assert!(t.increment("Boundless Rage", 2));
    assert!(t.increment("Enrage", 5));
    assert!(t.increment("Flurry", 5));
    assert!(t.inc("Death Wish"));
    assert!(t.inc("Bloodthirst"));
    t.clear_tree();
}

/// A 34 point dual wield Fury spec.
fn spec_dw_fury(t: &mut TalentTreeTest) {
    assert!(t.increment("Cruelty", 5));
    assert!(t.increment("Unbridled Wrath", 5));
    assert!(t.increment("Blood Craze", 3));
    assert!(t.increment("Boundless Rage", 2));
    assert!(t.increment("Dual Wield Specialization", 5));
    assert!(t.increment("Enrage", 5));
    assert!(t.increment("Flurry", 5));
    assert!(t.increment("Improved Execute", 2));
    assert!(t.inc("Death Wish"));
    assert!(t.inc("Bloodthirst"));
}

#[test]
fn refilling_tree_after_switching_talent_setup() {
    let mut t = tree();
    assert_eq!(t.tree_points(), 0);
    spec_dw_fury(&mut t);
    assert_eq!(t.tree_points(), 34);
    for setup in [1, 2] {
        t.switch_to_setup(setup);
        assert_eq!(t.tree_points(), 0);
        spec_dw_fury(&mut t);
        assert_eq!(t.tree_points(), 34);
    }
}
