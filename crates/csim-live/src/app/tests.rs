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
