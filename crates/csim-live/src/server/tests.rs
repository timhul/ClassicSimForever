//! Routing tests, without sockets.

use serde_json::Value;

use super::*;
use crate::session::tests::session_of;

fn call(session: &mut Session, method: &str, path: &str, body: &str) -> Reply {
    route(session, method, path, body, || 42)
}

fn json(reply: &Reply) -> Value {
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.content_type, "application/json");
    serde_json::from_str(&reply.body).unwrap()
}

#[test]
fn the_page_and_the_info() {
    let mut session = session_of("dw_fury_orc.yaml", 12_345_678_901_234_567_890);
    let page = call(&mut session, "GET", "/", "");
    assert_eq!(page.status, 200);
    assert!(page.content_type.starts_with("text/html"));
    assert!(page.body.starts_with("<!doctype html>"));

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
    let mut session = session_of("dw_fury_orc.yaml", 1);
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
    let mut session = session_of("combat_swords_human.yaml", 1);
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
    let mut session = session_of("dw_fury_orc.yaml", 1);
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
            reply.status, status,
            "{method} {path} {body}: {}",
            reply.body
        );
    }
    assert_eq!(session.info().seed, 1, "nothing restarted");
}
