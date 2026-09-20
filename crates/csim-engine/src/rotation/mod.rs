//! Rotations: what a character casts and when. Port of `Rotation/*`.
//!
//! - [`spec`]: the rotation file schema (`data/rotations/<class>/*.yaml`) and its loader.
//!   Port of the data half of `RotationFileReader` and the fields of `Rotation`.
//! - `condition` (Phase 5.2): the condition mini-language parser.
//! - `executor` (Phase 5.3): linking executors to spells and running the rotation.

pub mod spec;

pub use spec::{CastIfSpec, RotationDb, RotationSpec, RotationSpecError};
