//! One iteration of a character setup, played at the page's pace: the [`session`], the page and
//! the JSON API that drives it ([`server::route`]), and the keyboard bindings it can be played
//! with ([`keybinds`]).
//!
//! The native `csim-live` binary serves [`server::route`] over HTTP (feature `server`, on by
//! default); the browser build calls it directly, without the `server` feature.

pub mod keybinds;
pub mod server;
pub mod session;
