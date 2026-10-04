//! The character sheet of a session: the gear worn ([`worn_gear`]) and the stat summary
//! ([`StatSummary`]), as the C++ GUI's equipment tab showed them.

use csim_engine::character::Character;
use csim_engine::character_loader::CharacterSetup;
use csim_engine::data_bundle::DataBundle;
use csim_engine::ids::CharId;
use csim_engine::item::{EquipmentSlot, Quality};
use csim_engine::magic_school::MagicSchool;
use csim_engine::raid::RaidControl;
use csim_engine::sim_settings::SimSettings;
use csim_engine::stats::WeaponProfile;
use serde::Serialize;

use crate::session::{Icon, PLAYER};

/// An item worn in a slot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WornItem {
    pub slot: EquipmentSlot,
    pub id: u32,
    pub name: String,
    pub quality: Quality,
    pub icon: Option<Icon>,
    /// The permanent enchant's name, as the game shows it.
    pub enchant: Option<String>,
    pub temp_enchants: Vec<String>,
}

/// The character's stats, grouped as the C++ GUI's tabs. Chances are in percent; every value
/// is the one the sim uses against the setup's target (a +3 boss suppresses crit from auras).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatSummary {
    pub melee: MeleeStats,
    pub ranged: RangedStats,
    /// Per magic school (not physical).
    pub spell: Vec<SpellStats>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeleeStats {
    pub strength: u32,
    pub agility: u32,
    pub stamina: u32,
    pub intellect: u32,
    pub spirit: u32,
    /// Of a main-hand auto attack.
    pub crit: f64,
    pub hit: f64,
    pub attack_power: u32,
    pub mainhand_skill: u32,
    /// `None` without an off-hand weapon (a shield has no skill).
    pub offhand_skill: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RangedStats {
    pub attack_power: u32,
    pub crit: f64,
    pub hit: f64,
    /// `None` without a ranged weapon.
    pub skill: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpellStats {
    pub school: MagicSchool,
    pub damage: u32,
    pub crit: f64,
    pub hit: f64,
}

/// A chance in the engine's units (`100` = 1 %) in percent.
fn percent(chance: u32) -> f64 {
    f64::from(chance) / 100.0
}

/// The gear `character` wears, in slot order, with its enchants' names from `data`.
pub fn worn_gear(data: &DataBundle, character: &Character) -> Vec<WornItem> {
    let equipment = character.equipment();
    let enchants = data.equipment.enchants();
    let name = |enchant| {
        enchants
            .get(enchant)
            .map_or_else(|| format!("{enchant:?}"), |spec| spec.display_name.clone())
    };
    equipment
        .equipped_items()
        .map(|(slot, item)| WornItem {
            slot,
            id: item.id(),
            name: item.name().to_owned(),
            quality: item.quality(),
            icon: Icon::of_item(item.spec()),
            enchant: equipment.enchant(slot).map(name),
            temp_enchants: equipment
                .temp_enchants(slot)
                .iter()
                .map(|&enchant| name(enchant))
                .collect(),
        })
        .collect()
}

impl StatSummary {
    /// The stats of `setup` built under `settings` before its first iteration: gear,
    /// enchants, talents and the setup's buffs, without what the iteration's start casts
    /// (stance, Battle Shout, ...).
    ///
    /// # Errors
    /// The setup does not build.
    pub fn of_setup(
        data: &DataBundle,
        setup: &CharacterSetup,
        settings: &SimSettings,
    ) -> Result<StatSummary, String> {
        let mut raid = setup
            .build_raid(data, settings)
            .map_err(|error| error.to_string())?;
        // The talents' passive auras are applied here.
        raid.prepare_set_of_combat_iterations();
        Ok(StatSummary::of(&raid, PLAYER))
    }

    /// The stats of character `id` of `raid` as they are now.
    pub fn of(raid: &RaidControl, id: CharId) -> StatSummary {
        let character = raid.character(id);
        let view = raid.target().stat_view();
        let ctx = character.stat_context(&view);
        let stats = character.stats();
        let skilled = |weapon: Option<WeaponProfile>| {
            weapon.is_some_and(|weapon| weapon.weapon_type.has_weapon_skill())
        };
        StatSummary {
            melee: MeleeStats {
                strength: stats.get_strength(&ctx),
                agility: stats.get_agility(&ctx),
                stamina: stats.get_stamina(&ctx),
                intellect: stats.get_intellect(&ctx),
                spirit: stats.get_spirit(&ctx),
                crit: percent(stats.get_mh_crit_chance(&ctx)),
                hit: percent(stats.get_melee_hit_chance(&ctx)),
                attack_power: stats.get_melee_ap(&ctx),
                mainhand_skill: stats.get_mh_wpn_skill(&ctx),
                offhand_skill: skilled(ctx.offhand).then(|| stats.get_oh_wpn_skill(&ctx)),
            },
            ranged: RangedStats {
                attack_power: stats.get_ranged_ap(&ctx),
                crit: percent(stats.get_ranged_crit_chance(&ctx)),
                hit: percent(stats.get_ranged_hit_chance(&ctx)),
                skill: skilled(ctx.ranged).then(|| stats.get_ranged_wpn_skill(&ctx)),
            },
            spell: MagicSchool::ALL
                .into_iter()
                .filter(|&school| school != MagicSchool::Physical)
                .map(|school| SpellStats {
                    school,
                    damage: stats.get_spell_damage(&ctx, school),
                    crit: percent(stats.get_spell_crit_chance(&ctx, school)),
                    hit: percent(stats.get_spell_hit_chance(&ctx, school)),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
