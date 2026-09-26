//! Port of `Test/Warrior/TestWarrior` (`test_values_after_initialization`).

use crate::character::STANCE_COOLDOWN;
use crate::enchant::EnchantName;
use crate::faction::PlayerClass;
use crate::item::{ArmorType, EquipmentSlot};
use crate::race::Race;
use crate::testing::data;
use crate::testing::warrior::WarriorTest;

#[test]
fn values_after_initialization() {
    let mut test = WarriorTest::new("Warrior");
    let character = test.character();
    assert_eq!(character.class_kind(), PlayerClass::Warrior);
    assert_eq!(character.race(), Race::Orc);
    assert_eq!(character.class().highest_armor_type, ArmorType::Plate);
    assert_eq!(character.global_cooldown(), 1.5);
    assert_eq!(STANCE_COOLDOWN, 1.0);
    assert_eq!(
        test.stat(|stats, ctx| stats.get_melee_ability_crit_dmg_mod(ctx)),
        2.0
    );
    assert_eq!(
        test.stat(|stats, ctx| stats.get_spell_crit_dmg_mod(ctx)),
        1.5
    );

    // Shadow Oil on both hands.
    test.equip(EquipmentSlot::Mainhand, 12584);
    assert_eq!(
        test.character().equipment().mainhand().unwrap().name(),
        "Grand Marshal's Longsword"
    );
    test.equip(EquipmentSlot::Offhand, 12783);
    assert_eq!(
        test.character().equipment().offhand().unwrap().name(),
        "Heartseeker"
    );
    let db = &data().spells;
    for slot in [EquipmentSlot::Mainhand, EquipmentSlot::Offhand] {
        test.with_ctx(|ctx| ctx.set_temp_enchants(db, slot, &[EnchantName::ShadowOil]))
            .expect("Shadow Oil goes on a weapon");
    }
}
