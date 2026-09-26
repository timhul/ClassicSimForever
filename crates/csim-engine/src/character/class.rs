//! Class definitions. Port of the per-class constants spread over `Class/<Class>/<Class>.cpp`
//! (base stats, stat conversions, proficiencies, armor type, resource, default stance).
//!
//! Everything class-specific is data: a [`ClassSpec`] is loaded from `data/classes/<class>.yaml`
//! ([`ClassDb`] holds one per class). The stat conversions are the `ChrClasses` /
//! `PlayerExpectedStat` numbers; the enchant lists per slot are the C++ `<Class>Enchants.cpp`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::enchant::{EnchantContext, EnchantDb, EnchantName, EnchantSpec};
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
    /// Most combo points the character holds; a gain beyond it only restarts their window.
    #[serde(default = "default_max_combo_points")]
    pub max_combo_points: u32,
    /// Seconds combo points last after the last gain; absent, they do not lapse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo_point_duration: Option<f64>,
    /// The weapon types the class can wield per weapon slot.
    pub weapon_proficiencies: BTreeMap<EquipmentSlot, Vec<WeaponType>>,
    pub available_races: Vec<Race>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub race_stat_offsets: BTreeMap<Race, StatOffsets>,
    /// The permanent enchants the class considers per slot (whether one fits the equipped
    /// item is the enchant data's business).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub enchants: BTreeMap<EquipmentSlot, Vec<EnchantName>>,
    /// The temporary enchants (stones, oils, totems) per slot.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub temp_enchants: BTreeMap<EquipmentSlot, Vec<EnchantName>>,
}

fn default_stance() -> Stance {
    Stance::Caster
}

fn default_max_combo_points() -> u32 {
    5
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
    #[error("class {class:?}: enchant {enchant:?} is listed for {slot:?} twice")]
    DuplicateEnchant {
        class: PlayerClass,
        slot: EquipmentSlot,
        enchant: EnchantName,
    },
    #[error("class {class:?}: enchant {enchant:?} listed for {slot:?} is not in the enchant db")]
    UnknownEnchant {
        class: PlayerClass,
        slot: EquipmentSlot,
        enchant: EnchantName,
    },
    #[error("class {class:?}: enchant {enchant:?} does not go on {slot:?}")]
    EnchantSlot {
        class: PlayerClass,
        slot: EquipmentSlot,
        enchant: EnchantName,
    },
    #[error("class {class:?}: enchant {enchant:?} is {} but listed under {list}", if *temporary { "temporary" } else { "permanent" })]
    EnchantTemporariness {
        class: PlayerClass,
        enchant: EnchantName,
        temporary: bool,
        list: &'static str,
    },
    #[error("class file {path} defines {found:?}, expected {expected:?}")]
    ClassMismatch {
        path: PathBuf,
        found: PlayerClass,
        expected: PlayerClass,
    },
    #[error("class {class:?}: {message}")]
    ComboPoints {
        class: PlayerClass,
        message: &'static str,
    },
    #[error("class {0:?} is not defined in the class directory")]
    Missing(PlayerClass),
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
        let combo_points = |message| ClassSpecError::ComboPoints {
            class: self.class,
            message,
        };
        if !(1..=5).contains(&self.max_combo_points) {
            return Err(combo_points("max_combo_points must be 1 to 5"));
        }
        if self
            .combo_point_duration
            .is_some_and(|d| d.is_nan() || d <= 0.0)
        {
            return Err(combo_points("combo_point_duration must be positive"));
        }
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
        for lists in [&self.enchants, &self.temp_enchants] {
            for (slot, names) in lists {
                for (index, name) in names.iter().enumerate() {
                    if names[..index].contains(name) {
                        return Err(ClassSpecError::DuplicateEnchant {
                            class: self.class,
                            slot: *slot,
                            enchant: *name,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Checks the enchant lists against the enchant db: every enchant exists, goes on the slot
    /// it is listed for, and is permanent / temporary as its list says.
    pub fn validate_enchants(&self, enchants: &EnchantDb) -> Result<(), ClassSpecError> {
        for (lists, temporary, list) in [
            (&self.enchants, false, "enchants"),
            (&self.temp_enchants, true, "temp_enchants"),
        ] {
            for (slot, names) in lists {
                for &name in names {
                    let spec = enchants.get(name).ok_or(ClassSpecError::UnknownEnchant {
                        class: self.class,
                        slot: *slot,
                        enchant: name,
                    })?;
                    if !spec.slots.contains(slot) {
                        return Err(ClassSpecError::EnchantSlot {
                            class: self.class,
                            slot: *slot,
                            enchant: name,
                        });
                    }
                    if spec.temporary != temporary {
                        return Err(ClassSpecError::EnchantTemporariness {
                            class: self.class,
                            enchant: name,
                            temporary: spec.temporary,
                            list,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// The enchants the class lists for `slot`. Port of
    /// `CharacterEnchants::get_available_enchants` / `get_available_temp_enchants` without the
    /// item conditions.
    pub fn enchants_for_slot(&self, slot: EquipmentSlot, temporary: bool) -> &[EnchantName] {
        let lists = if temporary {
            &self.temp_enchants
        } else {
            &self.enchants
        };
        lists.get(&slot).map_or(&[], Vec::as_slice)
    }

    /// The class's enchants for the slot that fit the equipped item and faction of `ctx`, in
    /// the class file's order.
    pub fn available_enchants<'a>(
        &self,
        enchants: &'a EnchantDb,
        ctx: &EnchantContext,
        temporary: bool,
    ) -> Vec<&'a EnchantSpec> {
        self.enchants_for_slot(ctx.slot, temporary)
            .iter()
            .filter_map(|&name| enchants.get(name))
            .filter(|spec| spec.temporary == temporary && spec.valid_for(ctx))
            .collect()
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

/// The class definitions of `data/classes/`, one file per class.
#[derive(Debug, Clone, Default)]
pub struct ClassDb {
    specs: BTreeMap<PlayerClass, std::sync::Arc<ClassSpec>>,
}

impl ClassDb {
    /// Loads every `<class>.yaml` of `dir` and validates it (against `enchants` when given).
    pub fn load(dir: &Path, enchants: Option<&EnchantDb>) -> Result<Self, ClassSpecError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|source| ClassSpecError::Io {
                path: dir.to_path_buf(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        paths.sort();
        let mut db = Self::default();
        for path in paths {
            let spec = ClassSpec::load(&path)?;
            if let Some(enchants) = enchants {
                spec.validate_enchants(enchants)?;
            }
            let expected = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| serde_yaml::from_str::<PlayerClass>(&stem.to_uppercase()).ok());
            if expected.is_some_and(|expected| expected != spec.class) {
                return Err(ClassSpecError::ClassMismatch {
                    path,
                    found: spec.class,
                    expected: expected.expect("checked"),
                });
            }
            db.specs.insert(spec.class, std::sync::Arc::new(spec));
        }
        Ok(db)
    }

    pub fn get(&self, class: PlayerClass) -> Result<&std::sync::Arc<ClassSpec>, ClassSpecError> {
        self.specs.get(&class).ok_or(ClassSpecError::Missing(class))
    }

    pub fn classes(&self) -> impl Iterator<Item = PlayerClass> + '_ {
        self.specs.keys().copied()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
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
max_combo_points: 1
combo_point_duration: 6.0
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

    #[test]
    fn combo_point_limits_default_and_validate() {
        let yaml = WARRIOR_YAML
            .replace(
                "max_combo_points: 1
",
                "",
            )
            .replace(
                "combo_point_duration: 6.0
",
                "",
            );
        let spec: ClassSpec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(spec.max_combo_points, 5);
        assert_eq!(spec.combo_point_duration, None);

        for (max, duration) in [(0, None), (6, None), (1, Some(0.0)), (1, Some(-4.0))] {
            let mut spec = spec.clone();
            spec.max_combo_points = max;
            spec.combo_point_duration = duration;
            assert!(
                matches!(spec.validate(), Err(ClassSpecError::ComboPoints { .. })),
                "{max} points, {duration:?} s"
            );
        }
    }

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn shipped_warrior_file_loads_and_matches_the_enchant_db() {
        let enchants = EnchantDb::load(&data_dir().join("enchants.yaml")).unwrap();
        let db = ClassDb::load(&data_dir().join("classes"), Some(&enchants)).unwrap();
        assert_eq!(db.len(), 1);
        let warrior = db.get(PlayerClass::Warrior).unwrap();
        assert!(matches!(
            db.get(PlayerClass::Rogue),
            Err(ClassSpecError::Missing(PlayerClass::Rogue))
        ));
        assert_eq!(warrior.resource, ResourceType::Rage);
        assert_eq!(warrior.default_stance, Stance::Battle);
        assert_eq!(warrior.highest_armor_type, ArmorType::Plate);
        assert_eq!(warrior.base_stats.strength, 100);
        assert_eq!(warrior.base_stats.melee_ap, 160);
        assert_eq!(warrior.base_stats.melee_crit, 200);
        assert_eq!(warrior.stat_rules.rules().melee_ap_per_strength, 2);
        assert_eq!(warrior.stat_rules.rules().ranged_ap_per_agility, 2);
        assert_eq!(warrior.stat_rules.rules().agility_per_percent_crit, 20.0);
        assert_eq!(warrior.available_races.len(), Race::ALL.len());
        assert!(warrior.can_wield(EquipmentSlot::Mainhand, WeaponType::Polearm));
        assert!(!warrior.can_wield(EquipmentSlot::Mainhand, WeaponType::Wand));
        assert_eq!(
            warrior.enchants_for_slot(EquipmentSlot::Shoulders, false),
            [
                EnchantName::MightOfTheScourge,
                EnchantName::ZandalarSignetOfMight
            ]
        );
        assert!(warrior
            .enchants_for_slot(EquipmentSlot::Ranged, false)
            .is_empty());
        assert!(warrior
            .enchants_for_slot(EquipmentSlot::Head, true)
            .is_empty());
    }

    #[test]
    fn available_enchants_follow_the_equipped_weapon() {
        use crate::item::{WeaponData, WeaponSlot};
        let enchants = EnchantDb::load(&data_dir().join("enchants.yaml")).unwrap();
        let db = ClassDb::load(&data_dir().join("classes"), Some(&enchants)).unwrap();
        let warrior = db.get(PlayerClass::Warrior).unwrap();
        let names = |specs: Vec<&EnchantSpec>| specs.iter().map(|s| s.name).collect::<Vec<_>>();

        let sword = WeaponData {
            weapon_type: WeaponType::Sword,
            weapon_slot: WeaponSlot::OneHand,
            min_dmg: 1,
            max_dmg: 2,
            speed: 2.0,
        };
        let ctx = EnchantContext {
            slot: EquipmentSlot::Mainhand,
            weapon: Some(&sword),
            faction: crate::faction::Faction::Alliance,
            class: PlayerClass::Warrior,
        };
        assert_eq!(
            names(warrior.available_enchants(&enchants, &ctx, false)),
            [
                EnchantName::Crusader,
                EnchantName::FieryWeapon,
                EnchantName::EnchantWeaponStrength,
                EnchantName::SuperiorStriking,
                EnchantName::EnchantWeaponAgility,
            ],
            "the two-hand enchants need a two-hander"
        );
        assert_eq!(
            names(warrior.available_enchants(&enchants, &ctx, true)),
            [
                EnchantName::WindfuryTotem,
                EnchantName::DenseSharpeningStone,
                EnchantName::ElementalSharpeningStone,
                EnchantName::ConsecratedSharpeningStone,
                EnchantName::ShadowOil,
            ],
            "sharp weapon: no weightstones; Windfury for the Alliance too in Forever"
        );

        let mace = WeaponData {
            weapon_type: WeaponType::TwohandMace,
            weapon_slot: WeaponSlot::TwoHand,
            ..sword
        };
        let ctx = EnchantContext {
            weapon: Some(&mace),
            faction: crate::faction::Faction::Horde,
            ..ctx
        };
        let permanent = names(warrior.available_enchants(&enchants, &ctx, false));
        assert!(permanent.contains(&EnchantName::Enchant2HWeaponAgility));
        assert!(permanent.contains(&EnchantName::IronCounterweight));
        let temporary = names(warrior.available_enchants(&enchants, &ctx, true));
        assert_eq!(temporary[0], EnchantName::WindfuryTotem);
        assert!(temporary.contains(&EnchantName::SolidWeightstone));
        assert!(!temporary.contains(&EnchantName::DenseSharpeningStone));

        let ctx = EnchantContext {
            slot: EquipmentSlot::Mainhand,
            weapon: None,
            faction: crate::faction::Faction::Horde,
            class: PlayerClass::Warrior,
        };
        assert!(warrior
            .available_enchants(&enchants, &ctx, false)
            .is_empty());
    }

    #[test]
    fn enchant_lists_are_validated_against_the_db() {
        let enchants = EnchantDb::load(&data_dir().join("enchants.yaml")).unwrap();
        let mut spec: ClassSpec = serde_yaml::from_str(WARRIOR_YAML).unwrap();
        spec.enchants
            .insert(EquipmentSlot::Head, vec![EnchantName::Crusader]);
        assert!(matches!(
            spec.validate_enchants(&enchants),
            Err(ClassSpecError::EnchantSlot {
                slot: EquipmentSlot::Head,
                enchant: EnchantName::Crusader,
                ..
            })
        ));
        spec.enchants
            .insert(EquipmentSlot::Head, vec![EnchantName::ArcanumOfRapidity]);
        spec.temp_enchants
            .insert(EquipmentSlot::Mainhand, vec![EnchantName::Crusader]);
        assert!(matches!(
            spec.validate_enchants(&enchants),
            Err(ClassSpecError::EnchantTemporariness {
                enchant: EnchantName::Crusader,
                temporary: false,
                list: "temp_enchants",
                ..
            })
        ));
        spec.temp_enchants.insert(
            EquipmentSlot::Mainhand,
            vec![EnchantName::ShadowOil, EnchantName::ShadowOil],
        );
        assert!(matches!(
            spec.validate(),
            Err(ClassSpecError::DuplicateEnchant {
                enchant: EnchantName::ShadowOil,
                ..
            })
        ));
    }
}
