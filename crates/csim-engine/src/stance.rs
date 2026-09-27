//! Stances and shapeshift forms. Port of `Stance` in `Spells/SpellInfo.h`.

use serde::{Deserialize, Serialize};

use crate::spell::dbc::ShapeshiftForm;

/// The stance (Warrior) or form (Druid) a character is in. Serialized with the names used by the
/// spell data (`BATTLE_STANCE`, `BEAR_FORM`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stance {
    #[serde(rename = "CASTER_FORM")]
    Caster,
    #[serde(rename = "BATTLE_STANCE")]
    Battle,
    #[serde(rename = "DEFENSIVE_STANCE")]
    Defensive,
    #[serde(rename = "BERSERKER_STANCE")]
    Berserker,
    #[serde(rename = "BEAR_FORM")]
    Bear,
    #[serde(rename = "CAT_FORM")]
    Cat,
    #[serde(rename = "MOONKIN_FORM")]
    Moonkin,
    #[serde(rename = "STEALTH")]
    Stealth,
}

impl Stance {
    pub const ALL: [Stance; 8] = [
        Stance::Caster,
        Stance::Battle,
        Stance::Defensive,
        Stance::Berserker,
        Stance::Bear,
        Stance::Cat,
        Stance::Moonkin,
        Stance::Stealth,
    ];

    /// The stance a `MOD_SHAPESHIFT` aura's form id selects; `None` for forms no supported
    /// class uses.
    pub fn from_form(form: ShapeshiftForm) -> Option<Stance> {
        match form {
            ShapeshiftForm::None => Some(Stance::Caster),
            ShapeshiftForm::BattleStance => Some(Stance::Battle),
            ShapeshiftForm::DefensiveStance => Some(Stance::Defensive),
            ShapeshiftForm::BerserkerStance => Some(Stance::Berserker),
            ShapeshiftForm::BearForm | ShapeshiftForm::DireBearForm => Some(Stance::Bear),
            ShapeshiftForm::CatForm => Some(Stance::Cat),
            ShapeshiftForm::MoonkinForm => Some(Stance::Moonkin),
            ShapeshiftForm::Stealth => Some(Stance::Stealth),
            _ => None,
        }
    }

    /// The form id of this stance (`SpellShapeshiftForm.ID`).
    pub fn form(self) -> ShapeshiftForm {
        match self {
            Stance::Caster => ShapeshiftForm::None,
            Stance::Battle => ShapeshiftForm::BattleStance,
            Stance::Defensive => ShapeshiftForm::DefensiveStance,
            Stance::Berserker => ShapeshiftForm::BerserkerStance,
            Stance::Bear => ShapeshiftForm::BearForm,
            Stance::Cat => ShapeshiftForm::CatForm,
            Stance::Moonkin => ShapeshiftForm::MoonkinForm,
            Stance::Stealth => ShapeshiftForm::Stealth,
        }
    }

    /// Whether a spell with `SpellShapeshift.ShapeshiftMask_0 == mask` is usable in this stance
    /// (a mask of 0 allows every stance).
    pub fn allowed_by_mask(self, mask: u32) -> bool {
        mask == 0 || self.form().mask_bit().is_some_and(|bit| mask & bit != 0)
    }

    pub fn name(self) -> &'static str {
        match self {
            Stance::Caster => "Caster Form",
            Stance::Battle => "Battle Stance",
            Stance::Defensive => "Defensive Stance",
            Stance::Berserker => "Berserker Stance",
            Stance::Bear => "Bear Form",
            Stance::Cat => "Cat Form",
            Stance::Moonkin => "Moonkin Form",
            Stance::Stealth => "Stealth",
        }
    }

    /// Whether entering the stance starts the stance cooldown (and its global cooldown lag).
    /// Stealth is a form without one: the Rogue goes in and out of it without delay.
    pub fn has_swap_cooldown(self) -> bool {
        self != Stance::Stealth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_data_file_names() {
        assert_eq!(
            serde_yaml::from_str::<Stance>("BERSERKER_STANCE").unwrap(),
            Stance::Berserker
        );
        assert_eq!(
            serde_yaml::to_string(&Stance::Bear).unwrap().trim(),
            "BEAR_FORM"
        );
    }

    #[test]
    fn forms_and_masks() {
        assert_eq!(
            Stance::from_form(ShapeshiftForm::BerserkerStance),
            Some(Stance::Berserker)
        );
        assert_eq!(Stance::from_form(ShapeshiftForm::GhostWolf), None);
        for stance in Stance::ALL {
            assert_eq!(Stance::from_form(stance.form()), Some(stance));
        }
        assert!(Stance::Battle.allowed_by_mask(0));
        assert!(
            Stance::Battle.allowed_by_mask(327680),
            "Execute: Battle | Berserker"
        );
        assert!(Stance::Berserker.allowed_by_mask(327680));
        assert!(!Stance::Defensive.allowed_by_mask(327680));
        assert!(!Stance::Caster.allowed_by_mask(65536), "form 0 has no bit");
        assert!(
            Stance::Stealth.allowed_by_mask(536870912),
            "Ambush: Stealth (form 30)"
        );
        assert!(!Stance::Caster.allowed_by_mask(536870912));
    }
}
