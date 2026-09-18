//! Procs. Port of `Spells/Proc.*`, `Spells/ProcPPM.*` and `Spells/ProcInfo.h`.
//!
//! Only the proc sources are defined for now; the proc runtime follows in 3.8.

/// The events a proc can trigger on. Port of `ProcInfo::Source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProcSource {
    MainhandSwing,
    OffhandSwing,
    MainhandSpell,
    MeleeDodge,
    MeleeParry,
    MeleeMiss,
    MeleeCritical,
    MeleeHit,
    MeleeFullBlock,
    SpellFullResist,
    SpellCritical,
    SpellHit,
    RangedAutoShot,
    RangedSpell,
    MagicSpell,
    Manual,
}

impl ProcSource {
    pub const ALL: [ProcSource; 16] = [
        ProcSource::MainhandSwing,
        ProcSource::OffhandSwing,
        ProcSource::MainhandSpell,
        ProcSource::MeleeDodge,
        ProcSource::MeleeParry,
        ProcSource::MeleeMiss,
        ProcSource::MeleeCritical,
        ProcSource::MeleeHit,
        ProcSource::MeleeFullBlock,
        ProcSource::SpellFullResist,
        ProcSource::SpellCritical,
        ProcSource::SpellHit,
        ProcSource::RangedAutoShot,
        ProcSource::RangedSpell,
        ProcSource::MagicSpell,
        ProcSource::Manual,
    ];
}
