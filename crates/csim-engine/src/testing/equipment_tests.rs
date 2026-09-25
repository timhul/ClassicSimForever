//! The spells the equipment grants (`CharacterContext::sync_equipment_spells`) on the shipped
//! data: on-equip proc auras and auras, chance-on-hit spells and set bonuses.

use super::warrior::WarriorTest;
use super::SpellTest;
use crate::character_spells::SpellHandle;
use crate::ids::ProcId;
use crate::item::EquipmentSlot;
use crate::proc::runtime::PROC_ROLL_RANGE;
use crate::proc::ProcSource;

/// Hand of Justice (legacy trinket): on-equip proc aura 15600.
const HAND_OF_JUSTICE: u32 = 11815;
/// Thunderfury, Blessed Blade of the Windseeker: chance on hit 21992.
const THUNDERFURY: u32 = 19019;
/// Vis'kag the Bloodletter (legacy one-hander): chance on hit 1305394 (Fatal Wound).
const VISKAG: u32 = 17075;
/// General's Plate Gauntlets: on-equip 22778, Hamstring costs 3 rage less.
const GENERALS_GAUNTLETS: u32 = 16548;
/// Mace of Unending Life: on-equip 26153, +140 attack power in Cat and Bear forms only.
const MACE_OF_UNENDING_LIFE: u32 = 21407;
/// Dal'Rend's Arms (set 41): the two swords, +50 attack power with both.
const DAL_REND_MH: u32 = 12940;
const DAL_REND_OH: u32 = 12939;
/// Battlegear of Wrath (set 218) by slot: (3) Enhanced Battle Shout, (5) Warrior's Wrath,
/// (8) Parry.
const WRATH: [(EquipmentSlot, u32); 8] = [
    (EquipmentSlot::Wrist, 16959),
    (EquipmentSlot::Belt, 16960),
    (EquipmentSlot::Shoulders, 16961),
    (EquipmentSlot::Legs, 16962),
    (EquipmentSlot::Head, 16963),
    (EquipmentSlot::Gloves, 16964),
    (EquipmentSlot::Boots, 16965),
    (EquipmentSlot::Chest, 16966),
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

fn melee_ap(test: &SpellTest) -> u32 {
    let view = test.target().stat_view();
    test.character().melee_ap(&view)
}

fn hamstring_cost(test: &mut SpellTest) -> u32 {
    let id = test.spell("Hamstring");
    test.with_ctx(|ctx| ctx.with_spell(id, |spell, ctx| spell.resource_cost(ctx)))
}

#[test]
fn hand_of_justice_grants_an_extra_attack() {
    let mut test = test("Hand of Justice");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.equip(EquipmentSlot::Trinket1, HAND_OF_JUSTICE);

    let proc = test.proc("Hand of Justice");
    assert!(test.character().spells().procs().is_enabled(proc));
    // 1 % against anything but a Dwarf (the table's 3 % is the Dwarf chance).
    for source in [
        ProcSource::MainhandSwing,
        ProcSource::MainhandSpell,
        ProcSource::OffhandSwing,
    ] {
        assert_eq!(
            test.proc_range("Hand of Justice", source),
            PROC_ROLL_RANGE / 100
        );
    }
    assert!(!test.proc_conditions_fulfilled("Hand of Justice", ProcSource::RangedAutoShot));

    test.with_ctx(|ctx| ctx.perform_proc(proc));
    assert_eq!(test.character().pending_extra_attacks(), 1);
    let swings = test.with_ctx(|ctx| ctx.perform_extra_attacks());
    assert_eq!(swings.len(), 1);
    assert_eq!(test.character().pending_extra_attacks(), 0);
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
    let mut test = test("Vis'kag");
    test.given_a_mainhand_weapon_with_100_min_max_dmg();
    test.equip(EquipmentSlot::Offhand, VISKAG);
    assert_eq!(
        test.proc_range("Fatal Wound", ProcSource::OffhandSwing),
        260,
        "ClassicSim's 2.6 %"
    );
    assert!(test.proc_conditions_fulfilled("Fatal Wound", ProcSource::OffhandSwing));
    assert!(!test.proc_conditions_fulfilled("Fatal Wound", ProcSource::MainhandSwing));
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
    let mut test = test("Dal'Rend's Arms");
    let none = melee_ap(&test);
    test.equip(EquipmentSlot::Mainhand, DAL_REND_MH);
    let mainhand = melee_ap(&test);
    test.equip(EquipmentSlot::Offhand, DAL_REND_OH);
    let both = melee_ap(&test);
    test.unequip(EquipmentSlot::Mainhand);
    let offhand = melee_ap(&test);
    // Each sword's own stats count once; the pair adds the bonus.
    assert_eq!(both + none - mainhand - offhand, 50);
}

#[test]
fn set_bonuses_follow_the_pieces_worn() {
    let mut test = test("Battlegear of Wrath");
    // (3) Enhanced Battle Shout, (5) Warrior's Wrath, (8) Parry.
    let bonuses = |test: &mut SpellTest| {
        let shout = test
            .character()
            .spells()
            .owned_buff_by_name("Enhanced Battle Shout")
            .is_some_and(|id| test.with_buff_id(id, crate::buff::Buff::is_active));
        (
            shout,
            proc_enabled(test, "Warrior's Wrath"),
            proc_enabled(test, "Parry"),
        )
    };
    for (worn, &(slot, item)) in WRATH.iter().enumerate() {
        test.equip(slot, item);
        let pieces = worn as u32 + 1;
        assert_eq!(test.character().equipment().set_pieces(218), pieces);
        assert_eq!(
            bonuses(&mut test),
            (pieces >= 3, pieces >= 5, pieces >= 8),
            "{pieces} pieces"
        );
    }
    for &(slot, _) in &WRATH[4..] {
        test.unequip(slot);
    }
    assert_eq!(bonuses(&mut test), (true, false, false), "4 pieces");
    for &(slot, _) in &WRATH[..4] {
        test.unequip(slot);
    }
    assert_eq!(bonuses(&mut test), (false, false, false), "no pieces");
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
    test.equip(EquipmentSlot::Trinket1, HAND_OF_JUSTICE);
    test.equip(EquipmentSlot::Mainhand, THUNDERFURY);
    test.equip(EquipmentSlot::Gloves, GENERALS_GAUNTLETS);
    assert_eq!(enabled_equipment_spells(&test), 3);
    let registered = test.character().spells().equipment_spells().count();

    for slot in [
        EquipmentSlot::Trinket1,
        EquipmentSlot::Mainhand,
        EquipmentSlot::Gloves,
    ] {
        test.unequip(slot);
    }
    assert_eq!(enabled_equipment_spells(&test), 0);
    assert_eq!(hamstring_cost(&mut test), 10);

    // Equipping again reuses the registration.
    test.equip(EquipmentSlot::Trinket1, HAND_OF_JUSTICE);
    assert!(proc_enabled(&test, "Hand of Justice"));
    assert_eq!(
        test.character().spells().equipment_spells().count(),
        registered
    );
}
