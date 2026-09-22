//! Serde schema of the item database files (`data/items/*.yaml`).
//!
//! The schema follows the C++ XML item files (`Equipment/EquipmentDb/**/*.xml`) with the
//! attributes turned into typed fields. Procs and uses are kept as data (a generic name plus its
//! parameters); the spell system decides what to build from them when the item is equipped.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::faction::{Faction, PlayerClass};
use crate::phase::Phase;
use crate::proc::ProcSource;

use super::types::{EquipmentSlot, ItemSlot, ItemStat, ItemType, Quality};

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
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub req_lvl: u32,
    #[serde(default)]
    pub item_lvl: u32,
    #[serde(default)]
    pub boe: bool,
    #[serde(default)]
    pub icon: String,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub procs: Vec<ItemProcSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<ItemUseSpec>,
    /// Names of spells/buffs this item modifies when equipped (e.g. `Hamstring`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifies: Vec<String>,
    /// Items that cannot be equipped together with this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mutex: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub random_affixes: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub special_equip_effects: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
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
}

/// A proc granted by an item. `name` selects the generic proc (`EXTRA_ATTACK`,
/// `GENERIC_STAT_BUFF`, `FIRE_ATTACK`, ...); the remaining fields are its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemProcSpec {
    pub name: String,
    /// The client-table spell that implements the proc: a passive whose `ProcTypeMask` and
    /// `ProcChance` describe the trigger and whose aura casts the payload (Windfury Totem's
    /// 10612). The character registers a proc per equipped item or enchant that names one
    /// ([`crate::character::context::CharacterContext::sync_equipment_procs`]); a proc without
    /// a spell is data the engine cannot run yet and stays unregistered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spell: Option<u32>,
    /// Proc chance as a fraction, or procs per minute when `ppm` is set. The record of `spell`
    /// decides for a proc that names one.
    pub rate: f64,
    #[serde(default)]
    pub ppm: bool,
    #[serde(default)]
    pub internal_cd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub innate_threat: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instant: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_stacks: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The stat a `GENERIC_STAT_BUFF` proc changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat: Option<ItemStat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spell_dmg_coefficient: Option<f64>,
    /// Damage over time dealt by the proc in addition to its direct damage (Instant Fireball).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dmg_over_duration: Option<u32>,
    /// Which attacks can trigger the proc. Empty means the defaults for the equipment slot.
    #[serde(default, skip_serializing_if = "ProcSourceFlags::is_empty")]
    pub sources: ProcSourceFlags,
}

/// Explicit proc triggers of an item proc. Port of the `proc_*` attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcSourceFlags {
    pub magic_hit: bool,
    pub melee_auto: bool,
    pub melee_skill: bool,
    /// Only the swings/spells of the hand the item is wielded in.
    pub melee_weapon_side: bool,
    pub ranged_auto: bool,
    pub ranged_skill: bool,
}

impl ProcSourceFlags {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The sources these flags name for an item worn in `slot`. Port of
    /// `Item::add_proc_sources_from_map`.
    pub fn sources(&self, slot: EquipmentSlot) -> Vec<ProcSource> {
        let mut sources = Vec::new();
        let mut push = |source: ProcSource| {
            if !sources.contains(&source) {
                sources.push(source);
            }
        };
        if self.magic_hit {
            push(ProcSource::MagicSpell);
        }
        if self.ranged_auto {
            push(ProcSource::RangedAutoShot);
        }
        if self.ranged_skill {
            push(ProcSource::RangedSpell);
        }
        if self.melee_auto {
            push(ProcSource::MainhandSwing);
            push(ProcSource::OffhandSwing);
        }
        if self.melee_skill {
            push(ProcSource::MainhandSpell);
        }
        if self.melee_weapon_side {
            for source in slot.default_proc_sources() {
                push(source);
            }
        }
        sources
    }
}

/// An on-use effect of an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemUseSpec {
    pub name: String,
    #[serde(default)]
    pub cooldown: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat: Option<ItemStat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
icon: Inv_sword_36.png
damage: { min: 66, max: 124, speed: 2.7 }
mutex: [17743, 17753]
procs:
  - name: EXTRA_ATTACK
    amount: 1
    instant: false
    rate: 0.045
    internal_cd: 0
special_equip_effects:
  - "Chance on hit: Grants an extra attack on your next swing."
source: Quest reward. Requires killing Princess Theradras in Maraudon.
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
                speed: 2.7
            })
        );
        assert_eq!(item.mutex, vec![17743, 17753]);
        assert_eq!(item.procs.len(), 1);
        assert_eq!(item.procs[0].name, "EXTRA_ATTACK");
        assert_eq!(item.procs[0].amount, Some(1));
        assert_eq!(item.procs[0].instant, Some(false));
        assert!(item.procs[0].sources.is_empty());
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
icon: helm.png
faction: HORDE
class_restrictions: [WARRIOR, PALADIN]
stats:
  STRENGTH: 20
  CRIT_CHANCE: 0.01
  ARMOR: 500
uses:
  - name: GENERIC_STAT_BUFF
    stat: ATTACK_POWER
    value: 200
    duration: 20
    cooldown: 120
procs:
  - name: GENERIC_STAT_BUFF
    stat: ATTACK_SPEED
    amount: 30
    rate: 0.025
    duration: 5
    sources: { melee_weapon_side: true }
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
        assert_eq!(item.uses[0].stat, Some(ItemStat::AttackPower));
        assert_eq!(item.uses[0].cooldown, 120);
        assert!(item.procs[0].sources.melee_weapon_side);
        assert!(!item.procs[0].sources.magic_hit);
        assert!(!item.unique);
        assert!(item.damage.is_none());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let yaml = "id: 1\nname: x\nphase: 1\nslot: HEAD\ntype: PLATE\nquality: RARE\nbogus: 1\n";
        assert!(serde_yaml::from_str::<ItemSpec>(yaml).is_err());
    }

    #[test]
    fn round_trips_through_yaml() {
        let item: ItemSpec = serde_yaml::from_str(THRASH_BLADE).unwrap();
        let yaml = serde_yaml::to_string(&item).unwrap();
        let again: ItemSpec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(item, again);
    }
}
