//! Talents. Port of `Talent/*` and `Class/Warrior/TalentTrees/**` on the table model.
//!
//! - [`spec`]: the data (`data/talents/<class>.yaml`, exported from the Trait tables).

pub mod spec;

pub use spec::{TalentDb, TalentFile, TalentSpec, TalentSpecError, TalentTab};
