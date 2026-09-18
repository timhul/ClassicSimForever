//! The spell system. Port of `Spells/*`.
//!
//! Spells are fully data-driven: a [`spec::SpellGroupSpec`] (loaded from `data/spells/*.yaml`)
//! describes every rank, effect, buff, restriction and talent modification, and the runtime
//! (later substeps of Phase 3) interprets it.

pub mod runtime;
pub mod spec;
pub mod status;

pub use runtime::{AttackOutcome, CastReport, Spell, SpellHost};
pub use spec::{
    Affected, BuffRankSpec, Comparison, EffectTarget, GcdBehavior, Hand, ResourceCostType,
    RestrictionSpec, SpellDb, SpellDbError, SpellEffect, SpellEffectSpec, SpellFileSpec, SpellFlag,
    SpellGroupSpec, SpellRankSpec, StatisticsSpec, TalentModification, TalentModificationSpec,
};
pub use status::{SpellResult, SpellStatus};
