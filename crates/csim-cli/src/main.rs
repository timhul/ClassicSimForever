//! Headless command line runner for ClassicSimForever.
//!
//! The actual sub-commands (`run`, `validate`, ...) are added in Phase 5 of `TASKS.md`.

fn main() {
    println!(
        "csim {} (engine {})",
        env!("CARGO_PKG_VERSION"),
        csim_engine::VERSION
    );
}
