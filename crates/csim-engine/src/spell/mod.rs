//! The spell system. Port of `Spells/*`, driven by the client table data.
//!
//! Spells are fully data-driven: a [`record::SpellRecord`] (one joined table row, exported from
//! the client dump into `data/spells/*.yaml`) plus its [`overrides::SpellOverride`] (the
//! hand-written part) describe every effect, aura, cost, cooldown and restriction, and the
//! runtime ([`Spell`], [`crate::buff::Buff`], [`crate::effect::Effect`], [`crate::proc::Proc`],
//! [`Periodic`]) interprets the retail [`dbc`] vocabulary.

use serde::{Deserialize, Serialize};

pub mod auto_attack;
pub mod dbc;
pub mod modifiers;
pub mod overrides;
pub mod periodic;
pub mod rank_group;
pub mod record;
pub mod runtime;
pub mod status;
#[cfg(test)]
pub(crate) mod test_world;

pub use auto_attack::{AutoAttack, AutoAttackHost, SwingReport, swing_rage};
pub use dbc::{
    AuraState, AuraType, DefenseType, ImplicitTarget, Mechanic, PowerType, ProcFlags,
    ShapeshiftForm, SpellAttr0, SpellAttr1, SpellEffectName, SpellModOp, SpellSchoolMask,
};
pub use modifiers::{SpellModifier, SpellModifiers};
pub use overrides::{
    EffectScript, EventScript, OverrideDefaults, OverrideError, OverrideFile, Overrides,
    ProcHitMask, ProcOverride, ScriptKind, ScriptParams, SimFlag, SpellOverride, ThreatOverride,
};
pub use periodic::{Periodic, PeriodicKind, TickReport};
pub use rank_group::SpellRankGroup;
pub use record::{
    AuraOptions, AuraRestrictions, Categories, ClassOptions, Cooldown, EffectRecord, EquippedItems,
    GLOBAL_COOLDOWN_CATEGORY, Levels, PowerCost, SpellDb, SpellDbError, SpellFile, SpellRecord,
    Unsupported,
};
pub use runtime::{
    AttackOutcome, CastReport, Spell, SpellHost, SpellSetup, spell_coefficient_from_casting_time,
};
pub use status::{SpellResult, SpellStatus};

/// Rank number that selects the highest learned rank of a spell.
pub const MAX_RANK: u32 = 0;

/// Which weapon an effect or attack concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hand {
    Mainhand,
    Offhand,
}
