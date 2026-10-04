//! Character sheet tests on the shipped data.

use csim_engine::item::EquipmentSlot;

use crate::app::{LoadRequest, Loaded};
use crate::session::tests::empty_app;

fn load(request: LoadRequest) -> Loaded {
    empty_app().load(request, || 1).unwrap()
}

fn pasted(yaml: &str) -> Loaded {
    load(LoadRequest {
        setup_yaml: Some(yaml.to_owned()),
        ..LoadRequest::default()
    })
}

const NAKED: &str = "name: Naked\nclass: WARRIOR\nrace: ORC\nrotation: Protection\n";

#[test]
fn a_naked_orc_warrior_has_the_base_stats() {
    // The numbers of the C++ GUI's stat summary without gear.
    let stats = pasted(NAKED).info.stats;
    let melee = &stats.melee;
    assert_eq!(
        (
            melee.strength,
            melee.agility,
            melee.stamina,
            melee.intellect,
            melee.spirit
        ),
        (123, 77, 112, 27, 48)
    );
    assert_eq!(melee.crit, 5.85);
    assert_eq!(melee.hit, 0.0);
    assert_eq!(melee.attack_power, 406);
    assert_eq!(melee.mainhand_skill, 300);
    assert_eq!(melee.offhand_skill, None);
    assert_eq!(stats.ranged.skill, None);
    assert_eq!(stats.spell.len(), 6, "every magic school");
    assert!(pasted(NAKED).info.equipment.is_empty());
}

#[test]
fn the_gear_worn_and_its_stats() {
    let info = load(LoadRequest {
        setup: Some("warrior_fury_dw_orc".into()),
        ..LoadRequest::default()
    })
    .info;
    assert_eq!(info.equipment.len(), 17);
    let mainhand = &info.equipment[0];
    assert_eq!(mainhand.slot, EquipmentSlot::Mainhand);
    assert_eq!(
        (mainhand.id, mainhand.name.as_str()),
        (18866, "High Warlord's Bludgeon")
    );
    assert_eq!(mainhand.enchant.as_deref(), Some("Crusader"));
    assert_eq!(
        mainhand.temp_enchants,
        ["Elemental Sharpening Stone", "Windfury Totem"]
    );
    assert_eq!(
        mainhand.icon.as_ref().and_then(|icon| icon.name.as_deref()),
        Some("inv_hammer_20")
    );
    // Gear, enchants, talents and the setup's buffs, not the precombat Battle Shout.
    let melee = &info.stats.melee;
    assert_eq!((melee.strength, melee.agility), (486, 238), "{melee:?}");
    assert_eq!(melee.attack_power, 1523);
    // 10 % from gear (Dual Wield Specialization's 10 % is the off hand's only); Cruelty's 5 %
    // crit.
    assert_eq!((melee.hit, melee.crit), (10.0, 35.1));
    assert!(melee.offhand_skill.is_some());
    assert!(info.stats.ranged.skill.is_some());
}

#[test]
fn a_shield_has_no_weapon_skill() {
    // The Immovable Object.
    let yaml = format!("{NAKED}equipment:\n  OFFHAND: {{ item: 19321 }}\n");
    let info = pasted(&yaml).info;
    assert_eq!(info.equipment[0].slot, EquipmentSlot::Offhand);
    assert_eq!(info.stats.melee.offhand_skill, None);
}

#[test]
fn the_items_a_class_can_wear_in_which_slots() {
    use EquipmentSlot::{Mainhand, Offhand};
    use csim_engine::item::ItemType;

    let warrior = crate::session::tests::session_of("warrior_fury_dw_orc.yaml", 1).items();
    let rogue = crate::session::tests::session_of("rogue_combat_swords_human.yaml", 1).items();
    let find = |items: &[super::ItemEntry], id| items.iter().find(|item| item.id == id).cloned();
    assert!(
        warrior.windows(2).all(|pair| pair[0].id < pair[1].id),
        "by id"
    );

    // A one-hander a warrior dual wields; Arcanite Reaper, a two-hander, only in the main hand.
    assert_eq!(find(&warrior, 18866).unwrap().slots, [Mainhand, Offhand]);
    assert_eq!(find(&warrior, 12784).unwrap().slots, [Mainhand]);
    // A shield: off hand only, and no rogue's.
    assert_eq!(find(&warrior, 18826).unwrap().slots, [Offhand]);
    assert_eq!(find(&rogue, 18826), None);
    assert!(
        rogue
            .iter()
            .all(|item| item.item_type != ItemType::Plate && item.item_type != ItemType::Mail)
    );
    assert!(warrior.iter().any(|item| item.item_type == ItemType::Plate));
    // Benediction is a priest's; a warrior uses no wands.
    assert_eq!(find(&warrior, 18608), None);
    assert!(warrior.iter().all(|item| item.item_type != ItemType::Wand));
    // Black Dragonscale Breastplate, of Black Dragon Mail.
    let breastplate = find(&warrior, 15050).unwrap();
    assert_eq!(breastplate.set.as_deref(), Some("Black Dragon Mail"));
    assert_eq!(breastplate.item_level, 58);
    assert_eq!(breastplate.slots, [EquipmentSlot::Chest]);
}
