//! Equipped items of a character. Port of `Equipment/Equipment.*` and the bookkeeping half of
//! `Equipment/SetBonusControl.*`.
//!
//! The equipment owns the item slots, three saved setups, the aggregated [`Stats`] of everything
//! worn (item stats plus active set bonus stats) and the enchant selection per slot. It does not
//! know about the character: procs, on-use spells and spell modifications granted by items are
//! reported through [`EquipChange`] so the owning character can create or remove them.

use std::collections::HashMap;
use std::sync::Arc;

use crate::enchant::{EnchantContext, EnchantName, EnchantSpec};
use crate::faction::{Faction, PlayerClass};
use crate::item::{EquipmentDb, EquipmentSlot, Item, ItemStat, Weapon, WeaponType};
use crate::phase::Phase;
use crate::stats::{Stats, WeaponProfile};

/// Number of saved equipment setups.
pub const SETUP_COUNT: usize = 3;

/// Why an item could not be equipped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EquipError {
    #[error("item {item_id} is not available in the current phase")]
    UnknownItem { item_id: u32 },
    #[error("item {item_id} ({name}) does not fit slot {slot:?}")]
    WrongSlot {
        item_id: u32,
        name: String,
        slot: EquipmentSlot,
    },
    #[error("setup index {0} is out of range")]
    InvalidSetup(usize),
    #[error("item {item_id} ({name}): at most {quantity} {category} item(s) can be equipped")]
    LimitCategory {
        item_id: u32,
        name: String,
        category: String,
        quantity: u32,
    },
}

/// Why an enchant could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnchantError {
    #[error("enchant {0:?} is not in the enchant database")]
    UnknownEnchant(EnchantName),
    #[error("enchant {enchant:?} cannot be applied to the item in slot {slot:?}")]
    NotApplicable {
        enchant: EnchantName,
        slot: EquipmentSlot,
    },
    #[error("enchant {enchant:?} is a {} enchant", if *temporary { "temporary" } else { "permanent" })]
    WrongKind {
        enchant: EnchantName,
        temporary: bool,
    },
    #[error("slot {0:?} is empty")]
    EmptySlot(EquipmentSlot),
}

/// What an equipment operation changed, so the owner can update item procs/uses/modifications.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EquipChange {
    /// Items removed from their slots.
    pub unequipped: Vec<(EquipmentSlot, Arc<Item>)>,
    /// Items put into slots.
    pub equipped: Vec<(EquipmentSlot, Arc<Item>)>,
}

impl EquipChange {
    fn merge(&mut self, other: EquipChange) {
        self.unequipped.extend(other.unequipped);
        self.equipped.extend(other.equipped);
    }
}

/// An item in a slot together with its per-instance state.
#[derive(Debug, Clone)]
pub struct EquippedItem {
    item: Arc<Item>,
    weapon: Option<Weapon>,
    enchant: Option<EnchantName>,
    temp_enchant: Option<EnchantName>,
}

impl EquippedItem {
    pub fn item(&self) -> &Arc<Item> {
        &self.item
    }

    pub fn weapon(&self) -> Option<&Weapon> {
        self.weapon.as_ref()
    }

    pub fn enchant(&self) -> Option<EnchantName> {
        self.enchant
    }

    pub fn temp_enchant(&self) -> Option<EnchantName> {
        self.temp_enchant
    }
}

/// A saved set of item ids and enchants per slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Setup {
    pub items: [Option<u32>; EquipmentSlot::COUNT],
    pub enchants: [Option<EnchantName>; EquipmentSlot::COUNT],
    pub temp_enchants: [Option<EnchantName>; EquipmentSlot::COUNT],
}

/// The set bonuses currently active.
#[derive(Debug, Clone, Default)]
struct SetBonusState {
    /// Equipped item ids per set name.
    equipped_pieces: HashMap<String, Vec<u32>>,
}

/// Everything a character wears.
#[derive(Debug, Clone)]
pub struct Equipment {
    db: Arc<EquipmentDb>,
    phase: Phase,
    faction: Faction,
    class: PlayerClass,
    setup_index: usize,
    setups: [Setup; SETUP_COUNT],
    slots: [Option<EquippedItem>; EquipmentSlot::COUNT],
    stats: Stats,
    set_bonuses: SetBonusState,
}

impl Equipment {
    pub fn new(db: Arc<EquipmentDb>, phase: Phase, faction: Faction, class: PlayerClass) -> Self {
        Self {
            db,
            phase,
            faction,
            class,
            setup_index: 0,
            setups: Default::default(),
            slots: Default::default(),
            stats: Stats::new(),
            set_bonuses: SetBonusState::default(),
        }
    }

    pub fn db(&self) -> &Arc<EquipmentDb> {
        &self.db
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn faction(&self) -> Faction {
        self.faction
    }

    /// Changes the faction; enchants that are no longer valid are dropped. Items of the other
    /// faction are removed separately with [`Equipment::clear_items_not_available_for_faction`].
    pub fn set_faction(&mut self, faction: Faction) {
        self.faction = faction;
        self.revalidate_enchants();
    }

    pub fn class(&self) -> PlayerClass {
        self.class
    }

    /// Changes the content phase and re-equips the current setup; items not available in that
    /// phase are dropped.
    pub fn set_phase(&mut self, phase: Phase) -> EquipChange {
        self.phase = phase;
        self.reequip_items()
    }

    /// Aggregated stats of the equipped items and active set bonuses.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn setup_index(&self) -> usize {
        self.setup_index
    }

    pub fn setup(&self, index: usize) -> Option<&Setup> {
        self.setups.get(index)
    }

    // ---------------------------------------------------------------- slot access

    pub fn slot(&self, slot: EquipmentSlot) -> Option<&EquippedItem> {
        self.slots[slot.index()].as_ref()
    }

    pub fn item(&self, slot: EquipmentSlot) -> Option<&Arc<Item>> {
        self.slot(slot).map(EquippedItem::item)
    }

    pub fn item_id(&self, slot: EquipmentSlot) -> Option<u32> {
        self.item(slot).map(|item| item.id())
    }

    fn weapon(&self, slot: EquipmentSlot) -> Option<&Weapon> {
        self.slot(slot).and_then(EquippedItem::weapon)
    }

    fn weapon_mut(&mut self, slot: EquipmentSlot) -> Option<&mut Weapon> {
        self.slots[slot.index()]
            .as_mut()
            .and_then(|equipped| equipped.weapon.as_mut())
    }

    pub fn mainhand(&self) -> Option<&Weapon> {
        self.weapon(EquipmentSlot::Mainhand)
    }

    pub fn mainhand_mut(&mut self) -> Option<&mut Weapon> {
        self.weapon_mut(EquipmentSlot::Mainhand)
    }

    pub fn offhand(&self) -> Option<&Weapon> {
        self.weapon(EquipmentSlot::Offhand)
    }

    pub fn offhand_mut(&mut self) -> Option<&mut Weapon> {
        self.weapon_mut(EquipmentSlot::Offhand)
    }

    pub fn ranged(&self) -> Option<&Weapon> {
        self.weapon(EquipmentSlot::Ranged)
    }

    pub fn ranged_mut(&mut self) -> Option<&mut Weapon> {
        self.weapon_mut(EquipmentSlot::Ranged)
    }

    pub fn weapon_profile(&self, slot: EquipmentSlot) -> Option<WeaponProfile> {
        self.weapon(slot).map(Weapon::profile)
    }

    /// Whether an offhand weapon (not a shield or caster off-hand) is equipped.
    pub fn is_dual_wielding(&self) -> bool {
        self.offhand().is_some_and(|offhand| {
            !matches!(
                offhand.weapon_type(),
                WeaponType::CasterOffhand | WeaponType::Shield
            )
        })
    }

    pub fn has_two_hand_weapon(&self) -> bool {
        self.mainhand().is_some_and(Weapon::is_two_hand)
    }

    pub fn has_shield(&self) -> bool {
        self.offhand()
            .is_some_and(|offhand| offhand.weapon_type() == WeaponType::Shield)
    }

    /// Every equipped item with its slot, in slot order.
    pub fn equipped_items(&self) -> impl Iterator<Item = (EquipmentSlot, &Arc<Item>)> + '_ {
        EquipmentSlot::ALL
            .into_iter()
            .filter_map(|slot| self.item(slot).map(|item| (slot, item)))
    }

    pub fn is_item_equipped(&self, item_id: u32) -> bool {
        self.equipped_items().any(|(_, item)| item.id() == item_id)
    }

    /// Re-seeds the damage roll generators of the equipped weapons.
    pub fn set_seed(&mut self, seed: u64) {
        for (offset, slot) in [
            EquipmentSlot::Mainhand,
            EquipmentSlot::Offhand,
            EquipmentSlot::Ranged,
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(weapon) = self.weapon_mut(slot) {
                weapon.set_seed(seed.wrapping_add(offset as u64));
            }
        }
    }

    // ---------------------------------------------------------------- equipping

    /// Equips the item with `item_id` (as available in the current phase) in `slot`.
    ///
    /// Handles the interactions the C++ setters had: a two-hand weapon clears the offhand and vice
    /// versa, ranged weapons and relics exclude each other, unique items already worn in the
    /// paired slot (rings, trinkets, weapons) are moved out of the way, and items listed as
    /// mutually exclusive with the new item are removed. An item whose unique-equipped group
    /// (limit category) is already full with the items that stay equipped is refused.
    pub fn equip(&mut self, slot: EquipmentSlot, item_id: u32) -> Result<EquipChange, EquipError> {
        let item = self
            .db
            .get_item(item_id, self.phase)
            .cloned()
            .ok_or(EquipError::UnknownItem { item_id })?;
        self.equip_item(slot, item)
    }

    /// Equips an item that need not be in the database (used by tests and template weapons).
    pub fn equip_item(
        &mut self,
        slot: EquipmentSlot,
        item: Arc<Item>,
    ) -> Result<EquipChange, EquipError> {
        if !item.fits(slot) {
            return Err(EquipError::WrongSlot {
                item_id: item.id(),
                name: item.name().to_string(),
                slot,
            });
        }

        if let Some(category) = item.limit_category() {
            let vacated = self.slots_vacated_by(slot, &item);
            let worn = EquipmentSlot::ALL
                .into_iter()
                .filter(|other| !vacated.contains(other))
                .filter_map(|other| self.item(other))
                .filter(|other| other.limit_category().is_some_and(|c| c.id == category.id))
                .count();
            if worn >= category.quantity as usize {
                return Err(EquipError::LimitCategory {
                    item_id: item.id(),
                    name: item.name().to_string(),
                    category: category.name.clone(),
                    quantity: category.quantity,
                });
            }
        }

        let mut change = EquipChange::default();

        match slot {
            EquipmentSlot::Mainhand => {
                if item.is_two_hand() {
                    change.merge(self.unequip(EquipmentSlot::Offhand));
                } else if item.is_unique()
                    && self.item_id(EquipmentSlot::Offhand) == Some(item.id())
                {
                    change.merge(self.move_paired(EquipmentSlot::Offhand, EquipmentSlot::Mainhand));
                }
            }
            EquipmentSlot::Offhand => {
                let mainhand_conflicts = self.has_two_hand_weapon()
                    || (item.is_unique()
                        && self.item_id(EquipmentSlot::Mainhand) == Some(item.id()));
                if mainhand_conflicts {
                    change.merge(self.unequip(EquipmentSlot::Mainhand));
                }
            }
            EquipmentSlot::Ranged => change.merge(self.unequip(EquipmentSlot::Relic)),
            EquipmentSlot::Relic => change.merge(self.unequip(EquipmentSlot::Ranged)),
            EquipmentSlot::Ring1
            | EquipmentSlot::Ring2
            | EquipmentSlot::Trinket1
            | EquipmentSlot::Trinket2 => {
                let other = paired_slot(slot);
                if item.is_unique() && self.item_id(other) == Some(item.id()) {
                    change.merge(self.move_paired(other, slot));
                }
            }
            _ => {}
        }

        for &mutex_id in item.mutex_item_ids() {
            change.merge(self.unequip_item_id(mutex_id));
        }

        let previous = self.slots[slot.index()].take();
        let (enchant, temp_enchant) = previous
            .as_ref()
            .map(|equipped| (equipped.enchant, equipped.temp_enchant))
            .unwrap_or((None, None));
        if let Some(previous) = previous {
            change.merge(self.remove_from_slot(slot, previous));
        }

        self.stats.add(item.stats());
        self.equip_set_piece(item.id());
        self.setups[self.setup_index].items[slot.index()] = Some(item.id());

        let weapon = Weapon::new(item.clone());
        self.slots[slot.index()] = Some(EquippedItem {
            item: item.clone(),
            weapon,
            enchant: None,
            temp_enchant: None,
        });
        change.equipped.push((slot, item));

        // Enchants stay with the slot when the item is swapped, as long as they still apply.
        let _ = self.set_enchant(slot, enchant);
        let _ = self.set_temp_enchant(slot, temp_enchant);

        Ok(change)
    }

    /// The slots whose items are no longer worn after equipping `item` in `slot`, following the
    /// rules of [`Self::equip_item`]. When a unique item is equipped into the pair of a slot that
    /// holds it, the item in `slot` moves over and stays worn.
    fn slots_vacated_by(&self, slot: EquipmentSlot, item: &Item) -> Vec<EquipmentSlot> {
        let holds_item = |other| self.item_id(other) == Some(item.id());
        let mut vacated = Vec::new();
        match slot {
            EquipmentSlot::Mainhand => {
                if item.is_two_hand() {
                    vacated.extend([slot, EquipmentSlot::Offhand]);
                } else if item.is_unique() && holds_item(EquipmentSlot::Offhand) {
                    vacated.push(EquipmentSlot::Offhand);
                } else {
                    vacated.push(slot);
                }
            }
            EquipmentSlot::Offhand => {
                vacated.push(slot);
                if self.has_two_hand_weapon()
                    || (item.is_unique() && holds_item(EquipmentSlot::Mainhand))
                {
                    vacated.push(EquipmentSlot::Mainhand);
                }
            }
            EquipmentSlot::Ranged => vacated.extend([slot, EquipmentSlot::Relic]),
            EquipmentSlot::Relic => vacated.extend([slot, EquipmentSlot::Ranged]),
            EquipmentSlot::Ring1
            | EquipmentSlot::Ring2
            | EquipmentSlot::Trinket1
            | EquipmentSlot::Trinket2 => {
                let other = paired_slot(slot);
                if item.is_unique() && holds_item(other) {
                    vacated.push(other);
                } else {
                    vacated.push(slot);
                }
            }
            _ => vacated.push(slot),
        }
        for &mutex_id in item.mutex_item_ids() {
            vacated.extend(
                EquipmentSlot::ALL
                    .into_iter()
                    .filter(|&other| self.item_id(other) == Some(mutex_id)),
            );
        }
        vacated
    }

    /// Moves the item in `from` to `to` (after clearing `to`), as the C++ setters did for unique
    /// rings/trinkets equipped into the other slot.
    fn move_paired(&mut self, from: EquipmentSlot, to: EquipmentSlot) -> EquipChange {
        let mut change = self.unequip(from);
        if let Some(item) = self.item(to).cloned() {
            change.merge(self.unequip(to));
            if let Ok(moved) = self.equip_item(from, item) {
                change.merge(moved);
            }
        }
        change
    }

    /// Removes the item in `slot`, if any.
    pub fn unequip(&mut self, slot: EquipmentSlot) -> EquipChange {
        match self.slots[slot.index()].take() {
            Some(equipped) => self.remove_from_slot(slot, equipped),
            None => EquipChange::default(),
        }
    }

    fn remove_from_slot(&mut self, slot: EquipmentSlot, equipped: EquippedItem) -> EquipChange {
        for enchant in [equipped.enchant, equipped.temp_enchant]
            .into_iter()
            .flatten()
        {
            self.remove_enchant_stats(slot, enchant);
        }
        self.stats.remove(equipped.item.stats());
        self.unequip_set_piece(equipped.item.id());
        let setup = &mut self.setups[self.setup_index];
        setup.items[slot.index()] = None;
        setup.enchants[slot.index()] = None;
        setup.temp_enchants[slot.index()] = None;

        EquipChange {
            unequipped: vec![(slot, equipped.item)],
            equipped: Vec::new(),
        }
    }

    /// Removes `item_id` from every slot it is equipped in.
    pub fn unequip_item_id(&mut self, item_id: u32) -> EquipChange {
        let mut change = EquipChange::default();
        for slot in EquipmentSlot::ALL {
            if self.item_id(slot) == Some(item_id) {
                change.merge(self.unequip(slot));
            }
        }
        change
    }

    pub fn unequip_all(&mut self) -> EquipChange {
        let mut change = EquipChange::default();
        for slot in EquipmentSlot::ALL {
            change.merge(self.unequip(slot));
        }
        change
    }

    /// Removes items the character's faction cannot use.
    pub fn clear_items_not_available_for_faction(&mut self, faction: Faction) -> EquipChange {
        let mut change = EquipChange::default();
        for slot in EquipmentSlot::ALL {
            if self
                .item(slot)
                .is_some_and(|item| !item.available_for_faction(faction))
            {
                change.merge(self.unequip(slot));
            }
        }
        change
    }

    /// Re-equips the stored setup in the current phase. Items that are not available in it are
    /// dropped from the setup.
    pub fn reequip_items(&mut self) -> EquipChange {
        let stored = self.setups[self.setup_index].clone();
        let mut change = self.unequip_all();

        for slot in EquipmentSlot::ALL {
            let Some(item_id) = stored.items[slot.index()] else {
                continue;
            };
            if let Ok(equipped) = self.equip(slot, item_id) {
                change.merge(equipped);
                let _ = self.set_enchant(slot, stored.enchants[slot.index()]);
                let _ = self.set_temp_enchant(slot, stored.temp_enchants[slot.index()]);
            }
        }

        change
    }

    /// Switches to another saved setup.
    pub fn change_setup(&mut self, index: usize) -> Result<EquipChange, EquipError> {
        if index >= SETUP_COUNT {
            return Err(EquipError::InvalidSetup(index));
        }

        let preserved = self.setups[self.setup_index].clone();
        let mut change = self.unequip_all();
        self.setups[self.setup_index] = preserved;
        self.setup_index = index;
        change.merge(self.reequip_items());
        Ok(change)
    }

    // ---------------------------------------------------------------- enchants

    /// Selects the permanent enchant of the item in `slot` (`None` removes it). The enchant must
    /// exist and apply to the item, weapon, faction and class; its static stats are added to the
    /// equipment stats. Procs and dynamic stats are left to the character (see
    /// [`Equipment::active_enchants`]).
    pub fn set_enchant(
        &mut self,
        slot: EquipmentSlot,
        enchant: Option<EnchantName>,
    ) -> Result<(), EnchantError> {
        self.change_enchant(slot, enchant, false)
    }

    /// Selects the temporary enchant (sharpening stone, oil, Windfury, ...) of the item in `slot`.
    pub fn set_temp_enchant(
        &mut self,
        slot: EquipmentSlot,
        enchant: Option<EnchantName>,
    ) -> Result<(), EnchantError> {
        self.change_enchant(slot, enchant, true)
    }

    fn change_enchant(
        &mut self,
        slot: EquipmentSlot,
        enchant: Option<EnchantName>,
        temporary: bool,
    ) -> Result<(), EnchantError> {
        if self.slots[slot.index()].is_none() {
            return Err(EnchantError::EmptySlot(slot));
        }

        if let Some(name) = enchant {
            let spec = self
                .db
                .enchants()
                .get(name)
                .ok_or(EnchantError::UnknownEnchant(name))?;
            if spec.temporary != temporary {
                return Err(EnchantError::WrongKind {
                    enchant: name,
                    temporary: spec.temporary,
                });
            }
            if !spec.valid_for(&self.enchant_context(slot)) {
                return Err(EnchantError::NotApplicable {
                    enchant: name,
                    slot,
                });
            }
        }

        let current = if temporary {
            self.temp_enchant(slot)
        } else {
            self.enchant(slot)
        };
        if let Some(current) = current {
            self.remove_enchant_stats(slot, current);
        }
        if let Some(name) = enchant {
            self.add_enchant_stats(slot, name);
        }

        let equipped = self.slots[slot.index()]
            .as_mut()
            .expect("slot checked above");
        let setup = &mut self.setups[self.setup_index];
        if temporary {
            equipped.temp_enchant = enchant;
            setup.temp_enchants[slot.index()] = enchant;
        } else {
            equipped.enchant = enchant;
            setup.enchants[slot.index()] = enchant;
        }
        Ok(())
    }

    fn enchant_context(&self, slot: EquipmentSlot) -> EnchantContext<'_> {
        EnchantContext {
            slot,
            weapon: self.weapon(slot).map(Weapon::data),
            faction: self.faction,
            class: self.class,
        }
    }

    fn enchant_stats(&self, slot: EquipmentSlot, enchant: EnchantName) -> Stats {
        self.db
            .enchants()
            .get(enchant)
            .expect("enchant validated when applied")
            .static_stats(slot)
            .expect("enchant stats are validated at load")
    }

    fn add_enchant_stats(&mut self, slot: EquipmentSlot, enchant: EnchantName) {
        let stats = self.enchant_stats(slot, enchant);
        self.stats.add(&stats);
    }

    fn remove_enchant_stats(&mut self, slot: EquipmentSlot, enchant: EnchantName) {
        let stats = self.enchant_stats(slot, enchant);
        self.stats.remove(&stats);
    }

    /// Drops enchants that no longer apply (after a faction change).
    fn revalidate_enchants(&mut self) {
        for slot in EquipmentSlot::ALL {
            for temporary in [false, true] {
                let current = if temporary {
                    self.temp_enchant(slot)
                } else {
                    self.enchant(slot)
                };
                let Some(name) = current else { continue };
                let valid = self
                    .db
                    .enchants()
                    .get(name)
                    .is_some_and(|spec| spec.valid_for(&self.enchant_context(slot)));
                if !valid {
                    let _ = self.change_enchant(slot, None, temporary);
                }
            }
        }
    }

    /// Every active enchant with its slot and spec, in slot order (permanent before temporary).
    pub fn active_enchants(&self) -> Vec<(EquipmentSlot, &EnchantSpec)> {
        let mut active = Vec::new();
        for slot in EquipmentSlot::ALL {
            for name in [self.enchant(slot), self.temp_enchant(slot)]
                .into_iter()
                .flatten()
            {
                if let Some(spec) = self.db.enchants().get(name) {
                    active.push((slot, spec));
                }
            }
        }
        active
    }

    pub fn enchant(&self, slot: EquipmentSlot) -> Option<EnchantName> {
        self.slot(slot).and_then(EquippedItem::enchant)
    }

    pub fn temp_enchant(&self, slot: EquipmentSlot) -> Option<EnchantName> {
        self.slot(slot).and_then(EquippedItem::temp_enchant)
    }

    // ---------------------------------------------------------------- set bonuses

    /// Number of equipped pieces of the named set.
    pub fn set_pieces(&self, set_name: &str) -> u32 {
        self.set_bonuses
            .equipped_pieces
            .get(set_name)
            .map_or(0, |pieces| pieces.len() as u32)
    }

    /// `(set name, pieces)` for every set with at least one equipped piece, sorted by name.
    pub fn active_sets(&self) -> Vec<(String, u32)> {
        let mut sets: Vec<(String, u32)> = self
            .set_bonuses
            .equipped_pieces
            .iter()
            .filter(|(_, pieces)| !pieces.is_empty())
            .map(|(name, pieces)| (name.clone(), pieces.len() as u32))
            .collect();
        sets.sort();
        sets
    }

    /// Whether the `pieces`-piece bonus of `set_name` is active.
    pub fn set_bonus_active(&self, set_name: &str, pieces: u32) -> bool {
        self.set_pieces(set_name) >= pieces
    }

    fn equip_set_piece(&mut self, item_id: u32) {
        let Some(set) = self.db.sets().set_for_item(item_id) else {
            return;
        };
        let pieces = self
            .set_bonuses
            .equipped_pieces
            .entry(set.name.clone())
            .or_default();
        pieces.push(item_id);
        let count = pieces.len() as u32;

        if let Some((stat, value)) = set_bonus_stat(set.bonuses.iter(), count) {
            self.stats
                .apply_item_stat(stat, value)
                .expect("set bonus stats are validated at load");
        }
    }

    fn unequip_set_piece(&mut self, item_id: u32) {
        let Some(set) = self.db.sets().set_for_item(item_id) else {
            return;
        };
        let Some(pieces) = self.set_bonuses.equipped_pieces.get_mut(&set.name) else {
            return;
        };
        let count = pieces.len() as u32;
        if let Some(index) = pieces.iter().position(|&id| id == item_id) {
            pieces.remove(index);
        }

        if let Some((stat, value)) = set_bonus_stat(set.bonuses.iter(), count) {
            let mut delta = Stats::new();
            delta
                .apply_item_stat(stat, value)
                .expect("set bonus stats are validated at load");
            self.stats.remove(&delta);
        }
    }
}

fn set_bonus_stat<'a>(
    bonuses: impl Iterator<Item = &'a crate::item::SetBonusSpec>,
    pieces: u32,
) -> Option<(ItemStat, f64)> {
    bonuses
        .filter(|bonus| bonus.pieces == pieces)
        .find_map(|bonus| bonus.stat_bonus())
}

fn paired_slot(slot: EquipmentSlot) -> EquipmentSlot {
    match slot {
        EquipmentSlot::Ring1 => EquipmentSlot::Ring2,
        EquipmentSlot::Ring2 => EquipmentSlot::Ring1,
        EquipmentSlot::Trinket1 => EquipmentSlot::Trinket2,
        EquipmentSlot::Trinket2 => EquipmentSlot::Trinket1,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{
        ItemSlot, ItemSpec, ItemType, LimitCategory, Quality, SetBonusSpec, SetSpec,
        WeaponDamageSpec,
    };
    use crate::magic_school::MagicSchool;

    fn spec(id: u32, slot: ItemSlot, item_type: ItemType) -> ItemSpec {
        ItemSpec {
            id,
            name: format!("Item {id}"),
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
            effects: Vec::new(),
            set: None,
            limit_category: None,
            suffixes: Vec::new(),
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

    fn weapon(id: u32, slot: ItemSlot, item_type: ItemType, speed: f64) -> ItemSpec {
        let mut spec = spec(id, slot, item_type);
        spec.damage = Some(WeaponDamageSpec {
            min: 100,
            max: 100,
            speed,
            school: MagicSchool::Physical,
        });
        spec
    }

    fn db() -> Arc<EquipmentDb> {
        let mut sword = weapon(1, ItemSlot::OneHand, ItemType::Sword, 2.7);
        sword.stats.insert(ItemStat::Strength, 10.0);
        let mut unique_sword = weapon(2, ItemSlot::OneHand, ItemType::Sword, 1.8);
        unique_sword.unique = true;
        let two_hand = weapon(3, ItemSlot::TwoHand, ItemType::TwohandAxe, 3.6);
        let mut shield = weapon(4, ItemSlot::Offhand, ItemType::Shield, 0.0);
        shield.stats.insert(ItemStat::Armor, 2000.0);
        let orb = weapon(5, ItemSlot::Offhand, ItemType::CasterOffhand, 0.0);
        let bow = weapon(6, ItemSlot::Ranged, ItemType::Bow, 2.9);
        let relic = spec(7, ItemSlot::Relic, ItemType::Relic);

        let mut ring = spec(10, ItemSlot::Ring, ItemType::Ring);
        ring.unique = true;
        ring.stats.insert(ItemStat::Agility, 5.0);
        let mut ring2 = spec(11, ItemSlot::Ring, ItemType::Ring);
        ring2.stats.insert(ItemStat::Agility, 7.0);
        let mut trinket = spec(12, ItemSlot::Trinket, ItemType::Trinket);
        trinket.unique = true;
        let mut mutex_a = spec(13, ItemSlot::Trinket, ItemType::Trinket);
        mutex_a.mutex = vec![14];
        let mut mutex_b = spec(14, ItemSlot::Trinket, ItemType::Trinket);
        mutex_b.mutex = vec![13];
        let arena = |id| {
            let mut trinket = spec(id, ItemSlot::Trinket, ItemType::Trinket);
            trinket.limit_category = Some(LimitCategory {
                id: 718,
                name: "Arena Master".into(),
                quantity: 1,
            });
            trinket
        };
        let (arena_a, arena_b) = (arena(15), arena(16));
        let pair_ring = |id| {
            let mut ring = spec(id, ItemSlot::Ring, ItemType::Ring);
            ring.unique = true;
            ring.limit_category = Some(LimitCategory {
                id: 5,
                name: "Pair".into(),
                quantity: 2,
            });
            ring
        };
        let (pair_a, pair_b, pair_c) = (pair_ring(17), pair_ring(18), pair_ring(19));

        let mut horde_helm = spec(20, ItemSlot::Head, ItemType::Plate);
        horde_helm.faction = Some(Faction::Horde);
        horde_helm.stats.insert(ItemStat::Stamina, 20.0);
        let mut naxx_helm = spec(21, ItemSlot::Head, ItemType::Plate);
        naxx_helm.phase = Phase::Naxxramas;
        let mut helm = spec(22, ItemSlot::Head, ItemType::Plate);
        helm.stats.insert(ItemStat::Strength, 10.0);

        let set_chest = spec(30, ItemSlot::Chest, ItemType::Plate);
        let set_legs = spec(31, ItemSlot::Legs, ItemType::Plate);
        let set_boots = spec(32, ItemSlot::Boots, ItemType::Plate);

        Arc::new(
            EquipmentDb::from_specs(
                vec![
                    sword,
                    unique_sword,
                    two_hand,
                    shield,
                    orb,
                    bow,
                    relic,
                    ring,
                    ring2,
                    trinket,
                    mutex_a,
                    mutex_b,
                    arena_a,
                    arena_b,
                    pair_a,
                    pair_b,
                    pair_c,
                    horde_helm,
                    naxx_helm,
                    helm,
                    set_chest,
                    set_legs,
                    set_boots,
                ],
                vec![SetSpec {
                    name: "Set".into(),
                    items: vec![30, 31, 32],
                    bonuses: vec![
                        SetBonusSpec {
                            pieces: 2,
                            description: String::new(),
                            stat: Some(ItemStat::AttackPower),
                            value: Some(40.0),
                        },
                        SetBonusSpec {
                            pieces: 3,
                            description: String::new(),
                            stat: Some(ItemStat::CritChance),
                            value: Some(0.02),
                        },
                    ],
                }],
            )
            .unwrap(),
        )
    }

    fn enchants() -> crate::enchant::EnchantDb {
        let yaml = r#"
- name: Crusader
  display_name: Crusader
  unique_name: Enchant Weapon - Crusader
  slots: [MAINHAND, OFFHAND]
  weapon_slots: [1H, MH, OH, 2H]
  procs:
    - { name: GENERIC_STAT_BUFF, stat: STRENGTH, amount: 100, duration: 15, rate: 1.0, ppm: true }
- name: SuperiorStriking
  display_name: Superior Striking
  unique_name: Enchant Weapon - Superior Striking
  slots: [MAINHAND, OFFHAND]
  weapon_slots: [1H, MH, OH, 2H]
  weapon_damage: 5
- name: IronCounterweight
  display_name: Iron Counterweight
  unique_name: Iron Counterweight
  slots: [MAINHAND]
  weapon_slots: [2H]
  stats: { MELEE_ATTACK_SPEED: 3 }
- name: DenseSharpeningStone
  display_name: Dense Sharpening Stone
  unique_name: Dense Sharpening Stone
  temporary: true
  slots: [MAINHAND, OFFHAND]
  weapon_types: [AXE, TWOHAND_AXE, DAGGER, POLEARM, SWORD, TWOHAND_SWORD]
  weapon_damage: 8
- name: WindfuryTotem
  display_name: Windfury Totem
  unique_name: Windfury Totem
  temporary: true
  slots: [MAINHAND]
  faction: HORDE
  procs:
    - { name: WINDFURY_ATTACK, rate: 0.2, value: 315 }
- name: EnchantChestStats
  display_name: Stats
  unique_name: Enchant Chest - Stats
  slots: [CHEST]
  stats: { STRENGTH: 3, AGILITY: 3, STAMINA: 3, INTELLECT: 3, SPIRIT: 3 }
"#;
        crate::enchant::EnchantDb::new(serde_yaml::from_str(yaml).unwrap()).unwrap()
    }

    fn equipment() -> Equipment {
        let mut db = Arc::try_unwrap(db()).unwrap();
        db.set_enchants(enchants());
        Equipment::new(
            Arc::new(db),
            Phase::Naxxramas,
            Faction::Horde,
            PlayerClass::Warrior,
        )
    }

    fn ids(change: &[(EquipmentSlot, Arc<Item>)]) -> Vec<(EquipmentSlot, u32)> {
        change
            .iter()
            .map(|(slot, item)| (*slot, item.id()))
            .collect()
    }

    #[test]
    fn equipping_adds_stats_and_reports_change() {
        let mut eq = equipment();
        let change = eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        assert_eq!(ids(&change.equipped), vec![(EquipmentSlot::Mainhand, 1)]);
        assert!(change.unequipped.is_empty());
        assert_eq!(eq.stats().get_strength(), 10);
        assert_eq!(eq.mainhand().unwrap().speed(), 2.7);
        assert_eq!(
            eq.weapon_profile(EquipmentSlot::Mainhand)
                .unwrap()
                .weapon_type,
            WeaponType::Sword
        );
        assert!(!eq.is_dual_wielding());

        let change = eq.unequip(EquipmentSlot::Mainhand);
        assert_eq!(ids(&change.unequipped), vec![(EquipmentSlot::Mainhand, 1)]);
        assert_eq!(eq.stats(), &Stats::new());
        assert!(eq.mainhand().is_none());
    }

    #[test]
    fn wrong_slot_and_unknown_items_are_errors() {
        let mut eq = equipment();
        assert_eq!(
            eq.equip(EquipmentSlot::Head, 1).unwrap_err(),
            EquipError::WrongSlot {
                item_id: 1,
                name: "Item 1".into(),
                slot: EquipmentSlot::Head
            }
        );
        assert_eq!(
            eq.equip(EquipmentSlot::Head, 999).unwrap_err(),
            EquipError::UnknownItem { item_id: 999 }
        );
        assert_eq!(
            eq.equip(EquipmentSlot::Offhand, 3).unwrap_err(),
            EquipError::WrongSlot {
                item_id: 3,
                name: "Item 3".into(),
                slot: EquipmentSlot::Offhand
            }
        );
    }

    #[test]
    fn dual_wield_detection() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        assert!(!eq.is_dual_wielding());
        eq.equip(EquipmentSlot::Offhand, 2).unwrap();
        assert!(eq.is_dual_wielding());
        eq.equip(EquipmentSlot::Offhand, 4).unwrap();
        assert!(!eq.is_dual_wielding());
        assert!(eq.has_shield());
        assert_eq!(eq.stats().get_armor(), 2000);
        eq.equip(EquipmentSlot::Offhand, 5).unwrap();
        assert!(!eq.is_dual_wielding());
        assert!(!eq.has_shield());
    }

    #[test]
    fn two_hand_weapons_exclude_offhands() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        eq.equip(EquipmentSlot::Offhand, 2).unwrap();

        let change = eq.equip(EquipmentSlot::Mainhand, 3).unwrap();
        assert_eq!(
            ids(&change.unequipped),
            vec![(EquipmentSlot::Offhand, 2), (EquipmentSlot::Mainhand, 1)]
        );
        assert!(eq.has_two_hand_weapon());
        assert!(eq.offhand().is_none());

        let change = eq.equip(EquipmentSlot::Offhand, 4).unwrap();
        assert_eq!(ids(&change.unequipped), vec![(EquipmentSlot::Mainhand, 3)]);
        assert!(eq.mainhand().is_none());
        assert!(eq.has_shield());
    }

    #[test]
    fn unique_weapons_cannot_be_wielded_twice() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 2).unwrap();
        eq.equip(EquipmentSlot::Offhand, 2).unwrap();
        assert!(eq.mainhand().is_none());
        assert_eq!(eq.item_id(EquipmentSlot::Offhand), Some(2));

        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        // Equipping the unique sword in the mainhand moves the old mainhand to the offhand.
        eq.equip(EquipmentSlot::Mainhand, 2).unwrap();
        assert_eq!(eq.item_id(EquipmentSlot::Mainhand), Some(2));
        assert_eq!(eq.item_id(EquipmentSlot::Offhand), Some(1));
    }

    #[test]
    fn unique_rings_and_trinkets_swap_slots() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Ring1, 10).unwrap();
        eq.equip(EquipmentSlot::Ring2, 11).unwrap();
        assert_eq!(eq.stats().get_agility(), 12);

        // Equipping the unique ring into the other slot moves the non-unique one over.
        eq.equip(EquipmentSlot::Ring2, 10).unwrap();
        assert_eq!(eq.item_id(EquipmentSlot::Ring1), Some(11));
        assert_eq!(eq.item_id(EquipmentSlot::Ring2), Some(10));
        assert_eq!(eq.stats().get_agility(), 12);

        // A non-unique ring can be worn twice.
        eq.equip(EquipmentSlot::Ring1, 11).unwrap();
        eq.equip(EquipmentSlot::Ring2, 11).unwrap();
        assert_eq!(eq.stats().get_agility(), 14);

        eq.equip(EquipmentSlot::Trinket1, 12).unwrap();
        eq.equip(EquipmentSlot::Trinket2, 12).unwrap();
        assert!(eq.item(EquipmentSlot::Trinket1).is_none());
        assert_eq!(eq.item_id(EquipmentSlot::Trinket2), Some(12));
    }

    #[test]
    fn mutex_items_are_removed() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Trinket1, 13).unwrap();
        let change = eq.equip(EquipmentSlot::Trinket2, 14).unwrap();
        assert_eq!(ids(&change.unequipped), vec![(EquipmentSlot::Trinket1, 13)]);
        assert!(eq.item(EquipmentSlot::Trinket1).is_none());
        assert_eq!(eq.item_id(EquipmentSlot::Trinket2), Some(14));
    }

    #[test]
    fn full_unique_equipped_groups_refuse_another_item() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Trinket1, 15).unwrap();
        let error = eq.equip(EquipmentSlot::Trinket2, 16).unwrap_err();
        assert!(matches!(
            error,
            EquipError::LimitCategory {
                item_id: 16,
                quantity: 1,
                ..
            }
        ));
        assert_eq!(eq.item_id(EquipmentSlot::Trinket1), Some(15));
        assert!(eq.item(EquipmentSlot::Trinket2).is_none());

        // Replacing the item of the same group in its own slot is fine.
        eq.equip(EquipmentSlot::Trinket1, 16).unwrap();
        assert_eq!(eq.item_id(EquipmentSlot::Trinket1), Some(16));
        eq.equip(EquipmentSlot::Trinket2, 12).unwrap();
        eq.equip(EquipmentSlot::Trinket2, 15).unwrap_err();

        // A group of two: two different rings fit, and moving one to the other slot swaps them.
        eq.equip(EquipmentSlot::Ring1, 17).unwrap();
        eq.equip(EquipmentSlot::Ring2, 18).unwrap();
        eq.equip(EquipmentSlot::Ring2, 17).unwrap();
        assert_eq!(eq.item_id(EquipmentSlot::Ring1), Some(18));
        assert_eq!(eq.item_id(EquipmentSlot::Ring2), Some(17));
        eq.equip(EquipmentSlot::Ring1, 19).unwrap();
        assert_eq!(eq.item_id(EquipmentSlot::Ring1), Some(19));
    }

    #[test]
    fn ranged_and_relic_exclude_each_other() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Ranged, 6).unwrap();
        eq.equip(EquipmentSlot::Relic, 7).unwrap();
        assert!(eq.ranged().is_none());
        eq.equip(EquipmentSlot::Ranged, 6).unwrap();
        assert!(eq.item(EquipmentSlot::Relic).is_none());
    }

    #[test]
    fn set_bonuses_follow_piece_count() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Chest, 30).unwrap();
        assert_eq!(eq.set_pieces("Set"), 1);
        assert_eq!(eq.stats().get_base_melee_ap(), 0);

        eq.equip(EquipmentSlot::Legs, 31).unwrap();
        assert_eq!(eq.set_pieces("Set"), 2);
        assert!(eq.set_bonus_active("Set", 2));
        assert!(!eq.set_bonus_active("Set", 3));
        assert_eq!(eq.stats().get_base_melee_ap(), 40);
        assert_eq!(eq.stats().get_melee_crit_chance(), 0);

        eq.equip(EquipmentSlot::Boots, 32).unwrap();
        assert_eq!(eq.stats().get_base_melee_ap(), 40);
        assert_eq!(eq.stats().get_melee_crit_chance(), 200);
        assert_eq!(eq.active_sets(), vec![("Set".to_string(), 3)]);

        eq.unequip(EquipmentSlot::Legs);
        assert_eq!(eq.set_pieces("Set"), 2);
        assert_eq!(eq.stats().get_base_melee_ap(), 40);
        assert_eq!(eq.stats().get_melee_crit_chance(), 0);

        eq.unequip_all();
        assert_eq!(eq.set_pieces("Set"), 0);
        assert_eq!(eq.stats(), &Stats::new());
        assert!(eq.active_sets().is_empty());
    }

    #[test]
    fn faction_clearing() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Head, 20).unwrap();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        let change = eq.clear_items_not_available_for_faction(Faction::Horde);
        assert!(change.unequipped.is_empty());
        let change = eq.clear_items_not_available_for_faction(Faction::Alliance);
        assert_eq!(ids(&change.unequipped), vec![(EquipmentSlot::Head, 20)]);
        assert_eq!(eq.stats().get_stamina(), 0);
        assert!(eq.mainhand().is_some());
    }

    #[test]
    fn setups_are_stored_and_switched() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        eq.equip(EquipmentSlot::Head, 22).unwrap();
        eq.set_enchant(EquipmentSlot::Mainhand, Some(EnchantName::Crusader))
            .unwrap();
        eq.set_temp_enchant(
            EquipmentSlot::Mainhand,
            Some(EnchantName::DenseSharpeningStone),
        )
        .unwrap();

        let change = eq.change_setup(1).unwrap();
        assert_eq!(eq.setup_index(), 1);
        assert_eq!(
            ids(&change.unequipped),
            vec![(EquipmentSlot::Mainhand, 1), (EquipmentSlot::Head, 22)]
        );
        assert!(eq.mainhand().is_none());
        assert_eq!(eq.stats(), &Stats::new());

        eq.equip(EquipmentSlot::Mainhand, 3).unwrap();
        assert!(eq.has_two_hand_weapon());

        let change = eq.change_setup(0).unwrap();
        assert_eq!(
            ids(&change.equipped),
            vec![(EquipmentSlot::Mainhand, 1), (EquipmentSlot::Head, 22)]
        );
        assert_eq!(eq.item_id(EquipmentSlot::Mainhand), Some(1));
        assert_eq!(
            eq.enchant(EquipmentSlot::Mainhand),
            Some(EnchantName::Crusader)
        );
        assert_eq!(
            eq.temp_enchant(EquipmentSlot::Mainhand),
            Some(EnchantName::DenseSharpeningStone)
        );
        assert_eq!(eq.stats().get_strength(), 20);

        assert_eq!(eq.change_setup(3).unwrap_err(), EquipError::InvalidSetup(3));
        assert_eq!(
            eq.setup(1).unwrap().items[EquipmentSlot::Mainhand.index()],
            Some(3)
        );
    }

    #[test]
    fn enchant_selection_survives_item_swaps_in_the_same_slot() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        eq.set_enchant(EquipmentSlot::Mainhand, Some(EnchantName::Crusader))
            .unwrap();
        eq.equip(EquipmentSlot::Mainhand, 2).unwrap();
        assert_eq!(
            eq.enchant(EquipmentSlot::Mainhand),
            Some(EnchantName::Crusader)
        );
        eq.unequip(EquipmentSlot::Mainhand);
        assert_eq!(eq.enchant(EquipmentSlot::Mainhand), None);
        assert_eq!(
            eq.set_enchant(EquipmentSlot::Mainhand, Some(EnchantName::Crusader)),
            Err(EnchantError::EmptySlot(EquipmentSlot::Mainhand))
        );
        assert_eq!(eq.enchant(EquipmentSlot::Mainhand), None);
    }

    #[test]
    fn enchants_are_validated_and_add_stats() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        eq.equip(EquipmentSlot::Chest, 30).unwrap();

        eq.set_enchant(EquipmentSlot::Mainhand, Some(EnchantName::SuperiorStriking))
            .unwrap();
        assert_eq!(eq.stats().get_mh_weapon_damage(), 5);
        assert_eq!(eq.stats().get_oh_weapon_damage(), 0);
        eq.set_temp_enchant(
            EquipmentSlot::Mainhand,
            Some(EnchantName::DenseSharpeningStone),
        )
        .unwrap();
        assert_eq!(eq.stats().get_mh_weapon_damage(), 13);
        eq.set_enchant(EquipmentSlot::Chest, Some(EnchantName::EnchantChestStats))
            .unwrap();
        assert_eq!(eq.stats().get_strength(), 13);

        // A 2H-only enchant is rejected on a one-hander; wrong kind is rejected.
        assert_eq!(
            eq.set_enchant(
                EquipmentSlot::Mainhand,
                Some(EnchantName::IronCounterweight)
            ),
            Err(EnchantError::NotApplicable {
                enchant: EnchantName::IronCounterweight,
                slot: EquipmentSlot::Mainhand
            })
        );
        assert_eq!(
            eq.set_enchant(
                EquipmentSlot::Mainhand,
                Some(EnchantName::DenseSharpeningStone)
            ),
            Err(EnchantError::WrongKind {
                enchant: EnchantName::DenseSharpeningStone,
                temporary: true
            })
        );
        assert_eq!(
            eq.set_enchant(EquipmentSlot::Chest, Some(EnchantName::FieryWeapon)),
            Err(EnchantError::UnknownEnchant(EnchantName::FieryWeapon))
        );
        // Failed changes keep the previous enchant and stats.
        assert_eq!(
            eq.enchant(EquipmentSlot::Mainhand),
            Some(EnchantName::SuperiorStriking)
        );
        assert_eq!(eq.stats().get_mh_weapon_damage(), 13);

        let active: Vec<(EquipmentSlot, EnchantName)> = eq
            .active_enchants()
            .iter()
            .map(|(slot, spec)| (*slot, spec.name))
            .collect();
        assert_eq!(
            active,
            vec![
                (EquipmentSlot::Mainhand, EnchantName::SuperiorStriking),
                (EquipmentSlot::Mainhand, EnchantName::DenseSharpeningStone),
                (EquipmentSlot::Chest, EnchantName::EnchantChestStats),
            ]
        );

        // Swapping to the two-hand axe keeps both enchants (they apply to it too).
        eq.equip(EquipmentSlot::Mainhand, 3).unwrap();
        assert_eq!(
            eq.enchant(EquipmentSlot::Mainhand),
            Some(EnchantName::SuperiorStriking)
        );
        assert_eq!(
            eq.temp_enchant(EquipmentSlot::Mainhand),
            Some(EnchantName::DenseSharpeningStone)
        );
        eq.set_enchant(
            EquipmentSlot::Mainhand,
            Some(EnchantName::IronCounterweight),
        )
        .unwrap();
        assert_eq!(eq.stats().get_mh_weapon_damage(), 8);

        // Swapping to a shield in the offhand would drop a sharpening stone.
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        eq.equip(EquipmentSlot::Offhand, 2).unwrap();
        eq.set_temp_enchant(
            EquipmentSlot::Offhand,
            Some(EnchantName::DenseSharpeningStone),
        )
        .unwrap();
        assert_eq!(eq.stats().get_oh_weapon_damage(), 8);
        eq.equip(EquipmentSlot::Offhand, 4).unwrap();
        assert_eq!(eq.temp_enchant(EquipmentSlot::Offhand), None);
        assert_eq!(eq.stats().get_oh_weapon_damage(), 0);

        eq.unequip_all();
        assert_eq!(eq.stats(), &Stats::new());
    }

    #[test]
    fn faction_change_drops_invalid_enchants() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        eq.set_temp_enchant(EquipmentSlot::Mainhand, Some(EnchantName::WindfuryTotem))
            .unwrap();
        eq.set_faction(Faction::Alliance);
        assert_eq!(eq.temp_enchant(EquipmentSlot::Mainhand), None);
        assert_eq!(
            eq.set_temp_enchant(EquipmentSlot::Mainhand, Some(EnchantName::WindfuryTotem)),
            Err(EnchantError::NotApplicable {
                enchant: EnchantName::WindfuryTotem,
                slot: EquipmentSlot::Mainhand
            })
        );
    }

    #[test]
    fn phase_changes_drop_unavailable_items() {
        let mut eq = Equipment::new(
            db(),
            Phase::MoltenCore,
            Faction::Horde,
            PlayerClass::Warrior,
        );
        eq.equip(EquipmentSlot::Head, 22).unwrap();
        assert_eq!(eq.stats().get_strength(), 10);
        assert_eq!(
            eq.equip(EquipmentSlot::Head, 21).unwrap_err(),
            EquipError::UnknownItem { item_id: 21 }
        );

        eq.set_phase(Phase::AhnQiraj);
        assert_eq!(eq.stats().get_strength(), 10);
        assert_eq!(eq.item_id(EquipmentSlot::Head), Some(22));

        eq.equip(EquipmentSlot::Head, 21).unwrap_err();
        eq.set_phase(Phase::Naxxramas);
        eq.equip(EquipmentSlot::Head, 21).unwrap();
        let change = eq.set_phase(Phase::MoltenCore);
        assert!(eq.item(EquipmentSlot::Head).is_none());
        assert_eq!(ids(&change.unequipped), vec![(EquipmentSlot::Head, 21)]);
    }

    #[test]
    fn seeded_weapon_rolls_are_reproducible() {
        let mut a = equipment();
        let mut b = equipment();
        for eq in [&mut a, &mut b] {
            eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
            eq.set_seed(42);
        }
        for _ in 0..10 {
            assert_eq!(
                a.mainhand_mut().unwrap().random_dmg(),
                b.mainhand_mut().unwrap().random_dmg()
            );
        }
        assert!(a.offhand_mut().is_none());
        assert!(a.ranged_mut().is_none());
    }

    #[test]
    fn equipped_items_iterates_in_slot_order() {
        let mut eq = equipment();
        eq.equip(EquipmentSlot::Head, 22).unwrap();
        eq.equip(EquipmentSlot::Mainhand, 1).unwrap();
        let listed: Vec<(EquipmentSlot, u32)> = eq
            .equipped_items()
            .map(|(slot, item)| (slot, item.id()))
            .collect();
        assert_eq!(
            listed,
            vec![(EquipmentSlot::Mainhand, 1), (EquipmentSlot::Head, 22)]
        );
        assert!(eq.is_item_equipped(22));
        assert!(!eq.is_item_equipped(23));
    }
}
