//! Playable races. Port of `Character/Race/*`.
//!
//! The C++ code had one subclass per race returning hardcoded base attributes and weapon skill
//! bonuses. Here [`Race`] is a plain enum keyed on the client `ChrRaces` id and everything else is
//! data: `data/races.yaml` ([`RaceSpec`]) holds the base attributes, and the racial abilities are
//! ordinary spells in `data/spells/racials.yaml`, selected by their `race_mask` ([`Race::mask`]).
//!
//! ClassicSim's `get_int_multiplier` / `get_spirit_multiplier` are not ported: the C++ never read
//! them, and in Forever the equivalent (The Human Spirit) is a racial aura.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::faction::Faction;
use crate::item::WeaponType;
use crate::spell::record::{SpellDb, SpellRecord};
use crate::stats::RaceStats;

/// A playable race. The discriminant is the `ChrRaces` id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(u32)]
pub enum Race {
    Human = 1,
    Orc = 2,
    Dwarf = 3,
    NightElf = 4,
    Undead = 5,
    Tauren = 6,
    Gnome = 7,
    Troll = 8,
}

impl Race {
    pub const ALL: [Race; 8] = [
        Race::Human,
        Race::Orc,
        Race::Dwarf,
        Race::NightElf,
        Race::Undead,
        Race::Tauren,
        Race::Gnome,
        Race::Troll,
    ];

    /// The `ChrRaces` id.
    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn from_id(id: u32) -> Option<Race> {
        Race::ALL.iter().copied().find(|race| race.id() == id)
    }

    /// The race's bit in `SkillLineAbility.RaceMasks` (`ChrRaces.PlayableRaceBit` = id − 1).
    pub fn mask(self) -> u32 {
        1 << (self.id() - 1)
    }

    /// Whether a race mask (`0` = unrestricted) includes this race.
    pub fn in_mask(self, mask: u32) -> bool {
        mask == 0 || mask & self.mask() != 0
    }

    /// Display name (`ChrRaces.Name_lang`).
    pub fn name(self) -> &'static str {
        match self {
            Race::Human => "Human",
            Race::Orc => "Orc",
            Race::Dwarf => "Dwarf",
            Race::NightElf => "Night Elf",
            Race::Undead => "Undead",
            Race::Tauren => "Tauren",
            Race::Gnome => "Gnome",
            Race::Troll => "Troll",
        }
    }

    pub fn from_name(name: &str) -> Option<Race> {
        Race::ALL.iter().copied().find(|race| race.name() == name)
    }

    pub fn faction(self) -> Faction {
        match self {
            Race::Human | Race::Dwarf | Race::NightElf | Race::Gnome => Faction::Alliance,
            Race::Orc | Race::Undead | Race::Tauren | Race::Troll => Faction::Horde,
        }
    }

    /// The racial spells of this race: every record whose `race_mask` names it, sorted by id.
    pub fn racials(self, db: &SpellDb) -> Vec<&Arc<SpellRecord>> {
        let mut records: Vec<_> = db
            .records()
            .into_iter()
            .filter(|record| record.race_mask != 0 && self.in_mask(record.race_mask))
            .collect();
        records.sort_by_key(|record| record.id);
        records
    }
}

/// Base attributes of a race.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseStats {
    pub strength: u32,
    pub agility: u32,
    pub stamina: u32,
    pub intellect: u32,
    pub spirit: u32,
}

/// One entry of `data/races.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceSpec {
    pub race: Race,
    /// `ChrRaces.ID`; must equal `race.id()`.
    pub id: u32,
    /// Must equal `race.faction()`.
    pub faction: Faction,
    pub base_stats: BaseStats,
    /// Racial weapon skill bonuses keyed on the one-hand weapon type (`SWORD: 5` also covers
    /// two-hand swords). Empty in Forever, where the specialization racials grant crit instead.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weapon_skill_bonuses: BTreeMap<WeaponType, u32>,
}

impl RaceSpec {
    /// The race's contribution to [`crate::stats::CharacterStats`].
    pub fn race_stats(&self) -> RaceStats {
        let bonus = |weapon_type: WeaponType| {
            self.weapon_skill_bonuses
                .get(&weapon_type)
                .copied()
                .unwrap_or(0)
        };
        RaceStats {
            strength: self.base_stats.strength,
            agility: self.base_stats.agility,
            stamina: self.base_stats.stamina,
            intellect: self.base_stats.intellect,
            spirit: self.base_stats.spirit,
            axe_skill_bonus: bonus(WeaponType::Axe),
            sword_skill_bonus: bonus(WeaponType::Sword),
            mace_skill_bonus: bonus(WeaponType::Mace),
            bow_skill_bonus: bonus(WeaponType::Bow),
            gun_skill_bonus: bonus(WeaponType::Gun),
            thrown_skill_bonus: bonus(WeaponType::Thrown),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RaceDbError {
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
    #[error("race {0:?} is defined twice")]
    Duplicate(Race),
    #[error("race {0:?} is missing")]
    Missing(Race),
    #[error("race {race:?}: id {id} does not match ChrRaces id {expected}")]
    IdMismatch { race: Race, id: u32, expected: u32 },
    #[error("race {race:?}: faction {faction:?} does not match {expected:?}")]
    FactionMismatch {
        race: Race,
        faction: Faction,
        expected: Faction,
    },
    #[error("race {race:?}: weapon skill bonus for {weapon_type:?} must use the one-hand type")]
    WeaponSkillType { race: Race, weapon_type: WeaponType },
}

/// The race definitions of `data/races.yaml`. Every [`Race`] is defined exactly once.
#[derive(Debug, Clone)]
pub struct RaceDb {
    specs: Vec<RaceSpec>,
    by_race: HashMap<Race, usize>,
}

impl RaceDb {
    pub fn new(specs: Vec<RaceSpec>) -> Result<Self, RaceDbError> {
        let mut by_race = HashMap::new();
        for (index, spec) in specs.iter().enumerate() {
            spec.validate()?;
            if by_race.insert(spec.race, index).is_some() {
                return Err(RaceDbError::Duplicate(spec.race));
            }
        }
        if let Some(missing) = Race::ALL.iter().find(|race| !by_race.contains_key(race)) {
            return Err(RaceDbError::Missing(*missing));
        }
        Ok(Self { specs, by_race })
    }

    /// Loads a YAML file holding a list of race specs.
    pub fn load(path: &Path) -> Result<Self, RaceDbError> {
        let text = fs::read_to_string(path).map_err(|source| RaceDbError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let specs: Vec<RaceSpec> =
            serde_yaml::from_str(&text).map_err(|source| RaceDbError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        Self::new(specs)
    }

    pub fn get(&self, race: Race) -> &RaceSpec {
        &self.specs[self.by_race[&race]]
    }

    pub fn race_stats(&self, race: Race) -> RaceStats {
        self.get(race).race_stats()
    }

    pub fn specs(&self) -> &[RaceSpec] {
        &self.specs
    }
}

impl RaceSpec {
    fn validate(&self) -> Result<(), RaceDbError> {
        if self.id != self.race.id() {
            return Err(RaceDbError::IdMismatch {
                race: self.race,
                id: self.id,
                expected: self.race.id(),
            });
        }
        if self.faction != self.race.faction() {
            return Err(RaceDbError::FactionMismatch {
                race: self.race,
                faction: self.faction,
                expected: self.race.faction(),
            });
        }
        if let Some(weapon_type) = self
            .weapon_skill_bonuses
            .keys()
            .copied()
            .find(|weapon_type| {
                !matches!(
                    weapon_type,
                    WeaponType::Axe
                        | WeaponType::Sword
                        | WeaponType::Mace
                        | WeaponType::Bow
                        | WeaponType::Gun
                        | WeaponType::Thrown
                )
            })
        {
            return Err(RaceDbError::WeaponSkillType {
                race: self.race,
                weapon_type,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(race: Race) -> RaceSpec {
        RaceSpec {
            race,
            id: race.id(),
            faction: race.faction(),
            base_stats: BaseStats::default(),
            weapon_skill_bonuses: BTreeMap::new(),
        }
    }

    fn all_specs() -> Vec<RaceSpec> {
        Race::ALL.iter().copied().map(spec).collect()
    }

    #[test]
    fn ids_masks_and_names_round_trip() {
        for race in Race::ALL {
            assert_eq!(Race::from_id(race.id()), Some(race));
            assert_eq!(Race::from_name(race.name()), Some(race));
            assert_eq!(race.mask(), 1 << (race.id() - 1));
            assert!(race.in_mask(race.mask()));
            assert!(race.in_mask(0), "an empty mask is unrestricted");
        }
        assert_eq!(Race::from_id(0), None);
        assert_eq!(Race::from_id(9), None);
        assert_eq!(Race::Human.mask(), 1);
        assert_eq!(Race::Orc.mask(), 2);
        assert_eq!(Race::Troll.mask(), 128);
        assert!(!Race::Human.in_mask(Race::Orc.mask()));
        assert!(Race::Undead.in_mask(Race::Orc.mask() | Race::Undead.mask()));
    }

    #[test]
    fn serde_names() {
        assert_eq!(
            serde_yaml::from_str::<Race>("NIGHT_ELF").unwrap(),
            Race::NightElf
        );
        assert_eq!(
            serde_yaml::to_string(&Race::Undead).unwrap().trim(),
            "UNDEAD"
        );
    }

    #[test]
    fn factions() {
        for race in [Race::Human, Race::Dwarf, Race::NightElf, Race::Gnome] {
            assert_eq!(race.faction(), Faction::Alliance);
        }
        for race in [Race::Orc, Race::Undead, Race::Tauren, Race::Troll] {
            assert_eq!(race.faction(), Faction::Horde);
        }
    }

    #[test]
    fn race_stats_from_spec() {
        let yaml = "
race: HUMAN
id: 1
faction: ALLIANCE
base_stats: { strength: 20, agility: 21, stamina: 22, intellect: 23, spirit: 24 }
weapon_skill_bonuses: { SWORD: 5, MACE: 3 }
";
        let spec: RaceSpec = serde_yaml::from_str(yaml).unwrap();
        let stats = spec.race_stats();
        assert_eq!(
            stats,
            RaceStats {
                strength: 20,
                agility: 21,
                stamina: 22,
                intellect: 23,
                spirit: 24,
                sword_skill_bonus: 5,
                mace_skill_bonus: 3,
                ..RaceStats::default()
            }
        );
        assert_eq!(stats.weapon_skill_bonus(WeaponType::TwohandSword), 5);
        assert_eq!(stats.weapon_skill_bonus(WeaponType::Axe), 0);
    }

    #[test]
    fn db_validation() {
        assert!(RaceDb::new(all_specs()).is_ok());

        let mut specs = all_specs();
        specs.push(spec(Race::Orc));
        assert!(matches!(
            RaceDb::new(specs),
            Err(RaceDbError::Duplicate(Race::Orc))
        ));

        let mut specs = all_specs();
        specs.retain(|s| s.race != Race::Gnome);
        assert!(matches!(
            RaceDb::new(specs),
            Err(RaceDbError::Missing(Race::Gnome))
        ));

        let mut specs = all_specs();
        specs[0].id = 4;
        assert!(matches!(
            RaceDb::new(specs),
            Err(RaceDbError::IdMismatch {
                race: Race::Human,
                id: 4,
                expected: 1
            })
        ));

        let mut specs = all_specs();
        specs[1].faction = Faction::Alliance;
        assert!(matches!(
            RaceDb::new(specs),
            Err(RaceDbError::FactionMismatch {
                race: Race::Orc,
                ..
            })
        ));

        let mut specs = all_specs();
        specs[2]
            .weapon_skill_bonuses
            .insert(WeaponType::TwohandAxe, 5);
        assert!(matches!(
            RaceDb::new(specs),
            Err(RaceDbError::WeaponSkillType {
                race: Race::Dwarf,
                weapon_type: WeaponType::TwohandAxe
            })
        ));
    }

    #[test]
    fn shipped_data_file_defines_every_race() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/races.yaml");
        let db = RaceDb::load(&path).unwrap();
        assert_eq!(db.specs().len(), Race::ALL.len());
        let orc = db.race_stats(Race::Orc);
        assert_eq!(orc.strength, 23);
        assert_eq!(orc.spirit, 23);
        assert_eq!(
            db.race_stats(Race::Tauren).strength,
            25,
            "Tauren have the highest base strength"
        );
        for race in Race::ALL {
            let stats = db.race_stats(race);
            assert!(stats.strength > 0 && stats.spirit > 0, "{race:?}");
            for weapon_type in WeaponType::ALL {
                assert_eq!(
                    stats.weapon_skill_bonus(weapon_type),
                    0,
                    "Forever grants crit, not skill, through the specialization racials"
                );
            }
        }
    }

    #[test]
    fn shipped_racials_resolve_per_race() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/spells");
        let db = SpellDb::load(&dir).unwrap();
        let ids = |race: Race| -> Vec<u32> { race.racials(&db).iter().map(|r| r.id).collect() };

        let orc = ids(Race::Orc);
        assert!(orc.contains(&20572), "Blood Fury: {orc:?}");
        assert!(orc.contains(&20574), "Axe Specialization: {orc:?}");
        let troll = ids(Race::Troll);
        assert!(troll.contains(&20554), "Berserking: {troll:?}");
        assert!(troll.contains(&20557), "Beast Slaying: {troll:?}");
        let human = ids(Race::Human);
        assert!(human.contains(&20597), "Sword Specialization: {human:?}");
        assert!(human.contains(&20598), "The Human Spirit: {human:?}");
        assert!(!human.contains(&20572), "Blood Fury is Orc-only");

        for race in Race::ALL {
            for record in race.racials(&db) {
                assert!(race.in_mask(record.race_mask));
            }
            assert!(!ids(race).is_empty(), "{race:?} has racials");
        }
    }
}
