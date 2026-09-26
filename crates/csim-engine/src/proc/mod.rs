//! Procs. Port of `Spells/Proc.*`, `Spells/ProcPPM.*`, `Spells/ProcInfo.h` and
//! `Character/EnabledProcs.*`.
//!
//! The table data describes a proc by the events it reacts to (`SpellAuraOptions.ProcTypeMask`,
//! retail `ProcFlags`) and the hit results that count (the overrides' `ProcHitMask`); the engine
//! still runs proc checks by [`ProcSource`], the C++ event vocabulary, so
//! [`ProcSource::from_masks`] translates one into the other.

use serde::{Deserialize, Serialize};

use crate::spell::Hand;
use crate::spell::dbc::ProcFlags;
use crate::spell::overrides::ProcHitMask;

/// The events a proc can trigger on. Port of `ProcInfo::Source`.
///
/// A landed swing is reported by its hand (`MainhandSwing` / `OffhandSwing`), a landed melee
/// ability as `MainhandSpell` (or `OffhandSpell` for an ability's off-hand strike: Whirlwind
/// with Raging Blows), and a crit additionally as `MeleeCritical` (the C++
/// `melee_mh_white_hit_effect` / `melee_mh_yellow_hit_effect` plus `add_crit_dmg`); the
/// avoided results by their kind. The C++ `MeleeHit` result source is kept in the vocabulary
/// but nothing emits it: [`ProcSource::from_masks`] never listens to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcSource {
    MainhandSwing,
    OffhandSwing,
    MainhandSpell,
    OffhandSpell,
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
    /// A melee or ranged attack landed on the character (`TAKE_*` proc flags: Enrage, Shield
    /// Specialization); run by the incoming-damage event.
    AttackTaken,
    Manual,
}

impl ProcSource {
    pub const ALL: [ProcSource; 18] = [
        ProcSource::MainhandSwing,
        ProcSource::OffhandSwing,
        ProcSource::MainhandSpell,
        ProcSource::OffhandSpell,
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
        ProcSource::AttackTaken,
        ProcSource::Manual,
    ];

    /// The hand a source concerns (the off hand only for off-hand swings and strikes).
    pub fn hand(self) -> Hand {
        match self {
            ProcSource::OffhandSwing | ProcSource::OffhandSpell => Hand::Offhand,
            _ => Hand::Mainhand,
        }
    }

    /// The sources a proc with `proc_type_mask` and `hit_mask` listens to.
    ///
    /// Landed hits are reported per event kind (main-hand / off-hand swing, main-hand ability,
    /// ranged, magic) so that a proc never sees one hit twice; any other hit result in
    /// `hit_mask` (critical, miss, dodge, parry, block) is reported by its result kind, which
    /// covers swings and abilities alike. A crit-only proc (Flurry, Deep Wounds) therefore
    /// listens to `MeleeCritical` only.
    pub fn from_masks(proc_type_mask: ProcFlags, hit_mask: ProcHitMask) -> Vec<ProcSource> {
        let mut sources = Vec::new();
        let mut push = |source: ProcSource| {
            if !sources.contains(&source) {
                sources.push(source);
            }
        };
        let melee_swing = proc_type_mask.contains(ProcFlags::DEAL_MELEE_SWING);
        let melee_ability = proc_type_mask.contains(ProcFlags::DEAL_MELEE_ABILITY);
        let ranged_swing = proc_type_mask.contains(ProcFlags::DEAL_RANGED_ATTACK);
        let ranged_ability = proc_type_mask.contains(ProcFlags::DEAL_RANGED_ABILITY);
        let magic = proc_type_mask
            .intersects(ProcFlags::DEAL_HARMFUL_SPELL | ProcFlags::DEAL_HELPFUL_SPELL);
        let landed = hit_mask.contains(ProcHitMask::NORMAL);
        if landed {
            if melee_swing {
                if !proc_type_mask.contains(ProcFlags::OFF_HAND_WEAPON_SWING)
                    || proc_type_mask.contains(ProcFlags::MAIN_HAND_WEAPON_SWING)
                {
                    push(ProcSource::MainhandSwing);
                }
                if !proc_type_mask.contains(ProcFlags::MAIN_HAND_WEAPON_SWING)
                    || proc_type_mask.contains(ProcFlags::OFF_HAND_WEAPON_SWING)
                {
                    push(ProcSource::OffhandSwing);
                }
            }
            if melee_ability {
                push(ProcSource::MainhandSpell);
                push(ProcSource::OffhandSpell);
            }
            if ranged_swing {
                push(ProcSource::RangedAutoShot);
            }
            if ranged_ability {
                push(ProcSource::RangedSpell);
            }
            if magic {
                push(ProcSource::MagicSpell);
                push(ProcSource::SpellHit);
            }
        }
        if melee_swing || melee_ability {
            if hit_mask.contains(ProcHitMask::CRITICAL) && !landed {
                push(ProcSource::MeleeCritical);
            }
            if hit_mask.contains(ProcHitMask::MISS) {
                push(ProcSource::MeleeMiss);
            }
            if hit_mask.contains(ProcHitMask::DODGE) {
                push(ProcSource::MeleeDodge);
            }
            if hit_mask.contains(ProcHitMask::PARRY) {
                push(ProcSource::MeleeParry);
            }
            if hit_mask.intersects(ProcHitMask::BLOCK | ProcHitMask::FULL_BLOCK) {
                push(ProcSource::MeleeFullBlock);
            }
        }
        if magic {
            if hit_mask.contains(ProcHitMask::CRITICAL) && !landed {
                push(ProcSource::SpellCritical);
            }
            if hit_mask.contains(ProcHitMask::FULL_RESIST) {
                push(ProcSource::SpellFullResist);
            }
        }
        if proc_type_mask.takes() {
            push(ProcSource::AttackTaken);
        }
        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_translate_into_sources() {
        // Unbridled Wrath: melee swings, landed hits.
        assert_eq!(
            ProcSource::from_masks(ProcFlags::DEAL_MELEE_SWING, ProcHitMask::LANDED),
            [ProcSource::MainhandSwing, ProcSource::OffhandSwing]
        );
        // Dual Wield Specialization: off-hand swings only.
        assert_eq!(
            ProcSource::from_masks(ProcFlags::from_bits(0x800004), ProcHitMask::LANDED),
            [ProcSource::OffhandSwing]
        );
        assert_eq!(
            ProcSource::from_masks(ProcFlags::from_bits(0x400004), ProcHitMask::LANDED),
            [ProcSource::MainhandSwing]
        );
        // Deep Wounds / Flurry: any damage done, crits only.
        assert_eq!(
            ProcSource::from_masks(ProcFlags::DEAL_ANY_DAMAGE, ProcHitMask::CRITICAL),
            [ProcSource::MeleeCritical, ProcSource::SpellCritical]
        );
        // Weaponmaster: swings and abilities, landed.
        assert_eq!(
            ProcSource::from_masks(ProcFlags::from_bits(0x14), ProcHitMask::LANDED),
            [
                ProcSource::MainhandSwing,
                ProcSource::OffhandSwing,
                ProcSource::MainhandSpell,
                ProcSource::OffhandSpell
            ]
        );
        // Overpower-style reaction: the target dodged.
        assert_eq!(
            ProcSource::from_masks(ProcFlags::DEAL_MELEE_SWING, ProcHitMask::DODGE),
            [ProcSource::MeleeDodge]
        );
        // Enrage / Shield Specialization: hits taken.
        assert_eq!(
            ProcSource::from_masks(ProcFlags::from_bits(0x222a8), ProcHitMask::LANDED),
            [ProcSource::AttackTaken]
        );
        assert!(ProcSource::from_masks(ProcFlags::empty(), ProcHitMask::LANDED).is_empty());
        assert_eq!(ProcSource::OffhandSwing.hand(), Hand::Offhand);
        assert_eq!(ProcSource::OffhandSpell.hand(), Hand::Offhand);
        assert_eq!(ProcSource::MeleeCritical.hand(), Hand::Mainhand);
        assert_eq!(ProcSource::ALL.len(), 18);
    }
}

pub mod runtime;

pub use runtime::{EnabledProcs, PROC_ROLL_RANGE, Proc, ProcHost, ProcKind, ProcRate};
