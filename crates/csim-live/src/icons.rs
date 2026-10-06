//! The icons of what a character's statistics name: its spells, weapons, buffs, procs and
//! resource sources. The live frames ([`Session`](crate::session::Session)) and the sim results
//! ([`SimJob`](crate::sim::SimJob)) show them next to their rows.

use csim_engine::buff::Buff;
use csim_engine::character_spells::BuffSlot;
use csim_engine::data_bundle::DataBundle;
use csim_engine::item::EquipmentSlot;
use csim_engine::raid::RaidControl;
use csim_engine::spell::Hand;

use crate::session::{Icon, PLAYER};

/// The icons of the raid's player (its first character), from the data.
#[derive(Clone, Copy)]
pub(crate) struct IconLookup<'a> {
    pub data: &'a DataBundle,
    pub raid: &'a RaidControl,
}

impl<'a> IconLookup<'a> {
    /// The icon of the game spell `id` (0 for spells made in code: none).
    pub fn spell(&self, id: u32) -> Option<Icon> {
        self.data
            .spells
            .get(id)
            .and_then(|record| Icon::of_spell(record))
    }

    /// The icon of the weapon in `hand`.
    pub fn weapon(&self, hand: Hand) -> Option<Icon> {
        let slot = match hand {
            Hand::Mainhand => EquipmentSlot::Mainhand,
            Hand::Offhand => EquipmentSlot::Offhand,
        };
        let equipment = self.raid.character(PLAYER).equipment();
        equipment
            .item(slot)
            .and_then(|item| Icon::of_item(item.spec()))
    }

    /// The icon of what the statistics name a spell row or a resource source by (`" (rank N)"`
    /// appended above rank 1): a swing's weapon, a spell's or a proc's.
    pub fn source(&self, source: &str) -> Option<Icon> {
        let name = source
            .rsplit_once(" (rank ")
            .map_or(source, |(name, _)| name);
        let spells = self.raid.character(PLAYER).spells();
        if name == spells.mh_attack().name() {
            return self.weapon(Hand::Mainhand);
        }
        if name == spells.oh_attack().name() {
            return self.weapon(Hand::Offhand);
        }
        let spell = spells
            .spell_ids()
            .map(|id| spells.spell(id))
            .filter(|spell| spell.name() == name)
            .map(|spell| spell.game_id());
        spell
            .chain(self.proc_spells(name))
            .find_map(|id| self.spell(id))
    }

    /// The icon of the proc `name`: its spell's, or a spell's it casts.
    pub fn proc(&self, name: &str) -> Option<Icon> {
        self.proc_spells(name)
            .into_iter()
            .find_map(|id| self.spell(id))
    }

    /// The spells of the procs named `name`: each proc's own, then the ones it casts.
    fn proc_spells(&self, name: &str) -> Vec<u32> {
        let procs = self.raid.character(PLAYER).spells().procs().procs();
        procs
            .iter()
            .filter(|proc| proc.name() == name)
            .flat_map(|proc| std::iter::once(proc.game_id()).chain(proc.payload_spells()))
            .collect()
    }

    /// Every buff of the player: its own and the ones it shares.
    pub fn buffs(&self) -> Vec<&'a Buff> {
        let spells = self.raid.character(PLAYER).spells();
        spells
            .buff_ids()
            .filter_map(|id| match spells.buff_slot(id) {
                BuffSlot::Owned(buff) => Some(&**buff),
                BuffSlot::Shared(shared) => self.raid.shared_buffs().buffs().get(shared.index()),
            })
            .collect()
    }

    /// The icon of the buff of `buffs` the statistics name `name`: its spell's.
    pub fn buff(&self, buffs: &[&Buff], name: &str) -> Option<Icon> {
        buffs
            .iter()
            .find(|buff| buff.statistics_name() == name)
            .and_then(|buff| self.spell(buff.spell()))
    }
}
