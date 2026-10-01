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

use serde::{Deserialize, Serialize};

use crate::session::Session;

const PAGE: &str = include_str!("index.html");

/// A response: status, content type and body.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Reply {
    fn json(value: &impl Serialize) -> Reply {
        Reply {
            status: 200,
            content_type: "application/json",
            body: serde_json::to_string(value).expect("frames serialize"),
        }
    }

    fn error(status: u16, message: impl Into<String>) -> Reply {
        Reply {
            status,
            content_type: "text/plain; charset=utf-8",
            body: message.into(),
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

/// Answers one request. `new_seed` gives the seed of a restart that names none.
pub fn route(
    session: &mut Session,
    method: &str,
    path: &str,
    body: &str,
    new_seed: impl FnOnce() -> u64,
) -> Reply {
    match (method, path) {
        ("GET", "/") => Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: PAGE.to_owned(),
        },
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
        (_, "/" | "/api/info" | "/api/advance" | "/api/step" | "/api/restart") => {
            Reply::error(405, format!("{method} not allowed on {path}"))
        }
        _ => Reply::error(404, format!("no {path}")),
    }
}

/// Serves `session` on 127.0.0.1:`port` until the process is stopped.
///
/// # Errors
/// The port cannot be bound.
pub fn serve(
    mut session: Session,
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
                route(&mut session, &method, &path, &body, &new_seed)
            }
            Err(error) => Reply::error(400, error.to_string()),
        };
        let header = tiny_http::Header::from_bytes("Content-Type", reply.content_type)
            .expect("a valid header");
        let response = tiny_http::Response::from_string(reply.body)
            .with_status_code(reply.status)
            .with_header(header);
        if let Err(error) = request.respond(response) {
            eprintln!("cannot answer: {error}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
