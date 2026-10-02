//! Raid setup files: `data/raids/*.yaml` describe the parties of a raid by referring to the
//! character setups of `data/characters/`; [`RaidSetup::build_raid`] turns one, with the
//! player's own setup, into a [`RaidControl`] of up to [`PARTIES`] × [`PARTY_SIZE`] characters.
//! Replaces the C++ GUI's raid setup tab.
//!
//! ```yaml
//! name: Horde melee        # display name
//! player_party: 1          # the party the player joins (1-based, default 1)
//! parties:                 # at most 8 parties of at most 5, counting the player
//!   - [warrior_fury_2h_orc, warrior_fury_dw_orc]  # party 1: setups of data/characters/, `.yaml` optional
//!   - [warrior_fury_dw_orc]               # party 2
//! ```
//!
//! The player is added first, at the first place of its party, so it is the raid's first
//! character, whose statistics the sim reports; the members follow party by party in the order
//! listed. The player's setup decides the raid's target, phase and ruleset: the members'
//! `target`, `phase` and `ruleset` are ignored. Every member must be of the player's faction.
//! Without a player (validating a raid file) the first member takes its role.
//!
//! What the raid provides is up to the raid: every setup, the player's included, loses its
//! `debuffs` and the `buffs` that `data/external_buffs.yaml` marks `raid` (Blessing of Kings,
//! Trueshot Aura, ...). The consumables stay. The raid members' own spells (Battle Shout,
//! Sunder Armor) are what buffs the raid and debuffs the target.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::character_loader::{CharacterSetup, CharacterSetupError, SetupIssue};
use crate::data_bundle::DataBundle;
use crate::raid::{PARTIES, PARTY_SIZE, RaidControl};
use crate::sim_settings::SimSettings;

fn default_player_party() -> u8 {
    1
}

/// A `data/raids/*.yaml` file. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaidSetup {
    pub name: String,
    /// The party the player joins, 1-based.
    #[serde(default = "default_player_party")]
    pub player_party: u8,
    /// Per party, the character setups of its members by file name.
    #[serde(default)]
    pub parties: Vec<Vec<String>>,
    /// The file the setup was loaded from, for error messages.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// A raid member, its setup loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct RaidMember {
    /// 0-based.
    pub party: u8,
    /// The reference as the raid file writes it.
    pub reference: String,
    pub setup: CharacterSetup,
}

/// Why a raid setup could not be loaded or built.
#[derive(Debug, thiserror::Error)]
pub enum RaidSetupError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("raid setup {raid} is invalid:{}", issues.iter().map(|i| format!("\n  {i}")).collect::<String>())]
    Invalid {
        /// The file, or the raid name when it was not loaded from a file.
        raid: String,
        issues: Vec<SetupIssue>,
    },
}

/// Where a member is in the raid file, e.g. `parties.2[1] (warrior_fury_dw_orc)` (1-based party).
fn member_context(party: u8, index: usize, reference: &str) -> String {
    format!("parties.{}[{index}] ({reference})", party + 1)
}

impl RaidSetup {
    /// Parses a raid file. Loading the members is [`resolve`](Self::resolve)'s.
    pub fn load(path: &Path) -> Result<Self, RaidSetupError> {
        let text = fs::read_to_string(path).map_err(|source| RaidSetupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut setup: RaidSetup =
            serde_yaml::from_str(&text).map_err(|source| RaidSetupError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        setup.path = Some(path.to_path_buf());
        Ok(setup)
    }

    /// Parses every `*.yaml` raid of `dir`, sorted by file name.
    pub fn load_dir(dir: &Path) -> Result<Vec<Self>, RaidSetupError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|source| RaidSetupError::Io {
                path: dir.to_path_buf(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        paths.sort();
        paths.iter().map(|path| Self::load(path)).collect()
    }

    /// Loads the members' setups from `characters_dir` (`data/characters/`), in party order.
    /// Fails with every reference that could not be loaded.
    pub fn resolve(&self, characters_dir: &Path) -> Result<Vec<RaidMember>, RaidSetupError> {
        let mut members = Vec::new();
        let mut issues = Vec::new();
        for (party, references) in self.parties.iter().enumerate() {
            let party = u8::try_from(party).unwrap_or(u8::MAX);
            for (index, reference) in references.iter().enumerate() {
                let context = member_context(party, index, reference);
                match load_reference(characters_dir, reference) {
                    Ok(setup) => members.push(RaidMember {
                        party,
                        reference: reference.clone(),
                        setup,
                    }),
                    Err(message) => issues.push(SetupIssue { context, message }),
                }
            }
        }
        if issues.is_empty() {
            Ok(members)
        } else {
            Err(self.invalid(issues))
        }
    }

    /// Loads the members from `<data>/characters` and builds the raid without a player.
    pub fn validate(&self, data_dir: &Path, data: &DataBundle) -> Result<(), RaidSetupError> {
        let members = self.resolve(&data_dir.join("characters"))?;
        self.build_raid(None, &members, data, &SimSettings::default())
            .map(|_| ())
    }

    /// The raid of `player` and `members` (from [`resolve`](Self::resolve)), built from `data`
    /// under `settings` with the player's phase and ruleset. See the module documentation.
    /// Fails with every problem found.
    pub fn build_raid(
        &self,
        player: Option<&CharacterSetup>,
        members: &[RaidMember],
        data: &DataBundle,
        settings: &SimSettings,
    ) -> Result<RaidControl, RaidSetupError> {
        let mut issues = Vec::new();

        if self.parties.len() > usize::from(PARTIES) {
            push_issue(
                &mut issues,
                "parties",
                format!("{} parties, at most {PARTIES}", self.parties.len()),
            );
        }
        let player_party = self.player_party.wrapping_sub(1);
        if player.is_some() && player_party >= PARTIES {
            push_issue(
                &mut issues,
                "player_party",
                format!("{} is not in 1..={PARTIES}", self.player_party),
            );
        }
        for (party, references) in self.parties.iter().enumerate() {
            let with_player = player.is_some() && party == usize::from(player_party);
            let size = references.len() + usize::from(with_player);
            if size > usize::from(PARTY_SIZE) {
                push_issue(
                    &mut issues,
                    &format!("parties.{}", party + 1),
                    format!(
                        "{size} members{}, at most {PARTY_SIZE}",
                        if with_player {
                            " counting the player"
                        } else {
                            ""
                        }
                    ),
                );
            }
        }
        if player.is_none() && members.is_empty() {
            push_issue(
                &mut issues,
                "parties",
                "the raid has no members".to_string(),
            );
        }
        if !issues.is_empty() {
            return Err(self.invalid(issues));
        }

        // The player, else the first member, decides the target, the settings and the faction.
        let Some(lead) = player.or(members.first().map(|member| &member.setup)) else {
            unreachable!("checked above");
        };
        let settings = lead.sim_settings(settings);
        let faction = lead.race.faction();
        let mut raid = RaidControl::new(lead.target.target());

        if let Some(player) = player {
            let player = in_raid(player, data);
            if let Err(error) = player.add_to_raid_at(&mut raid, player_party, 0, data, &settings) {
                issues.extend(prefixed("player", error));
            }
        }
        let mut index_in_party = vec![0; self.parties.len()];
        for member in members {
            let index = &mut index_in_party[usize::from(member.party)];
            let context = member_context(member.party, *index, &member.reference);
            *index += 1;

            if member.setup.race.faction() != faction {
                push_issue(
                    &mut issues,
                    &context,
                    format!(
                        "{} is {}, the raid is {}",
                        member.setup.name,
                        member.setup.race.faction().name(),
                        faction.name()
                    ),
                );
                continue;
            }
            let Some(place) =
                (0..PARTY_SIZE).find(|&m| raid.character_at(member.party, m).is_none())
            else {
                unreachable!("the party sizes are checked above");
            };
            let setup = in_raid(&member.setup, data);
            if let Err(error) =
                setup.add_to_raid_at(&mut raid, member.party, place, data, &settings)
            {
                issues.extend(prefixed(&context, error));
            }
        }

        if issues.is_empty() {
            Ok(raid)
        } else {
            Err(self.invalid(issues))
        }
    }

    fn invalid(&self, issues: Vec<SetupIssue>) -> RaidSetupError {
        RaidSetupError::Invalid {
            raid: self
                .path
                .as_ref()
                .map_or_else(|| format!("{:?}", self.name), |p| p.display().to_string()),
            issues,
        }
    }
}

/// `setup` without what the raid provides: its debuffs and its buffs marked `raid`. Names that
/// are not external buffs stay, for the build to report. The phase and ruleset go too, the
/// raid's settings already hold the lead's.
fn in_raid(setup: &CharacterSetup, data: &DataBundle) -> CharacterSetup {
    let from_raid = |name: &String| {
        data.external_buffs
            .get(name)
            .is_some_and(|(spec, debuff)| spec.provided_by_raid(debuff))
    };
    CharacterSetup {
        phase: None,
        ruleset: None,
        buffs: setup
            .buffs
            .iter()
            .filter(|n| !from_raid(n))
            .cloned()
            .collect(),
        debuffs: Vec::new(),
        ..setup.clone()
    }
}

/// The setup of `characters_dir` a raid file refers to: a file name, `.yaml` optional.
fn load_reference(characters_dir: &Path, reference: &str) -> Result<CharacterSetup, String> {
    if reference.is_empty() || reference.contains(['/', '\\']) || reference.starts_with('.') {
        return Err("not a file name of data/characters/".to_string());
    }
    let has_extension = reference.ends_with(".yaml") || reference.ends_with(".yml");
    let candidates: Vec<PathBuf> = if has_extension {
        vec![characters_dir.join(reference)]
    } else {
        ["yaml", "yml"]
            .iter()
            .map(|ext| characters_dir.join(format!("{reference}.{ext}")))
            .collect()
    };
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return Err(format!("no character setup {}", candidates[0].display()));
    };
    CharacterSetup::load(path).map_err(|error| error.to_string())
}

fn push_issue(issues: &mut Vec<SetupIssue>, context: &str, message: String) {
    issues.push(SetupIssue {
        context: context.to_string(),
        message,
    });
}

/// A member's build problems, each under the member's context.
fn prefixed(context: &str, error: CharacterSetupError) -> Vec<SetupIssue> {
    match error {
        CharacterSetupError::Invalid { issues, .. } => issues
            .into_iter()
            .map(|issue| SetupIssue {
                context: format!("{context}: {}", issue.context),
                message: issue.message,
            })
            .collect(),
        other => vec![SetupIssue {
            context: context.to_string(),
            message: other.to_string(),
        }],
    }
}

#[cfg(test)]
mod tests;
