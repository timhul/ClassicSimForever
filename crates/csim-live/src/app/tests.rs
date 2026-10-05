//! App tests on the shipped data.

use std::path::Path;

use super::*;
use crate::session::tests::empty_app;

fn load(app: &mut App, request: LoadRequest) -> Result<Loaded, String> {
    app.load(request, || 42)
}

fn by_name(setup: &str) -> LoadRequest {
    LoadRequest {
        setup: Some(setup.to_owned()),
        ..LoadRequest::default()
    }
}

#[test]
fn the_catalog_lists_the_setups_keybinds_and_settings() {
    let catalog = empty_app().catalog();
    let fury = catalog
        .setups
        .iter()
        .find(|setup| setup.name == "warrior_fury_dw_orc")
        .unwrap();
    assert_eq!(fury.title, "DW Fury Orc");
    assert_eq!((fury.class, fury.race), ("Warrior", "Orc"));
    assert_eq!(fury.rotation, "DW Fury");
    assert_eq!(fury.error, None);
    assert!(
        catalog.setups.iter().all(|setup| setup.error.is_none()),
        "every shipped setup loads"
    );
    assert!(
        catalog
            .setups
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name),
        "sorted by name"
    );
    assert!(
        !catalog
            .setups
            .iter()
            .any(|setup| setup.name.contains("common")),
        "not the shared parts"
    );
    assert!(catalog.keybinds.contains(&"dw_fury".to_owned()));
    assert!(
        catalog
            .settings
            .iter()
            .any(|setting| setting.name == "rage_formula")
    );
    assert_eq!(catalog.length, SimSettings::default().combat_length);
}

#[test]
fn the_names_of_files_in_the_data_directory() {
    let app = empty_app();
    let dir = DataBundle::repository_dir();
    let characters = dir.join("characters");
    assert_eq!(
        app.setup_name(&characters.join("warrior_fury_dw_orc.yaml"))
            .as_deref(),
        Some("warrior_fury_dw_orc")
    );
    // The same file by another path.
    assert_eq!(
        app.setup_name(&characters.join("common/../warrior_fury_dw_orc.yaml"))
            .as_deref(),
        Some("warrior_fury_dw_orc")
    );
    assert_eq!(
        app.setup_name(&characters.join("common/base_buffs.yaml")),
        None
    );
    assert_eq!(app.setup_name(Path::new("elsewhere/warrior.yaml")), None);
    assert_eq!(
        app.keybinds_name(&dir.join("keybinds/dw_fury.yaml"))
            .as_deref(),
        Some("dw_fury")
    );
}

#[test]
fn a_failed_load_keeps_the_session() {
    let mut app = empty_app();
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    assert_eq!(loaded.info.seed, 42, "a new seed without one");
    for request in [
        by_name("no_such_setup"),
        by_name("../characters/warrior_fury_dw_orc"),
        by_name("common/base_buffs"),
        LoadRequest {
            seed: Some("soon".into()),
            ..by_name("rogue_combat_swords_human")
        },
    ] {
        assert!(load(&mut app, request).is_err());
    }
    assert_eq!(app.loaded().unwrap().info.name, "DW Fury Orc");
}

#[test]
fn a_load_changes_the_targets_creature_type_and_armor() {
    let mut app = empty_app();
    assert!(app.catalog().creature_types.contains(&"Undead"));
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    let setup_target = TargetChoice {
        setup_creature_type: CreatureType::Dragonkin,
        setup_armor: 3731,
        creature_type: None,
        armor: None,
    };
    assert_eq!(loaded.target, setup_target);

    let request = || LoadRequest {
        target_creature_type: Some(CreatureType::Undead),
        target_armor: Some(0),
        ..by_name("warrior_fury_dw_orc")
    };
    let loaded = load(&mut app, request()).unwrap();
    assert_eq!(loaded.target.creature_type, Some(CreatureType::Undead));
    assert_eq!(loaded.target.armor, Some(0));
    let session = app.session().unwrap();
    assert_eq!(session.target().creature_type, CreatureType::Undead);
    assert_eq!(session.target().armor, 0);

    // The setup's own values are no change.
    let loaded = load(
        &mut app,
        LoadRequest {
            target_creature_type: Some(CreatureType::Dragonkin),
            target_armor: Some(3731),
            ..by_name("warrior_fury_dw_orc")
        },
    )
    .unwrap();
    assert_eq!(loaded.target, setup_target);

    let negative = LoadRequest {
        target_armor: Some(-1),
        ..request()
    };
    assert!(load(&mut app, negative).is_err());
}

#[test]
fn a_load_changes_the_gear() {
    use csim_engine::character_loader::DroppedEnchant;
    use csim_engine::enchant::EnchantName;
    use csim_engine::item::EquipmentSlot::{Mainhand, Offhand};

    let mut app = empty_app();
    let change = |slot, item| GearChange { slot, item };
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    assert_eq!(loaded.gear, GearChanged::default());

    // Arcanite Reaper empties the off hand and keeps the main hand's enchants.
    let request: LoadRequest = serde_json::from_str(
        r#"{"setup": "warrior_fury_dw_orc", "gear": [{"slot": "MAINHAND", "item": 12784}]}"#,
    )
    .unwrap();
    let loaded = load(&mut app, request).unwrap();
    assert_eq!(
        loaded.gear.changes,
        [change(Mainhand, Some(12784)), change(Offhand, None)]
    );
    assert!(loaded.gear.dropped_enchants.is_empty());

    // High Warlord's Shield Wall takes no Crusader.
    let shield = LoadRequest {
        gear: vec![change(Offhand, Some(18826))],
        ..by_name("warrior_fury_dw_orc")
    };
    let loaded = load(&mut app, shield).unwrap();
    assert_eq!(loaded.gear.changes, [change(Offhand, Some(18826))]);
    assert_eq!(
        loaded.gear.dropped_enchants.first(),
        Some(&DroppedEnchant {
            slot: Offhand,
            enchant: EnchantName::Crusader
        })
    );
    let json = serde_json::to_value(&loaded).unwrap();
    assert_eq!(
        json["gear"]["dropped_enchants"][0],
        serde_json::json!({"slot": "OFFHAND", "enchant": "Crusader"})
    );

    // An unknown item, and an item the class cannot use (a priest's staff), keep the session.
    for item in [1, 18608] {
        let bad = LoadRequest {
            gear: vec![change(Mainhand, Some(item))],
            ..by_name("warrior_fury_dw_orc")
        };
        assert!(load(&mut app, bad).is_err(), "item {item}");
    }
    assert_eq!(
        app.loaded().unwrap().gear.changes,
        [change(Offhand, Some(18826))]
    );

    // Another setup by itself: the setup's gear.
    let loaded = load(&mut app, by_name("rogue_combat_swords_human")).unwrap();
    assert_eq!(loaded.gear, GearChanged::default());
}

/// The session's talents, as a load request names them.
fn spent_talents(app: &App) -> Talents {
    let talents = app.session().unwrap().talents().unwrap();
    talents
        .trees()
        .iter()
        .map(|tree| {
            let ranks = tree
                .talents()
                .iter()
                .filter(|talent| talent.rank() > 0)
                .map(|talent| (talent.name().to_owned(), talent.rank()))
                .collect();
            (tree.name().to_owned(), ranks)
        })
        .filter(|(_, ranks): &(String, BTreeMap<String, u32>)| !ranks.is_empty())
        .collect()
}

#[test]
fn a_load_changes_the_talents() {
    let mut app = empty_app();
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    assert!(!loaded.talents_changed);
    let own = spent_talents(&app);
    assert_eq!(own["Fury"]["Bloodthirst"], 1);

    // The setup's own talents, with a rank 0 entry, are no change.
    let mut same = own.clone();
    same.get_mut("Arms").unwrap().insert("Deflection".into(), 0);
    let request = |talents| LoadRequest {
        talents: Some(talents),
        ..by_name("warrior_fury_dw_orc")
    };
    let loaded = load(&mut app, request(same)).unwrap();
    assert!(!loaded.talents_changed);

    // Improved Heroic Strike's 3 points in Deflection instead.
    let mut moved = own.clone();
    let arms = moved.get_mut("Arms").unwrap();
    arms.remove("Improved Heroic Strike");
    arms.insert("Deflection".into(), 3);
    let loaded = load(&mut app, request(moved.clone())).unwrap();
    assert!(loaded.talents_changed);
    assert_eq!(spent_talents(&app), moved);
    let json = serde_json::to_value(&loaded).unwrap();
    assert_eq!(json["talents_changed"], true);

    // Talents that cannot be spent, and a build without the rotation's Bloodthirst, keep the
    // session.
    let edit = |tab: &str, name: &str, rank| {
        let mut talents = own.clone();
        talents
            .entry(tab.to_owned())
            .or_default()
            .insert(name.to_owned(), rank);
        talents
    };
    for (bad, why) in [
        (edit("Arms", "No Such Talent", 1), "unknown talent"),
        (edit("Holy", "Cruelty", 1), "unknown tab"),
        (edit("Fury", "Cruelty", 6), "above the maximum"),
        (edit("Arms", "Mortal Strike", 1), "tier not unlocked"),
        (
            edit("Fury", "Bloodthirst", 0),
            "the rotation's prerequisite",
        ),
    ] {
        assert!(load(&mut app, request(bad)).is_err(), "{why}");
    }
    assert_eq!(spent_talents(&app), moved);
    assert!(app.loaded().unwrap().talents_changed);

    // Another setup by itself: its own talents.
    let loaded = load(&mut app, by_name("rogue_combat_swords_human")).unwrap();
    assert!(!loaded.talents_changed);
}
