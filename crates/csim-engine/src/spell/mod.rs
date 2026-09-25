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

pub use auto_attack::{swing_rage, AutoAttack, AutoAttackHost, SwingReport};
pub use dbc::{
    AuraState, AuraType, DefenseType, ImplicitTarget, Mechanic, PowerType, ProcFlags,
    ShapeshiftForm, SpellAttr0, SpellEffectName, SpellModOp, SpellSchoolMask,
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
    Levels, PowerCost, SpellDb, SpellDbError, SpellFile, SpellRecord, Unsupported,
    GLOBAL_COOLDOWN_CATEGORY,
};
pub use runtime::{
    spell_coefficient_from_casting_time, AttackOutcome, CastReport, Spell, SpellHost, SpellSetup,
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
