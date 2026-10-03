use super::*;

#[test]
fn bindings_are_normalised() {
    for (binding, normal) in [
        ("1", "1"),
        ("q", "Q"),
        ("shift+e", "Shift+E"),
        ("Alt + Shift + ctrl + 3", "Ctrl+Shift+Alt+3"),
        ("Control+f1", "Ctrl+F1"),
        ("F12", "F12"),
        ("space", "Space"),
        ("Alt+numpad4", "Alt+Numpad4"),
        ("minus", "Minus"),
    ] {
        assert_eq!(normalize(binding).as_deref(), Ok(normal), "{binding}");
    }
}

#[test]
fn malformed_bindings_are_refused() {
    for binding in [
        "",
        "Shift",
        "Ctrl+Ctrl+1",
        "1+2",
        "F13",
        "Hyper+1",
        "Shift+Foo",
    ] {
        assert!(normalize(binding).is_err(), "{binding}");
    }
}

#[test]
fn a_file_is_a_map_of_spell_to_binding_in_file_order() {
    let keybinds = parse(
        "Bloodthirst: 1\n\
         Whirlwind: '2'\n\
         Heroic Strike: q\n\
         Execute: Ctrl+Shift+Alt+E\n",
    )
    .unwrap();
    let pairs: Vec<(&str, &str)> = keybinds
        .iter()
        .map(|keybind| (keybind.name.as_str(), keybind.binding.as_str()))
        .collect();
    assert!(keybinds.iter().all(|keybind| !keybind.is_macro));
    assert!(
        keybinds
            .iter()
            .all(|keybind| keybind.spells == [keybind.name.clone()])
    );
    assert_eq!(
        pairs,
        [
            ("Bloodthirst", "1"),
            ("Whirlwind", "2"),
            ("Heroic Strike", "Q"),
            ("Execute", "Ctrl+Shift+Alt+E"),
        ]
    );
}

#[test]
fn a_binding_used_twice_or_a_bad_file_is_refused() {
    let twice = parse("Bloodthirst: shift+1\nWhirlwind: Shift+1\n").unwrap_err();
    assert!(twice.contains("Bloodthirst"), "{twice}");
    assert!(parse("- Bloodthirst\n").is_err(), "a list");
    assert!(parse("Bloodthirst: [1, 2]\n").is_err(), "a list as binding");
    assert!(
        parse("Bloodthirst: Hyper+1\n")
            .unwrap_err()
            .contains("Bloodthirst")
    );
}

#[test]
fn a_macro_is_a_hotkey_and_the_spells_it_casts() {
    let keybinds = parse(
        "Bloodthirst: 1\n\
         Burst:\n  hotkey: shift+t\n  cast:\n    - Bloodrage\n    - Bloodthirst\n    - Heroic Strike\n",
    )
    .unwrap();
    let burst = &keybinds[1];
    assert_eq!(burst.name, "Burst");
    assert_eq!(burst.binding, "Shift+T");
    assert_eq!(burst.spells, ["Bloodrage", "Bloodthirst", "Heroic Strike"]);
    assert!(burst.is_macro);
}

#[test]
fn malformed_macros_are_refused() {
    for (text, says) in [
        ("Burst:\n  hotkey: T\n  cast: []\n", "casts nothing"),
        ("Burst:\n  cast: [Bloodrage]\n", "hotkey"),
        ("Burst:\n  hotkey: T\n", "cast"),
        (
            "Burst:\n  hotkey: T\n  cast: [Bloodrage]\n  extra: 1\n",
            "extra",
        ),
        (
            "Burst:\n  hotkey: 1\n  cast: [Bloodrage]\nBloodthirst: 1\n",
            "Burst's already",
        ),
    ] {
        let error = parse(text).unwrap_err();
        assert!(error.contains(says), "{text}: {error}");
    }
}

#[test]
fn the_bundled_keybinds_load_from_memory() {
    let mut files = csim_engine::files::MemFiles::new();
    files.insert(
        "keybinds/dw_fury.yaml",
        "Bloodthirst: 1\nExecute: Shift+E\n",
    );
    let keybinds = load_from(&files, Path::new("keybinds/dw_fury.yaml")).unwrap();
    assert_eq!(keybinds.len(), 2);
    assert_eq!(keybinds[1].binding, "Shift+E");
    let error = load_from(&files, Path::new("keybinds/missing.yaml")).unwrap_err();
    assert!(error.starts_with("keybinds/missing.yaml: "), "{error}");
}

/// What the page's keybinds editor writes: every name and binding single-quoted (`''` for a
/// quote), macros as `hotkey` and `cast`.
#[test]
fn the_editors_yaml_parses() {
    let keybinds = parse(
        "# Written by the keybinds editor of csim-live.\n\
         'Bloodthirst': '2'\n\
         'Heroic Strike': 'Ctrl+Shift+Alt+V'\n\
         'Raptor''s Strike: Rank 1': 'Shift+Numpad1'\n\
         'Cooldowns':\n  \
           hotkey: 'T'\n  \
           cast:\n    \
             - 'Blood Fury'\n    \
             - 'Death Wish'\n",
    )
    .unwrap();
    let names: Vec<(&str, &str)> = keybinds
        .iter()
        .map(|keybind| (keybind.name.as_str(), keybind.binding.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("Bloodthirst", "2"),
            ("Heroic Strike", "Ctrl+Shift+Alt+V"),
            ("Raptor's Strike: Rank 1", "Shift+Numpad1"),
            ("Cooldowns", "T"),
        ]
    );
    assert!(keybinds[3].is_macro);
    assert_eq!(keybinds[3].spells, ["Blood Fury", "Death Wish"]);
}
