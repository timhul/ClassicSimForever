//! csim-live in the browser. The page calls [`LiveApp::handle`] where the native viewer sends
//! an HTTP request: the same routes ([`csim_live::server::route`]) over the same kind of
//! [`App`], with the data embedded at build time ([`FILES`]) instead of read from disk.
//!
//! Built for `wasm32-unknown-unknown` and bound with `wasm-bindgen --target web`; the host
//! builds and tests it as an ordinary library.

use std::path::Path;
use std::sync::Arc;

use csim_engine::data_bundle::DataBundle;
use csim_engine::files::MemFiles;
use csim_engine::rng::Xoroshiro128Plus;
use csim_live::app::App;
use csim_live::server;
use wasm_bindgen::prelude::wasm_bindgen;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/files.rs"));
}

pub use embedded::FILES;

/// The embedded data files, rooted at the data directory.
pub fn bundled_files() -> MemFiles {
    FILES.iter().copied().collect()
}

/// The live viewer's server side, in the page.
#[wasm_bindgen]
pub struct LiveApp {
    app: App,
    /// The seeds of the loads and restarts that name none.
    seeds: Xoroshiro128Plus,
}

/// The answer to a request, as an HTTP response would carry it. (Not named `Response`: the
/// generated JS class would shadow the browser's `Response`, which wasm-bindgen's loader
/// checks for.)
#[wasm_bindgen(getter_with_clone)]
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    #[wasm_bindgen(js_name = contentType)]
    pub content_type: String,
    /// JSON, or plain text for an error.
    pub body: String,
}

#[wasm_bindgen]
impl LiveApp {
    /// Loads the embedded data, with no session yet. `seed_hi` and `seed_lo` (a random 64-bit
    /// value, from `crypto.getRandomValues`) seed the iterations the page does not name a seed
    /// for.
    ///
    /// # Errors
    /// The embedded data does not load (a broken build).
    #[wasm_bindgen(constructor)]
    pub fn new(seed_hi: u32, seed_lo: u32) -> Result<LiveApp, String> {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        let files = bundled_files();
        let data =
            DataBundle::load_from(&files, Path::new("")).map_err(|error| error.to_string())?;
        Ok(LiveApp {
            app: App::new(Box::new(files), "", Arc::new(data)),
            seeds: Xoroshiro128Plus::from_seed(u64::from(seed_hi) << 32 | u64::from(seed_lo)),
        })
    }

    /// Answers the request `method` `path` (`/api/...`) with `body`, as the native server does.
    pub fn handle(&mut self, method: &str, path: &str, body: &str) -> Reply {
        let seeds = &mut self.seeds;
        let reply = server::route(&mut self.app, &|_| None, method, path, body, || {
            seeds.next()
        });
        Reply {
            status: reply.status,
            content_type: reply.content_type.to_owned(),
            body: String::from_utf8_lossy(&reply.body).into_owned(),
        }
    }
}

#[cfg(test)]
mod tests;
