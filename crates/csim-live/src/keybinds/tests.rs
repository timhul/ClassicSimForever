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
        .map(|keybind| (keybind.spell.as_str(), keybind.binding.as_str()))
        .collect();
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
