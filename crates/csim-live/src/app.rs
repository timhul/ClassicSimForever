//! What the page talks to: the data, the files the setups come from, and the [`Session`] being
//! watched, if one is loaded. The page lists the bundled setups and keybinds
//! ([`App::catalog`]) and loads one ([`App::load`]): a setup by name or as pasted YAML, played
//! by its rotation or from the keyboard, with a seed, a length, named settings, the target's
//! creature type and armor, and changes of its race, gear, talents and external buffs and
//! debuffs. Instead of a setup it can load a bare character ([`Bare`]): a class and race with
//! nothing but a rotation, to put together in the page.
//!
//! Beside the session, the app runs at most one sim of many iterations ([`App::start_sim`],
//! [`SimJob`]), of a setup resolved as a load's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use csim_engine::character_loader::{CharacterSetup, GearChange, GearChanged, TargetSetup};
use csim_engine::data_bundle::DataBundle;
use csim_engine::faction::PlayerClass;
use csim_engine::files::{Files, MemFiles, Overlay, yaml_files};
use csim_engine::named_settings::{
    NAMED_SETTINGS, SettingKind, SettingRequirement, parse_setting_pairs,
};
use csim_engine::race::Race;
use csim_engine::sim_settings::SimSettings;
use csim_engine::statistics::NumberCruncher;
use csim_engine::target::CreatureType;
use serde::{Deserialize, Serialize};

use crate::keybinds::{self, Keybind};
use crate::session::{Info, Session};
use crate::sim::{
    MergeRequest, SimJob, SimProgress, SimRequest, SimResults, run_settings, run_shares,
    sim_results,
};
use crate::talents::{self, Talents};

/// The setups' directory under the data directory.
const CHARACTERS: &str = "characters";
/// The keybinds' directory under the data directory.
const KEYBINDS: &str = "keybinds";
/// The file name a pasted setup gets, in the setups' directory (so its `include:`s resolve as
/// a bundled setup's do).
const PASTED: &str = "pasted.yaml";

/// The data, its files and the session being watched.
pub struct App {
    files: Box<dyn Files>,
    /// The data directory within `files` (`""` when the files are rooted there).
    data_dir: PathBuf,
    data: Arc<DataBundle>,
    session: Option<Session>,
    source: Source,
    /// The race the load put in place of the setup's.
    race_override: Option<Race>,
    target: TargetChoice,
    gear: GearChanged,
    talents_changed: bool,
    externals: Externals,
    sim: Option<SimJob>,
}

/// What a load request resolves to: the setup as the request changed it, its settings, seed
/// and keybinds, where it comes from and how it differs from the setup's own.
struct Resolved {
    setup: CharacterSetup,
    settings: SimSettings,
    seed: u64,
    keybinds: Vec<Keybind>,
    source: Source,
    race_override: Option<Race>,
    target: TargetChoice,
    gear: GearChanged,
    talents_changed: bool,
    externals: Externals,
}

/// The session's external buffs and debuffs when they are not the setup's own (`None`: the
/// setup's list).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Externals {
    pub buffs: Option<Vec<String>>,
    pub debuffs: Option<Vec<String>>,
}

/// Where the session's setup and keybinds come from: their catalog names, `None` for pasted
/// text or a file outside the data directory (or no keybinds); or the bare character.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Source {
    pub setup: Option<String>,
    pub keybinds: Option<String>,
    pub bare: Option<Bare>,
}

/// A character of `class` and `race` with nothing but the class's `rotation` (by name): no
/// gear, talents, buffs, debuffs or consumables; the default target, level 60. It loads
/// although its rotation's prerequisites cannot hold without talents
/// ([`Info::missing_prerequisites`](crate::session::Info::missing_prerequisites)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bare {
    pub class: PlayerClass,
    pub race: Race,
    pub rotation: String,
}

impl Bare {
    /// Its setup: `Human Warrior`.
    fn setup(&self) -> CharacterSetup {
        CharacterSetup {
            name: format!("{} {}", self.race.name(), self.class.name()),
            class: self.class,
            race: self.race,
            level: 60,
            phase: None,
            ruleset: None,
            rotation: self.rotation.clone(),
            tanking: false,
            talents: BTreeMap::new(),
            equipment: BTreeMap::new(),
            buffs: Vec::new(),
            debuffs: Vec::new(),
            consumables: Vec::new(),
            target: TargetSetup::default(),
            path: None,
            allow_missing_prerequisites: true,
        }
    }
}

/// The session's info and where it comes from: the answer to `api/info` and `api/load`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Loaded {
    #[serde(flatten)]
    pub info: Info,
    pub source: Source,
    /// The race in place of the setup's (`info.race` is the one played), if the load changed
    /// it.
    pub race_override: Option<Race>,
    pub target: TargetChoice,
    /// How the session's gear differs from the setup's, and the setup's enchants that did
    /// not fit the new items.
    pub gear: GearChanged,
    /// The session's talents as a link carries them ([`talents::code`]) when they are not the
    /// setup's own.
    pub talents_code: Option<String>,
    /// The session's buffs and debuffs lists, each when not the setup's own (as sets).
    #[serde(flatten)]
    pub externals: Externals,
}

/// The target the session fights: the setup's, with the creature type and armor the load
/// changed (the page's Target controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TargetChoice {
    /// The setup's own creature type and base armor.
    pub setup_creature_type: CreatureType,
    pub setup_armor: i32,
    /// What the load changed; `None` keeps the setup's.
    pub creature_type: Option<CreatureType>,
    pub armor: Option<i32>,
}

impl TargetChoice {
    /// The setup's target `target`, unchanged.
    fn unchanged(target: &TargetSetup) -> Self {
        TargetChoice {
            setup_creature_type: target.creature_type,
            setup_armor: target.armor,
            creature_type: None,
            armor: None,
        }
    }

    /// The setup's target `target` with the changes `request` asks for (not the setup's own
    /// values: they are no change).
    fn of(target: &TargetSetup, request: &LoadRequest) -> Result<Self, String> {
        if let Some(armor) = request.target_armor.filter(|armor| *armor < 0) {
            return Err(format!("target armor {armor} is negative"));
        }
        Ok(TargetChoice {
            creature_type: request
                .target_creature_type
                .filter(|kind| *kind != target.creature_type),
            armor: request.target_armor.filter(|armor| *armor != target.armor),
            ..TargetChoice::unchanged(target)
        })
    }

    /// `target` changed as chosen.
    fn apply(&self, target: &mut TargetSetup) {
        target.creature_type = self.creature_type.unwrap_or(self.setup_creature_type);
        target.armor = self.armor.unwrap_or(self.setup_armor);
    }
}

/// What the page can load.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Catalog {
    pub setups: Vec<SetupEntry>,
    /// The classes of the data, with their races and rotations (what a bare character and a
    /// race change can be).
    pub classes: Vec<ClassEntry>,
    /// The keybind files, by name.
    pub keybinds: Vec<String>,
    /// The keybinds of each file of `keybinds` that loads, by its name (the editor's starting
    /// points).
    pub keybind_entries: BTreeMap<String, Vec<Keybind>>,
    /// The named keys a binding can use besides letters, digits and `F1`..`F12`.
    pub keys: &'static [&'static str],
    pub settings: Vec<SettingEntry>,
    /// The creature types a target can be, by name.
    pub creature_types: Vec<&'static str>,
    /// The encounter length in seconds, and its variance in percent, without a choice.
    pub length: u32,
    pub length_variance: f64,
    /// The game client build the data was exported from (the spell files' `build:`).
    pub build: Option<String>,
}

/// A bundled setup.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetupEntry {
    /// The file name without `.yaml`, as `api/load` takes it.
    pub name: String,
    /// The setup's own name.
    pub title: String,
    pub class: &'static str,
    pub race: &'static str,
    pub rotation: String,
    /// Why the file does not load; the other fields are empty then.
    pub error: Option<String>,
}

/// A class of the data.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClassEntry {
    /// As `api/load` takes it (`WARRIOR`).
    pub class: PlayerClass,
    pub name: &'static str,
    /// The races it can be, in the game's race order.
    pub races: Vec<RaceEntry>,
    /// Its rotations, by name.
    pub rotations: Vec<RotationEntry>,
}

/// A race a class can be.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaceEntry {
    /// As `api/load` takes it (`NIGHT_ELF`).
    pub race: Race,
    pub name: &'static str,
    pub faction: &'static str,
}

/// A rotation of a class.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RotationEntry {
    pub name: String,
    /// The spells it cannot do without (a bare character lacks the talented ones).
    pub prerequisites: Vec<String>,
}

/// A named setting (`--setting`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettingEntry {
    pub name: &'static str,
    pub help: &'static str,
    /// What it takes, for the page's control (a select, a number field).
    pub kind: SettingKind,
    pub requires: Option<SettingRequirement>,
}

/// An `api/load` request.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadRequest {
    /// A bundled setup, by name; or `setup_yaml`, a pasted one; or `bare`, a bare character.
    pub setup: Option<String>,
    pub setup_yaml: Option<String>,
    pub bare: Option<Bare>,
    /// The race instead of the setup's; its gear, talents and buffs stay.
    pub race: Option<Race>,
    /// Bundled keybinds, by name, or `keybinds_yaml`, pasted ones: played from the keyboard.
    /// Neither: the rotation plays.
    pub keybinds: Option<String>,
    pub keybinds_yaml: Option<String>,
    /// A string: seeds do not fit a JavaScript number. None: a new seed.
    pub seed: Option<String>,
    pub length: Option<u32>,
    pub length_variance: Option<f64>,
    /// Named settings, as `--setting` takes them: `name:value,name:value`.
    pub settings: Option<String>,
    /// The target's creature type and base armor instead of the setup's.
    pub target_creature_type: Option<CreatureType>,
    pub target_armor: Option<i32>,
    /// Changes of the setup's gear, worn in order: a later change wins over an earlier one it
    /// conflicts with ([`CharacterSetup::change_equipment`]).
    #[serde(default)]
    pub gear: Vec<GearChange>,
    /// Talents instead of the setup's: tab name → talent name → rank, as a setup file has
    /// them; or `talents_code`, as a link carries them ([`talents::code`]).
    pub talents: Option<Talents>,
    pub talents_code: Option<String>,
    /// External buffs and debuffs (`data/external_buffs.yaml` names) instead of the setup's.
    pub buffs: Option<Vec<String>>,
    pub debuffs: Option<Vec<String>>,
}

impl App {
    /// The data `data` loaded from `data_dir` of `files`, with no session yet.
    pub fn new(files: Box<dyn Files>, data_dir: impl Into<PathBuf>, data: Arc<DataBundle>) -> Self {
        App {
            files,
            data_dir: data_dir.into(),
            data,
            session: None,
            source: Source::default(),
            race_override: None,
            target: TargetChoice::unchanged(&TargetSetup::default()),
            gear: GearChanged::default(),
            talents_changed: false,
            externals: Externals::default(),
            sim: None,
        }
    }

    pub fn data(&self) -> &Arc<DataBundle> {
        &self.data
    }

    /// The session being watched, if one is loaded.
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    /// Watches `session`, its setup and keybinds coming from `source` (its target, gear and
    /// talents as the setup has them).
    pub fn set_session(&mut self, session: Session, source: Source) {
        self.race_override = None;
        self.target = TargetChoice::unchanged(session.target());
        self.gear = GearChanged::default();
        self.talents_changed = false;
        self.externals = Externals::default();
        self.session = Some(session);
        self.source = source;
    }

    /// The session's info and source, if one is loaded.
    pub fn loaded(&self) -> Option<Loaded> {
        self.session.as_ref().map(|session| Loaded {
            info: session.info(),
            source: self.source.clone(),
            race_override: self.race_override,
            target: self.target,
            gear: self.gear.clone(),
            talents_code: self
                .talents_changed
                .then(|| session.talents())
                .flatten()
                .map(|spent| talents::code(spent.file(), &spent.setup().into_iter().collect())),
            externals: self.externals.clone(),
        })
    }

    /// The catalog name of the setup file `path`: its file name without `.yaml` when it is in
    /// the setups' directory, else `None`.
    pub fn setup_name(&self, path: &Path) -> Option<String> {
        self.name_in(CHARACTERS, path)
    }

    /// The catalog name of the keybinds file `path` (see [`App::setup_name`]).
    pub fn keybinds_name(&self, path: &Path) -> Option<String> {
        self.name_in(KEYBINDS, path)
    }

    fn name_in(&self, dir: &str, path: &Path) -> Option<String> {
        let dir = self.files.canonical(&self.data_dir.join(dir));
        let parent = self.files.canonical(path.parent()?);
        let is_yaml = path.extension().is_some_and(|ext| ext == "yaml");
        let name = path.file_stem()?.to_str()?;
        (parent == dir && is_yaml && is_catalog_name(name)).then(|| name.to_owned())
    }

    /// The bundled setups (not the shared parts in `characters/common/`), keybinds and named
    /// settings.
    pub fn catalog(&self) -> Catalog {
        let setups = self
            .yaml_files(CHARACTERS)
            .iter()
            .filter_map(|path| {
                let name = path.file_stem()?.to_str()?.to_owned();
                is_catalog_name(&name).then(|| self.setup_entry(name, path))
            })
            .collect();
        let keybind_files: Vec<(String, PathBuf)> = self
            .yaml_files(KEYBINDS)
            .into_iter()
            .filter_map(|path| Some((path.file_stem()?.to_str()?.to_owned(), path)))
            .filter(|(name, _)| is_catalog_name(name))
            .collect();
        let keybind_entries = keybind_files
            .iter()
            .filter_map(|(name, path)| {
                let keybinds = keybinds::load_from(self.files.as_ref(), path).ok()?;
                Some((name.clone(), keybinds))
            })
            .collect();
        let keybinds = keybind_files.into_iter().map(|(name, _)| name).collect();
        let defaults = SimSettings::default();
        Catalog {
            setups,
            classes: self.class_entries(),
            keybinds,
            keybind_entries,
            keys: keybinds::NAMED_KEYS,
            settings: NAMED_SETTINGS
                .iter()
                .map(|setting| SettingEntry {
                    name: setting.name,
                    help: setting.help,
                    kind: setting.kind,
                    requires: setting.requires,
                })
                .collect(),
            creature_types: CreatureType::ALL.iter().map(|kind| kind.name()).collect(),
            length: defaults.combat_length,
            length_variance: defaults.length_variance,
            build: self.data.build().map(str::to_owned),
        }
    }

    fn class_entries(&self) -> Vec<ClassEntry> {
        self.data
            .classes
            .classes()
            .filter_map(|class| {
                let spec = self.data.classes.get(class).ok()?;
                let races = Race::ALL
                    .into_iter()
                    .filter(|race| spec.race_available(*race))
                    .map(|race| RaceEntry {
                        race,
                        name: race.name(),
                        faction: race.faction().name(),
                    })
                    .collect();
                let rotations = self
                    .data
                    .rotations
                    .rotations_for(class)
                    .iter()
                    .map(|rotation| RotationEntry {
                        name: rotation.name.clone(),
                        prerequisites: rotation.prerequisites.clone(),
                    })
                    .collect();
                Some(ClassEntry {
                    class,
                    name: class.name(),
                    races,
                    rotations,
                })
            })
            .collect()
    }

    fn yaml_files(&self, dir: &str) -> Vec<PathBuf> {
        yaml_files(self.files.as_ref(), &self.data_dir.join(dir)).unwrap_or_default()
    }

    fn setup_entry(&self, name: String, path: &Path) -> SetupEntry {
        let entry = |title, class, race, rotation, error| SetupEntry {
            name: name.clone(),
            title,
            class,
            race,
            rotation,
            error,
        };
        match CharacterSetup::load_from(self.files.as_ref(), path) {
            Ok(setup) => entry(
                setup.name,
                setup.class.name(),
                setup.race.name(),
                setup.rotation,
                None,
            ),
            Err(error) => entry(
                String::new(),
                "",
                "",
                String::new(),
                Some(error.to_string()),
            ),
        }
    }

    /// Loads the session `request` asks for and watches it instead of the current one, which
    /// stays when the request fails. `new_seed` gives the seed when it names none.
    ///
    /// # Errors
    /// The request is malformed (no setup, or more than one; an unknown name), a file does not
    /// load, the seed, a setting or a gear change is invalid, or the setup does not build (a
    /// race the class cannot be, talents that cannot be spent, a rotation's prerequisite not
    /// taken).
    pub fn load(
        &mut self,
        request: LoadRequest,
        new_seed: impl FnOnce() -> u64,
    ) -> Result<Loaded, String> {
        let resolved = self.resolve(request, new_seed)?;
        let session = Session::new(
            Arc::clone(&self.data),
            resolved.setup,
            resolved.settings,
            resolved.seed,
            resolved.keybinds,
        )?;
        self.set_session(session, resolved.source);
        self.race_override = resolved.race_override;
        self.target = resolved.target;
        self.gear = resolved.gear;
        self.talents_changed = resolved.talents_changed;
        self.externals = resolved.externals;
        Ok(self.loaded().expect("a session was just loaded"))
    }

    /// Starts the sim `request` asks for in place of the current one, which stays when the
    /// request fails. Its setup is the one `api/load` would load (but played by its rotation);
    /// `new_seed` gives the seed when it names none.
    ///
    /// # Errors
    /// As [`App::load`], and the iterations are not in `1..=`[`MAX_ITERATIONS`](crate::sim::MAX_ITERATIONS).
    pub fn start_sim(
        &mut self,
        request: SimRequest,
        new_seed: impl FnOnce() -> u64,
    ) -> Result<SimProgress, String> {
        let load = LoadRequest {
            keybinds: None,
            keybinds_yaml: None,
            ..request.load
        };
        let resolved = self.resolve(load, new_seed)?;
        let job = SimJob::new(
            Arc::clone(&self.data),
            resolved.setup,
            resolved.settings,
            resolved.seed,
            (request.iterations, request.threads, request.share),
        )?;
        Ok(self.sim.insert(job).progress())
    }

    /// The results of a run whose shares ran apart (in the browser's workers, each its own
    /// [`App::start_sim`]): `request` names the run as their starts did, with the seed they
    /// ran with, and gives each share's statistics in order.
    ///
    /// # Errors
    /// As [`App::start_sim`]; the request names no seed, or the shares are not the run's (their
    /// number, or one's iterations).
    pub fn merge_sim(&self, request: MergeRequest) -> Result<SimResults, String> {
        if request
            .load
            .seed
            .as_deref()
            .is_none_or(|seed| seed.trim().is_empty())
        {
            return Err("no seed: name the one the shares ran with".into());
        }
        if !request.elapsed_seconds.is_finite() || request.elapsed_seconds < 0.0 {
            return Err("`elapsed_seconds` must be a finite number >= 0".into());
        }
        let load = LoadRequest {
            keybinds: None,
            keybinds_yaml: None,
            ..request.load
        };
        let resolved = self.resolve(load, || unreachable!("the seed is given"))?;
        let shares = run_shares(request.iterations, request.threads, resolved.seed)?;
        if shares.len() != request.shares.len() {
            return Err(format!(
                "{} shares for {} threads of {} iterations: {} expected",
                request.shares.len(),
                request.threads,
                request.iterations,
                shares.len()
            ));
        }
        let mut cruncher = NumberCruncher::new();
        for (index, (share, statistics)) in shares.iter().zip(request.shares).enumerate() {
            if statistics.iterations() != u64::from(share.iterations) {
                return Err(format!(
                    "share {index} has {} iterations: {} expected",
                    statistics.iterations(),
                    share.iterations
                ));
            }
            cruncher.add_class_statistics(None, statistics);
        }
        let settings = run_settings(resolved.settings, request.iterations, request.threads)?;
        let raid = resolved
            .setup
            .build_raid(&self.data, &settings)
            .map_err(|error| error.to_string())?;
        Ok(sim_results(
            &self.data,
            &resolved.setup,
            &settings,
            resolved.seed,
            Duration::from_secs_f64(request.elapsed_seconds),
            &cruncher,
            &raid,
        ))
    }

    /// The sim started last, if any (until [`App::stop_sim`]).
    pub fn sim(&self) -> Option<&SimJob> {
        self.sim.as_ref()
    }

    pub fn sim_mut(&mut self) -> Option<&mut SimJob> {
        self.sim.as_mut()
    }

    /// Drops the sim, done or not.
    pub fn stop_sim(&mut self) {
        self.sim = None;
    }

    /// What `request` asks for, resolved against the data: see [`App::load`].
    fn resolve(
        &self,
        request: LoadRequest,
        new_seed: impl FnOnce() -> u64,
    ) -> Result<Resolved, String> {
        let (mut setup, setup_name) = self.load_setup(&request)?;
        let race_override = request.race.filter(|race| *race != setup.race);
        if let Some(race) = race_override {
            setup.race = race;
        }
        let target = TargetChoice::of(&setup.target, &request)?;
        target.apply(&mut setup.target);
        let (keybinds, keybinds_name) = self.load_keybinds(&request)?;
        let mut settings = setup.sim_settings(&SimSettings {
            combat_length: request
                .length
                .unwrap_or(SimSettings::default().combat_length),
            length_variance: request
                .length_variance
                .unwrap_or(SimSettings::default().length_variance),
            ..SimSettings::default()
        });
        if let Some(text) = request
            .settings
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            let pairs = parse_setting_pairs(text).map_err(|error| error.to_string())?;
            settings
                .apply_settings(&pairs)
                .map_err(|error| error.to_string())?;
        }
        let gear = setup.change_equipment(&self.data, settings.phase, &request.gear)?;
        let talents = match (request.talents, request.talents_code) {
            (Some(talents), None) => Some(talents),
            (None, Some(code)) => {
                let file = self
                    .data
                    .talents
                    .get(setup.class)
                    .ok_or_else(|| format!("no talent tree for the {:?}", setup.class))?;
                Some(talents::from_code(file, &code)?)
            }
            (None, None) => None,
            (Some(_), Some(_)) => return Err("both `talents` and `talents_code`: give one".into()),
        };
        let talents_changed = match talents {
            Some(talents) => {
                let talents = spent(talents);
                let changed = talents != spent(std::mem::take(&mut setup.talents));
                setup.talents = talents;
                changed
            }
            None => false,
        };
        let externals = Externals {
            buffs: replace_list(&mut setup.buffs, request.buffs),
            debuffs: replace_list(&mut setup.debuffs, request.debuffs),
        };
        let seed = match request.seed.as_deref().map(str::trim) {
            None | Some("") => new_seed(),
            Some(seed) => seed.parse().map_err(|_| format!("invalid seed '{seed}'"))?,
        };
        Ok(Resolved {
            setup,
            settings,
            seed,
            keybinds,
            source: Source {
                setup: setup_name,
                keybinds: keybinds_name,
                bare: request.bare,
            },
            race_override,
            target,
            gear,
            talents_changed,
            externals,
        })
    }

    fn load_setup(
        &self,
        request: &LoadRequest,
    ) -> Result<(CharacterSetup, Option<String>), String> {
        let given = [
            request.setup.is_some(),
            request.setup_yaml.is_some(),
            request.bare.is_some(),
        ];
        if given.into_iter().filter(|given| *given).count() > 1 {
            return Err("more than one of `setup`, `setup_yaml` and `bare`: give one".into());
        }
        if let Some(bare) = &request.bare {
            return Ok((bare.setup(), None));
        }
        let loaded = match (&request.setup, &request.setup_yaml) {
            (Some(name), None) => {
                let path = self.catalog_path(CHARACTERS, name)?;
                CharacterSetup::load_from(self.files.as_ref(), &path)
                    .map(|setup| (setup, Some(name.clone())))
            }
            (None, Some(text)) => {
                let path = self.data_dir.join(CHARACTERS).join(PASTED);
                let pasted: MemFiles = [(&path, text.as_str())].into_iter().collect();
                let files = Overlay {
                    top: &pasted,
                    base: self.files.as_ref(),
                };
                CharacterSetup::load_from(&files, &path).map(|setup| (setup, None))
            }
            (None, None) => {
                return Err(
                    "no setup: name one (`setup`), paste one (`setup_yaml`) or ask for a bare \
                     character (`bare`)"
                        .into(),
                );
            }
            (Some(_), Some(_)) => unreachable!("rejected above"),
        };
        loaded.map_err(|error| error.to_string())
    }

    fn load_keybinds(
        &self,
        request: &LoadRequest,
    ) -> Result<(Vec<Keybind>, Option<String>), String> {
        match (&request.keybinds, &request.keybinds_yaml) {
            (Some(name), None) => {
                let path = self.catalog_path(KEYBINDS, name)?;
                let keybinds = keybinds::load_from(self.files.as_ref(), &path)?;
                Ok((keybinds, Some(name.clone())))
            }
            (None, Some(text)) => keybinds::parse(text)
                .map(|keybinds| (keybinds, None))
                .map_err(|error| format!("pasted keybinds: {error}")),
            (None, None) => Ok((Vec::new(), None)),
            (Some(_), Some(_)) => Err("both `keybinds` and `keybinds_yaml`: give one".into()),
        }
    }

    /// The file of catalog entry `name` in `dir`. The name is a plain file name, so it cannot
    /// reach outside the directory.
    fn catalog_path(&self, dir: &str, name: &str) -> Result<PathBuf, String> {
        if !is_catalog_name(name) {
            return Err(format!("no {dir} entry '{name}'"));
        }
        Ok(self.data_dir.join(dir).join(format!("{name}.yaml")))
    }
}

/// `talents` without the talents of rank 0 and the tabs left empty: the same build.
fn spent(talents: Talents) -> Talents {
    talents
        .into_iter()
        .map(|(tab, ranks)| {
            (
                tab,
                ranks.into_iter().filter(|&(_, rank)| rank > 0).collect(),
            )
        })
        .filter(|(_, ranks): &(String, BTreeMap<String, u32>)| !ranks.is_empty())
        .collect()
}

/// `list` replaced by `new`, if any; `new` when it is not `list`'s entries (in any order).
fn replace_list(list: &mut Vec<String>, new: Option<Vec<String>>) -> Option<Vec<String>> {
    let new = new?;
    let as_set = |names: &[String]| names.iter().cloned().collect::<BTreeSet<_>>();
    let changed = as_set(list) != as_set(&new);
    *list = new.clone();
    changed.then_some(new)
}

/// A catalog name: a file name without extension, of letters, digits, `_` and `-`.
fn is_catalog_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests;
