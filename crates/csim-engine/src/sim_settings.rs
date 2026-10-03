//! Simulation settings. Port of `GUI/SimSettings.*` and `GUI/SimOption.h`.
//!
//! [`SimSettings`] holds what the user configures for a run: the content phase, the encounter
//! length, the iteration counts, the thread count, the execute threshold, the ruleset and the
//! stat-weight scaling options. The characters read the part they need as
//! [`SimParams`] ([`SimSettings::sim_params`]).
//!
//! The C++ `SimSettings` owned the `RulesetControl` and changed the character when the ruleset
//! changed; here the ruleset is plain data and the character applies it (see
//! [`crate::rulesets`]).

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::character::SimParams;
use crate::item::rating::interim_chance;
use crate::item::{ItemStat, WeaponType};
use crate::magic_school::MagicSchool;
use crate::phase::Phase;
use crate::rage_formula::RageFormula;
use crate::rulesets::Ruleset;
use crate::stats::CharacterStats;

/// A stat-weight scaling option: a run with a small amount of one stat added, compared with the
/// baseline run. Port of `SimOption::Name` (the C++ `NoScale` baseline is `None` where an
/// option is optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SimOption {
    ScaleAgility,
    ScaleStrength,
    ScaleHitChance,
    ScaleCritChance,
    ScaleAttackPower,
    ScaleAxeSkill,
    ScaleDaggerSkill,
    ScaleMaceSkill,
    ScaleSwordSkill,
    ScaleIntellect,
    ScaleSpirit,
    ScaleMp5,
    ScaleSpellDamage,
    ScaleSpellCritChance,
    ScaleSpellHitChance,
    ScaleSpellPenetration,
}

impl SimOption {
    pub const ALL: [SimOption; 16] = [
        SimOption::ScaleAgility,
        SimOption::ScaleStrength,
        SimOption::ScaleHitChance,
        SimOption::ScaleCritChance,
        SimOption::ScaleAttackPower,
        SimOption::ScaleAxeSkill,
        SimOption::ScaleDaggerSkill,
        SimOption::ScaleMaceSkill,
        SimOption::ScaleSwordSkill,
        SimOption::ScaleIntellect,
        SimOption::ScaleSpirit,
        SimOption::ScaleMp5,
        SimOption::ScaleSpellDamage,
        SimOption::ScaleSpellCritChance,
        SimOption::ScaleSpellHitChance,
        SimOption::ScaleSpellPenetration,
    ];

    /// What the option adds, for output.
    pub fn description(self) -> &'static str {
        match self {
            SimOption::ScaleAgility => "+10 Agility",
            SimOption::ScaleStrength => "+10 Strength",
            SimOption::ScaleHitChance => "+1% Hit",
            SimOption::ScaleCritChance => "+1% Critical Strike",
            SimOption::ScaleAttackPower => "+10 Attack Power",
            SimOption::ScaleAxeSkill => "+1 Axe Skill",
            SimOption::ScaleDaggerSkill => "+1 Dagger Skill",
            SimOption::ScaleMaceSkill => "+1 Mace Skill",
            SimOption::ScaleSwordSkill => "+1 Sword Skill",
            SimOption::ScaleIntellect => "+10 Intellect",
            SimOption::ScaleSpirit => "+10 Spirit",
            SimOption::ScaleMp5 => "+10 Mp5",
            SimOption::ScaleSpellDamage => "+10 Spell Damage",
            SimOption::ScaleSpellCritChance => "+1% Spell Critical Strike",
            SimOption::ScaleSpellHitChance => "+1% Spell Hit",
            SimOption::ScaleSpellPenetration => "+10 Spell Penetration",
        }
    }

    /// Adds the option's stat to `stats`. Port of `SimControl::add_option` (the statistics
    /// bookkeeping is the sim control's).
    pub fn add_to(self, stats: &mut CharacterStats) {
        match self {
            SimOption::ScaleAgility => stats.increase_agility(10),
            SimOption::ScaleStrength => stats.increase_strength(10),
            SimOption::ScaleAttackPower => {
                stats.increase_melee_ap(10);
                stats.increase_ranged_ap(10);
            }
            SimOption::ScaleHitChance => {
                stats.increase_melee_hit(100);
                stats.increase_ranged_hit(100);
            }
            SimOption::ScaleCritChance => {
                stats.increase_melee_aura_crit(100);
                stats.increase_ranged_crit(100);
            }
            SimOption::ScaleAxeSkill
            | SimOption::ScaleDaggerSkill
            | SimOption::ScaleMaceSkill
            | SimOption::ScaleSwordSkill => {
                stats.increase_wpn_skill(self.weapon_type().expect("skill option"), 1);
            }
            SimOption::ScaleIntellect => stats.increase_intellect(10),
            SimOption::ScaleSpirit => stats.increase_spirit(10),
            SimOption::ScaleMp5 => stats.increase_mp5(10),
            SimOption::ScaleSpellDamage => stats.increase_base_spell_damage(10),
            SimOption::ScaleSpellCritChance => stats.increase_spell_crit(100),
            SimOption::ScaleSpellHitChance => stats.increase_spell_hit(100),
            SimOption::ScaleSpellPenetration => {
                for school in MagicSchool::ALL
                    .into_iter()
                    .filter(|school| *school != MagicSchool::Physical)
                {
                    stats.increase_spell_penetration(school, 10);
                }
            }
        }
    }

    /// Removes what [`SimOption::add_to`] added. Port of `SimControl::remove_option`.
    pub fn remove_from(self, stats: &mut CharacterStats) {
        match self {
            SimOption::ScaleAgility => stats.decrease_agility(10),
            SimOption::ScaleStrength => stats.decrease_strength(10),
            SimOption::ScaleAttackPower => {
                stats.decrease_melee_ap(10);
                stats.decrease_ranged_ap(10);
            }
            SimOption::ScaleHitChance => {
                stats.decrease_melee_hit(100);
                stats.decrease_ranged_hit(100);
            }
            SimOption::ScaleCritChance => {
                stats.decrease_melee_aura_crit(100);
                stats.decrease_ranged_crit(100);
            }
            SimOption::ScaleAxeSkill
            | SimOption::ScaleDaggerSkill
            | SimOption::ScaleMaceSkill
            | SimOption::ScaleSwordSkill => {
                stats.decrease_wpn_skill(self.weapon_type().expect("skill option"), 1);
            }
            SimOption::ScaleIntellect => stats.decrease_intellect(10),
            SimOption::ScaleSpirit => stats.decrease_spirit(10),
            SimOption::ScaleMp5 => stats.decrease_mp5(10),
            SimOption::ScaleSpellDamage => stats.decrease_base_spell_damage(10),
            SimOption::ScaleSpellCritChance => stats.decrease_spell_crit(100),
            SimOption::ScaleSpellHitChance => stats.decrease_spell_hit(100),
            SimOption::ScaleSpellPenetration => {
                for school in MagicSchool::ALL
                    .into_iter()
                    .filter(|school| *school != MagicSchool::Physical)
                {
                    stats.decrease_spell_penetration(school, 10);
                }
            }
        }
    }

    /// The item stats that give the option's bonus, each with the amount (in data-file units)
    /// that equals the option: 10 `AGILITY` for "+10 Agility", 0.01 `HIT_CHANCE` or 10
    /// `HIT_RATING` for "+1% Hit". Dividing the option's stat weight by an amount gives the
    /// weight per point of that item stat.
    ///
    /// `RANGED_ATTACK_POWER` is not listed: the attack power option adds melee and ranged
    /// attack power together, so its weight cannot be split between them.
    pub fn item_stats(self) -> Vec<(ItemStat, f64)> {
        let rating = |rating: ItemStat| {
            let (_, per_percent) = interim_chance(rating).expect("a convertible rating");
            (rating, per_percent)
        };
        match self {
            SimOption::ScaleAgility => vec![(ItemStat::Agility, 10.0)],
            SimOption::ScaleStrength => vec![(ItemStat::Strength, 10.0)],
            SimOption::ScaleHitChance => {
                vec![(ItemStat::HitChance, 0.01), rating(ItemStat::HitRating)]
            }
            SimOption::ScaleCritChance => {
                vec![(ItemStat::CritChance, 0.01), rating(ItemStat::CritRating)]
            }
            SimOption::ScaleAttackPower => vec![
                (ItemStat::AttackPower, 10.0),
                (ItemStat::MeleeAttackPower, 10.0),
            ],
            SimOption::ScaleAxeSkill => vec![(ItemStat::AxeSkill, 1.0)],
            SimOption::ScaleDaggerSkill => vec![(ItemStat::DaggerSkill, 1.0)],
            SimOption::ScaleMaceSkill => vec![(ItemStat::MaceSkill, 1.0)],
            SimOption::ScaleSwordSkill => vec![(ItemStat::SwordSkill, 1.0)],
            SimOption::ScaleIntellect => vec![(ItemStat::Intellect, 10.0)],
            SimOption::ScaleSpirit => vec![(ItemStat::Spirit, 10.0)],
            SimOption::ScaleMp5 => vec![(ItemStat::ManaPer5, 10.0)],
            SimOption::ScaleSpellDamage => vec![(ItemStat::SpellDamage, 10.0)],
            SimOption::ScaleSpellCritChance => vec![(ItemStat::SpellCritChance, 0.01)],
            SimOption::ScaleSpellHitChance => vec![(ItemStat::SpellHitChance, 0.01)],
            SimOption::ScaleSpellPenetration => vec![(ItemStat::SpellPenetration, 10.0)],
        }
    }

    /// The weapon type a weapon skill option scales.
    pub fn weapon_type(self) -> Option<WeaponType> {
        match self {
            SimOption::ScaleAxeSkill => Some(WeaponType::Axe),
            SimOption::ScaleDaggerSkill => Some(WeaponType::Dagger),
            SimOption::ScaleMaceSkill => Some(WeaponType::Mace),
            SimOption::ScaleSwordSkill => Some(WeaponType::Sword),
            _ => None,
        }
    }
}

/// A setting is out of range.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SimSettingsError {
    #[error("combat length must be at least 1 second")]
    CombatLength,
    #[error("the number of iterations must be at least 1")]
    Iterations,
    #[error("the number of threads must be between 1 and {max}, got {threads}")]
    Threads { threads: usize, max: usize },
    #[error("the execute threshold must be between 0 and 1, got {0}")]
    ExecuteThreshold(f64),
    #[error("the length variance must be at least 0 and below 100 %, got {0}")]
    LengthVariance(f64),
}

/// The settings of a run. Port of `SimSettings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimSettings {
    /// The content phase gating items and enchants.
    pub phase: Phase,
    /// Encounter length in seconds.
    pub combat_length: u32,
    /// Variance of the encounter length in percent: each iteration lasts a uniformly
    /// distributed `combat_length × [1 - v/100, 1 + v/100]` seconds. 0 fixes it.
    pub length_variance: f64,
    /// Iterations of a quick sim (no scaling).
    pub iterations_quick_sim: u32,
    /// Iterations of a full sim, per scaling option.
    pub iterations_full_sim: u32,
    /// Worker threads (at most [`SimSettings::max_threads`]).
    pub threads: usize,
    /// Fraction of the encounter at its end that is the execute phase. A ruleset may impose
    /// its own ([`SimSettings::effective_execute_threshold`]).
    pub execute_threshold: f64,
    pub ruleset: Ruleset,
    /// The scaling options a full sim runs besides the baseline.
    pub options: BTreeSet<SimOption>,
    /// How landed white swings generate rage (a named setting, `rage_formula`; see
    /// [`crate::named_settings`]).
    pub rage_formula: RageFormula,
    /// Rage at the start of every iteration, for characters with rage (a named setting,
    /// `initial_rage`).
    pub initial_rage: u32,
}

impl Default for SimSettings {
    /// The C++ defaults: Naxxramas, 300 s, 1 000 quick / 10 000 full iterations, every
    /// available thread, execute below 20 %, the standard ruleset and no scaling. Unlike the
    /// C++, the encounter length varies by 10 %.
    fn default() -> Self {
        Self {
            phase: Phase::Naxxramas,
            combat_length: 300,
            length_variance: 10.0,
            iterations_quick_sim: 1000,
            iterations_full_sim: 10_000,
            threads: Self::max_threads(),
            execute_threshold: 0.2,
            ruleset: Ruleset::Standard,
            options: BTreeSet::new(),
            rage_formula: RageFormula::Forever,
            initial_rage: 0,
        }
    }
}

impl SimSettings {
    /// The threads the machine can run in parallel. `SimSettings::get_num_threads_max`.
    pub fn max_threads() -> usize {
        std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
    }

    /// Sets the thread count. Unlike the C++, which ignored an out-of-range count, this
    /// reports it.
    pub fn set_threads(&mut self, threads: usize) -> Result<(), SimSettingsError> {
        let max = Self::max_threads();
        if threads == 0 || threads > max {
            return Err(SimSettingsError::Threads { threads, max });
        }
        self.threads = threads;
        Ok(())
    }

    pub fn add_option(&mut self, option: SimOption) {
        self.options.insert(option);
    }

    pub fn remove_option(&mut self, option: SimOption) {
        self.options.remove(&option);
    }

    pub fn option_active(&self, option: SimOption) -> bool {
        self.options.contains(&option)
    }

    /// The execute threshold in effect: the ruleset's, else the configured one.
    pub fn effective_execute_threshold(&self) -> f64 {
        self.ruleset
            .execute_threshold()
            .unwrap_or(self.execute_threshold)
    }

    /// What the characters read.
    pub fn sim_params(&self) -> SimParams {
        SimParams {
            combat_length: f64::from(self.combat_length),
            execute_threshold: self.effective_execute_threshold(),
            ruleset: self.ruleset,
            rage_formula: self.rage_formula,
            initial_rage: self.initial_rage,
        }
    }

    /// Checks the ranges (a deserialized file is not checked otherwise). The thread count is
    /// only checked against 0: a file may be written on a machine with more threads.
    pub fn validate(&self) -> Result<(), SimSettingsError> {
        if self.combat_length == 0 {
            return Err(SimSettingsError::CombatLength);
        }
        if !(0.0..100.0).contains(&self.length_variance) {
            return Err(SimSettingsError::LengthVariance(self.length_variance));
        }
        if self.iterations_quick_sim == 0 || self.iterations_full_sim == 0 {
            return Err(SimSettingsError::Iterations);
        }
        if self.threads == 0 {
            return Err(SimSettingsError::Threads {
                threads: 0,
                max: Self::max_threads(),
            });
        }
        if !(0.0..=1.0).contains(&self.execute_threshold) {
            return Err(SimSettingsError::ExecuteThreshold(self.execute_threshold));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_cpp() {
        let settings = SimSettings::default();
        assert_eq!(settings.phase, Phase::Naxxramas);
        assert_eq!(settings.combat_length, 300);
        assert_eq!(settings.length_variance, 10.0);
        assert_eq!(settings.iterations_quick_sim, 1000);
        assert_eq!(settings.iterations_full_sim, 10_000);
        assert_eq!(settings.threads, SimSettings::max_threads());
        assert_eq!(settings.execute_threshold, 0.2);
        assert_eq!(settings.ruleset, Ruleset::Standard);
        assert!(settings.options.is_empty());
        assert_eq!(settings.validate(), Ok(()));
        assert_eq!(settings.sim_params(), SimParams::default());
    }

    #[test]
    fn thread_count_is_bounded() {
        let mut settings = SimSettings::default();
        assert_eq!(settings.set_threads(1), Ok(()));
        assert_eq!(settings.threads, 1);
        let max = SimSettings::max_threads();
        assert_eq!(
            settings.set_threads(0),
            Err(SimSettingsError::Threads { threads: 0, max })
        );
        assert_eq!(
            settings.set_threads(max + 1),
            Err(SimSettingsError::Threads {
                threads: max + 1,
                max
            })
        );
        assert_eq!(settings.threads, 1, "a rejected count changes nothing");
    }

    #[test]
    fn the_ruleset_overrides_the_execute_threshold() {
        let mut settings = SimSettings {
            execute_threshold: 0.3,
            ..SimSettings::default()
        };
        assert_eq!(settings.sim_params().execute_threshold, 0.3);

        settings.ruleset = Ruleset::Vaelastrasz;
        let params = settings.sim_params();
        assert_eq!(params.execute_threshold, 2.0 / 3.0);
        assert_eq!(params.ruleset, Ruleset::Vaelastrasz);

        settings.ruleset = Ruleset::Loatheb;
        assert_eq!(settings.sim_params().execute_threshold, 0.3);
    }

    #[test]
    fn options_are_a_set() {
        let mut settings = SimSettings::default();
        settings.add_option(SimOption::ScaleStrength);
        settings.add_option(SimOption::ScaleStrength);
        settings.add_option(SimOption::ScaleAgility);
        assert_eq!(settings.options.len(), 2);
        assert!(settings.option_active(SimOption::ScaleStrength));
        settings.remove_option(SimOption::ScaleStrength);
        settings.remove_option(SimOption::ScaleStrength);
        assert!(!settings.option_active(SimOption::ScaleStrength));
        assert!(settings.option_active(SimOption::ScaleAgility));
    }

    #[test]
    fn deserializes_with_defaults() {
        let settings: SimSettings = serde_yaml::from_str(
            "combat_length: 180\nphase: 3\nruleset: LOATHEB\noptions: [SCALE_HIT_CHANCE, SCALE_AGILITY]\n",
        )
        .unwrap();
        assert_eq!(settings.combat_length, 180);
        assert_eq!(settings.phase, Phase::BlackwingLair);
        assert_eq!(settings.ruleset, Ruleset::Loatheb);
        assert_eq!(
            settings.options.iter().copied().collect::<Vec<_>>(),
            [SimOption::ScaleAgility, SimOption::ScaleHitChance]
        );
        assert_eq!(settings.iterations_full_sim, 10_000);
        assert!(serde_yaml::from_str::<SimSettings>("combat_lenght: 180").is_err());
    }

    #[test]
    fn validation_rejects_out_of_range_settings() {
        let valid = SimSettings::default();
        let cases = [
            (
                SimSettings {
                    combat_length: 0,
                    ..valid.clone()
                },
                SimSettingsError::CombatLength,
            ),
            (
                SimSettings {
                    length_variance: -1.0,
                    ..valid.clone()
                },
                SimSettingsError::LengthVariance(-1.0),
            ),
            (
                SimSettings {
                    length_variance: 100.0,
                    ..valid.clone()
                },
                SimSettingsError::LengthVariance(100.0),
            ),
            (
                SimSettings {
                    iterations_full_sim: 0,
                    ..valid.clone()
                },
                SimSettingsError::Iterations,
            ),
            (
                SimSettings {
                    execute_threshold: 1.5,
                    ..valid.clone()
                },
                SimSettingsError::ExecuteThreshold(1.5),
            ),
        ];
        for (settings, error) in cases {
            assert_eq!(settings.validate(), Err(error));
        }
    }

    #[test]
    fn every_option_has_item_stat_equivalents() {
        use crate::stats::Stats;
        for option in SimOption::ALL {
            let stats: Vec<_> = option
                .item_stats()
                .into_iter()
                .map(|(stat, amount)| Stats::from_item_stats([(stat, amount)]).unwrap())
                .collect();
            assert!(!stats.is_empty(), "{option:?} has an item stat");
            for bag in &stats {
                assert_ne!(*bag, Stats::new(), "{option:?} item stats add something");
            }
        }
        // A rating amount converts to the same chance as the chance amount.
        for option in [SimOption::ScaleHitChance, SimOption::ScaleCritChance] {
            let [chance, rating] = &option.item_stats()[..] else {
                panic!("{option:?}: a chance and a rating");
            };
            assert_eq!(
                Stats::from_item_stats([*chance]).unwrap(),
                Stats::from_item_stats([*rating]).unwrap(),
                "{option:?}"
            );
        }
    }

    #[test]
    fn every_option_is_undone_by_its_removal() {
        for option in SimOption::ALL {
            let mut stats = CharacterStats::new();
            let before = format!("{:?}", stats);
            option.add_to(&mut stats);
            assert_ne!(format!("{:?}", stats), before, "{option:?} adds something");
            option.remove_from(&mut stats);
            assert_eq!(format!("{:?}", stats), before, "{option:?} is undone");
        }
    }
}
