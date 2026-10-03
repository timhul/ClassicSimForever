//! The wall clock, for the engine statistics and clock seeds. In the browser
//! (`wasm32-unknown-unknown`) `std::time::Instant::now()` and `SystemTime::now()` compile but
//! panic, so there the clock is `web_time`'s (`performance.now()` / `Date.now()`); everywhere
//! else it is `std::time`'s.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};
