//! Class definitions. Port of the per-class constants spread over `Class/<Class>/<Class>.cpp`
//! (base stats, stat conversions, proficiencies, armor type, resource, default stance).
//!
//! Everything class-specific is data: a [`ClassSpec`] is loaded from `data/classes/<class>.yaml`
//! (Phase 4.4). The stat conversions are the `ChrClasses` / `PlayerExpectedStat` numbers.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::faction::PlayerClass;
use crate::item::{ArmorType, EquipmentSlot, WeaponType};
use crate::race::{BaseStats, Race};
use crate::resource::ResourceType;
use crate::stance::Stance;
use crate::stats::ClassStatRules;

/// The class contribution to the base stats.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassBaseStats {
    pub strength: u32,
    pub agility: u32,
    pub stamina: u32,
    pub intellect: u32,
    pub spirit: u32,
    #[serde(default)]
    pub melee_ap: u32,
    #[serde(default)]
    pub ranged_ap: u32,
    /// Base melee crit in hundredths of a percent (200 = 2 %).
    #[serde(default)]
    pub melee_crit: u32,
    /// Base mana of mana users.
    #[serde(default)]
    pub mana: u32,
}

/// Stat conversion rules as written in the data file (percent-crit divisors and AP per stat).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatRules {
    /// Agility needed for 1 % physical crit.
    pub agility_per_percent_crit: f64,
    /// Intellect needed for 1 % spell crit; `None` for classes without spell crit.
    #[serde(default)]
    pub intellect_per_percent_spell_crit: Option<f64>,
    #[serde(default)]
    pub melee_ap_per_strength: u32,
    #[serde(default)]
    pub melee_ap_per_agility: u32,
    #[serde(default)]
    pub ranged_ap_per_agility: u32,
}

impl StatRules {
    pub fn rules(&self) -> ClassStatRules {
        ClassStatRules {
            agility_per_percent_crit: self.agility_per_percent_crit,
            intellect_per_percent_spell_crit: self
                .intellect_per_percent_spell_crit
                .unwrap_or(f64::MAX),
            melee_ap_per_strength: self.melee_ap_per_strength,
            melee_ap_per_agility: self.melee_ap_per_agility,
            ranged_ap_per_agility: self.ranged_ap_per_agility,
        }
    }
}

/// Per-race corrections to the class base stats (the C++ `set_special_statistics` cases:
/// Gnome mages +3 intellect, Human priests +4 spirit, Orc warlocks −1 stamina, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatOffsets {
    #[serde(default)]
    pub strength: i32,
    #[serde(default)]
    pub agility: i32,
    #[serde(default)]
    pub stamina: i32,
    #[serde(default)]
    pub intellect: i32,
    #[serde(default)]
    pub spirit: i32,
}

/// One class: `data/classes/<class>.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassSpec {
    pub class: PlayerClass,
    pub resource: ResourceType,
    pub base_stats: ClassBaseStats,
    pub stat_rules: StatRules,
    /// Length of the global cooldown in seconds.
    pub global_cooldown: f64,
    /// The stance the character starts in (the spell data decides what each stance allows).
    #[serde(default = "default_stance")]
    pub default_stance: Stance,
    pub highest_armor_type: ArmorType,
    /// The weapon types the class can wield per weapon slot.
    pub weapon_proficiencies: BTreeMap<EquipmentSlot, Vec<WeaponType>>,
    pub available_races: Vec<Race>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub race_stat_offsets: BTreeMap<Race, StatOffsets>,
}

fn default_stance() -> Stance {
    Stance::Caster
}

#[derive(Debug, thiserror::Error)]
pub enum ClassSpecError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("class {class:?}: race {race:?} in race_stat_offsets is not an available race")]
    UnavailableRace { class: PlayerClass, race: Race },
    #[error("class {class:?}: {slot:?} is not a weapon slot")]
    NotAWeaponSlot {
        class: PlayerClass,
        slot: EquipmentSlot,
    },
}

impl ClassSpec {
    pub fn load(path: &Path) -> Result<Self, ClassSpecError> {
        let text = fs::read_to_string(path).map_err(|source| ClassSpecError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let spec: ClassSpec =
            serde_yaml::from_str(&text).map_err(|source| ClassSpecError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<(), ClassSpecError> {
        if let Some(race) = self
            .race_stat_offsets
            .keys()
            .find(|race| !self.available_races.contains(race))
        {
            return Err(ClassSpecError::UnavailableRace {
                class: self.class,
                race: *race,
            });
        }
        if let Some(slot) = self.weapon_proficiencies.keys().find(|slot| {
            !matches!(
                slot,
                EquipmentSlot::Mainhand | EquipmentSlot::Offhand | EquipmentSlot::Ranged
            )
        }) {
            return Err(ClassSpecError::NotAWeaponSlot {
                class: self.class,
                slot: *slot,
            });
        }
        Ok(())
    }

    pub fn race_available(&self, race: Race) -> bool {
        self.available_races.contains(&race)
    }

    pub fn weapon_proficiencies_for_slot(&self, slot: EquipmentSlot) -> &[WeaponType] {
        self.weapon_proficiencies
            .get(&slot)
            .map_or(&[], Vec::as_slice)
    }

    pub fn can_wield(&self, slot: EquipmentSlot, weapon_type: WeaponType) -> bool {
        self.weapon_proficiencies_for_slot(slot)
            .contains(&weapon_type)
    }

    pub fn stat_offsets(&self, race: Race) -> StatOffsets {
        self.race_stat_offsets
            .get(&race)
            .copied()
            .unwrap_or_default()
    }

    /// The class part of the base attributes as a [`BaseStats`].
    pub fn base_attributes(&self) -> BaseStats {
        BaseStats {
            strength: self.base_stats.strength,
            agility: self.base_stats.agility,
            stamina: self.base_stats.stamina,
            intellect: self.base_stats.intellect,
            spirit: self.base_stats.spirit,
        }
    }
}

#[cfg(test)]
pub(crate) const WARRIOR_YAML: &str = "
class: WARRIOR
resource: rage
base_stats: { strength: 100, agility: 60, stamina: 90, intellect: 10, spirit: 25, melee_ap: 160, melee_crit: 200 }
stat_rules: { agility_per_percent_crit: 20.0, melee_ap_per_strength: 2 }
global_cooldown: 1.5
default_stance: BATTLE_STANCE
highest_armor_type: PLATE
weapon_proficiencies:
  MAINHAND: [AXE, DAGGER, FIST, MACE, SWORD, POLEARM, STAFF, TWOHAND_AXE, TWOHAND_MACE, TWOHAND_SWORD]
  OFFHAND: [AXE, DAGGER, FIST, MACE, SWORD, CASTER_OFFHAND, SHIELD]
  RANGED: [BOW, CROSSBOW, GUN, THROWN]
available_races: [DWARF, GNOME, HUMAN, NIGHT_ELF, ORC, TAUREN, TROLL, UNDEAD]
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_answers_proficiency_questions() {
        let spec: ClassSpec = serde_yaml::from_str(WARRIOR_YAML).unwrap();
        spec.validate().unwrap();
        assert_eq!(spec.class, PlayerClass::Warrior);
        assert_eq!(spec.resource, ResourceType::Rage);
        assert_eq!(spec.default_stance, Stance::Battle);
        assert!(spec.can_wield(EquipmentSlot::Mainhand, WeaponType::TwohandSword));
        assert!(!spec.can_wield(EquipmentSlot::Offhand, WeaponType::TwohandSword));
        assert!(spec.can_wield(EquipmentSlot::Offhand, WeaponType::Shield));
        assert!(!spec.can_wield(EquipmentSlot::Ranged, WeaponType::Wand));
        assert!(spec.race_available(Race::Orc));
        assert_eq!(
            spec.stat_rules.rules().intellect_per_percent_spell_crit,
            f64::MAX
        );
        assert_eq!(spec.stat_rules.rules().melee_ap_per_strength, 2);
        assert_eq!(spec.stat_offsets(Race::Gnome), StatOffsets::default());
    }

    #[test]
    fn validation_rejects_bad_offsets_and_slots() {
        let mut spec: ClassSpec = serde_yaml::from_str(WARRIOR_YAML).unwrap();
        spec.race_stat_offsets.insert(
            Race::Human,
            StatOffsets {
                spirit: 4,
                ..StatOffsets::default()
            },
        );
        spec.validate().unwrap();
        spec.available_races.retain(|r| *r != Race::Human);
        assert!(matches!(
            spec.validate(),
            Err(ClassSpecError::UnavailableRace {
                race: Race::Human,
                ..
            })
        ));

        let mut spec: ClassSpec = serde_yaml::from_str(WARRIOR_YAML).unwrap();
        spec.weapon_proficiencies
            .insert(EquipmentSlot::Head, vec![WeaponType::Axe]);
        assert!(matches!(
            spec.validate(),
            Err(ClassSpecError::NotAWeaponSlot {
                slot: EquipmentSlot::Head,
                ..
            })
        ));
    }
}
