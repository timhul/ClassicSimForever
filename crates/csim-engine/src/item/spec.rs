//! Serde schema of the item database files (`data/items/*.yaml`).
//!
//! Item files are exported from the client tables by `csim-tables export-items` as an
//! [`ItemFile`] (`build:` header plus `items:`). Their items name the spells they grant
//! ([`ItemEffect`]), their set, unique-equipped group and random-suffix pool.
//!
//! The export does not write `faction` yet (`ItemSparse.AllowableRace`); hand-written test items
//! set it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::faction::{Faction, PlayerClass};
use crate::magic_school::MagicSchool;
use crate::phase::Phase;

use super::types::{ItemSlot, ItemStat, ItemType, Quality};

/// One exported item file (`data/items/*.yaml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFile {
    /// The client build the items were exported from (e.g. `1.60.1.70291`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build: String,
    #[serde(default)]
    pub items: Vec<ItemSpec>,
}

/// The exported item sets (`data/item_sets.yaml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSetFile {
    /// The client build the sets were exported from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build: String,
    #[serde(default)]
    pub sets: Vec<ItemSetSpec>,
}

/// One item set (`ItemSet`) and its bonuses (`ItemSetSpell`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSetSpec {
    pub id: u32,
    pub name: String,
    /// The member item ids.
    pub items: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bonuses: Vec<ItemSetBonus>,
}

/// A set bonus: `spell` is active while at least `pieces` members are worn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSetBonus {
    pub pieces: u32,
    pub spell: u32,
}

/// One item as stored in the item database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSpec {
    pub id: u32,
    pub name: String,
    pub phase: Phase,
    pub slot: ItemSlot,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub quality: Quality,
    /// `Item.IconFileDataID`: the `FileDataID` of the icon texture; 0 = none.
    #[serde(default, skip_serializing_if = "is_no_icon")]
    pub icon: u32,
    /// The icon texture's file name (`inv_sword_39`), from the community listfile: Wowhead's
    /// CDN serves the icon by it. Absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_name: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
    #[serde(default)]
    pub req_lvl: u32,
    #[serde(default)]
    pub item_lvl: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub boe: bool,
    /// `None` means available to both factions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<Faction>,
    /// Empty means available to every class.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_restrictions: Vec<PlayerClass>,
    /// Present for weapons (including shields and caster off-hands, which have zero damage).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage: Option<WeaponDamageSpec>,
    /// Static stats, keyed by stat with data-file value semantics.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stats: BTreeMap<ItemStat, f64>,
    /// The spells the item grants (`ItemEffect`), in the item's slot order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ItemEffect>,
    /// The item set (`ItemSet.ID`) the item belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<u32>,
    /// The unique-equipped group the item counts towards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_category: Option<LimitCategory>,
    /// The random suffixes ("of the Bear") the item can roll, on top of `stats`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suffixes: Vec<ItemSuffix>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub flavour_text: String,
}

/// Damage range and speed of a weapon.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDamageSpec {
    pub min: u32,
    pub max: u32,
    pub speed: f64,
    /// The school of the weapon's damage (a few wands and staves deal magic damage).
    #[serde(default = "physical", skip_serializing_if = "is_physical")]
    pub school: MagicSchool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_no_icon(icon: &u32) -> bool {
    *icon == 0
}

fn physical() -> MagicSchool {
    MagicSchool::Physical
}

fn is_physical(school: &MagicSchool) -> bool {
    *school == MagicSchool::Physical
}

/// When an item spell takes effect (`ItemEffect.TriggerType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectTrigger {
    /// Cast on use (trigger types 0 and 5).
    Use,
    /// A passive aura while the item is equipped (1).
    Equip,
    /// Chance on hit (2); the chance is on the spell.
    OnHit,
}

/// A spell granted by an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEffect {
    pub trigger: EffectTrigger,
    pub spell: u32,
    /// The item's own cooldown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_ms: Option<u32>,
    /// The shared cooldown group (`SpellCategoryID`, e.g. the trinket category) and its
    /// cooldown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_cooldown_ms: Option<u32>,
    /// −1 unlimited, 0 not applicable.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub charges: i32,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

/// A unique-equipped group (`ItemLimitCategory`): at most `quantity` of its items can be worn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitCategory {
    pub id: u32,
    pub name: String,
    pub quantity: u32,
}

/// One random suffix of an item and the stats it adds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSuffix {
    pub name: String,
    pub stats: BTreeMap<ItemStat, f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_bundle::DataBundle;
    use crate::testing::with_build;

    const THRASH_BLADE: &str = r#"
id: 17705
name: Thrash Blade
phase: 1
slot: 1H
type: SWORD
quality: RARE
unique: true
req_lvl: 45
item_lvl: 53
boe: false
damage: { min: 66, max: 124, speed: 2.7 }
"#;

    #[test]
    fn parses_a_weapon() {
        let item: ItemSpec = serde_yaml::from_str(THRASH_BLADE).unwrap();
        assert_eq!(item.id, 17705);
        assert_eq!(item.slot, ItemSlot::OneHand);
        assert_eq!(item.item_type, ItemType::Sword);
        assert_eq!(item.quality, Quality::Rare);
        assert!(item.unique);
        assert_eq!(
            item.damage,
            Some(WeaponDamageSpec {
                min: 66,
                max: 124,
                speed: 2.7,
                school: MagicSchool::Physical,
            })
        );
        assert!(item.stats.is_empty());
        assert_eq!(item.faction, None);
    }

    #[test]
    fn parses_armor_with_stats_and_restrictions() {
        let yaml = r#"
id: 1
name: Test Helm
phase: 3
slot: HEAD
type: PLATE
quality: EPIC
faction: HORDE
class_restrictions: [WARRIOR, PALADIN]
stats:
  STRENGTH: 20
  CRIT_CHANCE: 0.01
  ARMOR: 500
"#;
        let item: ItemSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(item.phase, Phase::BlackwingLair);
        assert_eq!(item.faction, Some(Faction::Horde));
        assert_eq!(
            item.class_restrictions,
            vec![PlayerClass::Warrior, PlayerClass::Paladin]
        );
        assert_eq!(item.stats[&ItemStat::Strength], 20.0);
        assert_eq!(item.stats[&ItemStat::CritChance], 0.01);
        assert!(!item.unique);
        assert!(item.damage.is_none());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let yaml = "id: 1\nname: x\nphase: 1\nslot: HEAD\ntype: PLATE\nquality: RARE\nbogus: 1\n";
        assert!(serde_yaml::from_str::<ItemSpec>(yaml).is_err());
        // The fields of the C++ XML items are gone.
        let yaml = "id: 1
name: x
phase: 1
slot: HEAD
type: PLATE
quality: RARE
mutex: [2]
";
        assert!(serde_yaml::from_str::<ItemSpec>(yaml).is_err());
    }

    #[test]
    fn round_trips_through_yaml() {
        let item: ItemSpec = serde_yaml::from_str(THRASH_BLADE).unwrap();
        let yaml = serde_yaml::to_string(&item).unwrap();
        let again: ItemSpec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(item, again);
    }

    const GENERATED: &str = r#"
build: $BUILD
items:
  - id: 19019
    name: Thunderfury, Blessed Blade of the Windseeker
    phase: 1
    slot: 1H
    type: SWORD
    quality: LEGENDARY
    unique: true
    req_lvl: 60
    item_lvl: 80
    damage: { min: 65, max: 122, speed: 1.9 }
    stats: { AGILITY: 5, STAMINA: 8 }
    effects:
      - { trigger: ON_HIT, spell: 21992 }
  - id: 19024
    name: Arena Grand Master
    phase: 1
    slot: TRINKET
    type: TRINKET
    quality: EPIC
    limit_category: { id: 718, name: Arena Master, quantity: 1 }
    effects:
      - { trigger: EQUIP, spell: 23506 }
      - trigger: USE
        spell: 23506
        cooldown_ms: 120000
        category: 1141
        category_cooldown_ms: 20000
        charges: -1
  - id: 10504
    name: Green Lens
    phase: 1
    slot: HEAD
    type: CLOTH
    quality: RARE
    set: 209
    flavour_text: Look through it.
    damage: { min: 1, max: 2, speed: 1.5, school: arcane }
    suffixes:
      - name: of Magic
        stats: { SPELL_DAMAGE: 28 }
"#;

    #[test]
    fn parses_a_generated_item_file() {
        let file: ItemFile = serde_yaml::from_str(&with_build(GENERATED)).unwrap();
        assert_eq!(file.build, DataBundle::repository_build());
        let [thunderfury, arena, lens] = &file.items[..] else {
            panic!("{:?}", file.items)
        };
        assert_eq!(
            thunderfury.effects,
            [ItemEffect {
                trigger: EffectTrigger::OnHit,
                spell: 21992,
                cooldown_ms: None,
                category: None,
                category_cooldown_ms: None,
                charges: 0,
            }]
        );
        assert_eq!(thunderfury.damage.unwrap().school, MagicSchool::Physical);
        assert_eq!(
            arena.limit_category,
            Some(LimitCategory {
                id: 718,
                name: "Arena Master".into(),
                quantity: 1
            })
        );
        assert_eq!(arena.effects[1].trigger, EffectTrigger::Use);
        assert_eq!(arena.effects[1].category, Some(1141));
        assert_eq!(arena.effects[1].charges, -1);
        assert_eq!(lens.set, Some(209));
        assert_eq!(lens.flavour_text, "Look through it.");
        assert_eq!(lens.damage.unwrap().school, MagicSchool::Arcane);
        assert_eq!(lens.suffixes[0].name, "of Magic");
        assert_eq!(lens.suffixes[0].stats[&ItemStat::SpellDamage], 28.0);
    }

    #[test]
    fn item_file_round_trips_through_yaml() {
        let file: ItemFile = serde_yaml::from_str(&with_build(GENERATED)).unwrap();
        let yaml = serde_yaml::to_string(&file).unwrap();
        assert!(!yaml.contains("charges: 0"), "{yaml}");
        assert!(!yaml.contains("school: physical"), "{yaml}");
        let again: ItemFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(file, again);
    }
}
