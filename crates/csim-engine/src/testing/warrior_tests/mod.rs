//! The Warrior tests of the C++ `Test/Warrior` tree, one module per test class, on the
//! [`WarriorTest`](super::warrior::WarriorTest) harness.
//!
//! Each C++ `set_up()` / `tear_down()` pair is a fresh harness, so every C++ test method is a
//! `#[test]` of its own; the mandatory spell tests (`run_mandatory_tests`) keep their names.
//! Where Forever's data differs from what the hand-authored C++ spells assumed, the expected
//! values follow the Forever data and the comment next to them says what changed.

mod arms;
mod battle_shout;
mod berserker_rage;
mod berserker_stance;
mod bloodrage;
mod bloodthirst;
mod death_wish;
mod deep_wounds;
mod defiance;
mod execute;
mod flurry;
mod fury;
mod heroic_strike;
mod mainhand_attack;
mod mortal_strike;
mod offhand_attack;
mod overpower;
mod recklessness;
mod rend;
mod revenge;
mod slam;
mod sword_specialization;
mod talent_tree;
mod two_handed_weapon_specialization;
mod unbridled_wrath;
mod warrior;
mod whirlwind;
