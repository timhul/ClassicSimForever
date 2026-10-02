//! Watch one iteration of a character setup unfold in real time in a browser.
//!
//! `csim-live <character.yaml> [--seed S] [--length L] [--length-variance V]` builds the setup
//! like `csim run` and serves a page that plays its iteration of the seed: the same iteration
//! `csim run <character.yaml> --combat-log --seed S` logs. The engine is used as a library;
//! the page drives the pace (see `session`).
//!
//! With `--keybinds <file.yaml>` the rotation does not run: the character is played from the
//! keyboard, with the spells the file binds (see `keybinds`).

mod keybinds;
mod server;
mod session;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use csim_engine::character_loader::CharacterSetup;
use csim_engine::data_bundle::DataBundle;
use csim_engine::sim_settings::SimSettings;

use crate::session::Session;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Parser)]
#[command(
    name = "csim-live",
    version,
    about = "Watch one ClassicSimForever iteration live"
)]
struct Args {
    /// The character setup (a YAML file, e.g. data/characters/warrior_fury_dw_orc.yaml).
    setup: PathBuf,
    /// The data directory (default: ./data, else the repository's data/).
    #[arg(long, value_name = "DIR")]
    data: Option<PathBuf>,
    /// Seed fixing every random roll of the iteration (default: from the clock; printed).
    #[arg(long)]
    seed: Option<u64>,
    /// Encounter length in seconds.
    #[arg(long, short = 'l', default_value_t = SimSettings::default().combat_length)]
    length: u32,
    /// Encounter length variance in percent, as `csim run --length-variance`.
    #[arg(long, default_value_t = SimSettings::default().length_variance, value_name = "PERCENT")]
    length_variance: f64,
    /// The local port to serve the page on.
    #[arg(long, default_value_t = 7878)]
    port: u16,
    /// Play the character from the keyboard instead of its rotation: a YAML map of spell name
    /// to key (`Bloodthirst: 1`, `Execute: Shift+E`, `Recklessness: Ctrl+Alt+F1`).
    #[arg(long, value_name = "FILE")]
    keybinds: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<()> {
    let data_dir = args.data.clone().unwrap_or_else(default_data_dir);
    let data =
        DataBundle::load(&data_dir).map_err(|error| format!("{}: {error}", data_dir.display()))?;
    let setup = CharacterSetup::load(&args.setup)?;
    let settings = setup.sim_settings(&SimSettings {
        combat_length: args.length,
        length_variance: args.length_variance,
        ..SimSettings::default()
    });
    let seed = args.seed.unwrap_or_else(clock_seed);
    let keybinds = match &args.keybinds {
        Some(path) => keybinds::load(path)?,
        None => Vec::new(),
    };
    let session = Session::new(Arc::new(data), setup, settings, seed, keybinds)?;
    let info = session.info();
    println!(
        "Serving {} ({} {}, {}), seed {}, at http://127.0.0.1:{}",
        info.name, info.race, info.class, info.rotation, info.seed, args.port
    );
    if info.manual {
        println!("Played from the keyboard: {} keybinds", info.keybinds.len());
    }
    let icons = data_dir.join("icons");
    if !icons.is_dir() {
        println!(
            "No icons in {}: run `python tools/fetch_icons.py` to show them",
            icons.display()
        );
    }
    server::serve(session, &icons, args.port, clock_seed).map_err(
        |error| -> Box<dyn std::error::Error> {
            format!("cannot serve on port {}: {error}", args.port).into()
        },
    )
}

/// `./data` when it exists, else the repository's `data/` (as `csim`).
fn default_data_dir() -> PathBuf {
    let local = PathBuf::from("data");
    if local.is_dir() {
        local
    } else {
        DataBundle::repository_dir()
    }
}

/// A seed from the clock (as `csim`).
fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64)
}
