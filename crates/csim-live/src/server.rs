//! The page and a small JSON API over an [`App`]: [`route`] answers one request. The native
//! `csim-live` serves it over HTTP on 127.0.0.1; the browser build calls it directly.
//!
//! - `GET /`: the page.
//! - `GET /web.js`: an empty module. In the browser build the page's `web.js` runs the server
//!   side in the page; natively the page finds none and talks over HTTP.
//! - `GET /api/catalog`: the setups, keybinds and named settings the page can load
//!   ([`Catalog`](crate::app::Catalog)).
//! - `POST /api/load {"setup": "warrior_fury_dw_orc", ...}`: loads a session
//!   ([`LoadRequest`]); its info, as `api/info`.
//! - `POST /api/talents/edit {"class": "WARRIOR", "ranks": {node: rank}, "op": "increment",
//!   "node": n}`: a click in the talent calculator ([`EditRequest`]); the new
//!   [`State`](crate::talents::State).
//!
//! The others answer 409 until a session is loaded:
//!
//! - `GET /api/info`: the iteration's [`Info`](crate::session::Info), with the `source` of
//!   its setup and keybinds ([`Loaded`](crate::app::Loaded)).
//! - `GET /api/items`: the items the character can wear
//!   ([`ItemEntry`](crate::sheet::ItemEntry)s, by id).
//! - `GET /api/talents`: the character's talent tree ([`Layout`]) and the ranks it has
//!   (`state`, a [`State`]).
//! - `POST /api/advance {"to": t}`: runs the iteration up to sim time `t`; a frame.
//! - `POST /api/step {"kind": "event" | "cast"}`: runs one event, or up to the next cast; a
//!   frame.
//! - `POST /api/restart {"seed": "S"}`: starts the iteration of seed `S` (a string: seeds do
//!   not fit a JavaScript number), or of a new seed without one; the new info, as
//!   `api/info`.
//! - `POST /api/cast {"spell": "Bloodthirst", "at": t}`: a key press of a bound spell at sim
//!   time `t` (played from the keyboard only); a frame.
//! - `GET /icons/<FileDataID>.png`: an icon of the frames, from the icon lookup (natively the
//!   icon directory `<data>/icons/`, filled by `tools/fetch_icons.py`; see [`icon_dir`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::{App, LoadRequest};
use crate::session::Session;
use crate::talents::{EditRequest, Layout, State, edit};

/// The page.
pub const PAGE: &str = include_str!("index.html");

/// The native server's `web.js`: nothing (see the module documentation).
const WEB_JS: &str = "// The native server: the page talks to it over HTTP.\n";

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

/// The endpoints that need a loaded session.
const SESSION_PATHS: [&str; 7] = [
    "/api/info",
    "/api/items",
    "/api/talents",
    "/api/advance",
    "/api/step",
    "/api/restart",
    "/api/cast",
];

/// Answers one request. Icons come from `icons`; `new_seed` gives the seed of a load or a
/// restart that names none.
pub fn route(
    app: &mut App,
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
        ("GET", "/web.js") => Reply::ok("text/javascript", WEB_JS.as_bytes().to_vec()),
        ("GET", "/api/catalog") => Reply::json(&app.catalog()),
        ("POST", "/api/load") => match serde_json::from_str::<LoadRequest>(body) {
            Ok(request) => match app.load(request, new_seed) {
                Ok(loaded) => Reply::json(&loaded),
                Err(error) => Reply::error(400, error),
            },
            Err(error) => Reply::error(400, error.to_string()),
        },
        ("GET", "/api/info") => match app.loaded() {
            Some(loaded) => Reply::json(&loaded),
            None => no_session(),
        },
        ("GET", "/api/items") => match app.session() {
            Some(session) => Reply::json(&session.items()),
            None => no_session(),
        },
        ("GET", "/api/talents") => match app.session() {
            Some(session) => match session.talents() {
                Some(talents) => Reply::json(&Talents {
                    layout: Layout::of(talents.file()),
                    state: State::of(talents),
                }),
                None => Reply::error(404, "the character's class has no talent tree"),
            },
            None => no_session(),
        },
        ("POST", "/api/talents/edit") => match serde_json::from_str::<EditRequest>(body) {
            Ok(request) => match edit(app.data(), &request) {
                Ok(state) => Reply::json(&state),
                Err(error) => Reply::error(400, error),
            },
            Err(error) => Reply::error(400, error.to_string()),
        },
        ("POST", "/api/advance" | "/api/step" | "/api/restart" | "/api/cast") => {
            let Some(session) = app.session_mut() else {
                return no_session();
            };
            if path == "/api/restart" {
                return match restart_seed(body, new_seed) {
                    Ok(seed) => {
                        session.restart(seed);
                        Reply::json(&app.loaded())
                    }
                    Err(reply) => reply,
                };
            }
            session_route(session, path, body)
        }
        (_, "/" | "/web.js" | "/api/catalog" | "/api/load" | "/api/talents/edit") => {
            Reply::error(405, format!("{method} not allowed on {path}"))
        }
        (_, path) if SESSION_PATHS.contains(&path) => {
            Reply::error(405, format!("{method} not allowed on {path}"))
        }
        _ => Reply::error(404, format!("no {path}")),
    }
}

/// The answer of `api/talents`.
#[derive(Serialize)]
struct Talents {
    #[serde(flatten)]
    layout: Layout,
    state: State,
}

/// The answer of a session endpoint before a load.
fn no_session() -> Reply {
    Reply::error(409, "no session loaded: load one first (api/load)")
}

/// The seed of a restart request; a new one when it names none.
fn restart_seed(body: &str, new_seed: impl FnOnce() -> u64) -> Result<u64, Reply> {
    match serde_json::from_str::<Restart>(body) {
        Ok(Restart { seed: None }) => Ok(new_seed()),
        Ok(Restart { seed: Some(seed) }) => seed
            .parse()
            .map_err(|_| Reply::error(400, format!("invalid seed '{seed}'"))),
        Err(error) => Err(Reply::error(400, error.to_string())),
    }
}

/// A `POST` to `path`, one of the frame endpoints (`advance`, `step`, `cast`), of `session`.
fn session_route(session: &mut Session, path: &str, body: &str) -> Reply {
    match path {
        "/api/advance" => match serde_json::from_str::<Advance>(body) {
            Ok(Advance { to }) if to.is_finite() => Reply::json(&session.advance(to)),
            Ok(_) => Reply::error(400, "`to` must be a finite number"),
            Err(error) => Reply::error(400, error.to_string()),
        },
        "/api/step" => match serde_json::from_str::<Step>(body) {
            Ok(Step {
                kind: StepKind::Event,
            }) => Reply::json(&session.step_event()),
            Ok(Step {
                kind: StepKind::Cast,
            }) => Reply::json(&session.step_cast()),
            Err(error) => Reply::error(400, error.to_string()),
        },
        _ => match serde_json::from_str::<Cast>(body) {
            Ok(Cast { spell, at }) => match session.cast(&spell, at) {
                Ok(frame) => Reply::json(&frame),
                Err(error) => Reply::error(400, error),
            },
            Err(error) => Reply::error(400, error.to_string()),
        },
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
