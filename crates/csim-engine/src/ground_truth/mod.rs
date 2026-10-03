//! Ground-truth tests: the assertions and conclusions of the `classic-warrior` wiki
//! (`C:\rust\classic-warrior.wiki`) checked against the simulator.
//!
//! - `attack_table`: `Attack-table.md` — miss, hit suppression, dual wield, glancing, dodge,
//!   parry, block, crit and the measured attack breakdowns.
//! - `crit_aura_suppression`: `Crit-aura-suppression.md` — the flat 1.8 % reduction of crit
//!   gained from auras against +3 level mobs.
//! - `windfury_totem`: `Windfury-Totem.md` — proc triggers, the extra attack and the charged
//!   attack power aura.
//!
//! The wiki documents WoW Classic; where the Forever client tables carry rebalanced numbers
//! (Windfury Totem's attack power, duration and internal cooldown) the tests use the table
//! values and say so; the hit suppression above a defense difference of 10 is not in Forever
//! (confirmed by Magey), so the tests assert its absence. These tests document the expected behaviour: a failing test here is a
//! deviation of the simulator from the ground truth, not a broken test.

mod attack_table;
mod crit_aura_suppression;
mod windfury_totem;
