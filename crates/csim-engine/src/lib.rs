//! ClassicSimForever simulation engine.
//!
//! This crate is a Rust port of the ClassicSim C++ engine. It contains no GUI code; see `TASKS.md`
//! in the repository root for the porting plan and the module ↔ C++ file mapping.

pub mod combat_roll;
pub mod engine;
pub mod ids;
pub mod magic_school;
pub mod mechanics;
pub mod rng;

/// Engine crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
