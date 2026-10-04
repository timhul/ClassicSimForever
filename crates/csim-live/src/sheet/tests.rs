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
    // 10 % from gear and 10 % from Dual Wield Specialization; Cruelty's 5 % crit.
    assert_eq!((melee.hit, melee.crit), (20.0, 35.1));
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
