//! Routing tests, without sockets.

use std::path::PathBuf;

use serde_json::Value;

use super::*;
use crate::session::tests::{manual_session_of, session_of};

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

fn call_with(session: &mut Session, icons: &Path, method: &str, path: &str) -> Reply {
    route(session, icons, method, path, "", || 42)
}

fn call(session: &mut Session, method: &str, path: &str, body: &str) -> Reply {
    route(session, Path::new("no-icons"), method, path, body, || 42)
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
    let mut session = session_of("warrior_fury_dw_orc.yaml", 12_345_678_901_234_567_890);
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
    let mut session = session_of("warrior_fury_dw_orc.yaml", 1);
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
    let mut session = session_of("rogue_combat_swords_human.yaml", 1);
    let info = json(&call(
        &mut session,
        "POST",
        "/api/restart",
        r#"{"seed": "77"}"#,
    ));
    assert_eq!(info["seed"], "77");
    assert_eq!(session.info().seed, 77);
    let info = json(&call(&mut session, "POST", "/api/restart", "{}"));
    assert_eq!(info["seed"], "42");
}

#[test]
fn bad_requests_are_refused() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 1);
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
    assert_eq!(session.info().seed, 1, "nothing restarted");
}

#[test]
fn icons_are_served_from_the_icon_directory() {
    let icons = TempDir::new("icons");
    let png = b"PNG, as far as the route cares".to_vec();
    std::fs::write(icons.0.join("136012.png"), &png).unwrap();
    std::fs::write(icons.0.join("secret.png"), b"no").unwrap();
    let mut session = session_of("warrior_fury_dw_orc.yaml", 1);

    let reply = call_with(&mut session, &icons.0, "GET", "/icons/136012.png");
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
        let reply = call_with(&mut session, &icons.0, method, path);
        assert_eq!(reply.status, status, "{method} {path}: {}", text(&reply));
        assert_eq!(reply.cache_control, None);
    }
}

#[test]
fn a_key_press_is_cast_when_played_from_the_keyboard_only() {
    let mut manual = manual_session_of("warrior_fury_dw_orc.yaml", 3, "Hamstring: 1\n");
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

    let mut rotation = session_of("warrior_fury_dw_orc.yaml", 3);
    assert_eq!(
        json(&call(&mut rotation, "GET", "/api/info", ""))["manual"],
        false
    );
    assert_eq!(call(&mut rotation, "POST", "/api/cast", body).status, 400);
}
