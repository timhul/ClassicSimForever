//! Attack mode of a character's rotation. Port of `Class/Common/AttackMode.h`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackMode {
    /// Auto attacks with the melee weapons.
    #[serde(alias = "melee")]
    MeleeAttack,
    /// Auto shots with the ranged weapon.
    #[serde(alias = "ranged")]
    RangedAttack,
    /// No auto attacks; the rotation consists of spells.
    #[serde(alias = "magic")]
    MagicAttack,
}

/// Yards between a melee rotation's character and the target.
pub const MELEE_DISTANCE_YD: f64 = 5.0;
/// Yards between a ranged or caster rotation's character and the target.
pub const RANGED_DISTANCE_YD: f64 = 30.0;

impl AttackMode {
    /// The distance to the target the playstyle keeps, which missiles travel.
    pub fn target_distance_yd(self) -> f64 {
        match self {
            AttackMode::MeleeAttack => MELEE_DISTANCE_YD,
            AttackMode::RangedAttack | AttackMode::MagicAttack => RANGED_DISTANCE_YD,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_file_aliases_parse() {
        assert_eq!(
            serde_yaml::from_str::<AttackMode>("melee").unwrap(),
            AttackMode::MeleeAttack
        );
        assert_eq!(
            serde_yaml::from_str::<AttackMode>("ranged").unwrap(),
            AttackMode::RangedAttack
        );
        assert_eq!(
            serde_yaml::from_str::<AttackMode>("magic_attack").unwrap(),
            AttackMode::MagicAttack
        );
    }

    #[test]
    fn ranged_playstyles_keep_their_distance() {
        assert_eq!(AttackMode::MeleeAttack.target_distance_yd(), 5.0);
        assert_eq!(AttackMode::RangedAttack.target_distance_yd(), 30.0);
        assert_eq!(AttackMode::MagicAttack.target_distance_yd(), 30.0);
    }
}
