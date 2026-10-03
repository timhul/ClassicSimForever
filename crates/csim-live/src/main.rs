//! Watch one iteration of a character setup unfold in real time in a browser.
//!
//! `csim-live <character.yaml> [--seed S] [--length L] [--length-variance V]` builds the setup
//! like `csim run` and serves a page that plays its iteration of the seed: the same iteration
//! `csim run <character.yaml> --combat-log --seed S` logs. The engine is used as a library;
//! the page drives the pace (see `session`).
//!
//! With `--keybinds <file.yaml>` the rotation does not run: the character is played from the
//! keyboard, with the spells the file binds (see `keybinds`).
//!
//! The flags load the first session; the page can load others (a bundled setup, or pasted
//! YAML). Without a setup the page starts with one of its choice.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use csim_engine::character_loader::CharacterSetup;
use csim_engine::data_bundle::DataBundle;
use csim_engine::files::FsFiles;
use csim_engine::named_settings::SettingPairs;
use csim_engine::sim_settings::SimSettings;

use csim_live::app::{App, Source};
use csim_live::server::{self, Icons, Reply};
use csim_live::{keybinds, session::Session};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Parser)]
#[command(
    name = "csim-live",
    version,
    about = "Watch one ClassicSimForever iteration live"
)]
struct Args {
    /// The character setup (a YAML file, e.g. data/characters/warrior_fury_dw_orc.yaml).
    /// Without one the page loads one of its choice (it can load others either way).
    setup: Option<PathBuf>,
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
    /// Named settings, as `csim run --setting`: `name:value` pairs separated by commas; may be
    /// repeated. E.g. `--setting=rage_formula:marrow_sigmoid,sigmoid_ceiling:46`.
    #[arg(long = "setting", value_name = "NAME:VALUE,...")]
    settings: Vec<SettingPairs>,
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
    let mut app = App::new(Box::new(FsFiles), &data_dir, Arc::new(data));
    match &args.setup {
        Some(setup) => load_flags(&mut app, args, setup)?,
        None => println!(
            "Serving at http://127.0.0.1:{}: the page picks the setup",
            args.port
        ),
    }
    let icons = data_dir.join("icons");
    if !icons.is_dir() {
        println!(
            "No icons in {}: icons come from Wowhead's CDN only (offline, or for an icon \
             without a name: run `python tools/fetch_icons.py`)",
            icons.display()
        );
    }
    serve(app, &server::icon_dir(&icons), args.port, clock_seed).map_err(
        |error| -> Box<dyn std::error::Error> {
            format!("cannot serve on port {}: {error}", args.port).into()
        },
    )
}

/// Loads the session the flags ask for into `app`, the setup being the file `path`.
fn load_flags(app: &mut App, args: &Args, path: &Path) -> Result<()> {
    let setup = CharacterSetup::load(path)?;
    let mut settings = setup.sim_settings(&SimSettings {
        combat_length: args.length,
        length_variance: args.length_variance,
        ..SimSettings::default()
    });
    settings.apply_setting_flags(&args.settings)?;
    let seed = args.seed.unwrap_or_else(clock_seed);
    let keybinds = match &args.keybinds {
        Some(path) => keybinds::load(path)?,
        None => Vec::new(),
    };
    let session = Session::new(Arc::clone(app.data()), setup, settings, seed, keybinds)?;
    let info = session.info();
    println!(
        "Serving {} ({} {}, {}), seed {}, at http://127.0.0.1:{}",
        info.name, info.race, info.class, info.rotation, info.seed, args.port
    );
    if let Some(settings) = &info.settings {
        println!("Settings: {settings}");
    }
    if info.manual {
        println!("Played from the keyboard: {} keybinds", info.keybinds.len());
    }
    let source = Source {
        setup: app.setup_name(path),
        keybinds: args
            .keybinds
            .as_deref()
            .and_then(|path| app.keybinds_name(path)),
    };
    app.set_session(session, source);
    Ok(())
}

/// Serves `app` on 127.0.0.1:`port` until the process is stopped.
///
/// # Errors
/// The port cannot be bound.
fn serve(
    mut app: App,
    icons: Icons,
    port: u16,
    new_seed: impl Fn() -> u64,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = tiny_http::Server::http(("127.0.0.1", port))?;
    for mut request in server.incoming_requests() {
        let mut body = String::new();
        let reply = match request.as_reader().read_to_string(&mut body) {
            Ok(_) => {
                let method = request.method().as_str().to_owned();
                let path = request.url().split('?').next().unwrap_or("").to_owned();
                server::route(&mut app, icons, &method, &path, &body, &new_seed)
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
