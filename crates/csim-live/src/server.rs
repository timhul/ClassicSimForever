//! The HTTP server: the page and a small JSON API over one [`Session`], answered one request at
//! a time on 127.0.0.1.
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
//! - `GET /icons/<FileDataID>.png`: an icon of the frames, from the icon directory
//!   (`<data>/icons/`, filled by `tools/fetch_icons.py`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::session::Session;

const PAGE: &str = include_str!("index.html");

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

    fn error(status: u16, message: impl Into<String>) -> Reply {
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

/// Answers one request. Icons are read from `icons`; `new_seed` gives the seed of a restart
/// that names none.
pub fn route(
    session: &mut Session,
    icons: &Path,
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

/// The icon file `name` (`<FileDataID>.png`) of `icons`. The file name is rebuilt from the
/// parsed number, so a request cannot name anything else.
fn icon(icons: &Path, name: &str) -> Reply {
    let id = name
        .strip_suffix(".png")
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|id| id.parse::<u32>().ok());
    let Some(id) = id else {
        return Reply::error(404, format!("no icon {name}"));
    };
    match std::fs::read(icons.join(format!("{id}.png"))) {
        Ok(png) => Reply {
            cache_control: Some(ICON_CACHE),
            ..Reply::ok("image/png", png)
        },
        Err(_) => Reply::error(404, format!("no icon {id} (run tools/fetch_icons.py)")),
    }
}

/// Serves `session` on 127.0.0.1:`port` until the process is stopped.
///
/// # Errors
/// The port cannot be bound.
pub fn serve(
    mut session: Session,
    icons: &Path,
    port: u16,
    new_seed: impl Fn() -> u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = tiny_http::Server::http(("127.0.0.1", port))?;
    for mut request in server.incoming_requests() {
        let mut body = String::new();
        let reply = match request.as_reader().read_to_string(&mut body) {
            Ok(_) => {
                let method = request.method().as_str().to_owned();
                let path = request.url().split('?').next().unwrap_or("").to_owned();
                route(&mut session, icons, &method, &path, &body, &new_seed)
            }
            Err(error) => Reply::error(400, error.to_string()),
        };
        let header = |name: &str, value: &str| {
            tiny_http::Header::from_bytes(name, value).expect("a valid header")
        };
        let mut response = tiny_http::Response::from_data(reply.body)
            .with_status_code(reply.status)
            .with_header(header("Content-Type", reply.content_type));
        if let Some(cache_control) = reply.cache_control {
            response.add_header(header("Cache-Control", cache_control));
        }
        if let Err(error) = request.respond(response) {
            eprintln!("cannot answer: {error}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
