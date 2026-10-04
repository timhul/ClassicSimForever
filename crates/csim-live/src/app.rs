//! What the page talks to: the data, the files the setups come from, and the [`Session`] being
//! watched, if one is loaded. The page lists the bundled setups and keybinds
//! ([`App::catalog`]) and loads one ([`App::load`]): a setup by name or as pasted YAML, played
//! by its rotation or from the keyboard, with a seed, a length, named settings, the target's
//! creature type and armor, and changes of its gear.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use csim_engine::character_loader::{CharacterSetup, GearChange, GearChanged, TargetSetup};
use csim_engine::data_bundle::DataBundle;
use csim_engine::files::{Files, MemFiles, Overlay, yaml_files};
use csim_engine::named_settings::{
    NAMED_SETTINGS, SettingKind, SettingRequirement, parse_setting_pairs,
};
use csim_engine::sim_settings::SimSettings;
use csim_engine::target::CreatureType;
use serde::{Deserialize, Serialize};

use crate::keybinds::{self, Keybind};
use crate::session::{Info, Session};

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
    target: TargetChoice,
    gear: GearChanged,
}

/// Where the session's setup and keybinds come from: their catalog names, `None` for pasted
/// text or a file outside the data directory (or no keybinds).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Source {
    pub setup: Option<String>,
    pub keybinds: Option<String>,
}

/// The session's info and where it comes from: the answer to `api/info` and `api/load`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Loaded {
    #[serde(flatten)]
    pub info: Info,
    pub source: Source,
    pub target: TargetChoice,
    /// How the session's gear differs from the setup's, and the setup's enchants that did
    /// not fit the new items.
    pub gear: GearChanged,
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
    /// The game client build the data was exported from (`1.60.1.70205`).
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
    /// A bundled setup, by name; or `setup_yaml`, a pasted one.
    pub setup: Option<String>,
    pub setup_yaml: Option<String>,
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
            target: TargetChoice::unchanged(&TargetSetup::default()),
            gear: GearChanged::default(),
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

    /// Watches `session`, its setup and keybinds coming from `source` (its target and gear as
    /// the setup has them).
    pub fn set_session(&mut self, session: Session, source: Source) {
        self.target = TargetChoice::unchanged(session.target());
        self.gear = GearChanged::default();
        self.session = Some(session);
        self.source = source;
    }

    /// The session's info and source, if one is loaded.
    pub fn loaded(&self) -> Option<Loaded> {
        self.session.as_ref().map(|session| Loaded {
            info: session.info(),
            source: self.source.clone(),
            target: self.target,
            gear: self.gear.clone(),
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
    /// The request is malformed (no setup, or both a name and text; an unknown name), a file
    /// does not load, the seed, a setting or a gear change is invalid, or the setup does not
    /// build.
    pub fn load(
        &mut self,
        request: LoadRequest,
        new_seed: impl FnOnce() -> u64,
    ) -> Result<Loaded, String> {
        let (mut setup, setup_name) = self.load_setup(&request)?;
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
        let seed = match request.seed.as_deref().map(str::trim) {
            None | Some("") => new_seed(),
            Some(seed) => seed.parse().map_err(|_| format!("invalid seed '{seed}'"))?,
        };
        let session = Session::new(Arc::clone(&self.data), setup, settings, seed, keybinds)?;
        self.set_session(
            session,
            Source {
                setup: setup_name,
                keybinds: keybinds_name,
            },
        );
        self.target = target;
        self.gear = gear;
        Ok(self.loaded().expect("a session was just loaded"))
    }

    fn load_setup(
        &self,
        request: &LoadRequest,
    ) -> Result<(CharacterSetup, Option<String>), String> {
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
                return Err("no setup: name one (`setup`) or paste one (`setup_yaml`)".into());
            }
            (Some(_), Some(_)) => return Err("both `setup` and `setup_yaml`: give one".into()),
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

/// A catalog name: a file name without extension, of letters, digits, `_` and `-`.
fn is_catalog_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests;
