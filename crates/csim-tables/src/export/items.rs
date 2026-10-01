//! Item derivation: turns the `Item*` rows of one item into what the simulator needs — slot and
//! type, stats, armor, weapon damage, item spells, set, unique group and random-suffix pool —
//! following `data/ITEM_INSTRUCTIONS.md` §1.
//!
//! Everything here is a pure function of [`Tables`]. Ratings are kept as ratings (`HIT_RATING`,
//! ...); converting them is the engine's job (`csim_engine::item::rating`).
//!
//! `export-items` writes the derived items as one [`ItemFile`] per slot ([`item_file_name`]) and
//! the item sets as an [`ItemSetFile`]; [`compare_items`] lists where they differ from the
//! hand-authored Classic items.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use csim_engine::faction::PlayerClass;
pub use csim_engine::item::{EffectTrigger, ItemEffect, ItemSuffix, LimitCategory};
use csim_engine::item::{
    ItemFile, ItemSetBonus, ItemSetFile, ItemSetSpec, ItemSlot, ItemSpec, ItemStat, ItemType,
    Quality, WeaponDamageSpec, rating,
};
use csim_engine::magic_school::MagicSchool;
use csim_engine::phase::Phase;

use crate::Tables;
use crate::tables::{ItemDamageTable, ItemRow, ItemSetRow, ItemSparseRow};

/// Lowest `OverallQualityID` exported (Rare).
pub const MIN_QUALITY: u32 = 3;
/// Highest `OverallQualityID` exported (Legendary; the Artifact items are test items).
pub const MAX_QUALITY: u32 = 5;
/// Highest `RequiredLevel` of a real item; above are test / placeholder rows.
pub const MAX_REQUIRED_LEVEL: u32 = 60;

/// `ItemSparse.Flags_0` bit of deprecated items.
const FLAG_DEPRECATED: u32 = 0x10;
/// `ItemSparse.Flags_0` bit of unique-equipped items.
const FLAG_UNIQUE_EQUIPPED: u32 = 0x8_0000;
/// Name fragments of test, placeholder and deprecated items (§1.1).
const EXCLUDED_NAME_PARTS: &[&str] = &[
    "TEST",
    "[PH]",
    "Monster - ",
    "QA ",
    "UNUSED",
    "DEPRECATED",
    "Deprecated",
];

/// `ChrClasses.ID` of each class; `AllowableClass` bit = `1 << (id − 1)` (§1.3).
const CLASS_IDS: [(u32, PlayerClass); 9] = [
    (1, PlayerClass::Warrior),
    (2, PlayerClass::Paladin),
    (3, PlayerClass::Hunter),
    (4, PlayerClass::Rogue),
    (5, PlayerClass::Priest),
    (7, PlayerClass::Shaman),
    (8, PlayerClass::Mage),
    (9, PlayerClass::Warlock),
    (11, PlayerClass::Druid),
];

/// One item as derived from the tables.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedItem {
    pub id: u32,
    pub name: String,
    pub slot: ItemSlot,
    pub item_type: ItemType,
    pub quality: Quality,
    /// `Item.IconFileDataID`.
    pub icon: u32,
    pub item_level: u32,
    pub required_level: u32,
    /// Binds when equipped (`Bonding` 2).
    pub boe: bool,
    /// "Unique" (`MaxCount` 1) or "Unique-Equipped" (`Flags_0 & 0x80000`).
    pub unique: bool,
    pub limit_category: Option<LimitCategory>,
    /// Empty = every class.
    pub class_restrictions: Vec<PlayerClass>,
    /// Weapons, shields and held off-hands.
    pub damage: Option<WeaponDamage>,
    /// Stats including `ARMOR`; ratings unconverted.
    pub stats: BTreeMap<ItemStat, f64>,
    pub effects: Vec<ItemEffect>,
    /// `ItemSet.ID`.
    pub set: Option<u32>,
    /// The random suffixes the item can roll ("of the Bear"); its base stats are then empty.
    pub suffixes: Vec<ItemSuffix>,
    pub flavour_text: String,
}

/// Weapon damage range and speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponDamage {
    pub min: u32,
    pub max: u32,
    /// Seconds.
    pub speed: f64,
    /// `DamageType`: 0 physical, 1–6 holy, fire, nature, frost, shadow, arcane.
    pub school: u32,
}

/// Why an item is not exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Skip {
    /// The dump has no `ItemSparse` row (§1.1).
    NoSparseRow,
    /// Not a weapon or armor.
    NotEquipment,
    /// Quality outside [`MIN_QUALITY`]..=[`MAX_QUALITY`].
    Quality,
    Deprecated,
    /// Test / placeholder / monster item, or above [`MAX_REQUIRED_LEVEL`].
    TestItem,
    /// Shirts, tabards, fishing poles, misc weapons, ...
    UnsupportedSlot {
        class_id: u32,
        subclass_id: u32,
        inventory_type: u32,
    },
}

/// Something about an exported item the derivation could not fully resolve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemIssue {
    pub item_id: u32,
    pub message: String,
}

/// The result of [`derive_items`].
#[derive(Debug, Default)]
pub struct ItemReport {
    /// Exported items, sorted by id.
    pub items: Vec<DerivedItem>,
    /// Weapon / armor item ids that were not exported, by reason, sorted.
    pub skipped: BTreeMap<Skip, Vec<u32>>,
    pub issues: Vec<ItemIssue>,
}

/// What an `ItemSparse` `bonusStat` id means (§1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatKind {
    Stat(ItemStat),
    /// Bonus armor (50): the same number as `QualityModifier` on armor.
    BonusArmor,
    /// Profession skills: irrelevant to the sim.
    Ignored,
    /// A known stat the engine has no key for.
    Unsupported(&'static str),
    Unknown,
}

/// The meaning of a `bonusStat` id. The Classic-era ids follow the `ITEM_MOD_*` order of
/// `GlobalStrings`: weapon skills 90–105, professions 106–118, attack power vs creature type
/// 125–132 and spell damage vs creature type 133–140 (Humanoid, Elemental, Demon, Undead,
/// Dragonkin, Giant, Beast, Mechanical).
pub fn stat_kind(bonus_stat: i32) -> StatKind {
    use ItemStat as S;
    StatKind::Stat(match bonus_stat {
        3 => S::Agility,
        4 => S::Strength,
        5 => S::Intellect,
        6 => S::Spirit,
        7 => S::Stamina,
        12 => S::Defense,
        13 => S::DodgeRating,
        14 => S::ParryRating,
        15 => S::BlockRating,
        31 => S::HitRating,
        32 => S::CritRating,
        36 => S::HasteRating,
        37 => S::ExpertiseRating,
        38 => S::AttackPower,
        39 => S::RangedAttackPower,
        41 => S::HealingPower,
        42 => S::SpellDamage,
        43 => S::ManaPer5,
        44 => S::ArmorPenetrationRating,
        // Spell damage and healing; the sim only needs the damage.
        45 => S::SpellDamage,
        46 => S::HealthPer5,
        47 => S::SpellPenetration,
        48 => S::BlockValue,
        50 => return StatKind::BonusArmor,
        51 => S::FireResistance,
        52 => S::FrostResistance,
        53 => S::HolyResistance,
        54 => S::ShadowResistance,
        55 => S::NatureResistance,
        56 => S::ArcaneResistance,
        83 => S::WeaponDamage,
        84 => S::SpellDamageHoly,
        85 => S::SpellDamageFire,
        86 => S::SpellDamageNature,
        87 => S::SpellDamageFrost,
        88 => S::SpellDamageShadow,
        89 => S::SpellDamageArcane,
        90 => S::TwohandAxeSkill,
        91 => S::TwohandMaceSkill,
        92 => S::TwohandSwordSkill,
        93 => S::AxeSkill,
        94 => S::BowSkill,
        95 => S::CrossbowSkill,
        96 => S::DaggerSkill,
        98 => S::FistSkill,
        99 => S::GunSkill,
        100 => S::MaceSkill,
        103 => S::SwordSkill,
        124 => S::AllResistance,
        125 => S::AttackPowerHumanoid,
        126 => S::AttackPowerElemental,
        127 => S::AttackPowerDemon,
        128 => S::AttackPowerUndead,
        129 => S::AttackPowerDragonkin,
        130 => S::AttackPowerGiant,
        131 => S::AttackPowerBeast,
        132 => S::AttackPowerMechanical,
        133 => S::SpellDamageHumanoid,
        134 => S::SpellDamageElemental,
        135 => S::SpellDamageDemon,
        136 => S::SpellDamageUndead,
        137 => S::SpellDamageDragonkin,
        138 => S::SpellDamageGiant,
        139 => S::SpellDamageBeast,
        140 => S::SpellDamageMechanical,
        106..=118 => return StatKind::Ignored,
        0 => return StatKind::Unsupported("mana"),
        1 => return StatKind::Unsupported("health"),
        97 => return StatKind::Unsupported("dual wield skill"),
        101 => return StatKind::Unsupported("polearm skill"),
        102 => return StatKind::Unsupported("staff skill"),
        104 => return StatKind::Unsupported("thrown skill"),
        105 => return StatKind::Unsupported("wand skill"),
        _ => return StatKind::Unknown,
    })
}

/// The `Quality` of an `OverallQualityID` in the exported range.
fn quality(id: u32) -> Option<Quality> {
    Some(match id {
        1 => Quality::Common,
        2 => Quality::Uncommon,
        3 => Quality::Rare,
        4 => Quality::Epic,
        5 => Quality::Legendary,
        _ => return None,
    })
}

/// The item slot and type of an item, or `None` for slots the sim does not model.
pub fn slot_and_type(item: &ItemRow, inventory_type: u32) -> Option<(ItemSlot, ItemType)> {
    let (class, subclass) = (item.class_id, item.subclass_id);
    match class {
        2 => {
            let item_type = match subclass {
                0 => ItemType::Axe,
                1 => ItemType::TwohandAxe,
                2 => ItemType::Bow,
                3 => ItemType::Gun,
                4 => ItemType::Mace,
                5 => ItemType::TwohandMace,
                6 => ItemType::Polearm,
                7 => ItemType::Sword,
                8 => ItemType::TwohandSword,
                10 => ItemType::Staff,
                13 => ItemType::Fist,
                15 => ItemType::Dagger,
                16 => ItemType::Thrown,
                18 => ItemType::Crossbow,
                19 => ItemType::Wand,
                _ => return None,
            };
            let slot = match inventory_type {
                13 => ItemSlot::OneHand,
                21 => ItemSlot::Mainhand,
                22 => ItemSlot::Offhand,
                17 => ItemSlot::TwoHand,
                15 | 25 | 26 => ItemSlot::Ranged,
                _ => return None,
            };
            Some((slot, item_type))
        }
        4 => {
            let armor = match subclass {
                1 => Some(ItemType::Cloth),
                2 => Some(ItemType::Leather),
                3 => Some(ItemType::Mail),
                4 => Some(ItemType::Plate),
                _ => None,
            };
            Some(match inventory_type {
                1 => (ItemSlot::Head, armor?),
                3 => (ItemSlot::Shoulders, armor?),
                5 | 20 => (ItemSlot::Chest, armor?),
                6 => (ItemSlot::Belt, armor?),
                7 => (ItemSlot::Legs, armor?),
                8 => (ItemSlot::Boots, armor?),
                9 => (ItemSlot::Wrist, armor?),
                10 => (ItemSlot::Gloves, armor?),
                2 => (ItemSlot::Neck, ItemType::Amulet),
                11 => (ItemSlot::Ring, ItemType::Ring),
                12 => (ItemSlot::Trinket, ItemType::Trinket),
                16 => (ItemSlot::Back, ItemType::Cloth),
                14 if subclass == 6 => (ItemSlot::Offhand, ItemType::Shield),
                23 => (ItemSlot::Offhand, ItemType::CasterOffhand),
                28 => (
                    ItemSlot::Relic,
                    match subclass {
                        7 => ItemType::Libram,
                        8 => ItemType::Idol,
                        9 => ItemType::Totem,
                        _ => return None,
                    },
                ),
                _ => return None,
            })
        }
        _ => None,
    }
}

/// The `RandPropPoints` stat budget of an item (§1.4), or `None` for a slot without one.
pub fn stat_budget(tables: &Tables, sparse: &ItemSparseRow, subclass_id: u32) -> Option<u32> {
    let group = match sparse.inventory_type {
        1 | 5 | 7 | 20 | 17 | 25 => 0,
        3 | 6 | 8 | 10 | 12 => 1,
        2 | 9 | 11 | 14 | 16 | 23 => 2,
        13 | 21 | 22 => 3,
        26 if subclass_id == 19 => 3,
        15 | 26 | 28 => 4,
        _ => return None,
    };
    let points = tables.rand_prop_points(sparse.item_level)?;
    let column = match sparse.overall_quality_id {
        4..=6 => &points.epic,
        3 => &points.superior,
        _ => &points.good,
    };
    Some(column[group])
}

/// A stat value from its budget share: `floor(share · budget / 10000 + 0.5)`.
pub fn stat_value(share: i32, budget: u32) -> f64 {
    (f64::from(share) * f64::from(budget) / 10_000.0 + 0.5).floor()
}

/// Base armor from the item-level tables (§1.5), before bonus armor.
pub fn base_armor(tables: &Tables, item: &ItemRow, sparse: &ItemSparseRow) -> Option<f64> {
    if item.class_id != 4 {
        return Some(0.0);
    }
    let level = sparse.item_level;
    let quality = sparse.overall_quality_id as usize;
    if item.subclass_id == 6 {
        return Some(tables.item_armor_shield(level)?.quality[quality].round());
    }
    let inventory_type = sparse.inventory_type;
    let total = tables.item_armor_total(level)?;
    let quality_mod = tables.item_armor_quality(level)?.quality_mod[quality];
    let (material, location) = if inventory_type == 16 {
        (total.cloth, tables.armor_location(16)?.modifier)
    } else {
        // Robes use the chest row.
        let location = tables.armor_location(if inventory_type == 20 {
            5
        } else {
            inventory_type
        });
        match item.subclass_id {
            1 => (total.cloth, location?.cloth_modifier),
            2 => (total.leather, location?.leather_modifier),
            3 => (total.mail, location?.chain_modifier),
            4 => (total.plate, location?.plate_modifier),
            _ => return Some(0.0),
        }
    };
    Some((material * quality_mod * location).round())
}

/// Weapon damage (§1.6). `QualityModifier` on a weapon is a DPS percentage: Thunderfury −20,
/// Benediction −14, the AQ20 caster weapons −10.
pub fn weapon_damage(
    tables: &Tables,
    item: &ItemRow,
    sparse: &ItemSparseRow,
) -> Option<WeaponDamage> {
    let table = match sparse.inventory_type {
        17 => ItemDamageTable::TwoHand,
        13 | 21 | 22 => ItemDamageTable::OneHand,
        15 | 25 | 26 => match item.subclass_id {
            19 => ItemDamageTable::Wand,
            16 => ItemDamageTable::Thrown,
            _ => ItemDamageTable::Ranged,
        },
        _ => return None,
    };
    let row = tables.item_damage(table, sparse.item_level)?;
    let dps =
        row.quality[sparse.overall_quality_id as usize] * (1.0 + sparse.quality_modifier / 100.0);
    let speed = f64::from(sparse.item_delay) / 1000.0;
    let average = dps * speed;
    let variance = sparse.dmg_variance / 2.0;
    Some(WeaponDamage {
        min: (average * (1.0 - variance)).floor() as u32,
        max: (average * (1.0 + variance) + 0.5).floor() as u32,
        speed,
        school: sparse.damage_type,
    })
}

/// The classes an `AllowableClass` mask restricts the item to; empty = every class.
pub fn class_restrictions(allowable_class: i32) -> Vec<PlayerClass> {
    let mask = allowable_class as u32;
    let classes: Vec<PlayerClass> = CLASS_IDS
        .iter()
        .filter(|(id, _)| mask & (1 << (id - 1)) != 0)
        .map(|&(_, class)| class)
        .collect();
    if classes.len() == CLASS_IDS.len() {
        Vec::new()
    } else {
        classes
    }
}

/// Why the item would not be exported, checked before deriving it.
pub fn skip_reason(tables: &Tables, item: &ItemRow) -> Option<Skip> {
    if !matches!(item.class_id, 2 | 4) {
        return Some(Skip::NotEquipment);
    }
    let Some(sparse) = tables.item_sparse(item.id) else {
        return Some(Skip::NoSparseRow);
    };
    if !(MIN_QUALITY..=MAX_QUALITY).contains(&sparse.overall_quality_id) {
        return Some(Skip::Quality);
    }
    if sparse.flags[0] & FLAG_DEPRECATED != 0 {
        return Some(Skip::Deprecated);
    }
    if EXCLUDED_NAME_PARTS.iter().any(|p| sparse.name.contains(p))
        || sparse.required_level > MAX_REQUIRED_LEVEL
    {
        return Some(Skip::TestItem);
    }
    if slot_and_type(item, sparse.inventory_type).is_none() {
        return Some(Skip::UnsupportedSlot {
            class_id: item.class_id,
            subclass_id: item.subclass_id,
            inventory_type: sparse.inventory_type,
        });
    }
    None
}

/// Adds the stats of (`bonusStat`, share) pairs to `stats`. Bonus armor is added only when
/// `bonus_armor_as_stat` (weapons); on armor it is `QualityModifier` instead.
fn add_stats(
    stats: &mut BTreeMap<ItemStat, f64>,
    pairs: impl IntoIterator<Item = (i32, i32)>,
    budget: u32,
    bonus_armor_as_stat: bool,
    item_id: u32,
    issues: &mut Vec<ItemIssue>,
) {
    for (bonus_stat, share) in pairs {
        let value = stat_value(share, budget);
        let stat = match stat_kind(bonus_stat) {
            StatKind::Stat(stat) => stat,
            StatKind::BonusArmor if bonus_armor_as_stat => ItemStat::Armor,
            StatKind::BonusArmor | StatKind::Ignored => continue,
            StatKind::Unsupported(what) => {
                issues.push(ItemIssue {
                    item_id,
                    message: format!("stat {bonus_stat} ({what}) {value} is not supported"),
                });
                continue;
            }
            StatKind::Unknown => {
                issues.push(ItemIssue {
                    item_id,
                    message: format!("unknown stat {bonus_stat} {value}"),
                });
                continue;
            }
        };
        *stats.entry(stat).or_default() += value;
    }
    stats.retain(|_, value| *value != 0.0);
}

/// The random suffixes of an item: the context-0 nodes of its bonus trees (§1.9).
fn suffixes(
    tables: &Tables,
    item_id: u32,
    budget: Option<u32>,
    issues: &mut Vec<ItemIssue>,
) -> Vec<ItemSuffix> {
    let mut suffixes = Vec::new();
    for &tree in tables.item_bonus_trees(item_id) {
        for node in tables.item_bonus_tree_nodes(tree) {
            if node.item_context != 0 {
                continue;
            }
            let bonuses = tables.item_bonuses(node.child_item_bonus_list_id);
            let name = bonuses
                .iter()
                .filter(|b| b.bonus_type == 5)
                .find_map(|b| tables.item_name_description(b.value[0] as u32))
                .map(|d| d.description.clone());
            let pairs: Vec<(i32, i32)> = bonuses
                .iter()
                .filter(|b| b.bonus_type == 2)
                .map(|b| (b.value[0], b.value[1]))
                .collect();
            let Some(name) = name else {
                // Upgrade / appearance bookkeeping trees (Forever "Premier" gear).
                continue;
            };
            let mut stats = BTreeMap::new();
            match budget {
                Some(budget) => add_stats(&mut stats, pairs, budget, false, item_id, issues),
                None => issues.push(ItemIssue {
                    item_id,
                    message: format!("suffix {name:?}: no stat budget"),
                }),
            }
            suffixes.push(ItemSuffix { name, stats });
        }
    }
    suffixes
}

/// The item spells (§1.7). Learn-spell effects (trigger 6) are left out.
pub fn item_effects(tables: &Tables, item_id: u32, issues: &mut Vec<ItemIssue>) -> Vec<ItemEffect> {
    let positive = |ms: i32| u32::try_from(ms).ok().filter(|&ms| ms > 0);
    tables
        .item_effects_of_item(item_id)
        .filter_map(|effect| {
            let trigger = match effect.trigger_type {
                0 | 5 => EffectTrigger::Use,
                1 => EffectTrigger::Equip,
                2 => EffectTrigger::OnHit,
                6 => return None,
                other => {
                    issues.push(ItemIssue {
                        item_id,
                        message: format!(
                            "effect {} has unknown trigger {other}; left out",
                            effect.id
                        ),
                    });
                    return None;
                }
            };
            if !tables.spell_exists(effect.spell_id) {
                issues.push(ItemIssue {
                    item_id,
                    message: format!(
                        "effect {} names missing spell {}",
                        effect.id, effect.spell_id
                    ),
                });
            }
            Some(ItemEffect {
                trigger,
                spell: effect.spell_id,
                cooldown_ms: positive(effect.cooldown_ms),
                category: Some(effect.spell_category_id).filter(|&c| c != 0),
                category_cooldown_ms: positive(effect.category_cooldown_ms),
                charges: effect.charges,
            })
        })
        .collect()
}

/// Derives one item. `Err` says why it is not exported; `issues` collects what could not be
/// fully resolved on an exported item.
pub fn derive_item(
    tables: &Tables,
    item_id: u32,
    issues: &mut Vec<ItemIssue>,
) -> Result<DerivedItem, Option<Skip>> {
    let item = tables.item(item_id).ok_or(None)?;
    if let Some(skip) = skip_reason(tables, item) {
        return Err(Some(skip));
    }
    let sparse = tables
        .item_sparse(item_id)
        .expect("skip_reason checked the sparse row");
    let (slot, item_type) =
        slot_and_type(item, sparse.inventory_type).expect("skip_reason checked the slot");
    let quality = quality(sparse.overall_quality_id).expect("exported quality");

    let budget = stat_budget(tables, sparse, item.subclass_id);
    let pairs: Vec<(i32, i32)> = sparse
        .stat_modifier_bonus_stat
        .iter()
        .zip(&sparse.stat_percent_editor)
        .filter(|(stat, _)| **stat >= 0)
        .map(|(&stat, &share)| (stat, share))
        .collect();
    let mut stats = BTreeMap::new();
    match budget {
        Some(budget) => add_stats(
            &mut stats,
            pairs,
            budget,
            item.class_id != 4,
            item_id,
            issues,
        ),
        None if !pairs.is_empty() => issues.push(ItemIssue {
            item_id,
            message: format!(
                "no stat budget for item level {} slot {}",
                sparse.item_level, sparse.inventory_type
            ),
        }),
        None => {}
    }

    let armor = match base_armor(tables, item, sparse) {
        Some(base) => {
            let bonus = if item.class_id == 4 {
                sparse.quality_modifier.round()
            } else {
                0.0
            };
            base + bonus
        }
        None => {
            issues.push(ItemIssue {
                item_id,
                message: format!("no armor rows for item level {}", sparse.item_level),
            });
            0.0
        }
    };
    if armor != 0.0 {
        *stats.entry(ItemStat::Armor).or_default() += armor;
    }

    let damage = match item_type {
        // The engine models shields and held off-hands as zero-damage weapons.
        ItemType::Shield | ItemType::CasterOffhand => Some(WeaponDamage {
            min: 1,
            max: 1,
            speed: 1.0,
            school: 0,
        }),
        _ if item.class_id == 2 => {
            let damage = weapon_damage(tables, item, sparse);
            if damage.is_none() {
                issues.push(ItemIssue {
                    item_id,
                    message: format!("no damage row for item level {}", sparse.item_level),
                });
            }
            damage
        }
        _ => None,
    };

    let limit_category = match sparse.limit_category {
        0 => None,
        id => match tables.item_limit_category(id) {
            Some(limit) => Some(LimitCategory {
                id,
                name: limit.name.clone(),
                quantity: limit.quantity,
            }),
            None => {
                issues.push(ItemIssue {
                    item_id,
                    message: format!("limit category {id} does not exist"),
                });
                None
            }
        },
    };

    Ok(DerivedItem {
        id: item_id,
        name: sparse.name.clone(),
        slot,
        item_type,
        quality,
        icon: item.icon_file_data_id,
        item_level: sparse.item_level,
        required_level: sparse.required_level,
        boe: sparse.bonding == 2,
        unique: sparse.max_count == 1 || sparse.flags[0] & FLAG_UNIQUE_EQUIPPED != 0,
        limit_category,
        class_restrictions: class_restrictions(sparse.allowable_class),
        damage,
        stats,
        effects: item_effects(tables, item_id, issues),
        set: Some(sparse.item_set).filter(|&s| s != 0),
        suffixes: suffixes(tables, item_id, budget, issues),
        flavour_text: sparse.description.clone(),
    })
}

/// Derives every exportable weapon and armor item of the dump.
pub fn derive_items(tables: &Tables) -> ItemReport {
    let mut ids: Vec<u32> = tables
        .items()
        .filter(|item| matches!(item.class_id, 2 | 4))
        .map(|item| item.id)
        .collect();
    ids.sort_unstable();
    let mut report = ItemReport::default();
    for id in ids {
        match derive_item(tables, id, &mut report.issues) {
            Ok(item) => report.items.push(item),
            Err(Some(skip)) => report.skipped.entry(skip).or_default().push(id),
            Err(None) => {}
        }
    }
    report
}

/// The magic school of a `DamageType`.
pub fn damage_school(damage_type: u32) -> MagicSchool {
    match damage_type {
        1 => MagicSchool::Holy,
        2 => MagicSchool::Fire,
        3 => MagicSchool::Nature,
        4 => MagicSchool::Frost,
        5 => MagicSchool::Shadow,
        6 => MagicSchool::Arcane,
        _ => MagicSchool::Physical,
    }
}

impl DerivedItem {
    /// The item as the engine reads it. Every generated item is `phase: 1` (decision D4).
    pub fn to_spec(&self) -> ItemSpec {
        ItemSpec {
            id: self.id,
            name: self.name.clone(),
            phase: Phase::MoltenCore,
            slot: self.slot,
            item_type: self.item_type,
            quality: self.quality,
            icon: self.icon,
            unique: self.unique,
            req_lvl: self.required_level,
            item_lvl: self.item_level,
            boe: self.boe,
            faction: None,
            class_restrictions: self.class_restrictions.clone(),
            damage: self.damage.map(|damage| WeaponDamageSpec {
                min: damage.min,
                max: damage.max,
                speed: damage.speed,
                school: damage_school(damage.school),
            }),
            stats: self.stats.clone(),
            effects: self.effects.clone(),
            set: self.set,
            limit_category: self.limit_category.clone(),
            suffixes: self.suffixes.clone(),
            flavour_text: self.flavour_text.clone(),
        }
    }
}

/// The file (under `data/items/`, without `.yaml`) an item of `slot` is written to.
pub fn item_file_name(slot: ItemSlot) -> &'static str {
    match slot {
        ItemSlot::OneHand => "one_hand",
        ItemSlot::Mainhand => "main_hand",
        ItemSlot::Offhand => "off_hand",
        ItemSlot::TwoHand => "two_hand",
        ItemSlot::Ranged => "ranged",
        ItemSlot::Head => "head",
        ItemSlot::Neck => "neck",
        ItemSlot::Shoulders => "shoulders",
        ItemSlot::Back => "back",
        ItemSlot::Chest => "chest",
        ItemSlot::Wrist => "wrist",
        ItemSlot::Gloves => "gloves",
        ItemSlot::Belt => "belt",
        ItemSlot::Legs => "legs",
        ItemSlot::Boots => "boots",
        ItemSlot::Ring => "ring",
        ItemSlot::Trinket => "trinket",
        ItemSlot::Relic => "relic",
        ItemSlot::Projectile => "projectile",
        ItemSlot::Quiver => "quiver",
    }
}

/// The item files of the derived items, by file name; items stay sorted by id.
pub fn item_files(tables: &Tables, items: &[DerivedItem]) -> BTreeMap<&'static str, ItemFile> {
    let mut files: BTreeMap<&'static str, ItemFile> = BTreeMap::new();
    for item in items {
        files
            .entry(item_file_name(item.slot))
            .or_insert_with(|| ItemFile {
                build: tables.build().to_owned(),
                items: Vec::new(),
            })
            .items
            .push(item.to_spec());
    }
    files
}

/// The item sets with at least one derived member, sorted by id.
pub fn item_set_file(tables: &Tables, items: &[DerivedItem]) -> ItemSetFile {
    let ids: BTreeSet<u32> = items.iter().filter_map(|item| item.set).collect();
    let sets = ids
        .into_iter()
        .filter_map(|id| tables.item_set(id))
        .map(|set| ItemSetSpec {
            id: set.id,
            name: set.name.clone(),
            items: set_members(set, items),
            bonuses: tables
                .item_set_spells(set.id)
                .iter()
                .map(|bonus| ItemSetBonus {
                    pieces: bonus.threshold,
                    spell: bonus.spell_id,
                })
                .collect(),
        })
        .collect();
    ItemSetFile {
        build: tables.build().to_owned(),
        sets,
    }
}

/// The members of a set: its `ItemSet.ItemID` list, then the derived items whose
/// `ItemSparse.ItemSet` names the set without being listed (Forever variants such as
/// Spiritcaller Mantle), by id.
fn set_members(set: &ItemSetRow, items: &[DerivedItem]) -> Vec<u32> {
    let mut members: Vec<u32> = set.item_ids.iter().copied().filter(|&id| id != 0).collect();
    let mut pointing: Vec<u32> = items
        .iter()
        .filter(|item| item.set == Some(set.id) && !members.contains(&item.id))
        .map(|item| item.id)
        .collect();
    pointing.sort_unstable();
    members.extend(pointing);
    members
}

/// Renders an item file with a header naming its origin.
pub fn render_items(file: &ItemFile, command: &str) -> Result<String, serde_yaml::Error> {
    let body = crate::export::spells::flow_scalar_sequences(&serde_yaml::to_string(file)?);
    let body = whole_numbers(&body);
    Ok(format!(
        "# Generated by `csim-tables {command}`.\n\
         # Do not edit: re-export from a new table dump instead.\n\
         {body}"
    ))
}

/// Writes the whole-number float values of a rendered file without the `.0` (`STAMINA: 8`).
fn whole_numbers(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.lines() {
        match line.rsplit_once(": ") {
            Some((key, value))
                if value.ends_with(".0") && value[..value.len() - 2].parse::<i64>().is_ok() =>
            {
                out.push_str(key);
                out.push_str(": ");
                out.push_str(&value[..value.len() - 2]);
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// Renders the item set file with a header naming its origin.
pub fn render_item_sets(file: &ItemSetFile, command: &str) -> Result<String, serde_yaml::Error> {
    let body = crate::export::spells::flow_scalar_sequences(&serde_yaml::to_string(file)?);
    Ok(format!(
        "# Generated by `csim-tables {command}`.\n\
         # Do not edit: re-export from a new table dump instead. A bonus is the spell active\n\
         # while at least `pieces` members are worn.\n\
         {body}"
    ))
}

/// Why item specs could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadItemsError {
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
}

/// The item specs of one item file ([`ItemFile`]), or of every `*.yaml` file directly in a
/// directory.
pub fn read_item_specs(path: &Path) -> Result<Vec<ItemSpec>, ReadItemsError> {
    let io = |source| ReadItemsError::Io {
        path: path.to_path_buf(),
        source,
    };
    if path.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(io)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "yaml"))
            .collect();
        paths.sort();
        let mut specs = Vec::new();
        for p in paths {
            specs.extend(read_item_specs(&p)?);
        }
        return Ok(specs);
    }
    let yaml = |source| ReadItemsError::Yaml {
        path: path.to_path_buf(),
        source,
    };
    let text = std::fs::read_to_string(path).map_err(io)?;
    let file: ItemFile = serde_yaml::from_str(&text).map_err(yaml)?;
    Ok(file.items)
}

/// The legacy version an exported item is compared with: the newest phase.
pub fn newest_versions(legacy: Vec<ItemSpec>) -> BTreeMap<u32, ItemSpec> {
    let mut newest: BTreeMap<u32, ItemSpec> = BTreeMap::new();
    for spec in legacy {
        match newest.get(&spec.id) {
            Some(existing) if existing.phase >= spec.phase => {}
            _ => {
                newest.insert(spec.id, spec);
            }
        }
    }
    newest
}

/// The stats of an exported item in the legacy units: the ratings the interim factors convert
/// become the chance fractions the hand-authored items use.
fn comparable_stats(stats: &BTreeMap<ItemStat, f64>) -> BTreeMap<ItemStat, f64> {
    let mut result = BTreeMap::new();
    for (&stat, &value) in stats {
        match rating::interim_chance(stat) {
            Some((chance, per_percent)) => {
                *result.entry(chance).or_default() += value / per_percent / 100.0
            }
            None => *result.entry(stat).or_default() += value,
        }
    }
    result
}

/// A number without float noise (`0.020000000000000004` → `0.02`).
fn number(value: f64) -> String {
    let rounded = (value * 1e6).round() / 1e6;
    format!("{rounded}")
}

fn stat_name(stat: ItemStat) -> String {
    serde_yaml::to_string(&stat)
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| format!("{stat:?}"))
}

/// The differences between the exported items and the hand-authored Classic items with the same
/// id (their newest phase), one line per differing field: `id name: field: legacy -> exported`.
/// Ratings are compared through the interim factors. Items without a legacy version are skipped.
pub fn compare_items(exported: &[ItemSpec], legacy: &BTreeMap<u32, ItemSpec>) -> Vec<String> {
    let mut lines = Vec::new();
    for item in exported {
        let Some(old) = legacy.get(&item.id) else {
            continue;
        };
        let mut diff = |field: &str, old: String, new: String| {
            if old != new {
                lines.push(format!(
                    "{} {}: {field}: {old} -> {new}",
                    item.id, item.name
                ));
            }
        };
        diff("name", old.name.clone(), item.name.clone());
        diff(
            "slot",
            format!("{:?}", old.slot),
            format!("{:?}", item.slot),
        );
        diff(
            "type",
            format!("{:?}", old.item_type),
            format!("{:?}", item.item_type),
        );
        diff(
            "quality",
            format!("{:?}", old.quality),
            format!("{:?}", item.quality),
        );
        diff("unique", old.unique.to_string(), item.unique.to_string());
        diff("boe", old.boe.to_string(), item.boe.to_string());
        diff("req_lvl", old.req_lvl.to_string(), item.req_lvl.to_string());
        diff(
            "item_lvl",
            old.item_lvl.to_string(),
            item.item_lvl.to_string(),
        );
        let classes = |classes: &[PlayerClass]| {
            let mut names: Vec<&str> = classes.iter().map(|class| class.name()).collect();
            names.sort_unstable();
            format!("{names:?}")
        };
        diff(
            "classes",
            classes(&old.class_restrictions),
            classes(&item.class_restrictions),
        );
        // Shields and held off-hands carry placeholder damage in both.
        if !matches!(item.item_type, ItemType::Shield | ItemType::CasterOffhand) {
            let damage = |d: Option<WeaponDamageSpec>| {
                d.map_or("none".to_owned(), |d| {
                    format!("{}-{} / {}", d.min, d.max, number(d.speed))
                })
            };
            diff("damage", damage(old.damage), damage(item.damage));
        }
        let new_stats = comparable_stats(&item.stats);
        let keys: BTreeSet<ItemStat> = old.stats.keys().chain(new_stats.keys()).copied().collect();
        for stat in keys {
            let value = |stats: &BTreeMap<ItemStat, f64>| {
                stats.get(&stat).map_or("-".to_owned(), |&v| number(v))
            };
            diff(
                &format!("stat {}", stat_name(stat)),
                value(&old.stats),
                value(&new_stats),
            );
        }
    }
    lines
}
