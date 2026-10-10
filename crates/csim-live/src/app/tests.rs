//! App tests on the shipped data.

use std::path::Path;

use csim_engine::item::EquipmentSlot;

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
    assert_eq!(loaded.talents_code, None);
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
    assert_eq!(loaded.talents_code, None);

    // Improved Heroic Strike's 3 points in Deflection instead.
    let mut moved = own.clone();
    let arms = moved.get_mut("Arms").unwrap();
    arms.remove("Improved Heroic Strike");
    arms.insert("Deflection".into(), 3);
    let loaded = load(&mut app, request(moved.clone())).unwrap();
    assert_eq!(spent_talents(&app), moved);
    let code = loaded.talents_code.unwrap();
    assert!(code.starts_with("033"), "{code}");
    assert!(
        code.ends_with("-2"),
        "Improved Bloodrage's 2 Protection points: {code}"
    );

    // The same build from its code, as a link gives it.
    let by_code = |code: &str| LoadRequest {
        talents_code: Some(code.to_owned()),
        ..by_name("warrior_fury_dw_orc")
    };
    load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    let loaded = load(&mut app, by_code(&code)).unwrap();
    assert_eq!(loaded.talents_code.as_deref(), Some(code.as_str()));
    assert_eq!(spent_talents(&app), moved);
    let json = serde_json::to_value(&loaded).unwrap();
    assert_eq!(json["talents_code"], code.as_str());
    for bad in ["x", "0-0-0-0", "00000000000000000000000000"] {
        assert!(load(&mut app, by_code(bad)).is_err(), "{bad}");
    }
    let both = LoadRequest {
        talents: Some(moved.clone()),
        ..by_code(&code)
    };
    assert!(load(&mut app, both).is_err());

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
    assert_eq!(app.loaded().unwrap().talents_code, Some(code));

    // Another setup by itself: its own talents.
    let loaded = load(&mut app, by_name("rogue_combat_swords_human")).unwrap();
    assert_eq!(loaded.talents_code, None);
}

/// The names of the session's selected externals: its buffs, or its debuffs.
fn selected_externals(loaded: &Loaded, debuffs: bool) -> Vec<&str> {
    loaded
        .info
        .externals
        .iter()
        .filter(|external| external.debuff == debuffs && external.selected)
        .map(|external| external.name.as_str())
        .collect()
}

#[test]
fn a_load_changes_the_buffs_and_debuffs() {
    let mut app = empty_app();
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    assert_eq!(loaded.externals, Externals::default());
    assert!(selected_externals(&loaded, false).contains(&"Juju Power"));
    assert_eq!(
        selected_externals(&loaded, true),
        ["Sunder Armor", "Faerie Fire"]
    );
    let offered = |name: &str| loaded.info.externals.iter().find(|e| e.name == name);
    let juju = offered("Juju Power").unwrap();
    assert_eq!(juju.mutex.as_deref(), Some("strength_elixir"));
    assert!(juju.icon.is_some());
    assert!(offered("Elixir of Giants").is_some_and(|e| !e.selected && !e.debuff));
    assert!(offered("Curse of Recklessness").is_some_and(|e| !e.selected && e.debuff));
    // A warrior is not offered Battle Shout (its own).
    assert!(offered("Battle Shout").is_none());
    let own_buffs: Vec<String> = selected_externals(&loaded, false)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let stats = loaded.info.stats.clone();

    let request = |buffs: &[&str], debuffs: Option<&[&str]>| LoadRequest {
        buffs: Some(buffs.iter().map(|&name| name.to_owned()).collect()),
        debuffs: debuffs.map(|names| names.iter().map(|&name| name.to_owned()).collect()),
        ..by_name("warrior_fury_dw_orc")
    };

    // The setup's own buffs in another order are no change.
    let mut reversed: Vec<&str> = own_buffs.iter().map(String::as_str).collect();
    reversed.reverse();
    let loaded = load(&mut app, request(&reversed, None)).unwrap();
    assert_eq!(loaded.externals, Externals::default());

    // Elixir of Giants instead of Juju Power, Curse of Recklessness instead of Faerie Fire.
    let mut giants = reversed.clone();
    giants.retain(|&name| name != "Juju Power");
    giants.push("Elixir of Giants");
    let debuffs = ["Sunder Armor", "Curse of Recklessness"];
    let loaded = load(&mut app, request(&giants, Some(&debuffs))).unwrap();
    assert!(selected_externals(&loaded, false).contains(&"Elixir of Giants"));
    assert!(!selected_externals(&loaded, false).contains(&"Juju Power"));
    assert_eq!(selected_externals(&loaded, true), debuffs);
    assert_ne!(
        loaded.info.stats, stats,
        "Juju Power's 30 strength to Giants' 25"
    );
    let json = serde_json::to_value(&loaded).unwrap();
    assert_eq!(json["debuffs"], serde_json::json!(debuffs));
    assert_eq!(json["buffs"].as_array().unwrap().len(), giants.len());

    // No debuffs at all.
    let loaded = load(&mut app, request(&reversed, Some(&[]))).unwrap();
    assert!(selected_externals(&loaded, true).is_empty());
    assert_eq!(loaded.externals.buffs, None);
    assert_eq!(loaded.externals.debuffs, Some(Vec::new()));

    // An unknown name, a debuff among the buffs and two of a mutex keep the session.
    for bad in [
        request(&["No Such Buff"], None),
        request(&["Sunder Armor"], None),
        request(&["Juju Power", "Elixir of Giants"], None),
    ] {
        assert!(load(&mut app, bad).is_err());
    }
    assert_eq!(app.loaded().unwrap().externals.debuffs, Some(Vec::new()));

    // Another setup by itself: its own.
    let loaded = load(&mut app, by_name("rogue_combat_swords_human")).unwrap();
    assert_eq!(loaded.externals, Externals::default());
}

#[test]
fn the_catalog_lists_each_class_s_races_and_rotations() {
    let catalog = empty_app().catalog();
    let class = |class| {
        catalog
            .classes
            .iter()
            .find(|entry| entry.class == class)
            .unwrap()
    };
    let races = |of| {
        class(of)
            .races
            .iter()
            .map(|entry| entry.race)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        races(PlayerClass::Warrior),
        Race::ALL,
        "every race, in order"
    );
    assert_eq!(
        races(PlayerClass::Rogue),
        Race::ALL
            .into_iter()
            .filter(|race| *race != Race::Tauren)
            .collect::<Vec<_>>()
    );
    let warrior = class(PlayerClass::Warrior);
    assert_eq!(warrior.name, "Warrior");
    let human = &warrior.races[0];
    assert_eq!((human.name, human.faction), ("Human", "Alliance"));
    let fury = warrior
        .rotations
        .iter()
        .find(|rotation| rotation.name == "DW Fury")
        .unwrap();
    assert_eq!(fury.prerequisites, ["Bloodthirst"]);
    assert!(
        class(PlayerClass::Rogue)
            .rotations
            .iter()
            .any(|rotation| rotation.name == "Combat")
    );

    let json = serde_json::to_value(warrior).unwrap();
    assert_eq!(json["class"], "WARRIOR");
    assert_eq!(json["races"][3]["race"], "NIGHT_ELF");
}

#[test]
fn a_load_changes_the_race_and_keeps_the_gear() {
    let mut app = empty_app();
    let orc = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    let human = load(
        &mut app,
        LoadRequest {
            race: Some(Race::Human),
            ..by_name("warrior_fury_dw_orc")
        },
    )
    .unwrap();
    assert_eq!(human.race_override, Some(Race::Human));
    assert_eq!((orc.info.race, human.info.race), ("Orc", "Human"));
    assert_eq!(human.info.name, "DW Fury Orc", "still the setup");
    assert_eq!(human.source.setup.as_deref(), Some("warrior_fury_dw_orc"));
    // The Horde gear, talents and buffs stay; the racials change the stats.
    assert_eq!(human.info.equipment, orc.info.equipment);
    assert_eq!(spent_talents(&app)["Fury"]["Bloodthirst"], 1);
    assert_eq!(human.info.externals, orc.info.externals);
    assert_ne!(human.info.stats, orc.info.stats);
    assert_eq!(
        serde_json::to_value(&human).unwrap()["race_override"],
        "HUMAN"
    );

    // The setup's own race is no change; a race the class cannot be keeps the session.
    let own = LoadRequest {
        race: Some(Race::Orc),
        ..by_name("warrior_fury_dw_orc")
    };
    assert_eq!(load(&mut app, own).unwrap().race_override, None);
    let tauren_rogue = LoadRequest {
        race: Some(Race::Tauren),
        ..by_name("rogue_combat_swords_human")
    };
    let error = load(&mut app, tauren_rogue).unwrap_err();
    assert!(error.contains("Tauren is not available"), "{error}");
    assert_eq!(app.loaded().unwrap().info.race, "Orc");

    // Another load without one: the setup's race again.
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    assert_eq!((loaded.race_override, loaded.info.race), (None, "Orc"));
}

fn bare(class: PlayerClass, race: Race, rotation: &str) -> LoadRequest {
    LoadRequest {
        bare: Some(Bare {
            class,
            race,
            rotation: rotation.to_owned(),
        }),
        ..LoadRequest::default()
    }
}

#[test]
fn a_bare_character_has_nothing_but_its_rotation() {
    let mut app = empty_app();
    let loaded = load(
        &mut app,
        bare(PlayerClass::Warrior, Race::HighOrderSkyborne, "DW Fury"),
    )
    .unwrap();
    assert_eq!(loaded.info.name, "High Order Skyborne Warrior");
    assert_eq!(
        (
            loaded.info.class,
            loaded.info.race,
            loaded.info.rotation.as_str()
        ),
        ("Warrior", "High Order Skyborne", "DW Fury")
    );
    assert_eq!(loaded.source.setup, None);
    assert_eq!(
        loaded.source.bare,
        Some(Bare {
            class: PlayerClass::Warrior,
            race: Race::HighOrderSkyborne,
            rotation: "DW Fury".into(),
        })
    );
    assert!(loaded.info.equipment.is_empty());
    assert!(spent_talents(&app).is_empty());
    assert!(
        loaded
            .info
            .externals
            .iter()
            .all(|external| !external.selected)
    );
    // It loads without Bloodthirst, and says so.
    let missing = &loaded.info.missing_prerequisites;
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!(missing[0].spell, "Bloodthirst");
    assert!(missing[0].reason.contains("talent"), "{missing:?}");

    // Its gear and buffs change as a setup's do.
    let geared = load(
        &mut app,
        LoadRequest {
            gear: vec![GearChange {
                slot: EquipmentSlot::Mainhand,
                item: Some(19019),
            }],
            buffs: Some(vec!["Juju Power".into()]),
            ..bare(PlayerClass::Warrior, Race::HighOrderSkyborne, "DW Fury")
        },
    )
    .unwrap();
    assert_eq!(geared.info.equipment.len(), 1);
    assert_eq!(geared.info.equipment[0].id, 19019);
    // Not the bare character's (none): a link carries them.
    assert_eq!(geared.externals.buffs, Some(vec!["Juju Power".to_owned()]));
    assert!(
        geared
            .info
            .externals
            .iter()
            .any(|external| external.name == "Juju Power" && external.selected)
    );

    // A rotation without prerequisites misses none.
    let rogue = load(&mut app, bare(PlayerClass::Rogue, Race::Gnome, "Combat")).unwrap();
    assert!(rogue.info.missing_prerequisites.is_empty());
}

#[test]
fn a_bare_character_is_checked() {
    let mut app = empty_app();
    load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    for (request, message) in [
        (
            bare(PlayerClass::Warrior, Race::Human, "Combat"),
            "no rotation \"Combat\"",
        ),
        (
            bare(PlayerClass::Rogue, Race::Tauren, "Combat"),
            "Tauren is not available",
        ),
        (
            LoadRequest {
                setup: Some("warrior_fury_dw_orc".into()),
                ..bare(PlayerClass::Warrior, Race::Human, "DW Fury")
            },
            "more than one",
        ),
    ] {
        let error = load(&mut app, request).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    assert_eq!(app.loaded().unwrap().info.name, "DW Fury Orc");
    assert!(
        serde_json::from_str::<LoadRequest>(
            r#"{"bare": {"class": "WARRIOR", "race": "HUMAN", "rotation": "DW Fury", "gear": []}}"#
        )
        .is_err(),
        "unknown fields"
    );
}

#[test]
fn a_bundled_setup_still_needs_its_rotation_s_prerequisites() {
    let mut app = empty_app();
    let request = LoadRequest {
        talents: Some(Talents::new()),
        ..by_name("warrior_fury_dw_orc")
    };
    let error = load(&mut app, request).unwrap_err();
    assert!(error.contains("prerequisite \"Bloodthirst\""), "{error}");
    let loaded = load(&mut app, by_name("warrior_fury_dw_orc")).unwrap();
    assert!(loaded.info.missing_prerequisites.is_empty());
}

/// The Paladin in the catalog (its setups, its three races, its rotations with Twist of Light
/// as a prerequisite, the Ret keybinds) and loaded: played by its rotation or from the
/// keyboard, as another of its races, its talents by Wowhead's string.
#[test]
fn a_paladin_is_in_the_catalog_and_loads() {
    let catalog = empty_app().catalog();
    for race in ["human", "dwarf", "undead"] {
        let name = format!("paladin_ret_2h_{race}");
        let setup = catalog.setups.iter().find(|setup| setup.name == name);
        let setup = setup.unwrap_or_else(|| panic!("{name}"));
        assert_eq!(setup.class, "Paladin");
        assert_eq!(setup.rotation, "Seal Twisting");
        assert_eq!(setup.error, None);
    }
    assert!(catalog.keybinds.contains(&"ret".to_owned()));
    let paladin = catalog
        .classes
        .iter()
        .find(|entry| entry.class == PlayerClass::Paladin)
        .unwrap();
    assert_eq!(paladin.name, "Paladin");
    let races: Vec<Race> = paladin.races.iter().map(|entry| entry.race).collect();
    assert_eq!(races, [Race::Human, Race::Dwarf, Race::Undead]);
    let rotation = |name: &str| {
        paladin
            .rotations
            .iter()
            .find(|rotation| rotation.name == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    assert_eq!(rotation("Seal Twisting").prerequisites, ["Twist of Light"]);
    rotation("Seal of Command");
    rotation("Seal of the Crusader");

    let mut app = empty_app();
    let loaded = load(&mut app, by_name("paladin_ret_2h_human")).unwrap();
    assert_eq!((loaded.info.class, loaded.info.race), ("Paladin", "Human"));
    assert!(!loaded.info.manual);
    assert!(loaded.info.missing_prerequisites.is_empty());
    let code = loaded.talents_code.clone();
    assert_eq!(code, None, "the setup's own talents");

    let keyboard = LoadRequest {
        keybinds: Some("ret".to_owned()),
        ..by_name("paladin_ret_2h_human")
    };
    let loaded = load(&mut app, keyboard).unwrap();
    assert!(loaded.info.manual);
    let bound: Vec<&str> = loaded
        .info
        .keybinds
        .iter()
        .map(|key| key.name.as_str())
        .collect();
    for spell in [
        "Seal of Command",
        "Seal of Righteousness",
        "Judgement",
        "Holy Strike",
    ] {
        assert!(bound.contains(&spell), "{spell}: {bound:?}");
    }

    let undead = LoadRequest {
        race: Some(Race::Undead),
        ..by_name("paladin_ret_2h_human")
    };
    assert_eq!(load(&mut app, undead).unwrap().info.race, "Undead");
    let orc = LoadRequest {
        race: Some(Race::Orc),
        ..by_name("paladin_ret_2h_human")
    };
    assert!(load(&mut app, orc).is_err(), "no Orc Paladin");

    // Without Twist of Light the setup cannot twist.
    let mut talents = spent_talents(&app);
    let retribution = talents.get_mut("Retribution").unwrap();
    assert_eq!(retribution.remove("Twist of Light"), Some(1));
    let request = LoadRequest {
        talents: Some(talents),
        ..by_name("paladin_ret_2h_human")
    };
    let error = load(&mut app, request).unwrap_err();
    assert!(error.contains("prerequisite \"Twist of Light\""), "{error}");
}
