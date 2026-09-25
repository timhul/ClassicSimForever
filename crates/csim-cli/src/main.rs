//! Headless command line runner for ClassicSimForever.
//!
//! - `csim run <character.yaml>` simulates a character setup, alone or in a raid
//!   (`--raid <raid.yaml>`), and prints the results (`--output-format terminal|yaml|html`,
//!   `--output-file <path>`). `--scale` adds the stat weights; `--weights-file <path>` writes
//!   them per item stat point.
//! - `csim validate` loads the data directory and checks every character and raid setup
//!   against it.
//! - `csim list-items` / `list-spells` / `list-rotations` list what setups can refer to.
//! - `csim rank-items --weights <weights.yaml>` ranks items by their stats times the stat
//!   weights `csim run --scale --weights-file` wrote.
//!
//! Every command reads the data directory given by `--data`; without it, `./data` when it
//! exists, else the repository's `data/`.

mod html;
mod list;
mod rank;
mod run;
mod table;
mod weights;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use csim_engine::character_loader::CharacterSetup;
use csim_engine::data_bundle::DataBundle;
use csim_engine::raid_loader::RaidSetup;
use serde::de::DeserializeOwned;
use serde::Serialize;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Parser)]
#[command(name = "csim", version, about = "ClassicSimForever simulator")]
struct Cli {
    /// The data directory (default: ./data, else the repository's data/).
    #[arg(long, global = true, value_name = "DIR")]
    data: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Simulates a character setup and prints DPS, TPS and the breakdowns.
    Run(run::RunArgs),
    /// Loads and cross-validates every data file, character setup and raid setup.
    Validate {
        /// Character setups to check besides the ones in <data>/characters.
        setups: Vec<PathBuf>,
    },
    /// Lists the items, optionally of one slot or phase.
    ListItems(list::ItemArgs),
    /// Lists the spells, optionally of one class.
    ListSpells(list::SpellArgs),
    /// Lists the rotations, optionally of one class.
    ListRotations(list::RotationArgs),
    /// Ranks items by their stats times the stat weights of `run --scale --weights-file`.
    RankItems(rank::RankArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let data_dir = cli.data.unwrap_or_else(default_data_dir);
    let result = match cli.command {
        Command::Run(args) => run::run(&data_dir, &args),
        Command::Validate { setups } => validate(&data_dir, &setups),
        Command::ListItems(args) => load(&data_dir).map(|data| list::items(&data, &args)),
        Command::ListSpells(args) => load(&data_dir).map(|data| list::spells(&data, &args)),
        Command::ListRotations(args) => load(&data_dir).map(|data| list::rotations(&data, &args)),
        Command::RankItems(args) => load(&data_dir).and_then(|data| rank::items(&data, &args)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn default_data_dir() -> PathBuf {
    let local = PathBuf::from("data");
    if local.is_dir() {
        local
    } else {
        DataBundle::repository_dir()
    }
}

fn load(dir: &Path) -> Result<DataBundle> {
    DataBundle::load(dir).map_err(|error| format!("{}: {error}", dir.display()).into())
}

/// Loads the data, then builds every setup of `<data>/characters` and `extra` and every raid of
/// `<data>/raids` against it.
fn validate(dir: &Path, extra: &[PathBuf]) -> Result<()> {
    let data = load(dir)?;
    println!(
        "Data {}: {} spells, {} items, {} enchants, {} rotations",
        dir.display(),
        data.spells.len(),
        data.equipment.len(),
        data.equipment.enchants().len(),
        data.rotations.len()
    );

    let characters = dir.join("characters");
    let mut setups = if characters.is_dir() {
        CharacterSetup::load_dir(&characters)?
    } else {
        Vec::new()
    };
    for path in extra {
        setups.push(CharacterSetup::load(path)?);
    }

    let mut invalid = 0;
    for setup in &setups {
        let label = setup
            .path
            .as_ref()
            .map_or_else(|| setup.name.clone(), |path| path.display().to_string());
        match setup.validate(&data) {
            Ok(()) => println!("ok       {label}"),
            Err(error) => {
                invalid += 1;
                println!("INVALID  {error}");
            }
        }
    }
    if invalid > 0 {
        return Err(format!("{invalid} of {} character setups are invalid", setups.len()).into());
    }
    println!("{} character setups are valid", setups.len());

    let raids = dir.join("raids");
    let raids = if raids.is_dir() {
        RaidSetup::load_dir(&raids)?
    } else {
        Vec::new()
    };
    for raid in &raids {
        let label = raid
            .path
            .as_ref()
            .map_or_else(|| raid.name.clone(), |path| path.display().to_string());
        match raid.validate(dir, &data) {
            Ok(()) => println!("ok       {label}"),
            Err(error) => {
                invalid += 1;
                println!("INVALID  {error}");
            }
        }
    }
    if invalid > 0 {
        return Err(format!("{invalid} of {} raid setups are invalid", raids.len()).into());
    }
    println!("{} raid setups are valid", raids.len());
    Ok(())
}

/// The serde name of a unit variant (`MAINHAND`, `2H`, ...), as the data files spell it.
fn serde_name(value: &impl Serialize) -> String {
    serde_yaml::to_string(value)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Parses a unit variant from its serde name, case-insensitively for SCREAMING_SNAKE_CASE.
fn parse_serde_name<T: DeserializeOwned>(text: &str) -> std::result::Result<T, String> {
    serde_yaml::from_str(&text.to_uppercase())
        .or_else(|_| serde_yaml::from_str(text))
        .map_err(|_| format!("unknown value {text:?}"))
}
