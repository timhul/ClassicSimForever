//! Item set definitions: `data/item_sets.yaml`, exported from the `ItemSet` / `ItemSetSpell`
//! tables. Port of the data half of `Equipment/SetBonusControl.*`.
//!
//! A bonus is a spell that applies while at least `pieces` members of the set are worn; the
//! character applies it ([`crate::character::context::CharacterContext::sync_equipment_spells`]).
//! This module only holds the data and the item → set lookup.

use std::collections::HashMap;

use super::spec::ItemSetSpec;

/// Errors in the item set data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetBonusError {
    #[error("item {item_id} is part of two sets: '{first}' and '{second}'")]
    ItemInTwoSets {
        item_id: u32,
        first: String,
        second: String,
    },
    #[error("item set {id} is defined twice")]
    DuplicateSet { id: u32 },
}

/// All item sets with an item → set index.
#[derive(Debug, Clone, Default)]
pub struct SetBonusDb {
    sets: Vec<ItemSetSpec>,
    by_id: HashMap<u32, usize>,
    item_to_set: HashMap<u32, usize>,
}

impl SetBonusDb {
    /// Indexes the sets: no set id twice, no item in two sets.
    pub fn new(sets: Vec<ItemSetSpec>) -> Result<Self, SetBonusError> {
        let mut by_id = HashMap::new();
        let mut item_to_set = HashMap::new();
        for (index, set) in sets.iter().enumerate() {
            if by_id.insert(set.id, index).is_some() {
                return Err(SetBonusError::DuplicateSet { id: set.id });
            }
            for &item_id in &set.items {
                if let Some(&first) = item_to_set.get(&item_id) {
                    let first: &ItemSetSpec = &sets[first];
                    return Err(SetBonusError::ItemInTwoSets {
                        item_id,
                        first: first.name.clone(),
                        second: set.name.clone(),
                    });
                }
                item_to_set.insert(item_id, index);
            }
        }
        Ok(Self {
            sets,
            by_id,
            item_to_set,
        })
    }

    pub fn sets(&self) -> &[ItemSetSpec] {
        &self.sets
    }

    pub fn set(&self, id: u32) -> Option<&ItemSetSpec> {
        self.by_id.get(&id).map(|&index| &self.sets[index])
    }

    pub fn is_set_item(&self, item_id: u32) -> bool {
        self.item_to_set.contains_key(&item_id)
    }

    /// The set an item belongs to, if any.
    pub fn set_for_item(&self, item_id: u32) -> Option<&ItemSetSpec> {
        self.item_to_set
            .get(&item_id)
            .map(|&index| &self.sets[index])
    }

    pub fn set_by_name(&self, name: &str) -> Option<&ItemSetSpec> {
        self.sets.iter().find(|set| set.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemSetFile;

    const SETS: &str = r#"
build: 1.60.1.70009
sets:
- id: 1
  name: The Gladiator
  items: [11729, 11726, 11728, 11731, 11730]
  bonuses:
  - pieces: 2
    spell: 13669
  - pieces: 4
    spell: 9331
- id: 496
  name: Conqueror's Battlegear
  items: [21331, 21329]
  bonuses:
  - pieces: 3
    spell: 26114
"#;

    #[test]
    fn indexes_sets_by_id_and_member() {
        let file: ItemSetFile = serde_yaml::from_str(SETS).unwrap();
        let db = SetBonusDb::new(file.sets).unwrap();

        assert_eq!(db.sets().len(), 2);
        assert!(db.is_set_item(11729));
        assert!(!db.is_set_item(1));
        assert_eq!(
            db.set_for_item(21329).unwrap().name,
            "Conqueror's Battlegear"
        );
        assert_eq!(db.set_for_item(1), None);
        assert_eq!(db.set(1).unwrap().bonuses[1].spell, 9331);
        assert_eq!(db.set(2), None);
        assert_eq!(db.set_by_name("The Gladiator").unwrap().id, 1);
        assert_eq!(db.set_by_name("Nope"), None);
    }

    fn set(id: u32, name: &str, items: Vec<u32>) -> ItemSetSpec {
        ItemSetSpec {
            id,
            name: name.into(),
            items,
            bonuses: Vec::new(),
        }
    }

    #[test]
    fn item_in_two_sets_is_an_error() {
        let sets = vec![set(1, "A", vec![1, 2]), set(2, "B", vec![2])];
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
    fn set_defined_twice_is_an_error() {
        let sets = vec![set(1, "A", vec![1]), set(1, "A", vec![2])];
        assert_eq!(
            SetBonusDb::new(sets).unwrap_err(),
            SetBonusError::DuplicateSet { id: 1 }
        );
    }
}
