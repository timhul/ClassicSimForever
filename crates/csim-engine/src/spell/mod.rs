//! The spell system. Port of `Spells/*`.
//!
//! Spells are fully data-driven: a [`spec::SpellGroupSpec`] (loaded from `data/spells/*.yaml`)
//! describes every rank, effect, buff, restriction and talent modification, and the runtime
//! (later substeps of Phase 3) interprets it.

pub mod auto_attack;
pub mod periodic;
pub mod rank_group;
pub mod runtime;
pub mod spec;
pub mod status;

pub use auto_attack::{rage_gained_from_damage, AutoAttack, AutoAttackHost, SwingReport};
pub use periodic::{Periodic, PeriodicKind, TickReport};
pub use rank_group::SpellRankGroup;
pub use runtime::{
    spell_coefficient_from_casting_time, AttackOutcome, CastReport, Spell, SpellHost,
};
pub use spec::{
    Affected, BuffRankSpec, Comparison, EffectTarget, GcdBehavior, Hand, ProcSpec,
    ResourceCostType, RestrictionSpec, SpellDb, SpellDbError, SpellEffect, SpellEffectSpec,
    SpellFileSpec, SpellFlag, SpellGroupSpec, SpellRankSpec, StatisticsSpec, TalentModification,
    TalentModificationSpec, MAX_RANK,
};
pub use status::{SpellResult, SpellStatus};
