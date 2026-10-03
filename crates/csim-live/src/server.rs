//! The page and a small JSON API over one [`Session`]: [`route`] answers one request. The
//! native `csim-live` serves it over HTTP on 127.0.0.1; the browser build calls it directly.
//!
//! - `GET /`: the page.
//! - `GET /api/info`: the iteration's [`Info`](crate::session::Info).
//! - `POST /api/advance {"to": t}`: runs the iteration up to sim time `t`; a frame.
//! - `POST /api/step {"kind": "event" | "cast"}`: runs one event, or up to the next cast; a
//!   frame.
//! - `POST /api/restart {"seed": "S"}`: starts the iteration of seed `S` (a string: seeds do
//!   not fit a JavaScript number), or of a new seed without one; the new info.
//! - `POST /api/cast {"spell": "Bloodthirst", "at": t}`: a key press of a bound spell at sim
//!   time `t` (played from the keyboard only); a frame.
//! - `GET /icons/<FileDataID>.png`: an icon of the frames, from the icon lookup (natively the
//!   icon directory `<data>/icons/`, filled by `tools/fetch_icons.py`; see [`icon_dir`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::Session;

/// The page.
pub const PAGE: &str = include_str!("index.html");

/// The `Cache-Control` of an icon: a `FileDataID` always names the same texture.
const ICON_CACHE: &str = "public, max-age=604800, immutable";

/// A response: status, content type, caching and body.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub cache_control: Option<&'static str>,
    pub body: Vec<u8>,
}

impl Reply {
    fn ok(content_type: &'static str, body: Vec<u8>) -> Reply {
        Reply {
            status: 200,
            content_type,
            cache_control: None,
            body,
        }
    }

    fn json(value: &impl Serialize) -> Reply {
        let body = serde_json::to_vec(value).expect("frames serialize");
        Reply::ok("application/json", body)
    }

    /// An error reply: `status` and a plain-text `message`.
    pub fn error(status: u16, message: impl Into<String>) -> Reply {
        Reply {
            status,
            content_type: "text/plain; charset=utf-8",
            cache_control: None,
            body: message.into().into_bytes(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Advance {
    to: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StepKind {
    Event,
    Cast,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    kind: StepKind,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Restart {
    seed: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cast {
    spell: String,
    at: f64,
}

/// The PNG of icon `FileDataID`, if there is one.
pub type Icons<'a> = &'a dyn Fn(u32) -> Option<Vec<u8>>;

/// The icon lookup of the icon directory `dir`: `<dir>/<FileDataID>.png`.
pub fn icon_dir(dir: &Path) -> impl Fn(u32) -> Option<Vec<u8>> + use<> {
    let dir: PathBuf = dir.to_owned();
    move |id| std::fs::read(dir.join(format!("{id}.png"))).ok()
}

/// Answers one request. Icons come from `icons`; `new_seed` gives the seed of a restart that
/// names none.
pub fn route(
    session: &mut Session,
    icons: Icons,
    method: &str,
    path: &str,
    body: &str,
    new_seed: impl FnOnce() -> u64,
) -> Reply {
    if let Some(name) = path.strip_prefix("/icons/") {
        return match method {
            "GET" => icon(icons, name),
            _ => Reply::error(405, format!("{method} not allowed on {path}")),
        };
    }
    match (method, path) {
        ("GET", "/") => Reply::ok("text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
        ("GET", "/api/info") => Reply::json(&session.info()),
        ("POST", "/api/advance") => match serde_json::from_str::<Advance>(body) {
            Ok(Advance { to }) if to.is_finite() => Reply::json(&session.advance(to)),
            Ok(_) => Reply::error(400, "`to` must be a finite number"),
            Err(error) => Reply::error(400, error.to_string()),
        },
        ("POST", "/api/step") => match serde_json::from_str::<Step>(body) {
            Ok(Step {
                kind: StepKind::Event,
            }) => Reply::json(&session.step_event()),
            Ok(Step {
                kind: StepKind::Cast,
            }) => Reply::json(&session.step_cast()),
            Err(error) => Reply::error(400, error.to_string()),
        },
        ("POST", "/api/restart") => {
            let seed = match serde_json::from_str::<Restart>(body) {
                Ok(Restart { seed: None }) => new_seed(),
                Ok(Restart { seed: Some(seed) }) => match seed.parse() {
                    Ok(seed) => seed,
                    Err(_) => return Reply::error(400, format!("invalid seed '{seed}'")),
                },
                Err(error) => return Reply::error(400, error.to_string()),
            };
            session.restart(seed);
            Reply::json(&session.info())
        }
        ("POST", "/api/cast") => match serde_json::from_str::<Cast>(body) {
            Ok(Cast { spell, at }) => match session.cast(&spell, at) {
                Ok(frame) => Reply::json(&frame),
                Err(error) => Reply::error(400, error),
            },
            Err(error) => Reply::error(400, error.to_string()),
        },
        (_, "/" | "/api/info" | "/api/advance" | "/api/step" | "/api/restart" | "/api/cast") => {
            Reply::error(405, format!("{method} not allowed on {path}"))
        }
        _ => Reply::error(404, format!("no {path}")),
    }
}

/// The icon `name` (`<FileDataID>.png`) of `icons`. Only the parsed number reaches the lookup,
/// so a request cannot name anything else.
fn icon(icons: Icons, name: &str) -> Reply {
    let id = name
        .strip_suffix(".png")
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|id| id.parse::<u32>().ok());
    let Some(id) = id else {
        return Reply::error(404, format!("no icon {name}"));
    };
    match icons(id) {
        Some(png) => Reply {
            cache_control: Some(ICON_CACHE),
            ..Reply::ok("image/png", png)
        },
        None => Reply::error(404, format!("no icon {id} (run tools/fetch_icons.py)")),
    }
}

#[cfg(test)]
mod tests;
