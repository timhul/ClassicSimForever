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
}
