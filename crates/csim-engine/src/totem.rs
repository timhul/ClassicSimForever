//! Shaman totems: one per element, summoned by a spell whose `SUMMON` effect names the slot
//! (`SummonProperties`) and whose `SUMMON_TOTEM` script names what the totem does: an aura it
//! puts on the shaman while it stands, or an attack it casts again and again.

use serde::{Deserialize, Serialize};

use crate::character_spells::SpellHandle;
use crate::ids::BuffId;

/// Seconds a totem lasts past its duration: what it does as its time runs out (Magma Totem's
/// tenth pulse, Searing Totem's last attack) happens.
pub const TOTEM_GRACE: f64 = 0.001;

/// The element of a totem: its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TotemElement {
    Earth,
    Fire,
    Water,
    Air,
}

impl TotemElement {
    pub const ALL: [TotemElement; 4] = [
        TotemElement::Earth,
        TotemElement::Fire,
        TotemElement::Water,
        TotemElement::Air,
    ];

    /// The slot of a `SUMMON` effect's `SummonProperties` (its second misc value): 63 fire, 81
    /// earth, 82 water, 83 air.
    pub fn from_summon_properties(id: i32) -> Option<TotemElement> {
        match id {
            63 => Some(TotemElement::Fire),
            81 => Some(TotemElement::Earth),
            82 => Some(TotemElement::Water),
            83 => Some(TotemElement::Air),
            _ => None,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// What a totem spell summons (its `SUMMON_TOTEM` script).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TotemSummon {
    pub element: TotemElement,
    /// The spell the totem applies or casts.
    pub spell: u32,
    /// For an attacking totem (Searing Totem): seconds between its casts of `spell`.
    pub period: Option<f64>,
}

/// A standing totem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveTotem {
    /// The totem spell's buff: the totem's lifetime.
    pub buff: BuffId,
    pub summon: TotemSummon,
    /// The aura (a passive, or a proc aura) the totem switched on for the shaman; `None` for an
    /// attacking totem, or when something else already gives the shaman the same.
    pub applied: Option<SpellHandle>,
    /// Tells this totem's attack events from a replaced one's.
    pub iteration: u32,
}

/// The character's totems, one slot per element.
#[derive(Debug, Clone, Default)]
pub struct Totems {
    slots: [Option<ActiveTotem>; 4],
    iteration: u32,
}

impl Totems {
    pub fn get(&self, element: TotemElement) -> Option<&ActiveTotem> {
        self.slots[element.index()].as_ref()
    }

    pub fn set(&mut self, totem: ActiveTotem) {
        self.slots[totem.summon.element.index()] = Some(totem);
    }

    pub fn take(&mut self, element: TotemElement) -> Option<ActiveTotem> {
        self.slots[element.index()].take()
    }

    /// A fresh iteration number for the next totem.
    pub fn next_iteration(&mut self) -> u32 {
        self.iteration += 1;
        self.iteration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summon_properties_name_the_slot() {
        assert_eq!(
            TotemElement::from_summon_properties(63),
            Some(TotemElement::Fire)
        );
        assert_eq!(
            TotemElement::from_summon_properties(83),
            Some(TotemElement::Air)
        );
        assert_eq!(TotemElement::from_summon_properties(64), None);
    }
}
