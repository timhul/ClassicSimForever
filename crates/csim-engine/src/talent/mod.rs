//! Talents. Port of `Talent/*` and `Class/Warrior/TalentTrees/**` on the table model.
//!
//! - [`spec`]: the data (`data/talents/<class>.yaml`, exported from the Trait tables).
//! - [`tree`]: one tab's point bookkeeping (tiers, prerequisites, the points-per-tier rule).
//! - [`character_talents`]: the character's setups; what a rank change means for its spells
//!   is applied by the character context
//!   ([`CharacterContext::apply_talent_changes`](crate::character::context::CharacterContext)).

pub mod character_talents;
pub mod spec;
pub mod tree;

pub use character_talents::{CharacterTalents, SETUP_COUNT};
pub use spec::{TalentDb, TalentFile, TalentSpec, TalentSpecError, TalentTab};
pub use tree::{RankChange, TalentState, TalentTree};
