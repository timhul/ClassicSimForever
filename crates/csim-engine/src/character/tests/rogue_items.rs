//! The Rogue's consumables, energy items and set bonuses on the shipped data: Thistle Tea (a
//! consumable used from the bags), Renataki's Charm of Trickery, and the bonuses of Darkmantle,
//! Bonescythe, Deathdealer's Embrace, Emblems of Veiled Shadows and Madcap's Outfit.

use std::sync::OnceLock;

use super::energy::{NOTHING, data, energy, pull, rogue_with};
use super::rogue::{
    BACKSTAB, EVISCERATE, HEMORRHAGE, RUPTURE, SINISTER_STRIKE, SLICE_AND_DICE, buff_of, cast_at,
    damage_mod, highest_rank, set_combo_points,
};
use super::*;
use crate::buff::external::ExternalBuffDb;
use crate::character_spells::{EquipmentGrantor, SpellHandle};
use crate::ids::ProcId;
use crate::proc::ProcSource;

const STEALTH: u32 = 1787;
const THISTLE_TEA: u32 = 9512;
const BURST_OF_ENERGY: u32 = 24532;
const RENATAKIS_CHARM: u32 = 19954;
const ROGUE_ARMOR_ENERGIZE: u32 = 27787;
const HEAD_RUSH: u32 = 28812;
const REVEALED_FLAW: u32 = 28814;
const REVEALED_FLAW_BUFF: u32 = 28815;

/// The test items (the fixture's sword and dagger) and the shipped items, item sets and
/// enchants.
pub(super) fn shipped_equipment() -> Arc<EquipmentDb> {
    static DB: OnceLock<Arc<EquipmentDb>> = OnceLock::new();
    Arc::clone(DB.get_or_init(|| {
        let items: Vec<ItemSpec> = serde_yaml::from_str(ITEMS_YAML).unwrap();
        let mut db = EquipmentDb::from_specs(items, Vec::new()).unwrap();
        let mut files: Vec<_> = std::fs::read_dir(data().join("items"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        for path in files {
            db.load_item_file(&path).unwrap();
        }
        db.load_item_set_file(&data().join("item_sets.yaml"))
            .unwrap();
        db.set_enchants(crate::enchant::EnchantDb::load(&data().join("enchants.yaml")).unwrap());
        Arc::new(db)
    }))
}

/// A rogue wearing `items` (slot, id), at the pull, every roll rigged to a hit.
fn wearing(items: &[(EquipmentSlot, u32)]) -> Fixture {
    let mut f = rogue_with(shipped_equipment(), &[], NOTHING);
    for &(slot, item) in items {
        f.equip(slot, item);
    }
    f.ctx().prepare_set_of_combat_iterations();
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    f
}

/// A rogue at the pull with the consumables of `names` from the shipped registry.
fn with_consumables(names: &[&str]) -> Fixture {
    let registry = ExternalBuffDb::load(&data().join("external_buffs.yaml")).unwrap();
    let consumables = names
        .iter()
        .map(|name| registry.consumable(name).unwrap().clone())
        .collect();
    let mut f = rogue_with(shipped_equipment(), &[], NOTHING);
    let db = std::mem::take(&mut f.db);
    f.ctx().set_consumables(&db, consumables);
    f.db = db;
    f.ctx().prepare_set_of_combat_iterations();
    f.rig_rolls(PhysicalAttackResult::Hit);
    f
}

/// Casts the spell the rotation knows as `name` at `time`, past the global cooldown.
fn use_at(f: &mut Fixture, name: &str, time: f64) -> SpellResult {
    f.advance_to(time);
    let id = highest_rank(f, name);
    let ctx = f.ctx();
    let status = ctx.character.spells().spell(id).status(&ctx);
    assert_eq!(status, SpellStatus::Available, "{name} at {time}");
    f.ctx().cast(id).result
}

fn status_of(f: &mut Fixture, name: &str) -> SpellStatus {
    let id = highest_rank(f, name);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).status(&ctx)
}

/// The proc a set bonus registered for `game_id`.
pub(super) fn set_bonus_proc(f: &Fixture, game_id: u32) -> ProcId {
    find_set_bonus_proc(f, game_id).unwrap_or_else(|| panic!("no set bonus proc {game_id}"))
}

fn find_set_bonus_proc(f: &Fixture, game_id: u32) -> Option<ProcId> {
    let spells = f.character.spells();
    spells
        .equipment_spells()
        .find_map(|(key, handle)| match (key.grantor, handle) {
            (EquipmentGrantor::SetBonus(_), SpellHandle::Proc(id))
                if spells.procs().get(id).spell().game_id() == game_id =>
            {
                Some(id)
            }
            _ => None,
        })
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

/// Thistle Tea: used by its name, 100 energy up to the cap, off the global cooldown, on the
/// item's 5 min cooldown.
#[test]
fn thistle_tea_restores_energy_on_the_item_cooldown() {
    let mut f = with_consumables(&["Thistle Tea"]);
    pull(&mut f);
    let tea = highest_rank(&f, "Thistle Tea");
    assert_eq!(f.character.spells().spell(tea).game_id(), THISTLE_TEA);
    assert!(
        f.character.spells().rank_group("Restore Energy").is_none(),
        "cast by the consumable's name"
    );
    cast_at(&mut f, SINISTER_STRIKE, 0.0);
    cast_at(&mut f, SINISTER_STRIKE, 1.0);
    assert_eq!(energy(&f, 1.0), 20);
    assert_eq!(use_at(&mut f, "Thistle Tea", 1.0), SpellResult::Success);
    assert_eq!(energy(&f, 1.0), 100, "capped");
    assert_eq!(status_of(&mut f, "Thistle Tea"), SpellStatus::OnCooldown);
    f.advance_to(300.9);
    assert_eq!(status_of(&mut f, "Thistle Tea"), SpellStatus::OnCooldown);
    f.advance_to(301.0);
    assert_eq!(status_of(&mut f, "Thistle Tea"), SpellStatus::Available);
}

/// A consumable is only there when the setup lists it.
#[test]
fn thistle_tea_needs_the_consumable() {
    let f = with_consumables(&[]);
    assert!(f.character.spells().rank_group("Thistle Tea").is_none());
}

/// Using an item does not break Stealth.
#[test]
fn thistle_tea_keeps_stealth() {
    let mut f = with_consumables(&["Thistle Tea"]);
    f.ctx().reset();
    f.engine.prepare_iteration(-1.0);
    let stealth = f.spell_id(STEALTH);
    assert_eq!(f.ctx().cast(stealth).result, SpellResult::Success);
    let tea = highest_rank(&f, "Thistle Tea");
    assert_eq!(f.ctx().cast(tea).result, SpellResult::Success);
    let buff = buff_of(&f, STEALTH);
    assert!(f.ctx().buff_ref(buff).is_active());
}

/// Renataki's Charm of Trickery: 60 energy on a 3 min cooldown, sharing the trinkets' 10 s
/// category cooldown, and part of Madcap's Outfit.
#[test]
fn renatakis_charm_restores_energy() {
    let mut f = wearing(&[(EquipmentSlot::Trinket1, RENATAKIS_CHARM)]);
    cast_at(&mut f, SINISTER_STRIKE, 0.0);
    assert_eq!(energy(&f, 0.0), 55);
    assert_eq!(use_at(&mut f, "Burst of Energy", 0.0), SpellResult::Success);
    assert_eq!(energy(&f, 0.0), 100);
    let charm = highest_rank(&f, "Burst of Energy");
    assert_eq!(f.character.spells().spell(charm).game_id(), BURST_OF_ENERGY);
    f.advance_to(179.9);
    assert_eq!(
        status_of(&mut f, "Burst of Energy"),
        SpellStatus::OnCooldown
    );
    f.advance_to(180.0);
    assert_eq!(status_of(&mut f, "Burst of Energy"), SpellStatus::Available);
}

const DARKMANTLE: [(EquipmentSlot, u32); 4] = [
    (EquipmentSlot::Belt, 22002),
    (EquipmentSlot::Boots, 22003),
    (EquipmentSlot::Wrist, 22004),
    (EquipmentSlot::Head, 22005),
];

/// Darkmantle Armor 4: 20 energy at 3.5 % per white swing of either hand (not every swing,
/// as the table's `ProcChance` 100 would have it), never off abilities.
#[test]
fn darkmantle_restores_energy_on_white_swings() {
    let f3 = wearing(&DARKMANTLE[..3]);
    assert!(find_set_bonus_proc(&f3, ROGUE_ARMOR_ENERGIZE).is_none());
    let mut f4 = wearing(&DARKMANTLE);
    let proc = set_bonus_proc(&f4, ROGUE_ARMOR_ENERGIZE);
    {
        let ctx = f4.ctx();
        let energize = ctx.character.spells().procs().get(proc);
        assert_eq!(
            energize.sources(),
            [ProcSource::MainhandSwing, ProcSource::OffhandSwing]
        );
        assert_eq!(energize.proc_range(ProcSource::MainhandSwing, &ctx), 350);
    }
    cast_at(&mut f4, SINISTER_STRIKE, 0.0);
    let before = energy(&f4, 0.0);
    let mut procs = 0;
    for _ in 0..200 {
        procs += f4.ctx().run_proc_checks(&[ProcSource::MainhandSwing]).len();
    }
    assert!((1..=20).contains(&procs), "{procs} of 200 at 3.5 %");
    assert_eq!(energy(&f4, 0.0), (before + 20 * procs as u32).min(100));
}

const BONESCYTHE: [(EquipmentSlot, u32); 8] = [
    (EquipmentSlot::Wrist, 22483),
    (EquipmentSlot::Chest, 22476),
    (EquipmentSlot::Gloves, 22481),
    (EquipmentSlot::Head, 22478),
    (EquipmentSlot::Legs, 22477),
    (EquipmentSlot::Shoulders, 22479),
    (EquipmentSlot::Boots, 22480),
    (EquipmentSlot::Belt, 22482),
];

/// Bonescythe Armor 4, Head Rush: 5 energy when Sinister Strike crits (every time, with a
/// 500 ms internal cooldown), nothing when it only hits.
#[test]
fn bonescythe_head_rush_restores_energy_on_crits() {
    let mut f = wearing(&BONESCYTHE[..4]);
    let proc = set_bonus_proc(&f, HEAD_RUSH);
    assert_eq!(
        f.character.spells().procs().get(proc).sources(),
        [ProcSource::MeleeCritical]
    );
    cast_at(&mut f, SINISTER_STRIKE, 0.0);
    assert_eq!(energy(&f, 0.0), 55, "a hit");
    f.character.stats_mut().increase_melee_aura_crit(10000);
    let report = cast_at(&mut f, SINISTER_STRIKE, 1.0);
    assert_eq!(
        report.attack.unwrap().result,
        PhysicalAttackResult::Critical
    );
    assert_eq!(energy(&f, 1.0), 55 + 10 - 45 + 5);
    // A white crit is not an ability.
    assert!(
        f.ctx()
            .run_proc_checks(&[ProcSource::MeleeCritical])
            .is_empty()
    );
}

/// Bonescythe Armor 8, Revealed Flaw: an Eviscerate has 5 % per combo point spent to make the
/// next Backstab, Sinister Strike or Hemorrhage crit; other finishers do not trigger it.
#[test]
fn bonescythe_revealed_flaw_makes_the_next_strike_crit() {
    let mut f = wearing(&BONESCYTHE);
    let proc = set_bonus_proc(&f, REVEALED_FLAW);
    {
        let ctx = f.ctx();
        let flaw = ctx.character.spells().procs().get(proc);
        assert_eq!(flaw.sources(), [ProcSource::Finisher]);
        assert_eq!(
            flaw.proc_range(ProcSource::Finisher, &ctx),
            500,
            "per point"
        );
    }
    // 25 % at five points: Slice and Dice never triggers it, some Eviscerate soon does.
    let mut time = 0.0;
    for _ in 0..40 {
        f.character.gain_resource(ResourceType::Energy, 100, time);
        set_combo_points(&mut f, 5);
        cast_at(&mut f, SLICE_AND_DICE, time);
        time += 1.0;
    }
    assert!(!f.ctx().aura_active(REVEALED_FLAW_BUFF), "not Eviscerate");
    let mut eviscerates = 0;
    while !f.ctx().aura_active(REVEALED_FLAW_BUFF) {
        assert!(
            eviscerates < 60,
            "no Revealed Flaw in {eviscerates} Eviscerates"
        );
        f.character.gain_resource(ResourceType::Energy, 100, time);
        set_combo_points(&mut f, 5);
        cast_at(&mut f, EVISCERATE, time);
        eviscerates += 1;
        time += 1.0;
    }
    for spell in [SINISTER_STRIKE, BACKSTAB, HEMORRHAGE] {
        if f.character.spells().spell_by_game_id(spell).is_some() {
            let id = f.spell_id(spell);
            let ctx = f.ctx();
            let bonus = ctx.character.spells().spell(id).crit_chance_bonus(&ctx);
            assert_eq!(bonus, 10000, "{spell}");
        }
    }
    f.character.gain_resource(ResourceType::Energy, 100, time);
    let report = cast_at(&mut f, SINISTER_STRIKE, time);
    assert_eq!(
        report.attack.unwrap().result,
        PhysicalAttackResult::Critical
    );
    assert!(!f.ctx().aura_active(REVEALED_FLAW_BUFF), "used up");
}

/// Deathdealer's Embrace 5: Eviscerate deals 15 % more damage.
#[test]
fn deathdealer_raises_eviscerate_damage() {
    let mut base = wearing(&[]);
    let mut f = wearing(&[
        (EquipmentSlot::Boots, 21359),
        (EquipmentSlot::Head, 21360),
        (EquipmentSlot::Shoulders, 21361),
        (EquipmentSlot::Legs, 21362),
        (EquipmentSlot::Chest, 21364),
    ]);
    let ratio = damage_mod(&mut f, EVISCERATE) / damage_mod(&mut base, EVISCERATE);
    assert!((ratio - 1.15).abs() < 1e-9, "{ratio}");
}

/// Emblems of Veiled Shadows 3: Slice and Dice costs 10 energy less.
#[test]
fn veiled_shadows_lowers_slice_and_dice_cost() {
    let mut base = wearing(&[]);
    let mut f = wearing(&[
        (EquipmentSlot::Ring1, 21405),
        (EquipmentSlot::Back, 21406),
        (EquipmentSlot::Offhand, 21404),
    ]);
    assert_eq!(
        cost(&mut f, SLICE_AND_DICE),
        cost(&mut base, SLICE_AND_DICE) - 10
    );
}

/// Madcap's Outfit 5: Eviscerate and Rupture cost 5 energy less.
#[test]
fn madcap_lowers_eviscerate_and_rupture_cost() {
    let mut base = wearing(&[]);
    let mut f = wearing(&[
        (EquipmentSlot::Neck, 19617),
        (EquipmentSlot::Trinket1, RENATAKIS_CHARM),
        (EquipmentSlot::Wrist, 19836),
        (EquipmentSlot::Shoulders, 19835),
        (EquipmentSlot::Chest, 19834),
    ]);
    for spell in [EVISCERATE, RUPTURE] {
        assert_eq!(cost(&mut f, spell), cost(&mut base, spell) - 5, "{spell}");
    }
}
