//! Items and weapons. Port of `Equipment/Item/Item.*` and `Equipment/Item/Weapon.*`.
//!
//! An [`Item`] is immutable data (its spec plus the aggregated [`Stats`]) shared through `Arc`
//! between the database and the characters wearing it. Everything that varies per equipped
//! instance (enchants, the weapon damage roll generator, the procs and uses that get instantiated)
//! lives in the equipment, not in the item.

pub mod db;
pub mod rating;
pub mod set_bonus;
pub mod spec;
pub mod types;

use std::sync::Arc;

pub use db::{EquipmentDb, EquipmentDbError};
pub use set_bonus::{SetBonusDb, SetBonusError, SetBonusSpec, SetSpec};
pub use spec::{ItemProcSpec, ItemSpec, ItemUseSpec, ProcSourceFlags, WeaponDamageSpec};
pub use types::{
    ArmorType, EquipmentSlot, ItemSlot, ItemStat, ItemType, Quality, WeaponSlot, WeaponType,
};

use crate::faction::{Faction, PlayerClass};
use crate::phase::Phase;
use crate::rng::Random;
use crate::stats::{Stats, UnsupportedItemStat, WeaponProfile};

/// Why an item spec could not be turned into an item.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ItemError {
    #[error("item {id} ({name}): {source}")]
    UnsupportedStat {
        id: u32,
        name: String,
        #[source]
        source: UnsupportedItemStat,
    },
    #[error("item {id} ({name}): weapon slot {slot:?} requires a damage range")]
    MissingDamage {
        id: u32,
        name: String,
        slot: ItemSlot,
    },
    #[error("item {id} ({name}): weapon slot {slot:?} requires a weapon type, got {item_type:?}")]
    NotAWeaponType {
        id: u32,
        name: String,
        slot: ItemSlot,
        item_type: ItemType,
    },
    #[error("item {id} ({name}): damage range min {min} > max {max}")]
    InvalidDamageRange {
        id: u32,
        name: String,
        min: u32,
        max: u32,
    },
}

/// Weapon properties of an item in a weapon slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponData {
    pub weapon_type: WeaponType,
    pub weapon_slot: WeaponSlot,
    pub min_dmg: u32,
    pub max_dmg: u32,
    pub speed: f64,
}

impl WeaponData {
    pub fn dps(&self) -> f64 {
        f64::from(self.min_dmg + self.max_dmg) / 2.0 / self.speed
    }

    pub fn is_two_hand(&self) -> bool {
        self.weapon_slot == WeaponSlot::TwoHand
    }

    pub fn profile(&self) -> WeaponProfile {
        WeaponProfile {
            weapon_type: self.weapon_type,
            speed: self.speed,
        }
    }
}

/// An item from the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    spec: ItemSpec,
    stats: Stats,
    weapon: Option<WeaponData>,
}

impl Item {
    /// Validates a spec and precomputes its stats.
    pub fn from_spec(spec: ItemSpec) -> Result<Self, ItemError> {
        let stats = Stats::from_item_stats(spec.stats.iter().map(|(&stat, &value)| (stat, value)))
            .map_err(|source| ItemError::UnsupportedStat {
                id: spec.id,
                name: spec.name.clone(),
                source,
            })?;

        let weapon = match spec.slot.weapon_slot() {
            Some(weapon_slot) => {
                let damage = spec.damage.ok_or_else(|| ItemError::MissingDamage {
                    id: spec.id,
                    name: spec.name.clone(),
                    slot: spec.slot,
                })?;
                let weapon_type =
                    spec.item_type
                        .weapon_type()
                        .ok_or_else(|| ItemError::NotAWeaponType {
                            id: spec.id,
                            name: spec.name.clone(),
                            slot: spec.slot,
                            item_type: spec.item_type,
                        })?;
                if damage.min > damage.max {
                    return Err(ItemError::InvalidDamageRange {
                        id: spec.id,
                        name: spec.name.clone(),
                        min: damage.min,
                        max: damage.max,
                    });
                }
                Some(WeaponData {
                    weapon_type,
                    weapon_slot,
                    min_dmg: damage.min,
                    max_dmg: damage.max,
                    speed: damage.speed,
                })
            }
            None => None,
        };

        Ok(Self {
            spec,
            stats,
            weapon,
        })
    }

    pub fn spec(&self) -> &ItemSpec {
        &self.spec
    }

    pub fn id(&self) -> u32 {
        self.spec.id
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }

    pub fn phase(&self) -> Phase {
        self.spec.phase
    }

    pub fn slot(&self) -> ItemSlot {
        self.spec.slot
    }

    pub fn item_type(&self) -> ItemType {
        self.spec.item_type
    }

    pub fn quality(&self) -> Quality {
        self.spec.quality
    }

    pub fn icon(&self) -> &str {
        &self.spec.icon
    }

    pub fn is_unique(&self) -> bool {
        self.spec.unique
    }

    /// Static stats of the item.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Weapon data when the item is a weapon (including shields and caster off-hands).
    pub fn weapon(&self) -> Option<&WeaponData> {
        self.weapon.as_ref()
    }

    pub fn is_weapon(&self) -> bool {
        self.weapon.is_some()
    }

    pub fn weapon_type(&self) -> Option<WeaponType> {
        self.weapon.map(|weapon| weapon.weapon_type)
    }

    pub fn weapon_slot(&self) -> Option<WeaponSlot> {
        self.weapon.map(|weapon| weapon.weapon_slot)
    }

    pub fn is_two_hand(&self) -> bool {
        self.weapon.is_some_and(|weapon| weapon.is_two_hand())
    }

    pub fn procs(&self) -> &[ItemProcSpec] {
        &self.spec.procs
    }

    pub fn uses(&self) -> &[ItemUseSpec] {
        &self.spec.uses
    }

    pub fn modifies(&self) -> &[String] {
        &self.spec.modifies
    }

    pub fn mutex_item_ids(&self) -> &[u32] {
        &self.spec.mutex
    }

    pub fn available_for_phase(&self, phase: Phase) -> bool {
        self.spec.phase.available_in(phase)
    }

    pub fn available_for_faction(&self, faction: Faction) -> bool {
        self.spec.faction.is_none_or(|valid| valid == faction)
    }

    pub fn available_for_class(&self, class: PlayerClass) -> bool {
        self.spec.class_restrictions.is_empty() || self.spec.class_restrictions.contains(&class)
    }

    /// Whether the item can go into `slot`.
    pub fn fits(&self, slot: EquipmentSlot) -> bool {
        self.spec.slot.fits(slot)
    }
}

/// An equipped weapon: the item plus the generator rolling its damage.
#[derive(Debug, Clone)]
pub struct Weapon {
    item: Arc<Item>,
    data: WeaponData,
    random: Random,
}

impl Weapon {
    /// Wraps a weapon item. Returns `None` for items that are not weapons.
    pub fn new(item: Arc<Item>) -> Option<Self> {
        let data = *item.weapon()?;
        // The generator rolls `[min, max)`, so the upper bound is included by adding one.
        let random = Random::new(data.min_dmg, data.max_dmg + 1);
        Some(Self { item, data, random })
    }

    pub fn item(&self) -> &Arc<Item> {
        &self.item
    }

    pub fn id(&self) -> u32 {
        self.item.id()
    }

    pub fn name(&self) -> &str {
        self.item.name()
    }

    pub fn data(&self) -> &WeaponData {
        &self.data
    }

    pub fn weapon_type(&self) -> WeaponType {
        self.data.weapon_type
    }

    pub fn weapon_slot(&self) -> WeaponSlot {
        self.data.weapon_slot
    }

    pub fn min_dmg(&self) -> u32 {
        self.data.min_dmg
    }

    pub fn max_dmg(&self) -> u32 {
        self.data.max_dmg
    }

    /// Base weapon speed in seconds.
    pub fn speed(&self) -> f64 {
        self.data.speed
    }

    pub fn dps(&self) -> f64 {
        self.data.dps()
    }

    pub fn is_two_hand(&self) -> bool {
        self.data.is_two_hand()
    }

    pub fn profile(&self) -> WeaponProfile {
        self.data.profile()
    }

    /// Rolls a damage value in `[min, max]`.
    pub fn random_dmg(&mut self) -> u32 {
        let roll = self.random.get_roll();
        debug_assert!(
            roll >= self.data.min_dmg && roll <= self.data.max_dmg,
            "Weapon damage roll outside range"
        );
        roll
    }

    /// Re-seeds the damage roll generator.
    pub fn set_seed(&mut self, seed: u64) {
        self.random.set_gen_from_seed(seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magic_school::MagicSchool;

    fn spec(slot: ItemSlot, item_type: ItemType) -> ItemSpec {
        ItemSpec {
            id: 1,
            name: "Test".into(),
            phase: Phase::MoltenCore,
            slot,
            item_type,
            quality: Quality::Epic,
            unique: false,
            req_lvl: 60,
            item_lvl: 60,
            boe: false,
            icon: String::new(),
            faction: None,
            class_restrictions: Vec::new(),
            damage: None,
            stats: Default::default(),
            procs: Vec::new(),
            uses: Vec::new(),
            modifies: Vec::new(),
            mutex: Vec::new(),
            random_affixes: Vec::new(),
            special_equip_effects: Vec::new(),
            source: String::new(),
            flavour_text: String::new(),
        }
    }

    fn weapon_spec(
        slot: ItemSlot,
        item_type: ItemType,
        min: u32,
        max: u32,
        speed: f64,
    ) -> ItemSpec {
        let mut spec = spec(slot, item_type);
        spec.damage = Some(WeaponDamageSpec { min, max, speed });
        spec
    }

    #[test]
    fn armor_item_has_stats_and_no_weapon_data() {
        let mut spec = spec(ItemSlot::Chest, ItemType::Plate);
        spec.stats.insert(ItemStat::Strength, 30.0);
        spec.stats.insert(ItemStat::FireResistance, 10.0);
        spec.stats.insert(ItemStat::HitChance, 0.01);
        let item = Item::from_spec(spec).unwrap();

        assert!(!item.is_weapon());
        assert_eq!(item.stats().get_strength(), 30);
        assert_eq!(item.stats().get_resistance(MagicSchool::Fire), 10);
        assert_eq!(item.stats().get_melee_hit_chance(), 100);
        assert!(item.fits(EquipmentSlot::Chest));
        assert!(!item.fits(EquipmentSlot::Legs));
        assert!(item.available_for_faction(Faction::Horde));
        assert!(item.available_for_class(PlayerClass::Mage));
        assert!(item.available_for_phase(Phase::MoltenCore));
    }

    #[test]
    fn dynamic_stats_are_rejected_at_load() {
        let mut spec = spec(ItemSlot::Trinket, ItemType::Trinket);
        spec.stats.insert(ItemStat::AttackSpeed, 30.0);
        let error = Item::from_spec(spec).unwrap_err();
        assert!(matches!(error, ItemError::UnsupportedStat { id: 1, .. }));
    }

    #[test]
    fn weapon_item_carries_weapon_data() {
        let item = Item::from_spec(weapon_spec(
            ItemSlot::TwoHand,
            ItemType::TwohandAxe,
            100,
            200,
            3.4,
        ))
        .unwrap();
        let weapon = item.weapon().unwrap();
        assert_eq!(weapon.weapon_type, WeaponType::TwohandAxe);
        assert_eq!(weapon.weapon_slot, WeaponSlot::TwoHand);
        assert!(item.is_two_hand());
        assert!((weapon.dps() - 150.0 / 3.4).abs() < 1e-9);
        assert!(item.fits(EquipmentSlot::Mainhand));
        assert!(!item.fits(EquipmentSlot::Offhand));
    }

    #[test]
    fn shields_and_caster_offhands_are_weapons_with_zero_damage() {
        let shield =
            Item::from_spec(weapon_spec(ItemSlot::Offhand, ItemType::Shield, 0, 0, 0.0)).unwrap();
        assert_eq!(shield.weapon_type(), Some(WeaponType::Shield));
        assert!(shield.fits(EquipmentSlot::Offhand));
        assert!(!shield.fits(EquipmentSlot::Mainhand));
    }

    #[test]
    fn weapon_slot_without_damage_is_an_error() {
        let error = Item::from_spec(spec(ItemSlot::Mainhand, ItemType::Sword)).unwrap_err();
        assert!(matches!(error, ItemError::MissingDamage { .. }));

        let error = Item::from_spec(weapon_spec(ItemSlot::Mainhand, ItemType::Plate, 1, 2, 1.0))
            .unwrap_err();
        assert!(matches!(error, ItemError::NotAWeaponType { .. }));

        let error = Item::from_spec(weapon_spec(ItemSlot::Mainhand, ItemType::Sword, 5, 2, 1.0))
            .unwrap_err();
        assert!(matches!(error, ItemError::InvalidDamageRange { .. }));
    }

    #[test]
    fn availability_restrictions() {
        let mut spec = spec(ItemSlot::Head, ItemType::Mail);
        spec.faction = Some(Faction::Alliance);
        spec.class_restrictions = vec![PlayerClass::Hunter, PlayerClass::Shaman];
        spec.phase = Phase::AhnQiraj;
        let item = Item::from_spec(spec).unwrap();

        assert!(item.available_for_faction(Faction::Alliance));
        assert!(!item.available_for_faction(Faction::Horde));
        assert!(item.available_for_class(PlayerClass::Hunter));
        assert!(!item.available_for_class(PlayerClass::Warrior));
        assert!(!item.available_for_phase(Phase::ZulGurub));
        assert!(item.available_for_phase(Phase::Naxxramas));
    }

    #[test]
    fn weapon_rolls_within_inclusive_range() {
        let item = Arc::new(
            Item::from_spec(weapon_spec(
                ItemSlot::OneHand,
                ItemType::Sword,
                66,
                124,
                2.7,
            ))
            .unwrap(),
        );
        let mut weapon = Weapon::new(item.clone()).unwrap();
        weapon.set_seed(3);

        let mut seen_min = false;
        let mut seen_max = false;
        for _ in 0..10_000 {
            let roll = weapon.random_dmg();
            assert!((66..=124).contains(&roll));
            seen_min |= roll == 66;
            seen_max |= roll == 124;
        }
        assert!(seen_min && seen_max);
        assert_eq!(weapon.speed(), 2.7);
        assert_eq!(weapon.profile().weapon_type, WeaponType::Sword);

        let armor = Arc::new(Item::from_spec(spec(ItemSlot::Head, ItemType::Plate)).unwrap());
        assert!(Weapon::new(armor).is_none());
    }

    #[test]
    fn fixed_damage_weapon_always_rolls_that_value() {
        let item = Arc::new(
            Item::from_spec(weapon_spec(
                ItemSlot::Mainhand,
                ItemType::Mace,
                100,
                100,
                2.0,
            ))
            .unwrap(),
        );
        let mut weapon = Weapon::new(item).unwrap();
        for _ in 0..100 {
            assert_eq!(weapon.random_dmg(), 100);
        }
    }
}
