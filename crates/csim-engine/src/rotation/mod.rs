//! Rotations: what a character casts and when. Port of `Rotation/*`.
//!
//! - [`spec`]: the rotation file schema (`data/rotations/<class>/*.yaml`) and its loader.
//!   Port of the data half of `RotationFileReader` and the fields of `Rotation`.
//! - [`condition`]: the condition mini-language parser and evaluation. Port of the sentence
//!   grammar in `RotationFileReader`, `Rotation::add_conditionals` and `Rotation/Conditions/*`.
//! - `executor` (Phase 5.3): linking executors to spells and running the rotation.

pub mod condition;
pub mod spec;

pub use condition::{
    BuiltinVariable, Comparator, Condition, ConditionContext, ConditionParseError, Measure,
    Sentence, Test,
};
pub use spec::{CastIfSpec, RotationDb, RotationSpec, RotationSpecError};
