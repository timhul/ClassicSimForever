//! Rotations: what a character casts and when. Port of `Rotation/*`.
//!
//! - [`spec`]: the rotation file schema (`data/rotations/<class>/*.yaml`) and its loader.
//!   Port of the data half of `RotationFileReader` and the fields of `Rotation`.
//! - [`condition`]: the condition mini-language parser and evaluation. Port of the sentence
//!   grammar in `RotationFileReader`, `Rotation::add_conditionals` and `Rotation/Conditions/*`.
//! - [`executor`]: the runtime — executors linked to a character's spells, the precombat
//!   actions, `perform_rotation` and the executor statistics. Port of the runtime half of
//!   `Rotation.cpp` and `RotationExecutor.*`.

pub mod condition;
pub mod executor;
pub mod spec;

pub use condition::{
    BuiltinVariable, Comparator, Condition, ConditionContext, ConditionParseError, Measure,
    NextChange, Sentence, Test, Watched,
};
pub use executor::{
    DecidedBy, ExecutorStatistics, LinkedExecutor, Rotation, RotationDecision, RotationExecutor,
    RotationHost, SkipReason,
};
pub use spec::{CastIfSpec, RotationDb, RotationSpec, RotationSpecError};
