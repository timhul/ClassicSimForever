//! The spells the equipment grants (`CharacterContext::sync_equipment_spells`) on the shipped
//! data: on-equip proc auras and auras, chance-on-hit spells, set bonuses and on-use spells.

use std::sync::Arc;

use super::warrior::WarriorTest;
use super::SpellTest;
use crate::character_spells::SpellHandle;
use crate::ids::ProcId;
use crate::item::EquipmentSlot;
use crate::proc::runtime::PROC_ROLL_RANGE;
use crate::proc::ProcSource;
use crate::rotation::{RotationHost, RotationSpec};
use crate::spell::{SpellStatus, MAX_RANK};

/// Blazefury Medallion: on-equip proc aura 7711, Fire Strike (7712) on every melee hit.
const BLAZEFURY_MEDALLION: u32 = 17111;
const BLAZEFURY_PROC: &str = "Add Fire Dam - Weap 02";
/// Thunderfury, Blessed Blade of the Windseeker: chance on hit 21992.
const THUNDERFURY: u32 = 19019;
/// Ebon Hand: chance on hit 18211 (Shadow Bolt).
const EBON_HAND: u32 = 19170;
/// General's Plate Gauntlets: on-equip 22778, Hamstring costs 3 rage less.
const GENERALS_GAUNTLETS: u32 = 16548;
/// Mace of Unending Life: on-equip 26153, +140 attack power in Cat and Bear forms only.
const MACE_OF_UNENDING_LIFE: u32 = 21407;
/// Lieutenant Commander's Battlegear (set 282): (2) +40 attack power.
const PREMIER_PLATE_GAUNTLETS: u32 = 272717;
const PREMIER_PLATE_BOOTS: u32 = 272716;
/// Battlegear of Heroism (set 511) by slot: (2) Increased All Resist 08, (4) Warrior's Resolve,
/// (6) Attack Power 40, (8) Increased Armor 200.
const HEROISM: [(EquipmentSlot, u32); 8] = [
    (EquipmentSlot::Belt, 21994),
    (EquipmentSlot::Boots, 21995),
    (EquipmentSlot::Wrist, 21996),
    (EquipmentSlot::Chest, 21997),
    (EquipmentSlot::Gloves, 21998),
    (EquipmentSlot::Head, 21999),
    (EquipmentSlot::Legs, 22000),
    (EquipmentSlot::Shoulders, 22001),
];

fn test(label: &str) -> WarriorTest {
    WarriorTest::new(label)
}

/// The proc `name`, if the character has one.
fn find_proc(test: &SpellTest, name: &str) -> Option<ProcId> {
    let procs = test.character().spells().procs().procs();
    procs
        .iter()
        .position(|proc| proc.name() == name)
        .map(|index| ProcId(index as u32))
}

fn proc_enabled(test: &SpellTest, name: &str) -> bool {
    find_proc(test, name).is_some_and(|id| test.character().spells().procs().is_enabled(id))
}

/// Whether the character's aura `name` is up.
fn aura_active(test: &mut SpellTest, name: &str) -> bool {
    test.character()
        .spells()
        .owned_buff_by_name(name)
        .is_some_and(|id| test.with_buff_id(id, crate::buff::Buff::is_active))
}

fn melee_ap(test: &SpellTest) -> u32 {
    let view = test.target().stat_view();
    test.character().melee_ap(&view)
}

fn hamstring_cost(test: &mut SpellTest) -> u32 {
    let id = test.spell("Hamstring");
    test.with_ctx(|ctx| ctx.with_spell(id, |spell, ctx| spell.resource_cost(ctx)))
}

#[test]
fn an_on_equip_proc_aura_strikes_on_melee_hits() {
    let mut test = test("Blazefury Medallion");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.equip(EquipmentSlot::Neck, BLAZEFURY_MEDALLION);

    let proc = test.proc(BLAZEFURY_PROC);
    assert!(test.character().spells().procs().is_enabled(proc));
    // Every melee hit.
    for source in [
        ProcSource::MainhandSwing,
        ProcSource::MainhandSpell,
        ProcSource::OffhandSwing,
    ] {
        assert_eq!(test.proc_range(BLAZEFURY_PROC, source), PROC_ROLL_RANGE);
    }
    assert!(!test.proc_conditions_fulfilled(BLAZEFURY_PROC, ProcSource::RangedAutoShot));

    test.given_no_previous_damage_dealt();
    test.with_ctx(|ctx| ctx.perform_proc(proc));
    assert_eq!(test.damage_dealt_by("Fire Strike"), 2);
}

#[test]
fn thunderfury_strikes_the_target_on_a_main_hand_hit() {
    let mut test = test("Thunderfury");
    test.equip(EquipmentSlot::Mainhand, THUNDERFURY);

    let proc = test.proc("Thunderfury");
    assert_eq!(
        test.proc_range("Thunderfury", ProcSource::MainhandSwing),
        PROC_ROLL_RANGE / 5
    );
    assert!(test.proc_conditions_fulfilled("Thunderfury", ProcSource::MainhandSwing));
    assert!(test.proc_conditions_fulfilled("Thunderfury", ProcSource::MainhandSpell));
    assert!(!test.proc_conditions_fulfilled("Thunderfury", ProcSource::OffhandSwing));

    test.given_no_previous_damage_dealt();
    test.with_ctx(|ctx| ctx.perform_proc(proc));
    // 300 Nature damage (the magic table is not ported: it always lands), and the Nature
    // resistance debuff.
    assert_eq!(test.damage_dealt_by("Thunderfury"), 300);
    assert!(test.buff_is_active("Thunderfury"));
    // A proc, not a cast: no global cooldown.
    assert!(!test.on_global_cooldown());
}

#[test]
fn an_off_hand_on_hit_spell_procs_off_the_off_hand() {
    let mut test = test("Ebon Hand");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.equip(EquipmentSlot::Offhand, EBON_HAND);
    assert_eq!(
        test.proc_range("Shadow Bolt", ProcSource::OffhandSwing),
        383,
        "ClassicSim's 3.83 %"
    );
    assert!(test.proc_conditions_fulfilled("Shadow Bolt", ProcSource::OffhandSwing));
    assert!(!test.proc_conditions_fulfilled("Shadow Bolt", ProcSource::MainhandSwing));
}

#[test]
fn an_on_equip_aura_modifies_a_spell_while_worn() {
    let mut test = test("General's Plate Gauntlets");
    assert_eq!(hamstring_cost(&mut test), 10);
    test.equip(EquipmentSlot::Gloves, GENERALS_GAUNTLETS);
    assert_eq!(hamstring_cost(&mut test), 7);
    test.unequip(EquipmentSlot::Gloves);
    assert_eq!(hamstring_cost(&mut test), 10);
}

#[test]
fn feral_attack_power_needs_a_druid_form() {
    let mut test = test("Mace of Unending Life");
    test.equip(EquipmentSlot::Mainhand, MACE_OF_UNENDING_LIFE);
    // The aura is granted, but its shapeshift requirement (Cat, Bear and Dire Bear forms)
    // keeps it down for a warrior.
    let feral = test
        .character()
        .spells()
        .owned_buff_by_name("Attack Power - Feral (+140)")
        .expect("the on-equip aura is registered");
    assert!(!test.with_buff_id(feral, crate::buff::Buff::is_active));
}

#[test]
fn a_set_bonus_aura_adds_attack_power() {
    let mut test = test("Lieutenant Commander's Battlegear");
    let none = melee_ap(&test);
    test.equip(EquipmentSlot::Gloves, PREMIER_PLATE_GAUNTLETS);
    let gloves = melee_ap(&test);
    test.equip(EquipmentSlot::Boots, PREMIER_PLATE_BOOTS);
    let both = melee_ap(&test);
    test.unequip(EquipmentSlot::Gloves);
    let boots = melee_ap(&test);
    // Each piece's own stats count once; the pair adds the bonus.
    assert_eq!(both + none - gloves - boots, 40);
}

#[test]
fn set_bonuses_follow_the_pieces_worn() {
    let mut test = test("Battlegear of Heroism");
    // (2) Increased All Resist 08, (4) Warrior's Resolve, (6) Attack Power 40,
    // (8) Increased Armor 200.
    let bonuses = |test: &mut SpellTest| {
        (
            aura_active(test, "Increased All Resist 08"),
            proc_enabled(test, "Warrior's Resolve"),
            aura_active(test, "Attack Power 40"),
            aura_active(test, "Increased Armor 200"),
        )
    };
    for (worn, &(slot, item)) in HEROISM.iter().enumerate() {
        test.equip(slot, item);
        let pieces = worn as u32 + 1;
        assert_eq!(test.character().equipment().set_pieces(511), pieces);
        assert_eq!(
            bonuses(&mut test),
            (pieces >= 2, pieces >= 4, pieces >= 6, pieces >= 8),
            "{pieces} pieces"
        );
    }
    for &(slot, _) in &HEROISM[4..] {
        test.unequip(slot);
    }
    assert_eq!(bonuses(&mut test), (true, true, false, false), "4 pieces");
    for &(slot, _) in &HEROISM[..4] {
        test.unequip(slot);
    }
    assert_eq!(
        bonuses(&mut test),
        (false, false, false, false),
        "no pieces"
    );
}

/// The equipment spells and procs that are enabled.
fn enabled_equipment_spells(test: &SpellTest) -> usize {
    let spells = test.character().spells();
    spells
        .equipment_spells()
        .filter(|&(_, handle)| match handle {
            SpellHandle::Spell(id) => spells.spell(id).is_enabled(),
            SpellHandle::Proc(id) => spells.procs().is_enabled(id),
        })
        .count()
}

#[test]
fn unequipping_disables_everything_the_equipment_granted() {
    let mut test = test("unequip");
    test.equip(EquipmentSlot::Neck, BLAZEFURY_MEDALLION);
    test.equip(EquipmentSlot::Mainhand, THUNDERFURY);
    test.equip(EquipmentSlot::Gloves, GENERALS_GAUNTLETS);
    assert_eq!(enabled_equipment_spells(&test), 3);
    let registered = test.character().spells().equipment_spells().count();

    for slot in [
        EquipmentSlot::Neck,
        EquipmentSlot::Mainhand,
        EquipmentSlot::Gloves,
    ] {
        test.unequip(slot);
    }
    assert_eq!(enabled_equipment_spells(&test), 0);
    assert_eq!(hamstring_cost(&mut test), 10);

    // Equipping again reuses the registration.
    test.equip(EquipmentSlot::Neck, BLAZEFURY_MEDALLION);
    assert!(proc_enabled(&test, BLAZEFURY_PROC));
    assert_eq!(
        test.character().spells().equipment_spells().count(),
        registered
    );
}

/// Kiss of the Spider: use 28866, 20 % attack speed for 15 s; 2 min cooldown, 15 s on the
/// trinket category (1141).
const KISS_OF_THE_SPIDER: u32 = 22954;
/// Slayer's Crest: use 28777, +260 attack power for 20 s; 2 min cooldown, 20 s on 1141.
const SLAYERS_CREST: u32 = 23041;
/// Earthstrike: use 25891, +280 attack power for 20 s; 2 min cooldown, 20 s on 1141.
const EARTHSTRIKE: u32 = 21180;
/// Zandalarian Hero Medallion: use 24661 (Restless Strength), a `DUMMY` aura without a script.
const ZANDALARIAN_HERO_MEDALLION: u32 = 19949;
/// Badge of the Swarmguard: use 26480, a proc aura while the buff is up.
const BADGE_OF_THE_SWARMGUARD: u32 = 21670;

#[test]
fn on_use_trinkets_have_their_own_and_the_shared_trinket_cooldown() {
    let mut test = test("on-use trinkets");
    test.equip(EquipmentSlot::Trinket1, KISS_OF_THE_SPIDER);
    test.equip(EquipmentSlot::Trinket2, SLAYERS_CREST);
    test.then_status_is("Kiss of the Spider", SpellStatus::Available);
    test.then_status_is("Slayer's Crest", SpellStatus::Available);
    let ap = melee_ap(&test);

    test.cast("Kiss of the Spider");
    assert!(test.buff_is_active("Kiss of the Spider"));
    // No global cooldown; the item's cooldown, not the spell's (none).
    assert!(!test.on_global_cooldown());
    assert_eq!(test.cooldown_remaining("Kiss of the Spider"), 120.0);
    // Kiss of the Spider's 15 s on the trinket category.
    test.then_status_is("Slayer's Crest", SpellStatus::OnCooldown);
    assert_eq!(test.cooldown_remaining("Slayer's Crest"), 15.0);

    test.given_engine_priority_at(15.0);
    test.then_status_is("Slayer's Crest", SpellStatus::Available);
    test.cast("Slayer's Crest");
    assert_eq!(melee_ap(&test), ap + 260);
    // Slayer's Crest's 20 s on the category is shorter than what is left of Kiss's own.
    assert_eq!(test.cooldown_remaining("Kiss of the Spider"), 105.0);
    assert_eq!(test.cooldown_remaining("Slayer's Crest"), 120.0);
}

#[test]
fn an_on_use_spell_comes_and_goes_with_its_item() {
    let mut test = test("on-use unequip");
    assert!(test
        .with_ctx(|ctx| ctx.spell_by_name("Earthstrike", MAX_RANK))
        .is_none());
    test.equip(EquipmentSlot::Trinket1, EARTHSTRIKE);
    test.then_status_is("Earthstrike", SpellStatus::Available);
    test.unequip(EquipmentSlot::Trinket1);
    test.then_status_is("Earthstrike", SpellStatus::NotEnabled);
    // Worn in the other slot, the item's use is a spell of its own that takes the name.
    test.equip(EquipmentSlot::Trinket2, EARTHSTRIKE);
    test.then_status_is("Earthstrike", SpellStatus::Available);
    let ap = melee_ap(&test);
    test.cast("Earthstrike");
    assert_eq!(melee_ap(&test), ap + 280);
}

#[test]
fn on_use_spells_the_sim_cannot_run_are_not_castable() {
    let mut test = test("unsupported uses");
    test.equip(EquipmentSlot::Trinket1, ZANDALARIAN_HERO_MEDALLION);
    test.equip(EquipmentSlot::Trinket2, BADGE_OF_THE_SWARMGUARD);
    for name in ["Restless Strength", "Badge of the Swarmguard"] {
        assert!(
            test.with_ctx(|ctx| ctx.spell_by_name(name, MAX_RANK))
                .is_none(),
            "{name}"
        );
    }
    // Not registered at all, so a cast cannot start the trinket category for nothing.
    assert_eq!(test.character().spells().equipment_spells().count(), 0);
}

const TRINKET_ROTATION: &str = r#"
class: WARRIOR
name: Trinkets
cast_if:
  - name: Kiss of the Spider
  - name: Slayer's Crest
"#;

#[test]
fn the_rotation_casts_on_use_trinkets_by_name() {
    let mut test = test("on-use rotation");
    let spec: RotationSpec = serde_yaml::from_str(TRINKET_ROTATION).unwrap();
    test.with_ctx(|ctx| ctx.set_rotation(Arc::new(spec)));
    let active = |test: &SpellTest| {
        let rotation = test.character().rotation().unwrap();
        rotation.active_executors().count()
    };
    assert_eq!(active(&test), 0, "nothing equipped");
    test.equip(EquipmentSlot::Trinket1, KISS_OF_THE_SPIDER);
    test.equip(EquipmentSlot::Trinket2, SLAYERS_CREST);
    assert_eq!(active(&test), 2, "equipping links the executors");

    test.with_ctx(|ctx| ctx.perform_rotation());
    assert!(test.buff_is_active("Kiss of the Spider"));
    assert!(!test.buff_is_active("Slayer's Crest"));
    let stats = test
        .character()
        .rotation()
        .unwrap()
        .statistics_by_spell()
        .into_iter()
        .map(|(name, stats)| (name.to_string(), stats))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(stats["Kiss of the Spider"].successful_casts, 1);
    assert_eq!(stats["Slayer's Crest"].successful_casts, 0);
    assert_eq!(
        stats["Slayer's Crest"].spell_status[&SpellStatus::OnCooldown],
        1
    );

    test.unequip(EquipmentSlot::Trinket2);
    assert_eq!(active(&test), 1, "unequipping unlinks the executor");
}

/// Jom Gabbar: use 29602, +65 attack power at once and every 2 s for 20 s (stacks of 29604).
const JOM_GABBAR: u32 = 23570;

#[test]
fn jom_gabbar_stacks_end_with_the_trinket_aura() {
    let mut test = test("Jom Gabbar");
    test.equip(EquipmentSlot::Trinket1, JOM_GABBAR);
    let ap = melee_ap(&test);
    test.cast("Jom Gabbar");
    assert_eq!(melee_ap(&test), ap + 65);
    test.when_running_queued_events_until(19.0);
    assert_eq!(melee_ap(&test), ap + 650, "ten stacks");
    // The stacks have no duration of their own (`ends_auras`).
    test.when_running_queued_events_until(21.0);
    assert_eq!(melee_ap(&test), ap);
}
