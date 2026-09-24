//! ClassicSimForever simulation engine.
//!
//! This crate is a Rust port of the ClassicSim C++ engine. It contains no GUI code; see `TASKS.md`
//! in the repository root for the porting plan and the module ↔ C++ file mapping.

pub mod attack_mode;
pub mod buff;
pub mod character;
pub mod character_spells;
pub mod combat_roll;
pub mod cooldown;
pub mod effect;
pub mod enchant;
pub mod engine;
pub mod equipment;
pub mod faction;
pub mod ids;
pub mod item;
pub mod magic_school;
pub mod mechanics;
pub mod phase;
pub mod proc;
pub mod race;
pub mod raid;
pub mod resource;
pub mod rng;
pub mod rotation;
pub mod rulesets;
pub mod sim_settings;
pub mod spell;
pub mod stance;
pub mod statistics;
pub mod stats;
pub mod talent;
pub mod target;

#[cfg(test)]
mod ground_truth;

/// Engine crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
