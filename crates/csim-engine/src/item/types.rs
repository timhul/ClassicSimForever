//! Item-related enums. Port of `Equipment/Item/ItemNamespace.h` and `ItemStatsEnum.*`.
//!
//! The serde names match the strings used in the item database files (`SWORD`, `STRENGTH`, ...).

use serde::{Deserialize, Serialize};

use crate::magic_school::MagicSchool;
use crate::proc::ProcSource;
use crate::target::CreatureType;

/// Weapon (and off-hand / relic) types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WeaponType {
    Axe,
    Dagger,
    Fist,
    Mace,
    Polearm,
    Staff,
    Sword,
    Bow,
    Crossbow,
    Gun,
    Thrown,
    Wand,
    Idol,
    Libram,
    Totem,
    Shield,
    CasterOffhand,
    TwohandAxe,
    TwohandMace,
    TwohandSword,
}

impl WeaponType {
    pub const ALL: [WeaponType; 20] = [
        WeaponType::Axe,
        WeaponType::Dagger,
        WeaponType::Fist,
        WeaponType::Mace,
        WeaponType::Polearm,
        WeaponType::Staff,
        WeaponType::Sword,
        WeaponType::Bow,
        WeaponType::Crossbow,
        WeaponType::Gun,
        WeaponType::Thrown,
        WeaponType::Wand,
        WeaponType::Idol,
        WeaponType::Libram,
        WeaponType::Totem,
        WeaponType::Shield,
        WeaponType::CasterOffhand,
        WeaponType::TwohandAxe,
        WeaponType::TwohandMace,
        WeaponType::TwohandSword,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether the weapon occupies both hands.
    pub fn is_two_hand(self) -> bool {
        matches!(
            self,
            WeaponType::Polearm
                | WeaponType::Staff
                | WeaponType::TwohandAxe
                | WeaponType::TwohandMace
                | WeaponType::TwohandSword
        )
    }

    /// The `(ItemClass, ItemSubClass)` ids of the type: weapons are class 2 with the subclass
    /// ids of `ItemSubClass`, shields / relics / held items are class 4. This is what a
    /// `SpellEquippedItems` requirement is matched against (`subclass_mask` bit `1 << subclass`).
    pub fn item_class_subclass(self) -> (u32, u32) {
        match self {
            WeaponType::Axe => (2, 0),
            WeaponType::TwohandAxe => (2, 1),
            WeaponType::Bow => (2, 2),
            WeaponType::Gun => (2, 3),
            WeaponType::Mace => (2, 4),
            WeaponType::TwohandMace => (2, 5),
            WeaponType::Polearm => (2, 6),
            WeaponType::Sword => (2, 7),
            WeaponType::TwohandSword => (2, 8),
            WeaponType::Staff => (2, 10),
            WeaponType::Fist => (2, 13),
            WeaponType::Dagger => (2, 15),
            WeaponType::Thrown => (2, 16),
            WeaponType::Crossbow => (2, 18),
            WeaponType::Wand => (2, 19),
            WeaponType::CasterOffhand => (4, 0),
            WeaponType::Shield => (4, 6),
            WeaponType::Libram => (4, 7),
            WeaponType::Idol => (4, 8),
            WeaponType::Totem => (4, 9),
        }
    }

    /// Whether the weapon has a weapon skill that gear/race can raise.
    pub fn has_weapon_skill(self) -> bool {
        matches!(
            self,
            WeaponType::Axe
                | WeaponType::Dagger
                | WeaponType::Fist
                | WeaponType::Mace
                | WeaponType::Sword
                | WeaponType::TwohandAxe
                | WeaponType::TwohandMace
                | WeaponType::TwohandSword
                | WeaponType::Bow
                | WeaponType::Crossbow
                | WeaponType::Gun
        )
    }
}

/// Where a weapon can be wielded. Port of `WeaponSlots`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeaponSlot {
    /// Can be wielded in either hand.
    #[serde(rename = "1H")]
    OneHand,
    #[serde(rename = "MH")]
    Mainhand,
    #[serde(rename = "OH")]
    Offhand,
    #[serde(rename = "2H")]
    TwoHand,
    #[serde(rename = "RANGED")]
    Ranged,
}

impl WeaponSlot {
    pub fn fits_mainhand(self) -> bool {
        matches!(
            self,
            WeaponSlot::OneHand | WeaponSlot::Mainhand | WeaponSlot::TwoHand
        )
    }

    pub fn fits_offhand(self) -> bool {
        matches!(self, WeaponSlot::OneHand | WeaponSlot::Offhand)
    }
}

/// The slot an item is designed for, as written in the item database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ItemSlot {
    #[serde(rename = "1H")]
    OneHand,
    #[serde(rename = "MH")]
    Mainhand,
    #[serde(rename = "OH")]
    Offhand,
    #[serde(rename = "2H")]
    TwoHand,
    Ranged,
    Head,
    Neck,
    Shoulders,
    Back,
    Chest,
    Wrist,
    Gloves,
    Belt,
    Legs,
    Boots,
    Ring,
    Trinket,
    Relic,
    Projectile,
    Quiver,
}

impl ItemSlot {
    /// The weapon slot for weapon item slots.
    pub fn weapon_slot(self) -> Option<WeaponSlot> {
        Some(match self {
            ItemSlot::OneHand => WeaponSlot::OneHand,
            ItemSlot::Mainhand => WeaponSlot::Mainhand,
            ItemSlot::Offhand => WeaponSlot::Offhand,
            ItemSlot::TwoHand => WeaponSlot::TwoHand,
            ItemSlot::Ranged => WeaponSlot::Ranged,
            _ => return None,
        })
    }

    /// Whether an item of this slot can be equipped in `equipment_slot`.
    pub fn fits(self, equipment_slot: EquipmentSlot) -> bool {
        match equipment_slot {
            EquipmentSlot::Mainhand => self.weapon_slot().is_some_and(WeaponSlot::fits_mainhand),
            EquipmentSlot::Offhand => self.weapon_slot().is_some_and(WeaponSlot::fits_offhand),
            EquipmentSlot::Ranged => self == ItemSlot::Ranged,
            EquipmentSlot::Head => self == ItemSlot::Head,
            EquipmentSlot::Neck => self == ItemSlot::Neck,
            EquipmentSlot::Shoulders => self == ItemSlot::Shoulders,
            EquipmentSlot::Back => self == ItemSlot::Back,
            EquipmentSlot::Chest => self == ItemSlot::Chest,
            EquipmentSlot::Wrist => self == ItemSlot::Wrist,
            EquipmentSlot::Gloves => self == ItemSlot::Gloves,
            EquipmentSlot::Belt => self == ItemSlot::Belt,
            EquipmentSlot::Legs => self == ItemSlot::Legs,
            EquipmentSlot::Boots => self == ItemSlot::Boots,
            EquipmentSlot::Ring1 | EquipmentSlot::Ring2 => self == ItemSlot::Ring,
            EquipmentSlot::Trinket1 | EquipmentSlot::Trinket2 => self == ItemSlot::Trinket,
            EquipmentSlot::Relic => self == ItemSlot::Relic,
            EquipmentSlot::Projectile => self == ItemSlot::Projectile,
            EquipmentSlot::Quiver => self == ItemSlot::Quiver,
        }
    }
}

/// A slot on the character. Port of `EquipmentSlot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EquipmentSlot {
    Mainhand,
    Offhand,
    Ranged,
    Head,
    Neck,
    Shoulders,
    Back,
    Chest,
    Wrist,
    Gloves,
    Belt,
    Legs,
    Boots,
    Ring1,
    Ring2,
    Trinket1,
    Trinket2,
    Relic,
    Projectile,
    Quiver,
}

impl EquipmentSlot {
    pub const ALL: [EquipmentSlot; 20] = [
        EquipmentSlot::Mainhand,
        EquipmentSlot::Offhand,
        EquipmentSlot::Ranged,
        EquipmentSlot::Head,
        EquipmentSlot::Neck,
        EquipmentSlot::Shoulders,
        EquipmentSlot::Back,
        EquipmentSlot::Chest,
        EquipmentSlot::Wrist,
        EquipmentSlot::Gloves,
        EquipmentSlot::Belt,
        EquipmentSlot::Legs,
        EquipmentSlot::Boots,
        EquipmentSlot::Ring1,
        EquipmentSlot::Ring2,
        EquipmentSlot::Trinket1,
        EquipmentSlot::Trinket2,
        EquipmentSlot::Relic,
        EquipmentSlot::Projectile,
        EquipmentSlot::Quiver,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn is_weapon_slot(self) -> bool {
        matches!(
            self,
            EquipmentSlot::Mainhand | EquipmentSlot::Offhand | EquipmentSlot::Ranged
        )
    }

    /// The attacks a proc of an item worn in this slot reacts to when nothing narrows them: a
    /// weapon only procs off its own hand, anything else off any melee attack. Port of
    /// `Item::add_default_proc_sources`.
    pub fn default_proc_sources(self) -> Vec<ProcSource> {
        match self {
            EquipmentSlot::Mainhand => vec![ProcSource::MainhandSwing, ProcSource::MainhandSpell],
            EquipmentSlot::Offhand => vec![ProcSource::OffhandSwing, ProcSource::OffhandSpell],
            EquipmentSlot::Ranged => vec![ProcSource::RangedAutoShot, ProcSource::RangedSpell],
            _ => vec![
                ProcSource::MainhandSwing,
                ProcSource::MainhandSpell,
                ProcSource::OffhandSwing,
                ProcSource::OffhandSpell,
            ],
        }
    }
}

/// Armor classes. Port of `ArmorTypes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArmorType {
    Cloth,
    Leather,
    Mail,
    Plate,
}

/// The type of an item: a weapon type, an armor class or one of the jewelry/relic kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ItemType {
    Axe,
    Dagger,
    Fist,
    Mace,
    Polearm,
    Staff,
    Sword,
    Bow,
    Crossbow,
    Gun,
    Thrown,
    Wand,
    Idol,
    Libram,
    Totem,
    Shield,
    CasterOffhand,
    TwohandAxe,
    TwohandMace,
    TwohandSword,
    Cloth,
    Leather,
    Mail,
    Plate,
    Ring,
    Amulet,
    Trinket,
    Relic,
    Arrow,
    Bullet,
    Quiver,
    AmmoPouch,
}

impl ItemType {
    pub fn weapon_type(self) -> Option<WeaponType> {
        Some(match self {
            ItemType::Axe => WeaponType::Axe,
            ItemType::Dagger => WeaponType::Dagger,
            ItemType::Fist => WeaponType::Fist,
            ItemType::Mace => WeaponType::Mace,
            ItemType::Polearm => WeaponType::Polearm,
            ItemType::Staff => WeaponType::Staff,
            ItemType::Sword => WeaponType::Sword,
            ItemType::Bow => WeaponType::Bow,
            ItemType::Crossbow => WeaponType::Crossbow,
            ItemType::Gun => WeaponType::Gun,
            ItemType::Thrown => WeaponType::Thrown,
            ItemType::Wand => WeaponType::Wand,
            ItemType::Idol => WeaponType::Idol,
            ItemType::Libram => WeaponType::Libram,
            ItemType::Totem => WeaponType::Totem,
            ItemType::Shield => WeaponType::Shield,
            ItemType::CasterOffhand => WeaponType::CasterOffhand,
            ItemType::TwohandAxe => WeaponType::TwohandAxe,
            ItemType::TwohandMace => WeaponType::TwohandMace,
            ItemType::TwohandSword => WeaponType::TwohandSword,
            _ => return None,
        })
    }

    pub fn armor_type(self) -> Option<ArmorType> {
        Some(match self {
            ItemType::Cloth => ArmorType::Cloth,
            ItemType::Leather => ArmorType::Leather,
            ItemType::Mail => ArmorType::Mail,
            ItemType::Plate => ArmorType::Plate,
            _ => return None,
        })
    }
}

/// Item quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Quality {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

/// Stat keys used by items, set bonuses, enchants and generic stat buffs.
///
/// The value semantics differ per key: chances (`CRIT_CHANCE`, `HIT_CHANCE`, `DODGE_CHANCE`, ...)
/// are fractions in data files (`0.01` = 1%), speed keys are percentages, everything else is a
/// flat amount. See [`crate::stats::Stats::apply_item_stat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ItemStat {
    Agility,
    Intellect,
    Spirit,
    Stamina,
    Strength,
    Armor,
    Defense,
    BlockValue,
    BlockChance,
    DodgeChance,
    ParryChance,
    ArcaneResistance,
    FireResistance,
    FrostResistance,
    HolyResistance,
    NatureResistance,
    ShadowResistance,
    AllResistance,
    AxeSkill,
    DaggerSkill,
    FistSkill,
    MaceSkill,
    SwordSkill,
    TwohandAxeSkill,
    TwohandMaceSkill,
    TwohandSwordSkill,
    BowSkill,
    CrossbowSkill,
    GunSkill,
    HitChance,
    CritChance,
    RangedHitChance,
    AttackSpeed,
    MeleeAttackSpeed,
    RangedAttackSpeed,
    CastingSpeed,
    AttackPower,
    MeleeAttackPower,
    RangedAttackPower,
    FeralAttackPower,
    AttackPowerBeast,
    AttackPowerDemon,
    AttackPowerDragonkin,
    AttackPowerElemental,
    AttackPowerGiant,
    AttackPowerHumanoid,
    AttackPowerMechanical,
    AttackPowerUndead,
    WeaponDamage,
    #[serde(rename = "MANA_PER_5")]
    ManaPer5,
    #[serde(rename = "HEALTH_PER_5")]
    HealthPer5,
    ManaSkillReduction,
    SpellDamage,
    SpellDamageArcane,
    SpellDamageFire,
    SpellDamageFrost,
    SpellDamageHoly,
    SpellDamageNature,
    SpellDamageShadow,
    SpellDamageBeast,
    SpellDamageDemon,
    SpellDamageDragonkin,
    SpellDamageElemental,
    SpellDamageGiant,
    SpellDamageHumanoid,
    SpellDamageMechanical,
    SpellDamageUndead,
    SpellCritChance,
    SpellHitChance,
    SpellPenetration,
    /// Healing done. The sim does no healing, so it has no effect.
    HealingPower,
    /// Combat ratings as the tables store them (not converted). Until the engine has a rating
    /// system they are turned into chances by [`crate::item::rating`].
    HitRating,
    CritRating,
    DodgeRating,
    ParryRating,
    BlockRating,
    HasteRating,
    ExpertiseRating,
    ArmorPenetrationRating,
}

impl ItemStat {
    /// The creature type an `ATTACK_POWER_*` / `SPELL_DAMAGE_*` creature stat applies to.
    pub fn creature_type(self) -> Option<CreatureType> {
        Some(match self {
            ItemStat::AttackPowerBeast | ItemStat::SpellDamageBeast => CreatureType::Beast,
            ItemStat::AttackPowerDemon | ItemStat::SpellDamageDemon => CreatureType::Demon,
            ItemStat::AttackPowerDragonkin | ItemStat::SpellDamageDragonkin => {
                CreatureType::Dragonkin
            }
            ItemStat::AttackPowerElemental | ItemStat::SpellDamageElemental => {
                CreatureType::Elemental
            }
            ItemStat::AttackPowerGiant | ItemStat::SpellDamageGiant => CreatureType::Giant,
            ItemStat::AttackPowerHumanoid | ItemStat::SpellDamageHumanoid => CreatureType::Humanoid,
            ItemStat::AttackPowerMechanical | ItemStat::SpellDamageMechanical => {
                CreatureType::Mechanical
            }
            ItemStat::AttackPowerUndead | ItemStat::SpellDamageUndead => CreatureType::Undead,
            _ => return None,
        })
    }

    /// The magic school a `SPELL_DAMAGE_<school>` / `<SCHOOL>_RESISTANCE` stat applies to.
    pub fn magic_school(self) -> Option<MagicSchool> {
        Some(match self {
            ItemStat::SpellDamageArcane | ItemStat::ArcaneResistance => MagicSchool::Arcane,
            ItemStat::SpellDamageFire | ItemStat::FireResistance => MagicSchool::Fire,
            ItemStat::SpellDamageFrost | ItemStat::FrostResistance => MagicSchool::Frost,
            ItemStat::SpellDamageHoly | ItemStat::HolyResistance => MagicSchool::Holy,
            ItemStat::SpellDamageNature | ItemStat::NatureResistance => MagicSchool::Nature,
            ItemStat::SpellDamageShadow | ItemStat::ShadowResistance => MagicSchool::Shadow,
            _ => return None,
        })
    }

    /// The weapon type a `<TYPE>_SKILL` stat applies to.
    pub fn weapon_type(self) -> Option<WeaponType> {
        Some(match self {
            ItemStat::AxeSkill => WeaponType::Axe,
            ItemStat::DaggerSkill => WeaponType::Dagger,
            ItemStat::FistSkill => WeaponType::Fist,
            ItemStat::MaceSkill => WeaponType::Mace,
            ItemStat::SwordSkill => WeaponType::Sword,
            ItemStat::TwohandAxeSkill => WeaponType::TwohandAxe,
            ItemStat::TwohandMaceSkill => WeaponType::TwohandMace,
            ItemStat::TwohandSwordSkill => WeaponType::TwohandSword,
            ItemStat::BowSkill => WeaponType::Bow,
            ItemStat::CrossbowSkill => WeaponType::Crossbow,
            ItemStat::GunSkill => WeaponType::Gun,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names_match_data_files() {
        assert_eq!(
            serde_yaml::from_str::<WeaponType>("TWOHAND_SWORD").unwrap(),
            WeaponType::TwohandSword
        );
        assert_eq!(
            serde_yaml::from_str::<WeaponType>("CASTER_OFFHAND").unwrap(),
            WeaponType::CasterOffhand
        );
        assert_eq!(
            serde_yaml::from_str::<ItemStat>("MANA_PER_5").unwrap(),
            ItemStat::ManaPer5
        );
        assert_eq!(
            serde_yaml::from_str::<ItemStat>("HEALTH_PER_5").unwrap(),
            ItemStat::HealthPer5
        );
        assert_eq!(
            serde_yaml::from_str::<ItemStat>("ATTACK_POWER_UNDEAD").unwrap(),
            ItemStat::AttackPowerUndead
        );
        assert_eq!(
            serde_yaml::from_str::<ItemStat>("SPELL_CRIT_CHANCE").unwrap(),
            ItemStat::SpellCritChance
        );
        assert_eq!(
            serde_yaml::to_string(&ItemStat::TwohandAxeSkill)
                .unwrap()
                .trim(),
            "TWOHAND_AXE_SKILL"
        );
    }

    #[test]
    fn weapon_type_classification() {
        assert!(WeaponType::TwohandAxe.is_two_hand());
        assert!(WeaponType::Staff.is_two_hand());
        assert!(!WeaponType::Sword.is_two_hand());
        assert!(WeaponType::Gun.has_weapon_skill());
        assert!(!WeaponType::Shield.has_weapon_skill());
        assert!(!WeaponType::Polearm.has_weapon_skill());
        assert_eq!(WeaponType::ALL.len(), WeaponType::COUNT);
    }

    #[test]
    fn slot_and_type_serde() {
        assert_eq!(
            serde_yaml::from_str::<ItemSlot>("1H").unwrap(),
            ItemSlot::OneHand
        );
        assert_eq!(
            serde_yaml::from_str::<ItemSlot>("MH").unwrap(),
            ItemSlot::Mainhand
        );
        assert_eq!(
            serde_yaml::from_str::<ItemSlot>("OH").unwrap(),
            ItemSlot::Offhand
        );
        assert_eq!(
            serde_yaml::from_str::<ItemSlot>("2H").unwrap(),
            ItemSlot::TwoHand
        );
        assert_eq!(
            serde_yaml::from_str::<ItemSlot>("RANGED").unwrap(),
            ItemSlot::Ranged
        );
        assert_eq!(
            serde_yaml::from_str::<ItemSlot>("SHOULDERS").unwrap(),
            ItemSlot::Shoulders
        );
        assert_eq!(
            serde_yaml::to_string(&ItemSlot::TwoHand).unwrap().trim(),
            "2H"
        );
        assert_eq!(
            serde_yaml::from_str::<ItemType>("PLATE").unwrap(),
            ItemType::Plate
        );
        assert_eq!(
            serde_yaml::from_str::<ItemType>("CASTER_OFFHAND").unwrap(),
            ItemType::CasterOffhand
        );
        assert_eq!(
            serde_yaml::from_str::<Quality>("EPIC").unwrap(),
            Quality::Epic
        );
        assert_eq!(
            serde_yaml::from_str::<EquipmentSlot>("TRINKET2").unwrap(),
            EquipmentSlot::Trinket2
        );
    }

    #[test]
    fn item_slots_fit_equipment_slots() {
        assert!(ItemSlot::OneHand.fits(EquipmentSlot::Mainhand));
        assert!(ItemSlot::OneHand.fits(EquipmentSlot::Offhand));
        assert!(ItemSlot::Mainhand.fits(EquipmentSlot::Mainhand));
        assert!(!ItemSlot::Mainhand.fits(EquipmentSlot::Offhand));
        assert!(ItemSlot::Offhand.fits(EquipmentSlot::Offhand));
        assert!(!ItemSlot::Offhand.fits(EquipmentSlot::Mainhand));
        assert!(ItemSlot::TwoHand.fits(EquipmentSlot::Mainhand));
        assert!(!ItemSlot::TwoHand.fits(EquipmentSlot::Offhand));
        assert!(ItemSlot::Ranged.fits(EquipmentSlot::Ranged));
        assert!(!ItemSlot::Ranged.fits(EquipmentSlot::Mainhand));
        assert!(ItemSlot::Ring.fits(EquipmentSlot::Ring1));
        assert!(ItemSlot::Ring.fits(EquipmentSlot::Ring2));
        assert!(!ItemSlot::Ring.fits(EquipmentSlot::Trinket1));
        assert!(ItemSlot::Trinket.fits(EquipmentSlot::Trinket2));
        assert!(ItemSlot::Head.fits(EquipmentSlot::Head));
        assert!(!ItemSlot::Head.fits(EquipmentSlot::Chest));
        assert_eq!(EquipmentSlot::ALL.len(), EquipmentSlot::COUNT);
        assert!(EquipmentSlot::Ranged.is_weapon_slot());
        assert!(!EquipmentSlot::Relic.is_weapon_slot());
    }

    #[test]
    fn item_type_lookups() {
        assert_eq!(
            ItemType::TwohandMace.weapon_type(),
            Some(WeaponType::TwohandMace)
        );
        assert_eq!(ItemType::Shield.weapon_type(), Some(WeaponType::Shield));
        assert_eq!(ItemType::Plate.weapon_type(), None);
        assert_eq!(ItemType::Mail.armor_type(), Some(ArmorType::Mail));
        assert_eq!(ItemType::Ring.armor_type(), None);
        assert!(Quality::Rare < Quality::Epic);
    }

    #[test]
    fn stat_lookups() {
        assert_eq!(
            ItemStat::AttackPowerDemon.creature_type(),
            Some(CreatureType::Demon)
        );
        assert_eq!(
            ItemStat::SpellDamageGiant.creature_type(),
            Some(CreatureType::Giant)
        );
        assert_eq!(ItemStat::Strength.creature_type(), None);
        assert_eq!(
            ItemStat::FrostResistance.magic_school(),
            Some(MagicSchool::Frost)
        );
        assert_eq!(
            ItemStat::SpellDamageShadow.magic_school(),
            Some(MagicSchool::Shadow)
        );
        assert_eq!(ItemStat::SpellDamage.magic_school(), None);
        assert_eq!(
            ItemStat::CrossbowSkill.weapon_type(),
            Some(WeaponType::Crossbow)
        );
        assert_eq!(ItemStat::Armor.weapon_type(), None);
    }
}
