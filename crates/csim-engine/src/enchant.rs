//! Enchants. Port of `Class/Common/Enchants/*` and `Character/CharacterEnchants.*`.
//!
//! The C++ code had one hardcoded class per enchant (`EnchantStatic` with a giant switch,
//! `EnchantProc` for Crusader / Fiery Weapon / Windfury / Shadow Oil) plus per-class availability
//! lists. Here every enchant is a data record (`data/enchants.yaml`): the slots and weapons it
//! applies to, its static stats, the flat damage it adds to the enchanted weapon and its procs
//! (using the same generic proc schema as items). Per-class availability lists live in the class
//! data (Phase 4).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::faction::{Faction, PlayerClass};
use crate::item::{EquipmentSlot, ItemProcSpec, ItemStat, WeaponData, WeaponSlot, WeaponType};
use crate::stats::{Stats, UnsupportedItemStat};

/// Identifier of a permanent or temporary enchant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EnchantName {
    ArcanumOfFocus,
    ArcanumOfRapidity,
    Biznicks247x128Accurascope,
    BrilliantManaOil,
    BrilliantWizardOil,
    ConsecratedSharpeningStone,
    Crusader,
    DeathsEmbrace,
    DenseSharpeningStone,
    DenseWeightstone,
    ElementalSharpeningStone,
    Enchant2HWeaponAgility,
    Enchant2HWeaponSuperiorImpact,
    EnchantBootsAgility,
    EnchantBootsGreaterAgility,
    EnchantBootsMinorSpeed,
    EnchantBootsSpirit,
    EnchantBracerGreaterIntellect,
    EnchantBracerGreaterStrength,
    EnchantBracerManaRegeneration,
    EnchantBracerMinorAgility,
    EnchantBracerSuperiorStrength,
    EnchantChestGreaterStats,
    EnchantChestMajorMana,
    EnchantChestStats,
    EnchantCloakLesserAgility,
    EnchantGlovesFirePower,
    EnchantGlovesFrostPower,
    EnchantGlovesShadowPower,
    EnchantGlovesGreaterAgility,
    EnchantGlovesGreaterStrength,
    EnchantGlovesMinorHaste,
    EnchantGlovesSuperiorAgility,
    EnchantWeaponAgility,
    EnchantWeaponSpellPower,
    EnchantWeaponStrength,
    FalconsCall,
    FieryWeapon,
    InstantPoison,
    IronCounterweight,
    LesserArcanumOfVoracityAgility,
    LesserArcanumOfVoracityIntellect,
    LesserArcanumOfVoracitySpirit,
    LesserArcanumOfVoracityStamina,
    LesserArcanumOfVoracityStrength,
    LesserManaOil,
    MightOfTheScourge,
    PowerOfTheScourge,
    PresenceOfSight,
    HoodooHex,
    ShadowOil,
    SniperScope,
    SolidWeightstone,
    SuperiorStriking,
    WindfuryTotem,
    ZandalarSignetOfMight,
    ZandalarSignetOfMojo,
}

impl EnchantName {
    /// Every enchant, in declaration order.
    pub const ALL: [EnchantName; 57] = [
        EnchantName::ArcanumOfFocus,
        EnchantName::ArcanumOfRapidity,
        EnchantName::Biznicks247x128Accurascope,
        EnchantName::BrilliantManaOil,
        EnchantName::BrilliantWizardOil,
        EnchantName::ConsecratedSharpeningStone,
        EnchantName::Crusader,
        EnchantName::DeathsEmbrace,
        EnchantName::DenseSharpeningStone,
        EnchantName::DenseWeightstone,
        EnchantName::ElementalSharpeningStone,
        EnchantName::Enchant2HWeaponAgility,
        EnchantName::Enchant2HWeaponSuperiorImpact,
        EnchantName::EnchantBootsAgility,
        EnchantName::EnchantBootsGreaterAgility,
        EnchantName::EnchantBootsMinorSpeed,
        EnchantName::EnchantBootsSpirit,
        EnchantName::EnchantBracerGreaterIntellect,
        EnchantName::EnchantBracerGreaterStrength,
        EnchantName::EnchantBracerManaRegeneration,
        EnchantName::EnchantBracerMinorAgility,
        EnchantName::EnchantBracerSuperiorStrength,
        EnchantName::EnchantChestGreaterStats,
        EnchantName::EnchantChestMajorMana,
        EnchantName::EnchantChestStats,
        EnchantName::EnchantCloakLesserAgility,
        EnchantName::EnchantGlovesFirePower,
        EnchantName::EnchantGlovesFrostPower,
        EnchantName::EnchantGlovesShadowPower,
        EnchantName::EnchantGlovesGreaterAgility,
        EnchantName::EnchantGlovesGreaterStrength,
        EnchantName::EnchantGlovesMinorHaste,
        EnchantName::EnchantGlovesSuperiorAgility,
        EnchantName::EnchantWeaponAgility,
        EnchantName::EnchantWeaponSpellPower,
        EnchantName::EnchantWeaponStrength,
        EnchantName::FalconsCall,
        EnchantName::FieryWeapon,
        EnchantName::InstantPoison,
        EnchantName::IronCounterweight,
        EnchantName::LesserArcanumOfVoracityAgility,
        EnchantName::LesserArcanumOfVoracityIntellect,
        EnchantName::LesserArcanumOfVoracitySpirit,
        EnchantName::LesserArcanumOfVoracityStamina,
        EnchantName::LesserArcanumOfVoracityStrength,
        EnchantName::LesserManaOil,
        EnchantName::MightOfTheScourge,
        EnchantName::PowerOfTheScourge,
        EnchantName::PresenceOfSight,
        EnchantName::HoodooHex,
        EnchantName::ShadowOil,
        EnchantName::SniperScope,
        EnchantName::SolidWeightstone,
        EnchantName::SuperiorStriking,
        EnchantName::WindfuryTotem,
        EnchantName::ZandalarSignetOfMight,
        EnchantName::ZandalarSignetOfMojo,
    ];
}

/// One enchant as stored in `data/enchants.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnchantSpec {
    pub name: EnchantName,
    pub display_name: String,
    /// Name used in saved characters.
    pub unique_name: String,
    #[serde(default)]
    pub effect: String,
    /// Temporary enchants (stones, oils, poisons, Windfury) share a separate slot on weapons.
    #[serde(default)]
    pub temporary: bool,
    /// Equipment slots the enchant can be applied to.
    pub slots: Vec<EquipmentSlot>,
    /// For weapon slots: the weapon slot types the enchant fits. Empty means any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weapon_slots: Vec<WeaponSlot>,
    /// For weapon slots: the weapon types the enchant fits. Empty means any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weapon_types: Vec<WeaponType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<Faction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<PlayerClass>,
    /// Stats granted while the enchant is active (data-file value semantics).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stats: BTreeMap<ItemStat, f64>,
    /// Flat damage added to the enchanted weapon.
    #[serde(default)]
    pub weapon_damage: u32,
    /// Maximum mana granted (Enchant Chest - Major Mana).
    #[serde(default)]
    pub mana: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub procs: Vec<ItemProcSpec>,
}

/// Where an enchant is about to be applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnchantContext<'a> {
    pub slot: EquipmentSlot,
    /// The weapon in the slot, for weapon slots.
    pub weapon: Option<&'a WeaponData>,
    pub faction: Faction,
    pub class: PlayerClass,
}

impl EnchantSpec {
    /// Whether the enchant may be applied in the given context.
    pub fn valid_for(&self, ctx: &EnchantContext) -> bool {
        if !self.slots.contains(&ctx.slot) {
            return false;
        }
        if self.faction.is_some_and(|faction| faction != ctx.faction) {
            return false;
        }
        if self.class.is_some_and(|class| class != ctx.class) {
            return false;
        }

        if ctx.slot.is_weapon_slot() {
            let Some(weapon) = ctx.weapon else {
                return false;
            };
            if !self.weapon_slots.is_empty() && !self.weapon_slots.contains(&weapon.weapon_slot) {
                return false;
            }
            if !self.weapon_types.is_empty() && !self.weapon_types.contains(&weapon.weapon_type) {
                return false;
            }
        }

        true
    }

    /// The static stats of the enchant as a stat bag, with the weapon damage assigned to the
    /// weapon side of `slot`.
    pub fn static_stats(&self, slot: EquipmentSlot) -> Result<Stats, UnsupportedItemStat> {
        let mut stats = Stats::new();
        for (&stat, &value) in &self.stats {
            if !is_dynamic(stat) {
                stats.apply_item_stat(stat, value)?;
            }
        }
        match slot {
            EquipmentSlot::Mainhand => stats.increase_mh_weapon_damage(self.weapon_damage),
            EquipmentSlot::Offhand => stats.increase_oh_weapon_damage(self.weapon_damage),
            EquipmentSlot::Ranged => stats.increase_ranged_weapon_damage(self.weapon_damage),
            _ => {}
        }
        Ok(stats)
    }

    /// Stats that cannot live in a stat bag (attack/casting speed, mana skill reduction); the
    /// character applies these through `CharacterStats::increase_stat`.
    pub fn dynamic_stats(&self) -> impl Iterator<Item = (ItemStat, f64)> + '_ {
        self.stats
            .iter()
            .filter(|(&stat, _)| is_dynamic(stat))
            .map(|(&stat, &value)| (stat, value))
    }
}

fn is_dynamic(stat: ItemStat) -> bool {
    matches!(
        stat,
        ItemStat::AttackSpeed
            | ItemStat::MeleeAttackSpeed
            | ItemStat::CastingSpeed
            | ItemStat::ManaSkillReduction
    )
}

/// Errors while loading the enchant data.
#[derive(Debug, thiserror::Error)]
pub enum EnchantDbError {
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
    #[error("enchant {0:?} is defined twice")]
    Duplicate(EnchantName),
    #[error("enchant {name:?}: {source}")]
    UnsupportedStat {
        name: EnchantName,
        #[source]
        source: UnsupportedItemStat,
    },
}

/// All enchant definitions.
#[derive(Debug, Clone, Default)]
pub struct EnchantDb {
    specs: Vec<EnchantSpec>,
    by_name: HashMap<EnchantName, usize>,
}

impl EnchantDb {
    pub fn new(specs: Vec<EnchantSpec>) -> Result<Self, EnchantDbError> {
        let mut by_name = HashMap::new();
        for (index, spec) in specs.iter().enumerate() {
            if by_name.insert(spec.name, index).is_some() {
                return Err(EnchantDbError::Duplicate(spec.name));
            }
            spec.static_stats(EquipmentSlot::Mainhand)
                .map_err(|source| EnchantDbError::UnsupportedStat {
                    name: spec.name,
                    source,
                })?;
        }
        Ok(Self { specs, by_name })
    }

    /// Loads a YAML file holding a list of enchant specs.
    pub fn load(path: &Path) -> Result<Self, EnchantDbError> {
        let text = fs::read_to_string(path).map_err(|source| EnchantDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let specs: Vec<EnchantSpec> =
            serde_yaml::from_str(&text).map_err(|source| EnchantDbError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        Self::new(specs)
    }

    pub fn get(&self, name: EnchantName) -> Option<&EnchantSpec> {
        self.by_name.get(&name).map(|&index| &self.specs[index])
    }

    pub fn get_by_unique_name(&self, unique_name: &str) -> Option<&EnchantSpec> {
        self.specs
            .iter()
            .find(|spec| spec.unique_name == unique_name)
    }

    pub fn specs(&self) -> &[EnchantSpec] {
        &self.specs
    }

    /// The client-table spells the enchant procs name: what
    /// `csim-tables export-spells --enchants` walks into `data/spells/enchants.yaml`.
    pub fn spell_ids(&self) -> BTreeSet<u32> {
        self.specs
            .iter()
            .flat_map(|spec| spec.procs.iter())
            .filter_map(|proc| proc.spell)
            .collect()
    }

    /// Enchants applicable in `ctx`, optionally filtered by temporariness.
    pub fn available(&self, ctx: &EnchantContext, temporary: bool) -> Vec<&EnchantSpec> {
        self.specs
            .iter()
            .filter(|spec| spec.temporary == temporary && spec.valid_for(ctx))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magic_school::MagicSchool;

    const ENCHANTS: &str = r#"
- name: Crusader
  display_name: Crusader
  unique_name: Enchant Weapon - Crusader
  effect: Crusader
  slots: [MAINHAND, OFFHAND]
  weapon_slots: [1H, MH, OH, 2H]
  procs:
    - name: GENERIC_STAT_BUFF
      display_name: Holy Strength
      stat: STRENGTH
      amount: 100
      duration: 15
      rate: 1.0
      ppm: true
- name: IronCounterweight
  display_name: Iron Counterweight
  unique_name: Iron Counterweight
  effect: "+3% Attack Speed"
  slots: [MAINHAND]
  weapon_slots: [2H]
  stats: { MELEE_ATTACK_SPEED: 3 }
- name: SuperiorStriking
  display_name: Superior Striking
  unique_name: Enchant Weapon - Superior Striking
  effect: "+5 Damage"
  slots: [MAINHAND, OFFHAND]
  weapon_slots: [1H, MH, OH, 2H]
  weapon_damage: 5
- name: DenseSharpeningStone
  display_name: Dense Sharpening Stone
  unique_name: Dense Sharpening Stone
  effect: "+8 Damage"
  temporary: true
  slots: [MAINHAND, OFFHAND]
  weapon_types: [AXE, TWOHAND_AXE, DAGGER, POLEARM, SWORD, TWOHAND_SWORD]
  weapon_damage: 8
- name: WindfuryTotem
  display_name: Windfury Totem
  unique_name: Windfury Totem
  effect: Windfury
  temporary: true
  slots: [MAINHAND]
  faction: HORDE
  procs:
    - name: WINDFURY_ATTACK
      rate: 0.2
      value: 315
- name: InstantPoison
  display_name: Instant Poison
  unique_name: Instant Poison
  temporary: true
  slots: [MAINHAND, OFFHAND]
  class: ROGUE
- name: MightOfTheScourge
  display_name: Might of the Scourge
  unique_name: Might of the Scourge
  slots: [SHOULDERS]
  stats: { ATTACK_POWER: 26, CRIT_CHANCE: 0.01 }
- name: EnchantGlovesFirePower
  display_name: Fire Power
  unique_name: Enchant Gloves - Fire Power
  slots: [GLOVES]
  stats: { SPELL_DAMAGE_FIRE: 20 }
"#;

    fn db() -> EnchantDb {
        EnchantDb::new(serde_yaml::from_str(ENCHANTS).unwrap()).unwrap()
    }

    fn weapon(weapon_type: WeaponType, weapon_slot: WeaponSlot) -> WeaponData {
        WeaponData {
            weapon_type,
            weapon_slot,
            min_dmg: 1,
            max_dmg: 2,
            speed: 2.0,
        }
    }

    fn ctx<'a>(
        slot: EquipmentSlot,
        weapon: Option<&'a WeaponData>,
        faction: Faction,
        class: PlayerClass,
    ) -> EnchantContext<'a> {
        EnchantContext {
            slot,
            weapon,
            faction,
            class,
        }
    }

    #[test]
    fn parses_and_indexes() {
        let db = db();
        assert_eq!(db.len(), 8);
        assert_eq!(
            db.get(EnchantName::Crusader).unwrap().display_name,
            "Crusader"
        );
        assert_eq!(
            db.get_by_unique_name("Enchant Weapon - Crusader")
                .unwrap()
                .name,
            EnchantName::Crusader
        );
        assert!(db.get(EnchantName::FieryWeapon).is_none());
        assert!(db.get(EnchantName::Crusader).unwrap().procs[0].ppm);
    }

    #[test]
    fn weapon_slot_validity() {
        let db = db();
        let sword = weapon(WeaponType::Sword, WeaponSlot::OneHand);
        let axe_2h = weapon(WeaponType::TwohandAxe, WeaponSlot::TwoHand);
        let mace = weapon(WeaponType::Mace, WeaponSlot::Mainhand);
        let warrior = |slot, weapon| ctx(slot, weapon, Faction::Alliance, PlayerClass::Warrior);

        let crusader = db.get(EnchantName::Crusader).unwrap();
        assert!(crusader.valid_for(&warrior(EquipmentSlot::Mainhand, Some(&sword))));
        assert!(crusader.valid_for(&warrior(EquipmentSlot::Offhand, Some(&sword))));
        assert!(crusader.valid_for(&warrior(EquipmentSlot::Mainhand, Some(&axe_2h))));
        assert!(!crusader.valid_for(&warrior(EquipmentSlot::Ranged, Some(&sword))));
        assert!(!crusader.valid_for(&warrior(EquipmentSlot::Mainhand, None)));
        assert!(!crusader.valid_for(&warrior(EquipmentSlot::Head, None)));

        let counterweight = db.get(EnchantName::IronCounterweight).unwrap();
        assert!(counterweight.valid_for(&warrior(EquipmentSlot::Mainhand, Some(&axe_2h))));
        assert!(!counterweight.valid_for(&warrior(EquipmentSlot::Mainhand, Some(&sword))));

        let stone = db.get(EnchantName::DenseSharpeningStone).unwrap();
        assert!(stone.valid_for(&warrior(EquipmentSlot::Mainhand, Some(&sword))));
        assert!(stone.valid_for(&warrior(EquipmentSlot::Offhand, Some(&axe_2h))));
        assert!(!stone.valid_for(&warrior(EquipmentSlot::Mainhand, Some(&mace))));
    }

    #[test]
    fn faction_and_class_validity() {
        let db = db();
        let sword = weapon(WeaponType::Sword, WeaponSlot::OneHand);

        let windfury = db.get(EnchantName::WindfuryTotem).unwrap();
        assert!(windfury.valid_for(&ctx(
            EquipmentSlot::Mainhand,
            Some(&sword),
            Faction::Horde,
            PlayerClass::Warrior
        )));
        assert!(!windfury.valid_for(&ctx(
            EquipmentSlot::Mainhand,
            Some(&sword),
            Faction::Alliance,
            PlayerClass::Warrior
        )));
        assert!(!windfury.valid_for(&ctx(
            EquipmentSlot::Offhand,
            Some(&sword),
            Faction::Horde,
            PlayerClass::Warrior
        )));

        let poison = db.get(EnchantName::InstantPoison).unwrap();
        assert!(poison.valid_for(&ctx(
            EquipmentSlot::Offhand,
            Some(&sword),
            Faction::Horde,
            PlayerClass::Rogue
        )));
        assert!(!poison.valid_for(&ctx(
            EquipmentSlot::Offhand,
            Some(&sword),
            Faction::Horde,
            PlayerClass::Warrior
        )));

        let shoulders = db.get(EnchantName::MightOfTheScourge).unwrap();
        assert!(shoulders.valid_for(&ctx(
            EquipmentSlot::Shoulders,
            None,
            Faction::Horde,
            PlayerClass::Mage
        )));
        assert!(!shoulders.valid_for(&ctx(
            EquipmentSlot::Chest,
            None,
            Faction::Horde,
            PlayerClass::Mage
        )));
    }

    #[test]
    fn static_and_dynamic_stats() {
        let db = db();

        let shoulders = db.get(EnchantName::MightOfTheScourge).unwrap();
        let stats = shoulders.static_stats(EquipmentSlot::Shoulders).unwrap();
        assert_eq!(stats.get_base_melee_ap(), 26);
        assert_eq!(stats.get_base_ranged_ap(), 26);
        assert_eq!(stats.get_melee_crit_chance(), 100);
        assert_eq!(shoulders.dynamic_stats().count(), 0);

        let gloves = db.get(EnchantName::EnchantGlovesFirePower).unwrap();
        assert_eq!(
            gloves
                .static_stats(EquipmentSlot::Gloves)
                .unwrap()
                .get_spell_damage(MagicSchool::Fire),
            20
        );

        let striking = db.get(EnchantName::SuperiorStriking).unwrap();
        let mh = striking.static_stats(EquipmentSlot::Mainhand).unwrap();
        assert_eq!(mh.get_mh_weapon_damage(), 5);
        assert_eq!(mh.get_oh_weapon_damage(), 0);
        let oh = striking.static_stats(EquipmentSlot::Offhand).unwrap();
        assert_eq!(oh.get_oh_weapon_damage(), 5);
        assert_eq!(oh.get_mh_weapon_damage(), 0);

        let counterweight = db.get(EnchantName::IronCounterweight).unwrap();
        assert_eq!(
            counterweight.static_stats(EquipmentSlot::Mainhand).unwrap(),
            Stats::new()
        );
        assert_eq!(
            counterweight.dynamic_stats().collect::<Vec<_>>(),
            vec![(ItemStat::MeleeAttackSpeed, 3.0)]
        );
    }

    #[test]
    fn available_lists_applicable_enchants() {
        let db = db();
        let sword = weapon(WeaponType::Sword, WeaponSlot::OneHand);
        let horde = ctx(
            EquipmentSlot::Mainhand,
            Some(&sword),
            Faction::Horde,
            PlayerClass::Warrior,
        );
        let permanent: Vec<EnchantName> =
            db.available(&horde, false).iter().map(|s| s.name).collect();
        assert_eq!(
            permanent,
            vec![EnchantName::Crusader, EnchantName::SuperiorStriking]
        );
        let temporary: Vec<EnchantName> =
            db.available(&horde, true).iter().map(|s| s.name).collect();
        assert_eq!(
            temporary,
            vec![
                EnchantName::DenseSharpeningStone,
                EnchantName::WindfuryTotem
            ]
        );
    }

    #[test]
    fn shipped_data_file_defines_every_enchant() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/enchants.yaml");
        let db = EnchantDb::load(&path).unwrap();
        for name in EnchantName::ALL {
            assert!(
                db.get(name).is_some(),
                "{name:?} missing from data/enchants.yaml"
            );
        }
        assert_eq!(db.len(), EnchantName::ALL.len());
        assert_eq!(db.get(EnchantName::Crusader).unwrap().procs.len(), 1);
        assert_eq!(
            db.get(EnchantName::EnchantChestGreaterStats)
                .unwrap()
                .static_stats(EquipmentSlot::Chest)
                .unwrap()
                .get_strength(),
            4
        );
    }

    #[test]
    fn duplicates_are_rejected() {
        let mut specs: Vec<EnchantSpec> = serde_yaml::from_str(ENCHANTS).unwrap();
        specs.push(specs[0].clone());
        assert!(matches!(
            EnchantDb::new(specs).unwrap_err(),
            EnchantDbError::Duplicate(EnchantName::Crusader)
        ));
    }
}
