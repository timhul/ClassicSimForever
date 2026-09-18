//! Item set definitions. Port of `Equipment/EquipmentDb/SetBonusFileReader.*` and the data half
//! of `Equipment/SetBonusControl.*` (`data/set_bonuses.yaml`).
//!
//! Applying the bonuses to a character is done by the equipment (Phase 2.5); this module only
//! holds the data and the item → set lookup.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::ItemStat;
use crate::stats::{Stats, UnsupportedItemStat};

/// One item set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSpec {
    pub name: String,
    /// Item ids that count towards the set.
    pub items: Vec<u32>,
    #[serde(default)]
    pub bonuses: Vec<SetBonusSpec>,
}

/// A bonus granted when `pieces` items of the set are equipped.
///
/// Only stat bonuses are modelled as data; bonuses that change spells are matched by set name
/// and piece count from the spell data (`modified_by_set`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetBonusSpec {
    pub pieces: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Stat granted by the bonus, with data-file value semantics (chances as fractions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat: Option<ItemStat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

impl SetBonusSpec {
    /// The stat bonus as `(stat, value)` when the bonus grants one.
    pub fn stat_bonus(&self) -> Option<(ItemStat, f64)> {
        Some((self.stat?, self.value?))
    }
}

/// Errors in the set bonus data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetBonusError {
    #[error("item {item_id} is part of two sets: '{first}' and '{second}'")]
    ItemInTwoSets {
        item_id: u32,
        first: String,
        second: String,
    },
    #[error("set '{set}' ({pieces} pieces): {source}")]
    UnsupportedStat {
        set: String,
        pieces: u32,
        #[source]
        source: UnsupportedItemStat,
    },
}

/// All item sets with an item → set index.
#[derive(Debug, Clone, Default)]
pub struct SetBonusDb {
    sets: Vec<SetSpec>,
    item_to_set: HashMap<u32, usize>,
}

impl SetBonusDb {
    /// Validates the sets: no item in two sets, and only static stat bonuses.
    pub fn new(sets: Vec<SetSpec>) -> Result<Self, SetBonusError> {
        let mut item_to_set = HashMap::new();
        for (index, set) in sets.iter().enumerate() {
            for &item_id in &set.items {
                if let Some(&first) = item_to_set.get(&item_id) {
                    let first: &SetSpec = &sets[first];
                    return Err(SetBonusError::ItemInTwoSets {
                        item_id,
                        first: first.name.clone(),
                        second: set.name.clone(),
                    });
                }
                item_to_set.insert(item_id, index);
            }

            for bonus in &set.bonuses {
                if let Some((stat, value)) = bonus.stat_bonus() {
                    Stats::new()
                        .apply_item_stat(stat, value)
                        .map_err(|source| SetBonusError::UnsupportedStat {
                            set: set.name.clone(),
                            pieces: bonus.pieces,
                            source,
                        })?;
                }
            }
        }
        Ok(Self { sets, item_to_set })
    }

    pub fn sets(&self) -> &[SetSpec] {
        &self.sets
    }

    pub fn is_set_item(&self, item_id: u32) -> bool {
        self.item_to_set.contains_key(&item_id)
    }

    /// The set an item belongs to, if any.
    pub fn set_for_item(&self, item_id: u32) -> Option<&SetSpec> {
        self.item_to_set
            .get(&item_id)
            .map(|&index| &self.sets[index])
    }

    pub fn set_by_name(&self, name: &str) -> Option<&SetSpec> {
        self.sets.iter().find(|set| set.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETS: &str = r#"
- name: The Gladiator
  items: [11729, 11726, 11728, 11731, 11730]
  bonuses:
    - pieces: 2
      description: "(2) Set: +20 Armor."
    - pieces: 4
      description: "(4) Set: +10 Attack Power."
      stat: ATTACK_POWER
      value: 10
    - pieces: 5
      description: "(5) Set: Increases your chance to get a critical strike by 1%."
      stat: CRIT_CHANCE
      value: 0.01
- name: Conqueror's Battlegear
  items: [21331, 21329]
  bonuses:
    - pieces: 3
      description: "(3) Set: Decreases the rage cost of all Warrior shouts by 35%."
"#;

    #[test]
    fn parses_sets_and_indexes_items() {
        let sets: Vec<SetSpec> = serde_yaml::from_str(SETS).unwrap();
        let db = SetBonusDb::new(sets).unwrap();

        assert_eq!(db.sets().len(), 2);
        assert!(db.is_set_item(11729));
        assert!(!db.is_set_item(1));
        assert_eq!(
            db.set_for_item(21329).unwrap().name,
            "Conqueror's Battlegear"
        );
        assert_eq!(db.set_for_item(1), None);

        let gladiator = db.set_by_name("The Gladiator").unwrap();
        assert_eq!(gladiator.bonuses[0].stat_bonus(), None);
        assert_eq!(
            gladiator.bonuses[1].stat_bonus(),
            Some((ItemStat::AttackPower, 10.0))
        );
        assert_eq!(
            gladiator.bonuses[2].stat_bonus(),
            Some((ItemStat::CritChance, 0.01))
        );
        assert_eq!(db.set_by_name("Nope"), None);
    }

    #[test]
    fn item_in_two_sets_is_an_error() {
        let sets = vec![
            SetSpec {
                name: "A".into(),
                items: vec![1, 2],
                bonuses: Vec::new(),
            },
            SetSpec {
                name: "B".into(),
                items: vec![2],
                bonuses: Vec::new(),
            },
        ];
        assert_eq!(
            SetBonusDb::new(sets).unwrap_err(),
            SetBonusError::ItemInTwoSets {
                item_id: 2,
                first: "A".into(),
                second: "B".into()
            }
        );
    }

    #[test]
    fn dynamic_stat_bonus_is_an_error() {
        let sets = vec![SetSpec {
            name: "A".into(),
            items: vec![1],
            bonuses: vec![SetBonusSpec {
                pieces: 2,
                description: String::new(),
                stat: Some(ItemStat::AttackSpeed),
                value: Some(10.0),
            }],
        }];
        assert!(matches!(
            SetBonusDb::new(sets).unwrap_err(),
            SetBonusError::UnsupportedStat { pieces: 2, .. }
        ));
    }
}
