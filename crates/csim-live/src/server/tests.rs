//! Routing tests, without sockets.

use std::path::PathBuf;

use serde_json::Value;

use super::*;
use crate::session::tests::{app_of, empty_app, manual_session_of, session_of};

/// A directory of its own under the temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("csim-live-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn call_with(app: &mut App, icons: Icons, method: &str, path: &str) -> Reply {
    route(app, icons, method, path, "", || 42)
}

fn call(app: &mut App, method: &str, path: &str, body: &str) -> Reply {
    route(app, &|_| None, method, path, body, || 42)
}

fn text(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

fn json(reply: &Reply) -> Value {
    assert_eq!(reply.status, 200, "{}", text(reply));
    assert_eq!(reply.content_type, "application/json");
    serde_json::from_slice(&reply.body).unwrap()
}

#[test]
fn the_page_and_the_info() {
    let mut session = app_of(session_of(
        "warrior_fury_dw_orc.yaml",
        12_345_678_901_234_567_890,
    ));
    let page = call(&mut session, "GET", "/", "");
    assert_eq!(page.status, 200);
    assert!(page.content_type.starts_with("text/html"));
    assert!(text(&page).starts_with("<!doctype html>"));

    let info = json(&call(&mut session, "GET", "/api/info", ""));
    assert_eq!(
        info["seed"], "12345678901234567890",
        "a string, as JS cannot hold it"
    );
    assert_eq!(info["name"], "DW Fury Orc");
    assert_eq!(info["class"], "Warrior");
    assert!(info["start_at"].as_f64().unwrap() < 0.0);
}

#[test]
fn advancing_and_stepping_answer_frames() {
    let mut session = app_of(session_of("warrior_fury_dw_orc.yaml", 1));
    let frame = json(&call(
        &mut session,
        "POST",
        "/api/advance",
        r#"{"to": 12.5}"#,
    ));
    assert_eq!(frame["time"], 12.5);
    assert_eq!(frame["done"], false);
    assert!(!frame["damage"].as_array().unwrap().is_empty());
    assert_eq!(frame["state"]["resource"]["kind"], "Rage");

    let frame = json(&call(
        &mut session,
        "POST",
        "/api/step",
        r#"{"kind": "event"}"#,
    ));
    assert!(frame["event"].is_string());
    assert!(frame["time"].as_f64().unwrap() > 12.5);
    let frame = json(&call(
        &mut session,
        "POST",
        "/api/step",
        r#"{"kind": "cast"}"#,
    ));
    assert!(frame["time"].as_f64().unwrap() > 12.5);
}

#[test]
fn restarting_takes_the_seed_given_or_a_new_one() {
    let mut session = app_of(session_of("rogue_combat_swords_human.yaml", 1));
    let info = json(&call(
        &mut session,
        "POST",
        "/api/restart",
        r#"{"seed": "77"}"#,
    ));
    assert_eq!(info["seed"], "77");
    assert_eq!(session.session().unwrap().info().seed, 77);
    let info = json(&call(&mut session, "POST", "/api/restart", "{}"));
    assert_eq!(info["seed"], "42");
}

#[test]
fn bad_requests_are_refused() {
    let mut session = app_of(session_of("warrior_fury_dw_orc.yaml", 1));
    for (method, path, body, status) in [
        ("POST", "/api/advance", "", 400),
        ("POST", "/api/advance", r#"{"to": "soon"}"#, 400),
        ("POST", "/api/advance", r#"{"until": 3}"#, 400),
        ("POST", "/api/step", r#"{"kind": "swing"}"#, 400),
        ("POST", "/api/restart", r#"{"seed": "-1"}"#, 400),
        ("POST", "/api/restart", r#"{"seed": 5}"#, 400),
        ("GET", "/api/advance", "", 405),
        ("POST", "/", "", 405),
        ("GET", "/favicon.ico", "", 404),
    ] {
        let reply = call(&mut session, method, path, body);
        assert_eq!(
            reply.status,
            status,
            "{method} {path} {body}: {}",
            text(&reply)
        );
    }
    assert_eq!(
        session.session().unwrap().info().seed,
        1,
        "nothing restarted"
    );
}

#[test]
fn icons_are_served_from_the_icon_directory() {
    let icons = TempDir::new("icons");
    let png = b"PNG, as far as the route cares".to_vec();
    std::fs::write(icons.0.join("136012.png"), &png).unwrap();
    std::fs::write(icons.0.join("secret.png"), b"no").unwrap();
    let mut session = app_of(session_of("warrior_fury_dw_orc.yaml", 1));

    let lookup = icon_dir(&icons.0);
    let reply = call_with(&mut session, &lookup, "GET", "/icons/136012.png");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.cache_control, Some(ICON_CACHE));
    assert_eq!(reply.body, png);

    for (method, path, status) in [
        ("GET", "/icons/132369.png", 404),
        ("GET", "/icons/secret.png", 404),
        ("GET", "/icons/../icons/136012.png", 404),
        ("GET", "/icons/..%2F136012.png", 404),
        ("GET", "/icons/136012", 404),
        ("GET", "/icons/.png", 404),
        ("GET", "/icons/99999999999.png", 404),
        ("POST", "/icons/136012.png", 405),
    ] {
        let reply = call_with(&mut session, &lookup, method, path);
        assert_eq!(reply.status, status, "{method} {path}: {}", text(&reply));
        assert_eq!(reply.cache_control, None);
    }
}

#[test]
fn a_key_press_is_cast_when_played_from_the_keyboard_only() {
    let mut manual = app_of(manual_session_of(
        "warrior_fury_dw_orc.yaml",
        3,
        "Hamstring: 1\n",
    ));
    let info = json(&call(&mut manual, "GET", "/api/info", ""));
    assert_eq!(info["manual"], true);
    assert_eq!(info["keybinds"][0]["binding"], "1");
    let body = r#"{"spell": "Hamstring", "at": 10}"#;
    let frame = json(&call(&mut manual, "POST", "/api/cast", body));
    assert_eq!(frame["decisions"][0]["by"], "input");
    assert_eq!(frame["decisions"][0]["spell"], "Hamstring");

    let unbound = call(
        &mut manual,
        "POST",
        "/api/cast",
        r#"{"spell": "Execute", "at": 11}"#,
    );
    assert_eq!(unbound.status, 400);
    assert_eq!(call(&mut manual, "POST", "/api/cast", "{}").status, 400);
    assert_eq!(call(&mut manual, "GET", "/api/cast", "").status, 405);

    let mut rotation = app_of(session_of("warrior_fury_dw_orc.yaml", 3));
    assert_eq!(
        json(&call(&mut rotation, "GET", "/api/info", ""))["manual"],
        false
    );
    assert_eq!(call(&mut rotation, "POST", "/api/cast", body).status, 400);
}

#[test]
fn the_native_web_js_is_an_empty_module() {
    let mut app = empty_app();
    let reply = call(&mut app, "GET", "/web.js", "");
    assert_eq!((reply.status, reply.content_type), (200, "text/javascript"));
    assert!(
        text(&reply).trim_start().starts_with("//"),
        "only a comment"
    );
    assert_eq!(call(&mut app, "POST", "/web.js", "").status, 405);
    // The page loads it before its own script, as a module.
    let page = text(&call(&mut app, "GET", "/", ""));
    let web_js = page.find(r#"<script type="module" src="web.js"></script>"#);
    let script = page.find(r#"<script type="module">"#);
    assert!(web_js.is_some() && web_js < script, "web.js first");
    assert!(!page.contains(r#""/api/"#), "relative API paths only");
}

#[test]
fn before_a_load_only_the_page_catalog_and_load_answer() {
    let mut app = empty_app();
    assert_eq!(call(&mut app, "GET", "/", "").status, 200);
    let catalog = json(&call(&mut app, "GET", "/api/catalog", ""));
    assert!(catalog["setups"].as_array().unwrap().len() > 3);
    assert!(catalog["build"].as_str().unwrap().starts_with("1.60."));
    assert_eq!(
        catalog["keybinds"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|name| *name == "dw_fury")
            .count(),
        1
    );
    // The editor's starting points: each file's keybinds, and the keys it can bind.
    let dw_fury = catalog["keybind_entries"]["dw_fury"].as_array().unwrap();
    assert!(
        dw_fury
            .iter()
            .any(|keybind| keybind["name"] == "Bloodthirst"
                && keybind["binding"] == "2"
                && keybind["macro"] == false)
    );
    assert!(dw_fury.iter().any(|keybind| keybind["macro"] == true));
    let keys = catalog["keys"].as_array().unwrap();
    assert!(keys.contains(&Value::from("Numpad1")) && keys.contains(&Value::from("Space")));
    for (method, path, body) in [
        ("GET", "/api/info", ""),
        ("GET", "/api/items", ""),
        ("POST", "/api/advance", r#"{"to": 1}"#),
        ("POST", "/api/step", r#"{"kind": "event"}"#),
        ("POST", "/api/restart", "{}"),
        ("POST", "/api/cast", r#"{"spell": "Bloodthirst", "at": 1}"#),
    ] {
        let reply = call(&mut app, method, path, body);
        assert_eq!(reply.status, 409, "{method} {path}: {}", text(&reply));
    }
    assert_eq!(call(&mut app, "GET", "/api/advance", "").status, 405);
    assert_eq!(call(&mut app, "GET", "/api/load", "").status, 405);
    assert_eq!(call(&mut app, "POST", "/api/catalog", "").status, 405);
}

#[test]
fn a_setup_loads_by_name_with_its_seed_length_and_settings() {
    let mut app = empty_app();
    let body = r#"{"setup": "warrior_fury_dw_orc", "seed": "7", "length": 60,
        "length_variance": 0,
        "settings": "initial_rage:30,target_start_health_percent:30"}"#;
    let info = json(&call(&mut app, "POST", "/api/load", body));
    assert_eq!(info["name"], "DW Fury Orc");
    assert_eq!(info["seed"], "7");
    assert_eq!(info["combat_length"], 60);
    assert_eq!(info["end_at"], 60.0);
    assert_eq!(info["target_start_health"], 0.3);
    assert_eq!(
        info["settings"],
        "initial_rage:30,target_start_health_percent:30"
    );
    assert_eq!(info["manual"], false);
    assert_eq!(info["source"]["setup"], "warrior_fury_dw_orc");
    assert_eq!(info["source"]["keybinds"], Value::Null);
    assert_eq!(json(&call(&mut app, "GET", "/api/info", "")), info);
    let frame = json(&call(&mut app, "POST", "/api/advance", r#"{"to": 5}"#));
    assert!(!frame["damage"].as_array().unwrap().is_empty());

    // A restart keeps the setup and the settings.
    let restarted = json(&call(&mut app, "POST", "/api/restart", r#"{"seed": "8"}"#));
    assert_eq!(restarted["seed"], "8");
    assert_eq!(
        restarted["settings"],
        "initial_rage:30,target_start_health_percent:30"
    );
    assert_eq!(restarted["source"], info["source"]);
    let restarted = json(&call(&mut app, "POST", "/api/restart", "{}"));
    assert_eq!(restarted["seed"], "42");
    assert_eq!(restarted["combat_length"], 60);
}

#[test]
fn a_load_changes_the_target_and_a_restart_keeps_it() {
    let mut app = empty_app();
    let body = r#"{"setup": "warrior_fury_dw_orc", "target_creature_type": "Undead",
        "target_armor": 0}"#;
    let info = json(&call(&mut app, "POST", "/api/load", body));
    assert_eq!(info["target"]["setup_creature_type"], "Dragonkin");
    assert_eq!(info["target"]["setup_armor"], 3731);
    assert_eq!(info["target"]["creature_type"], "Undead");
    assert_eq!(info["target"]["armor"], 0);
    let restarted = json(&call(&mut app, "POST", "/api/restart", "{}"));
    assert_eq!(restarted["target"], info["target"]);
    let catalog = json(&call(&mut app, "GET", "/api/catalog", ""));
    assert!(
        catalog["creature_types"]
            .as_array()
            .unwrap()
            .contains(&Value::from("Beast"))
    );
    let bad = r#"{"setup": "warrior_fury_dw_orc", "target_creature_type": "Murloc"}"#;
    assert_eq!(call(&mut app, "POST", "/api/load", bad).status, 400);
}

#[test]
fn pasted_setups_and_keybinds_load() {
    let mut app = empty_app();
    let body = serde_json::json!({
        "setup_yaml": "include: warrior_fury_dw_orc.yaml\nname: Pasted Fury\n",
        "keybinds": "dw_fury",
    })
    .to_string();
    let info = json(&call(&mut app, "POST", "/api/load", &body));
    assert_eq!(
        info["name"], "Pasted Fury",
        "the include resolves to the bundled setup"
    );
    assert_eq!(info["race"], "Orc");
    assert_eq!(info["manual"], true);
    assert_eq!(info["source"]["setup"], Value::Null);
    assert_eq!(info["source"]["keybinds"], "dw_fury");

    let body = serde_json::json!({
        "setup": "warrior_fury_dw_orc",
        "keybinds_yaml": "Bloodthirst: 1\n",
    })
    .to_string();
    let info = json(&call(&mut app, "POST", "/api/load", &body));
    assert_eq!(info["keybinds"][0]["name"], "Bloodthirst");
    assert_eq!(info["source"]["keybinds"], Value::Null);
}

#[test]
fn the_editors_keybinds_play_from_the_keyboard() {
    let mut app = empty_app();
    let rotation = json(&call(
        &mut app,
        "POST",
        "/api/load",
        r#"{"setup": "warrior_fury_dw_orc"}"#,
    ));
    // The editor offers the bindable spells while the rotation plays.
    let bindable = rotation["bindable"].as_array().unwrap();
    assert!(bindable.iter().any(|spell| spell["name"] == "Bloodthirst"));

    let body = serde_json::json!({
        "setup": "warrior_fury_dw_orc",
        "keybinds_yaml": "'Bloodthirst': '2'\n'Charge': 'R'\n\
            'Cooldowns':\n  hotkey: 'Shift+T'\n  cast:\n    - 'Blood Fury'\n    - 'Death Wish'\n",
    })
    .to_string();
    let info = json(&call(&mut app, "POST", "/api/load", &body));
    assert_eq!(info["manual"], true);
    assert_eq!(info["bindable"], rotation["bindable"]);
    let names: Vec<&str> = info["keybinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|keybind| keybind["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Bloodthirst", "Charge", "Cooldowns"]);
    let at = info["start_at"].as_f64().unwrap() + 1.0;
    let cast = serde_json::json!({"spell": "Charge", "at": at}).to_string();
    assert_eq!(call(&mut app, "POST", "/api/cast", &cast).status, 200);
}

#[test]
fn bad_loads_are_refused_and_keep_the_session() {
    let mut app = empty_app();
    call(
        &mut app,
        "POST",
        "/api/load",
        r#"{"setup": "rogue_combat_swords_human", "seed": "3"}"#,
    );
    for (body, says) in [
        ("", "EOF"),
        ("{}", "no setup"),
        (r#"{"setup": "a", "setup_yaml": "b"}"#, "more than one"),
        (r#"{"setup": "../../Cargo"}"#, "no characters entry"),
        (r#"{"setup": "missing"}"#, "missing.yaml"),
        (r#"{"setup_yaml": "name: [unclosed"}"#, "pasted.yaml"),
        (
            r#"{"setup_yaml": "include: nowhere.yaml\n"}"#,
            "nowhere.yaml",
        ),
        (
            r#"{"setup": "warrior_fury_dw_orc", "keybinds": "missing"}"#,
            "missing.yaml",
        ),
        (
            r#"{"setup": "warrior_fury_dw_orc", "keybinds_yaml": "Bloodthirst: [1]"}"#,
            "pasted keybinds",
        ),
        (
            r#"{"setup": "warrior_fury_dw_orc", "settings": "sigmoid_celing:3"}"#,
            "unknown setting",
        ),
        (
            r#"{"setup": "warrior_fury_dw_orc", "seed": "-1"}"#,
            "invalid seed",
        ),
        (r#"{"setup": "warrior_fury_dw_orc", "extra": 1}"#, "extra"),
    ] {
        let reply = call(&mut app, "POST", "/api/load", body);
        assert_eq!(reply.status, 400, "{body}: {}", text(&reply));
        assert!(text(&reply).contains(says), "{body}: {}", text(&reply));
    }
    let info = json(&call(&mut app, "GET", "/api/info", ""));
    assert_eq!(
        (info["race"].as_str(), info["seed"].as_str()),
        (Some("Human"), Some("3"))
    );
}

#[test]
fn the_items_the_character_can_wear() {
    let mut app = app_of(session_of("warrior_fury_dw_orc.yaml", 1));
    let items = json(&call(&mut app, "GET", "/api/items", ""));
    let items = items.as_array().unwrap();
    let earthstrike = items.iter().find(|item| item["id"] == 21180).unwrap();
    assert_eq!(earthstrike["name"], "Earthstrike");
    assert_eq!(earthstrike["type"], "TRINKET");
    assert_eq!(
        earthstrike["slots"],
        serde_json::json!(["TRINKET1", "TRINKET2"])
    );
    assert_eq!(earthstrike["effects"][0]["trigger"], "USE");
    assert_eq!(earthstrike["effects"][0]["name"], "Earthstrike");
    assert_eq!(earthstrike["weapon"], Value::Null);
    let bludgeon = items.iter().find(|item| item["id"] == 18866).unwrap();
    assert_eq!(bludgeon["quality"], "EPIC");
    assert_eq!(bludgeon["icon"]["name"], "inv_hammer_20");
    assert!(bludgeon["weapon"]["dps"].as_f64().unwrap() > 0.0);
    assert!(
        bludgeon["stats"]
            .as_object()
            .is_some_and(|stats| !stats.is_empty())
    );
    assert_eq!(call(&mut app, "POST", "/api/items", "").status, 405);
}

#[test]
fn the_talent_tree_and_its_edits() {
    let mut app = empty_app();
    assert_eq!(call(&mut app, "GET", "/api/talents", "").status, 409);
    // Edits need no session.
    let state = json(&call(
        &mut app,
        "POST",
        "/api/talents/edit",
        r#"{"class": "WARRIOR", "ranks": {}, "op": "max", "node": 105957}"#,
    ));
    assert_eq!(state["ranks"]["105957"], 5);
    assert_eq!(state["points_left"], 46);
    let bad = call(
        &mut app,
        "POST",
        "/api/talents/edit",
        r#"{"class": "WARRIOR", "ranks": {"105950": 1}, "op": "none"}"#,
    );
    assert_eq!(bad.status, 400, "Deep Wounds without its tier");
    assert_eq!(call(&mut app, "GET", "/api/talents/edit", "").status, 405);

    let mut session = app_of(session_of("warrior_fury_dw_orc.yaml", 1));
    let talents = json(&call(&mut session, "GET", "/api/talents", ""));
    assert_eq!(talents["class"], "WARRIOR");
    assert_eq!(talents["tabs"][1]["name"], "Fury");
    let state = &talents["state"];
    assert_eq!(state["tab_points"], serde_json::json!([18, 31, 2]));
    assert_eq!(
        (
            state["points_left"].as_u64(),
            state["required_level"].as_u64()
        ),
        (Some(0), Some(60))
    );
    let bloodthirst = talents["tabs"][1]["talents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|talent| talent["name"] == "Bloodthirst")
        .unwrap();
    let node = bloodthirst["node"].as_u64().unwrap().to_string();
    assert_eq!(state["ranks"][&node], 1);
    // No points left: only the talents with points are lit.
    let lit = state["available"].as_array().unwrap();
    assert_eq!(lit.len(), state["ranks"].as_object().unwrap().len());
}

#[test]
fn a_sim_runs_over_the_api() {
    let mut app = empty_app();
    let started = json(&call(
        &mut app,
        "POST",
        "/api/sim/start",
        r#"{"load": {"setup": "warrior_fury_dw_orc", "seed": "5"}, "iterations": 30}"#,
    ));
    assert_eq!(
        started,
        serde_json::json!({"name": "DW Fury Orc", "seed": "5", "done": 0, "total": 30})
    );
    let not_done = call(
        &mut app,
        "POST",
        "/api/sim/results",
        r#"{"elapsed_seconds": 0}"#,
    );
    assert_eq!(not_done.status, 409);
    assert!(text(&not_done).contains("0 of 30"), "{}", text(&not_done));

    let step = |app: &mut App, iterations: u32| {
        json(&call(
            app,
            "POST",
            "/api/sim/step",
            &format!(r#"{{"iterations": {iterations}}}"#),
        ))["done"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(step(&mut app, 12), 12);
    assert_eq!(step(&mut app, 100), 30);
    let results = json(&call(
        &mut app,
        "POST",
        "/api/sim/results",
        r#"{"elapsed_seconds": 2.5}"#,
    ));
    assert_eq!(results["run"]["iterations"], 30);
    assert_eq!(results["run"]["seed"], 5);
    assert_eq!(results["run"]["elapsed_seconds"], 2.5);
    assert!(results["dps"]["mean"].as_f64().unwrap() > 0.0);
    assert!(results["icons"]["spells"]["Mainhand Attack"].is_object());

    // The session is apart: none was loaded.
    assert_eq!(call(&mut app, "GET", "/api/info", "").status, 409);

    assert_eq!(
        json(&call(&mut app, "POST", "/api/sim/stop", "")),
        serde_json::json!({})
    );
    for path in ["/api/sim/step", "/api/sim/results"] {
        let body = if path.ends_with("step") {
            r#"{"iterations": 1}"#
        } else {
            r#"{"elapsed_seconds": 1}"#
        };
        let reply = call(&mut app, "POST", path, body);
        assert_eq!(reply.status, 409, "{path}");
        assert!(text(&reply).contains("no sim"), "{}", text(&reply));
    }
    // Stopping without a sim is no error.
    assert_eq!(call(&mut app, "POST", "/api/sim/stop", "").status, 200);
}

#[test]
fn bad_sim_requests_are_refused() {
    let mut app = empty_app();
    for (body, says) in [
        (
            r#"{"load": {"setup": "warrior_fury_dw_orc"}}"#,
            "iterations",
        ),
        (
            r#"{"load": {"setup": "warrior_fury_dw_orc"}, "iterations": 0}"#,
            "from 1 to",
        ),
        (
            r#"{"load": {"setup": "missing"}, "iterations": 10}"#,
            "missing.yaml",
        ),
        (
            r#"{"load": {"setup": "warrior_fury_dw_orc"}, "iterations": 10, "workers": 4}"#,
            "workers",
        ),
    ] {
        let reply = call(&mut app, "POST", "/api/sim/start", body);
        assert_eq!(reply.status, 400, "{body}: {}", text(&reply));
        assert!(text(&reply).contains(says), "{body}: {}", text(&reply));
    }
    json(&call(
        &mut app,
        "POST",
        "/api/sim/start",
        r#"{"load": {"setup": "warrior_fury_dw_orc"}, "iterations": 2}"#,
    ));
    for (path, body) in [
        ("/api/sim/step", r#"{"iterations": -1}"#),
        ("/api/sim/results", r#"{"elapsed_seconds": -1}"#),
        ("/api/sim/results", r#"{}"#),
    ] {
        assert_eq!(
            call(&mut app, "POST", path, body).status,
            400,
            "{path} {body}"
        );
    }
    for path in [
        "/api/sim/start",
        "/api/sim/step",
        "/api/sim/results",
        "/api/sim/stop",
    ] {
        assert_eq!(call(&mut app, "GET", path, "").status, 405, "{path}");
    }
}
