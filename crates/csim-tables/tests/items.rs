//! Item derivation on the fixture dump (`tests/fixtures/tables`): the worked examples of
//! `data/ITEM_INSTRUCTIONS.md` §1.4–1.9 plus the Forever-specific rules found on the real dump.

use std::collections::BTreeMap;
use std::path::Path;

use csim_engine::faction::PlayerClass;
use csim_engine::item::{
    EquipmentDb, ItemFile, ItemSetFile, ItemSlot, ItemSpec, ItemStat, ItemType, Quality,
};
use csim_engine::magic_school::MagicSchool;
use csim_engine::phase::Phase;
use csim_tables::export;
use csim_tables::export::items::{
    DerivedItem, EffectTrigger, ItemEffect, LimitCategory, Skip, StatKind, WeaponDamage,
    class_restrictions, damage_school, derive_item, derive_items, newest_versions, stat_kind,
    stat_value,
};
use csim_tables::{IconNames, Tables};

fn tables() -> Tables {
    Tables::load_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tables")).unwrap()
}

fn derive(t: &Tables, id: u32) -> DerivedItem {
    let mut issues = Vec::new();
    let item = derive_item(t, id, &mut issues).unwrap_or_else(|skip| panic!("{id}: {skip:?}"));
    assert!(issues.is_empty(), "{id}: {issues:?}");
    item
}

fn stats(pairs: &[(ItemStat, f64)]) -> BTreeMap<ItemStat, f64> {
    pairs.iter().copied().collect()
}

#[test]
fn thunderfury_stats_damage_and_proc() {
    let t = tables();
    let item = derive(&t, 19019);
    assert_eq!(item.name, "Thunderfury, Blessed Blade of the Windseeker");
    assert_eq!(
        (item.slot, item.item_type, item.quality),
        (ItemSlot::OneHand, ItemType::Sword, Quality::Legendary)
    );
    assert_eq!((item.item_level, item.required_level), (80, 60));
    assert!(item.unique);
    assert!(!item.boe);
    // §1.4: Epic_3[80] = 23.
    assert_eq!(
        item.stats,
        stats(&[
            (ItemStat::Agility, 5.0),
            (ItemStat::Stamina, 8.0),
            (ItemStat::FireResistance, 8.0),
            (ItemStat::NatureResistance, 9.0),
        ])
    );
    // QualityModifier −20: 20 % below the one-hand DPS of its item level and quality.
    assert_eq!(
        item.damage,
        Some(WeaponDamage {
            min: 65,
            max: 122,
            speed: 1.9,
            school: 0
        })
    );
    assert_eq!(
        item.effects,
        [ItemEffect {
            trigger: EffectTrigger::OnHit,
            spell: 21992,
            cooldown_ms: None,
            category: None,
            category_cooldown_ms: None,
            charges: 0,
        }]
    );
    assert!(item.class_restrictions.is_empty());
    assert_eq!(item.set, None);
}

#[test]
fn ratings_are_kept_as_ratings() {
    let t = tables();
    // Lionheart Helm: Epic_0[61] = 45, plate head armor.
    let helm = derive(&t, 12640);
    assert_eq!(
        (helm.slot, helm.item_type),
        (ItemSlot::Head, ItemType::Plate)
    );
    assert_eq!(
        helm.stats,
        stats(&[
            (ItemStat::Strength, 18.0),
            (ItemStat::Armor, 565.0),
            (ItemStat::HitRating, 20.0),
            (ItemStat::CritRating, 28.0),
        ])
    );
    // Drake Fang Talisman: stats only.
    let talisman = derive(&t, 19406);
    assert_eq!(
        talisman.stats,
        stats(&[
            (ItemStat::AttackPower, 56.0),
            (ItemStat::HitRating, 20.0),
            (ItemStat::DodgeRating, 12.0),
        ])
    );
    assert!(talisman.effects.is_empty());
}

#[test]
fn ranged_thrown_and_two_hand_weapons() {
    let t = tables();
    // AQ40 bow: the fifth budget column (Epic_4[78] = 17).
    let bow = derive(&t, 21616);
    assert_eq!((bow.slot, bow.item_type), (ItemSlot::Ranged, ItemType::Bow));
    assert_eq!(bow.stats, stats(&[(ItemStat::Agility, 18.0)]));
    assert_eq!(
        bow.damage,
        Some(WeaponDamage {
            min: 87,
            max: 163,
            speed: 2.7,
            school: 0
        })
    );
    let thrown = derive(&t, 21135);
    assert_eq!(
        (thrown.slot, thrown.item_type),
        (ItemSlot::Ranged, ItemType::Thrown)
    );
    assert_eq!(thrown.damage.unwrap().speed, 3.0);
    let greatsword = derive(&t, 18877);
    assert_eq!(
        (greatsword.slot, greatsword.item_type),
        (ItemSlot::TwoHand, ItemType::TwohandSword)
    );
    assert_eq!(
        greatsword.damage,
        Some(WeaponDamage {
            min: 235,
            max: 353,
            speed: 3.8,
            school: 0
        })
    );
}

#[test]
fn armor_shields_and_bonus_armor() {
    let t = tables();
    let shield = derive(&t, 18168);
    assert_eq!(
        (shield.slot, shield.item_type),
        (ItemSlot::Offhand, ItemType::Shield)
    );
    assert_eq!(shield.stats[&ItemStat::Armor], 2548.0);
    assert_eq!(
        shield.damage,
        Some(WeaponDamage {
            min: 1,
            max: 1,
            speed: 1.0,
            school: 0
        })
    );
    assert_eq!(shield.effects.len(), 2);
    assert!(
        shield
            .effects
            .iter()
            .all(|e| e.trigger == EffectTrigger::Equip)
    );

    let cloak = derive(&t, 18509);
    assert_eq!(
        (cloak.slot, cloak.item_type),
        (ItemSlot::Back, ItemType::Cloth)
    );
    assert_eq!(cloak.stats[&ItemStat::Armor], 48.0);

    // Sandstalker Breastplate: QualityModifier 120 is bonus armor on top of the mail chest.
    let chest = derive(&t, 20478);
    assert_eq!(
        (chest.slot, chest.item_type),
        (ItemSlot::Chest, ItemType::Mail)
    );
    assert_eq!(chest.stats[&ItemStat::Armor], 485.0);

    let tome = derive(&t, 19308);
    assert_eq!(
        (tome.slot, tome.item_type),
        (ItemSlot::Offhand, ItemType::CasterOffhand)
    );
    assert_eq!(tome.stats[&ItemStat::SpellDamageArcane], 34.0);
    assert!(!tome.stats.contains_key(&ItemStat::Armor));
}

#[test]
fn relics_uses_limits_sets_and_classes() {
    let t = tables();
    let libram = derive(&t, 23203);
    assert_eq!(
        (libram.slot, libram.item_type),
        (ItemSlot::Relic, ItemType::Libram)
    );
    assert_eq!(libram.effects[0].trigger, EffectTrigger::Equip);
    assert_eq!(libram.effects[0].spell, 28852);

    let earthstrike = derive(&t, 21180);
    assert_eq!(
        earthstrike.effects,
        [ItemEffect {
            trigger: EffectTrigger::Use,
            spell: 25891,
            cooldown_ms: Some(120_000),
            category: Some(1141),
            category_cooldown_ms: Some(20_000),
            charges: 0,
        }]
    );

    let grand_master = derive(&t, 19024);
    assert_eq!(
        grand_master.limit_category,
        Some(LimitCategory {
            id: 718,
            name: "Arena Master".into(),
            quantity: 1
        })
    );
    assert_eq!(grand_master.stats, stats(&[(ItemStat::DodgeRating, 12.0)]));
    let triggers: Vec<EffectTrigger> = grand_master.effects.iter().map(|e| e.trigger).collect();
    assert_eq!(
        triggers,
        [EffectTrigger::Equip, EffectTrigger::Use],
        "legacy slot order"
    );

    let conqueror = derive(&t, 21331);
    assert_eq!(conqueror.set, Some(496));
    assert_eq!(conqueror.class_restrictions, [PlayerClass::Warrior]);
}

#[test]
fn random_suffixes_use_the_item_budget() {
    let t = tables();
    // Green Lens: rare head, Superior_0[49] = 28, plus a suffix pool.
    let lens = derive(&t, 10504);
    assert_eq!(lens.stats[&ItemStat::Stamina], 10.0);
    assert_eq!(lens.suffixes.len(), 10);
    assert_eq!(lens.suffixes[0].name, "of Magic");
    assert_eq!(
        lens.suffixes[0].stats,
        stats(&[(ItemStat::SpellDamage, 28.0)])
    );
    let healing = &lens.suffixes[1];
    assert_eq!(healing.name, "of Healing");
    assert!(healing.stats.contains_key(&ItemStat::HealingPower));
}

#[test]
fn skipped_items_say_why() {
    let t = tables();
    let mut issues = Vec::new();
    let skip = |id| derive_item(&t, id, &mut Vec::new()).unwrap_err();
    assert_eq!(skip(11815), Some(Skip::NoSparseRow), "Hand of Justice");
    assert_eq!(skip(18706), Some(Skip::Quality), "Arena Master is uncommon");
    assert_eq!(skip(8210), Some(Skip::Quality));
    assert_eq!(skip(17802), Some(Skip::Deprecated));
    assert_eq!(skip(1), None);

    let report = derive_items(&t);
    let ids: Vec<u32> = report.items.iter().map(|i| i.id).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    assert!(ids.contains(&19019) && !ids.contains(&11815));
    assert_eq!(report.skipped[&Skip::NoSparseRow], [11815]);
    assert_eq!(report.skipped[&Skip::Quality], [8210, 18706]);
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert!(derive_item(&t, 19019, &mut issues).is_ok());
}

#[test]
fn stat_ids_follow_the_item_mod_order() {
    assert_eq!(stat_kind(31), StatKind::Stat(ItemStat::HitRating));
    assert_eq!(stat_kind(32), StatKind::Stat(ItemStat::CritRating));
    assert_eq!(stat_kind(90), StatKind::Stat(ItemStat::TwohandAxeSkill));
    assert_eq!(stat_kind(96), StatKind::Stat(ItemStat::DaggerSkill));
    assert_eq!(stat_kind(98), StatKind::Stat(ItemStat::FistSkill));
    assert_eq!(stat_kind(103), StatKind::Stat(ItemStat::SwordSkill));
    assert_eq!(stat_kind(127), StatKind::Stat(ItemStat::AttackPowerDemon));
    assert_eq!(stat_kind(131), StatKind::Stat(ItemStat::AttackPowerBeast));
    assert_eq!(stat_kind(136), StatKind::Stat(ItemStat::SpellDamageUndead));
    assert_eq!(stat_kind(83), StatKind::Stat(ItemStat::WeaponDamage));
    assert_eq!(stat_kind(50), StatKind::BonusArmor);
    assert_eq!(stat_kind(117), StatKind::Ignored);
    assert!(matches!(stat_kind(101), StatKind::Unsupported(_)));
    assert_eq!(stat_kind(119), StatKind::Unknown);
    assert_eq!(stat_value(2174, 23), 5.0);
    assert_eq!(stat_value(3913, 23), 9.0);
    assert_eq!(stat_value(-5000, 10), -5.0);
}

#[test]
fn class_masks() {
    assert!(class_restrictions(-1).is_empty());
    assert!(class_restrictions(32767).is_empty());
    assert_eq!(class_restrictions(1), [PlayerClass::Warrior]);
    assert_eq!(
        class_restrictions(2 | 1024),
        [PlayerClass::Paladin, PlayerClass::Druid]
    );
}

#[test]
fn item_files_render_by_slot_and_load_through_the_engine() {
    let t = tables();
    let report = derive_items(&t);
    let files = export::item_files(&t, &report.items);
    assert_eq!(
        files["one_hand"]
            .items
            .iter()
            .map(|i| i.id)
            .collect::<Vec<_>>(),
        [19019]
    );
    assert!(files.values().all(|f| f.build == t.build()));
    assert!(
        files
            .values()
            .all(|f| f.items.iter().all(|i| i.phase == Phase::MoltenCore))
    );

    let dir = std::env::temp_dir().join(format!("csim-export-items-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, file) in &files {
        let text = export::render_items(file, "export-items").unwrap();
        assert!(text.starts_with("# Generated by `csim-tables export-items`"));
        assert!(!text.contains(".0\n"), "whole numbers are written bare");
        let again: ItemFile = serde_yaml::from_str(&text).unwrap();
        assert_eq!(&again, file);
        std::fs::write(dir.join(format!("{name}.yaml")), text).unwrap();
    }
    let db = EquipmentDb::load(&dir, None, None).unwrap();
    assert_eq!(db.len(), report.items.len());
    assert_eq!(db.build(), Some(t.build()));
    let thunderfury = db.get_item(19019, Phase::MoltenCore).unwrap();
    assert_eq!(thunderfury.effects()[0].spell, 21992);
    assert_eq!(thunderfury.damage_school(), Some(MagicSchool::Physical));
    assert_eq!(
        db.get_item(19024, Phase::MoltenCore)
            .unwrap()
            .limit_category()
            .unwrap()
            .id,
        718
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn item_sets_list_members_and_bonus_spells() {
    let t = tables();
    let report = derive_items(&t);
    let sets = export::item_set_file(&t, &report.items);
    let conqueror = sets.sets.iter().find(|s| s.id == 496).unwrap();
    assert_eq!(conqueror.name, "Conqueror's Battlegear");
    for member in [21329, 21330, 21331, 21332, 21333] {
        assert!(conqueror.items.contains(&member), "{member}");
    }
    assert!(!conqueror.bonuses.is_empty());
    assert!(
        conqueror
            .bonuses
            .windows(2)
            .all(|w| w[0].pieces <= w[1].pieces)
    );
    assert!(sets.sets.windows(2).all(|w| w[0].id < w[1].id));

    let text = export::render_item_sets(&sets, "export-items").unwrap();
    let again: ItemSetFile = serde_yaml::from_str(&text).unwrap();
    assert_eq!(again, sets);
}

#[test]
fn damage_types_are_schools() {
    assert_eq!(damage_school(0), MagicSchool::Physical);
    assert_eq!(damage_school(2), MagicSchool::Fire);
    assert_eq!(damage_school(5), MagicSchool::Shadow);
    assert_eq!(damage_school(6), MagicSchool::Arcane);
}

#[test]
fn comparison_converts_ratings_and_names_each_difference() {
    let t = tables();
    let helm = derive(&t, 12640).to_spec();
    // The Classic helm: 2 % hit and 2 % crit as chance fractions.
    let mut classic = helm.clone();
    classic.stats.remove(&ItemStat::HitRating);
    classic.stats.remove(&ItemStat::CritRating);
    classic.stats.insert(ItemStat::HitChance, 0.02);
    classic.stats.insert(ItemStat::CritChance, 0.02);
    classic.flavour_text = "Not compared.".into();
    classic.class_restrictions = vec![PlayerClass::Paladin, PlayerClass::Warrior];
    let mut exported = helm.clone();
    exported.class_restrictions = vec![PlayerClass::Warrior, PlayerClass::Paladin];
    let legacy = newest_versions(vec![classic.clone()]);
    assert!(export::compare_items(&[exported.clone()], &legacy).is_empty());

    // The newest phase is the one compared.
    let mut old = classic.clone();
    old.stats.insert(ItemStat::Strength, 1.0);
    let mut newer = classic;
    newer.phase = Phase::AhnQiraj;
    newer.stats.insert(ItemStat::Strength, 20.0);
    newer.req_lvl = 55;
    let legacy = newest_versions(vec![newer, old]);
    assert_eq!(
        export::compare_items(&[exported], &legacy),
        [
            format!("12640 Lionheart Helm: req_lvl: 55 -> {}", helm.req_lvl),
            "12640 Lionheart Helm: stat STRENGTH: 20 -> 18".to_owned(),
        ]
    );
}

#[test]
fn icon_names_come_from_the_listfile() {
    let mut t = tables();
    let item = derive(&t, 19019);
    assert_eq!((item.icon, item.icon_name.as_deref()), (135349, None));
    let yaml = serde_yaml::to_string(&item.to_spec()).unwrap();
    assert!(!yaml.contains("icon_name"), "no listfile, no name: {yaml}");

    let listfile = "135349;interface/icons/inv_sword_39.blp";
    t.set_icon_names(IconNames::parse(listfile, Path::new("listfile-icons.csv")).unwrap());
    let item = derive(&t, 19019);
    assert_eq!(item.icon_name.as_deref(), Some("inv_sword_39"));
    let spec = item.to_spec();
    let yaml = serde_yaml::to_string(&spec).unwrap();
    assert!(
        yaml.contains("icon: 135349\nicon_name: inv_sword_39\n"),
        "{yaml}"
    );
    assert_eq!(serde_yaml::from_str::<ItemSpec>(&yaml).unwrap(), spec);
    // An icon the listfile does not name.
    let helm = derive(&t, 12640);
    assert_ne!(helm.icon, 0);
    assert_eq!(helm.icon_name, None);
}
