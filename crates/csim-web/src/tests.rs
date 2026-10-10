//! The embedded data and the routes over it, on the host.

use csim_engine::files::FsFiles;
use serde_json::Value;

use super::*;

fn json(reply: &Reply) -> Value {
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.content_type, "application/json");
    serde_json::from_str(&reply.body).unwrap()
}

#[test]
fn the_embedded_files_are_the_data_the_viewer_reads() {
    let data = DataBundle::repository_dir();
    let paths: Vec<&str> = FILES.iter().map(|(path, _)| *path).collect();
    for expected in [
        "characters/warrior_fury_dw_orc.yaml",
        "characters/common/base_buffs.yaml",
        "keybinds/dw_fury.yaml",
        "rotations/warrior/dw_fury.yaml",
        "spells/warrior.yaml",
        "spells/overrides/warrior.yaml",
        "items/one_hand.yaml",
        "item_sets.yaml",
        "races.yaml",
        "characters/paladin_ret_2h_human.yaml",
        "characters/common/base_paladin_buffs.yaml",
        "classes/paladin.yaml",
        "keybinds/ret.yaml",
        "rotations/paladin/seal_twisting.yaml",
        "spells/paladin.yaml",
        "spells/overrides/paladin.yaml",
        "talents/paladin.yaml",
    ] {
        assert!(paths.contains(&expected), "{expected} is embedded");
    }
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]), "sorted");
    assert!(
        paths
            .iter()
            .all(|path| { path.ends_with(".yaml") || path.ends_with(".yml") }),
        "YAML only (no READMEs)"
    );
    for excluded in ["tables/", "icons/", "raids/", "sweeps/"] {
        assert!(
            !paths.iter().any(|path| path.starts_with(excluded)),
            "{excluded}"
        );
    }
    for (path, text) in FILES {
        assert_eq!(
            *text,
            std::fs::read_to_string(data.join(path)).unwrap(),
            "{path} is the file on disk"
        );
    }
}

#[test]
fn the_web_app_answers_as_the_native_server() {
    let mut web = LiveApp::new(0, 1).unwrap();
    let data = Arc::new(DataBundle::load(&DataBundle::repository_dir()).unwrap());
    let mut native = App::new(Box::new(FsFiles), DataBundle::repository_dir(), data);
    let mut native_handle = |method: &str, path: &str, body: &str| {
        let reply = server::route(&mut native, &|_| None, method, path, body, || 99);
        String::from_utf8(reply.body).unwrap()
    };

    assert_eq!(
        web.handle("GET", "/api/info", "").status,
        409,
        "no session yet"
    );
    let catalog = json(&web.handle("GET", "/api/catalog", ""));
    assert_eq!(
        catalog,
        serde_json::from_str::<Value>(&native_handle("GET", "/api/catalog", "")).unwrap()
    );

    let load = r#"{"setup": "warrior_fury_dw_orc", "seed": "1", "length": 60}"#;
    let info = json(&web.handle("POST", "/api/load", load));
    assert_eq!(info["name"], "DW Fury Orc");
    assert_eq!(info["source"]["setup"], "warrior_fury_dw_orc");
    assert_eq!(
        info,
        serde_json::from_str::<Value>(&native_handle("POST", "/api/load", load)).unwrap()
    );

    let advance = r#"{"to": 10}"#;
    let frame = web.handle("POST", "/api/advance", advance);
    let decisions = &json(&frame)["decisions"];
    assert!(
        decisions
            .as_array()
            .unwrap()
            .iter()
            .any(|decision| decision["by"] == "entry"),
        "the rotation cast: {decisions}"
    );
    assert_eq!(
        frame.body,
        native_handle("POST", "/api/advance", advance),
        "the same frame"
    );

    let icon = web.handle("GET", "/icons/136012.png", "");
    assert_eq!(icon.status, 404, "no local icons: the page uses Wowhead's");
}

#[test]
fn unnamed_seeds_follow_the_seed_given() {
    let first_seed = |hi, lo| {
        let mut web = LiveApp::new(hi, lo).unwrap();
        let info = json(&web.handle("POST", "/api/load", r#"{"setup": "warrior_fury_dw_orc"}"#));
        let restarted = json(&web.handle("POST", "/api/restart", "{}"));
        (info["seed"].clone(), restarted["seed"].clone())
    };
    let (a, b) = (first_seed(7, 8), first_seed(7, 8));
    assert_eq!(a, b);
    assert_ne!(a.0, a.1, "each draws the next");
    let mut seeds = Xoroshiro128Plus::from_seed(7 << 32 | 8);
    assert_eq!(a.0, seeds.next().to_string());
    assert_ne!(first_seed(7, 9), a);
}

#[test]
fn the_web_app_sims_as_the_native_server() {
    let mut web = LiveApp::new(0, 1).unwrap();
    let data = Arc::new(DataBundle::load(&DataBundle::repository_dir()).unwrap());
    let mut native = App::new(Box::new(FsFiles), DataBundle::repository_dir(), data);
    let mut native_handle = |method: &str, path: &str, body: &str| {
        let reply = server::route(&mut native, &|_| None, method, path, body, || 99);
        String::from_utf8(reply.body).unwrap()
    };

    let start =
        r#"{"load": {"setup": "rogue_combat_swords_human", "seed": "4"}, "iterations": 20}"#;
    let started = json(&web.handle("POST", "/api/sim/start", start));
    assert_eq!(started["total"], 20);
    assert_eq!(started["seed"], "4");
    native_handle("POST", "/api/sim/start", start);
    for _ in 0..3 {
        let step = r#"{"iterations": 8}"#;
        let progress = web.handle("POST", "/api/sim/step", step);
        assert_eq!(progress.body, native_handle("POST", "/api/sim/step", step));
    }
    let request = r#"{"elapsed_seconds": 1.5}"#;
    let results = web.handle("POST", "/api/sim/results", request);
    assert_eq!(json(&results)["run"]["iterations"], 20);
    assert_eq!(
        results.body,
        native_handle("POST", "/api/sim/results", request),
        "the same results"
    );
}

/// A Paladin as the native server shows it: from the keyboard with the Ret keybinds (Seal of
/// Command's key pressed), and simmed.
#[test]
fn the_web_app_plays_and_sims_a_paladin_as_the_native_server() {
    let mut web = LiveApp::new(0, 1).unwrap();
    let data = Arc::new(DataBundle::load(&DataBundle::repository_dir()).unwrap());
    let mut native = App::new(Box::new(FsFiles), DataBundle::repository_dir(), data);
    let mut native_handle = |method: &str, path: &str, body: &str| {
        let reply = server::route(&mut native, &|_| None, method, path, body, || 99);
        String::from_utf8(reply.body).unwrap()
    };

    let load = r#"{"setup": "paladin_ret_2h_human", "keybinds": "ret", "seed": "1", "length": 60}"#;
    let info = json(&web.handle("POST", "/api/load", load));
    assert_eq!(
        (&info["class"], &info["race"]),
        (&"Paladin".into(), &"Human".into())
    );
    assert_eq!(info["manual"], true);
    assert_eq!(
        info,
        serde_json::from_str::<Value>(&native_handle("POST", "/api/load", load)).unwrap()
    );

    let start = info["start_at"].as_f64().unwrap();
    let advance = format!(r#"{{"to": {}}}"#, start + 1.0);
    let frame = json(&web.handle("POST", "/api/advance", &advance));
    assert_eq!(frame["state"]["resource"]["kind"], "Mana");
    native_handle("POST", "/api/advance", &advance);
    let cast = format!(r#"{{"spell": "Seal of Command", "at": {}}}"#, start + 1.0);
    let pressed = web.handle("POST", "/api/cast", &cast);
    assert_eq!(
        pressed.body,
        native_handle("POST", "/api/cast", &cast),
        "the same press"
    );
    let advance = format!(r#"{{"to": {}}}"#, start + 3.0);
    let frame = web.handle("POST", "/api/advance", &advance);
    let buffs = &json(&frame)["state"]["buffs"];
    assert!(
        buffs
            .as_array()
            .unwrap()
            .iter()
            .any(|buff| buff["name"] == "Seal of Command"),
        "{buffs}"
    );
    assert_eq!(
        frame.body,
        native_handle("POST", "/api/advance", &advance),
        "the same frame"
    );

    let start = r#"{"load": {"setup": "paladin_ret_2h_dwarf", "seed": "2"}, "iterations": 10}"#;
    json(&web.handle("POST", "/api/sim/start", start));
    native_handle("POST", "/api/sim/start", start);
    let step = r#"{"iterations": 10}"#;
    assert_eq!(
        web.handle("POST", "/api/sim/step", step).body,
        native_handle("POST", "/api/sim/step", step)
    );
    let request = r#"{"elapsed_seconds": 1.0}"#;
    let results = web.handle("POST", "/api/sim/results", request);
    let resources = &json(&results)["resources"];
    assert!(resources.to_string().contains("Mana"), "{resources}");
    assert_eq!(
        results.body,
        native_handle("POST", "/api/sim/results", request),
        "the same results"
    );
}
