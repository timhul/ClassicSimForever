//! Per-spell statistics: attempts by outcome, damage and threat per outcome, damage / threat
//! per resource and per execution time. Port of `Statistics/StatisticsSpell.*`.
//!
//! Deviations from the C++:
//! - `add` merged the max damage / threat with `<` (keeping the smaller); here the larger wins.
//! - The C++ averaged damage per resource and per execution time over every success, also the
//!   successes it skipped for a zero cost / time. Each [`Ratio`] here counts its own samples.
//! - Threat is tallied in whole points like the C++ `int thrt` parameter (truncated).

use crate::combat_roll::{MagicAttackResult, MagicResistResult, PhysicalAttackResult};
use crate::spell::AttackOutcome;

/// How one attempt of a spell ended. Port of `StatisticsSpell::Outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Outcome {
    Miss,
    FullResist,
    Dodge,
    Parry,
    FullBlock,
    PartialResist25,
    PartialResist50,
    PartialResist75,
    PartialResistCrit25,
    PartialResistCrit50,
    PartialResistCrit75,
    PartialBlock,
    PartialBlockCrit,
    Glancing,
    Hit,
    Crit,
}

impl Outcome {
    /// Every outcome, in declaration order.
    pub const ALL: [Outcome; 16] = [
        Outcome::Miss,
        Outcome::FullResist,
        Outcome::Dodge,
        Outcome::Parry,
        Outcome::FullBlock,
        Outcome::PartialResist25,
        Outcome::PartialResist50,
        Outcome::PartialResist75,
        Outcome::PartialResistCrit25,
        Outcome::PartialResistCrit50,
        Outcome::PartialResistCrit75,
        Outcome::PartialBlock,
        Outcome::PartialBlockCrit,
        Outcome::Glancing,
        Outcome::Hit,
        Outcome::Crit,
    ];

    /// The outcomes that deal damage. Port of `possible_success_outcomes`.
    pub const SUCCESSES: [Outcome; 11] = [
        Outcome::PartialResist25,
        Outcome::PartialResist50,
        Outcome::PartialResist75,
        Outcome::PartialResistCrit25,
        Outcome::PartialResistCrit50,
        Outcome::PartialResistCrit75,
        Outcome::PartialBlock,
        Outcome::PartialBlockCrit,
        Outcome::Glancing,
        Outcome::Hit,
        Outcome::Crit,
    ];

    /// Whether the attempt connected.
    pub fn is_success(self) -> bool {
        !matches!(
            self,
            Outcome::Miss
                | Outcome::FullResist
                | Outcome::Dodge
                | Outcome::Parry
                | Outcome::FullBlock
        )
    }

    fn index(self) -> usize {
        self as usize
    }

    /// The outcome of a physical attack that dealt `damage`. A blocked attack whose damage the
    /// block value absorbed entirely is a full block.
    pub fn from_physical(result: PhysicalAttackResult, damage: u32) -> Outcome {
        match result {
            PhysicalAttackResult::Miss => Outcome::Miss,
            PhysicalAttackResult::Dodge => Outcome::Dodge,
            PhysicalAttackResult::Parry => Outcome::Parry,
            PhysicalAttackResult::Glancing => Outcome::Glancing,
            PhysicalAttackResult::Hit => Outcome::Hit,
            PhysicalAttackResult::Critical => Outcome::Crit,
            PhysicalAttackResult::Block if damage == 0 => Outcome::FullBlock,
            PhysicalAttackResult::Block => Outcome::PartialBlock,
            PhysicalAttackResult::BlockCritical if damage == 0 => Outcome::FullBlock,
            PhysicalAttackResult::BlockCritical => Outcome::PartialBlockCrit,
        }
    }

    /// The outcome of a magic attack after its resist roll. Port of the `resist_result`
    /// switches of `add_spell_hit_dmg` / `add_spell_crit_dmg`.
    pub fn from_magic(result: MagicAttackResult, resist: MagicResistResult) -> Outcome {
        match (result, resist) {
            (MagicAttackResult::Miss, _) => Outcome::Miss,
            (_, MagicResistResult::FullResist) => Outcome::FullResist,
            (MagicAttackResult::Hit, MagicResistResult::NoResist) => Outcome::Hit,
            (MagicAttackResult::Hit, MagicResistResult::Partial25) => Outcome::PartialResist25,
            (MagicAttackResult::Hit, MagicResistResult::Partial50) => Outcome::PartialResist50,
            (MagicAttackResult::Hit, MagicResistResult::Partial75) => Outcome::PartialResist75,
            (MagicAttackResult::Critical, MagicResistResult::NoResist) => Outcome::Crit,
            (MagicAttackResult::Critical, MagicResistResult::Partial25) => {
                Outcome::PartialResistCrit25
            }
            (MagicAttackResult::Critical, MagicResistResult::Partial50) => {
                Outcome::PartialResistCrit50
            }
            (MagicAttackResult::Critical, MagicResistResult::Partial75) => {
                Outcome::PartialResistCrit75
            }
        }
    }

    /// Display name, as used by the C++ damage breakdown columns.
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Miss => "Miss",
            Outcome::FullResist => "Full resist",
            Outcome::Dodge => "Dodge",
            Outcome::Parry => "Parry",
            Outcome::FullBlock => "Full block",
            Outcome::PartialResist25 => "Partial resist (25%)",
            Outcome::PartialResist50 => "Partial resist (50%)",
            Outcome::PartialResist75 => "Partial resist (75%)",
            Outcome::PartialResistCrit25 => "Partial resist crit (25%)",
            Outcome::PartialResistCrit50 => "Partial resist crit (50%)",
            Outcome::PartialResistCrit75 => "Partial resist crit (75%)",
            Outcome::PartialBlock => "Partial block",
            Outcome::PartialBlockCrit => "Partial block crit",
            Outcome::Glancing => "Glancing",
            Outcome::Hit => "Hit",
            Outcome::Crit => "Crit",
        }
    }
}

/// Total, min and max of the damage (or threat) dealt with one outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    total: u64,
    min: Option<u32>,
    max: Option<u32>,
    count: u64,
}

impl Tally {
    fn add(&mut self, value: u32) {
        self.total += u64::from(value);
        self.count += 1;
        self.min = Some(self.min.map_or(value, |min| min.min(value)));
        self.max = Some(self.max.map_or(value, |max| max.max(value)));
    }

    fn merge(&mut self, other: &Tally) {
        self.total += other.total;
        self.count += other.count;
        self.min = match (self.min, other.min) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.max = match (self.max, other.max) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// The smallest value recorded, 0 without any.
    pub fn min(&self) -> u32 {
        self.min.unwrap_or(0)
    }

    /// The largest value recorded, 0 without any.
    pub fn max(&self) -> u32 {
        self.max.unwrap_or(0)
    }

    /// The mean value, 0 without any.
    pub fn avg(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total as f64 / self.count as f64
        }
    }
}

/// Running min / max / mean of a per-success ratio: damage or threat per resource point, or
/// per second of execution time.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ratio {
    min: f64,
    max: f64,
    avg: f64,
    count: u64,
}

impl Ratio {
    fn add(&mut self, value: f64) {
        self.count += 1;
        if self.count == 1 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.avg += (value - self.avg) / self.count as f64;
    }

    fn merge(&mut self, other: &Ratio) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = *other;
            return;
        }
        let total = (self.count + other.count) as f64;
        self.avg =
            self.avg * (self.count as f64 / total) + other.avg * (other.count as f64 / total);
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.count += other.count;
    }

    /// Whether any sample was recorded (`dpr_set` etc. in the C++).
    pub fn is_set(&self) -> bool {
        self.count > 0
    }

    pub fn min(&self) -> f64 {
        self.min
    }

    pub fn max(&self) -> f64 {
        self.max
    }

    pub fn avg(&self) -> f64 {
        self.avg
    }

    pub fn samples(&self) -> u64 {
        self.count
    }
}

/// An execution time below this counts as none (the `delta(execution_time, 0) < 0.0001` of
/// `add_dpet`).
const EXECUTION_TIME_EPSILON: f64 = 0.0001;

/// Statistics of one spell (or auto attack, or proc) of a character. Port of `StatisticsSpell`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellStatistics {
    name: String,
    rank: u32,
    attempts: [u64; Outcome::ALL.len()],
    damage: [Tally; Outcome::ALL.len()],
    threat: [Tally; Outcome::ALL.len()],
    dpr: Ratio,
    dpet: Ratio,
    tpr: Ratio,
    tpet: Ratio,
}

impl SpellStatistics {
    pub fn new(name: impl Into<String>, rank: u32) -> Self {
        SpellStatistics {
            name: name.into(),
            rank,
            attempts: [0; Outcome::ALL.len()],
            damage: [Tally::default(); Outcome::ALL.len()],
            threat: [Tally::default(); Outcome::ALL.len()],
            dpr: Ratio::default(),
            dpet: Ratio::default(),
            tpr: Ratio::default(),
            tpet: Ratio::default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rank(&self) -> u32 {
        self.rank
    }

    /// The name with the rank appended for ranks above one, as the C++ recorded it.
    pub fn display_name(&self) -> String {
        if self.rank > 1 {
            format!("{} (rank {})", self.name, self.rank)
        } else {
            self.name.clone()
        }
    }

    /// Clears every counter.
    pub fn reset(&mut self) {
        *self = SpellStatistics::new(std::mem::take(&mut self.name), self.rank);
    }

    // --- Recording ---

    /// Counts an attempt with `outcome` and nothing dealt (an avoided attack). Port of the
    /// `increment_*` methods.
    pub fn increment(&mut self, outcome: Outcome) {
        self.attempts[outcome.index()] += 1;
    }

    /// Counts a connecting attempt with its damage and threat. `resource_cost` and
    /// `execution_time` feed the per-resource and per-execution-time ratios (skipped when
    /// zero). Port of the `add_*_dmg` + `add_*_thrt` pairs.
    pub fn add_success(
        &mut self,
        outcome: Outcome,
        damage: u32,
        threat: u32,
        resource_cost: f64,
        execution_time: f64,
    ) {
        self.attempts[outcome.index()] += 1;
        self.damage[outcome.index()].add(damage);
        self.threat[outcome.index()].add(threat);
        if resource_cost != 0.0 {
            self.dpr.add(f64::from(damage) / resource_cost);
            self.tpr.add(f64::from(threat) / resource_cost);
        }
        if execution_time.abs() >= EXECUTION_TIME_EPSILON {
            self.dpet.add(f64::from(damage) / execution_time);
            self.tpet.add(f64::from(threat) / execution_time);
        }
    }

    /// Records the attack outcome of one cast or swing (`resource_cost` in displayed units).
    pub fn record_attack(&mut self, attack: &AttackOutcome, resource_cost: f64) {
        let outcome = match attack.spell {
            Some(spell) => Outcome::from_magic(spell.roll.result, spell.roll.resist),
            None => Outcome::from_physical(attack.result, attack.damage),
        };
        if outcome.is_success() {
            self.add_success(
                outcome,
                attack.damage,
                attack.threat.max(0.0) as u32,
                resource_cost,
                attack.execution_time,
            );
        } else {
            self.increment(outcome);
        }
    }

    /// Records a periodic tick: a hit (or a partial resist) for its damage and threat. Port of
    /// the `add_hit_dmg` calls of the C++ periodic spells.
    pub fn record_tick(
        &mut self,
        damage: u32,
        threat: f64,
        resource_cost: f64,
        execution_time: f64,
        resist: MagicResistResult,
    ) {
        self.add_success(
            Outcome::from_magic(MagicAttackResult::Hit, resist),
            damage,
            threat.max(0.0) as u32,
            resource_cost,
            execution_time,
        );
    }

    /// Merges `other` into `self` (the same spell from another thread).
    pub fn add(&mut self, other: &SpellStatistics) {
        for outcome in Outcome::ALL {
            let i = outcome.index();
            self.attempts[i] += other.attempts[i];
            self.damage[i].merge(&other.damage[i]);
            self.threat[i].merge(&other.threat[i]);
        }
        self.dpr.merge(&other.dpr);
        self.dpet.merge(&other.dpet);
        self.tpr.merge(&other.tpr);
        self.tpet.merge(&other.tpet);
    }

    // --- Attempts ---

    pub fn attempts(&self, outcome: Outcome) -> u64 {
        self.attempts[outcome.index()]
    }

    pub fn total_attempts(&self) -> u64 {
        self.attempts.iter().sum()
    }

    pub fn misses(&self) -> u64 {
        self.attempts(Outcome::Miss)
    }

    pub fn full_resists(&self) -> u64 {
        self.attempts(Outcome::FullResist)
    }

    pub fn dodges(&self) -> u64 {
        self.attempts(Outcome::Dodge)
    }

    pub fn parries(&self) -> u64 {
        self.attempts(Outcome::Parry)
    }

    pub fn full_blocks(&self) -> u64 {
        self.attempts(Outcome::FullBlock)
    }

    pub fn partial_blocks(&self) -> u64 {
        self.attempts(Outcome::PartialBlock)
    }

    pub fn partial_block_crits(&self) -> u64 {
        self.attempts(Outcome::PartialBlockCrit)
    }

    /// Hits and crits partially resisted for 25 %.
    pub fn partial_resists_25(&self) -> u64 {
        self.attempts(Outcome::PartialResist25) + self.attempts(Outcome::PartialResistCrit25)
    }

    pub fn partial_resists_50(&self) -> u64 {
        self.attempts(Outcome::PartialResist50) + self.attempts(Outcome::PartialResistCrit50)
    }

    pub fn partial_resists_75(&self) -> u64 {
        self.attempts(Outcome::PartialResist75) + self.attempts(Outcome::PartialResistCrit75)
    }

    pub fn glances(&self) -> u64 {
        self.attempts(Outcome::Glancing)
    }

    pub fn hits(&self) -> u64 {
        self.attempts(Outcome::Hit)
    }

    pub fn hits_including_partial_resists(&self) -> u64 {
        self.hits()
            + self.attempts(Outcome::PartialResist25)
            + self.attempts(Outcome::PartialResist50)
            + self.attempts(Outcome::PartialResist75)
    }

    pub fn crits(&self) -> u64 {
        self.attempts(Outcome::Crit)
    }

    pub fn crits_including_partial_resists(&self) -> u64 {
        self.crits()
            + self.attempts(Outcome::PartialResistCrit25)
            + self.attempts(Outcome::PartialResistCrit50)
            + self.attempts(Outcome::PartialResistCrit75)
    }

    /// Share of the attempts that ended with `outcome`, 0 without attempts.
    pub fn outcome_rate(&self, outcome: Outcome) -> f64 {
        let total = self.total_attempts();
        if total == 0 {
            0.0
        } else {
            self.attempts(outcome) as f64 / total as f64
        }
    }

    // --- Damage and threat ---

    /// Total, min, max and mean damage dealt with `outcome`.
    pub fn damage(&self, outcome: Outcome) -> &Tally {
        &self.damage[outcome.index()]
    }

    /// Total, min, max and mean threat dealt with `outcome`.
    pub fn threat(&self, outcome: Outcome) -> &Tally {
        &self.threat[outcome.index()]
    }

    pub fn total_damage(&self) -> u64 {
        Outcome::SUCCESSES
            .iter()
            .map(|o| self.damage[o.index()].total)
            .sum()
    }

    pub fn total_threat(&self) -> u64 {
        Outcome::SUCCESSES
            .iter()
            .map(|o| self.threat[o.index()].total)
            .sum()
    }

    /// This spell's share of `total_damage` (the character's total), 0 for a zero total. Port
    /// of `set_percentage_of_damage_dealt` / `get_percentage_of_damage_dealt`.
    pub fn damage_share(&self, total_damage: u64) -> f64 {
        if total_damage == 0 {
            0.0
        } else {
            self.total_damage() as f64 / total_damage as f64
        }
    }

    pub fn threat_share(&self, total_threat: u64) -> f64 {
        if total_threat == 0 {
            0.0
        } else {
            self.total_threat() as f64 / total_threat as f64
        }
    }

    /// Damage per resource point.
    pub fn dpr(&self) -> &Ratio {
        &self.dpr
    }

    /// Damage per second of execution time.
    pub fn dpet(&self) -> &Ratio {
        &self.dpet
    }

    /// Threat per resource point.
    pub fn tpr(&self) -> &Ratio {
        &self.tpr
    }

    /// Threat per second of execution time.
    pub fn tpet(&self) -> &Ratio {
        &self.tpet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(damage: u32) -> AttackOutcome {
        AttackOutcome {
            result: PhysicalAttackResult::Hit,
            spell: None,
            damage,
            threat: f64::from(damage) * 1.5,
            execution_time: 1.5,
        }
    }

    #[test]
    fn outcome_from_physical_distinguishes_full_and_partial_blocks() {
        assert_eq!(
            Outcome::from_physical(PhysicalAttackResult::Block, 0),
            Outcome::FullBlock
        );
        assert_eq!(
            Outcome::from_physical(PhysicalAttackResult::Block, 10),
            Outcome::PartialBlock
        );
        assert_eq!(
            Outcome::from_physical(PhysicalAttackResult::BlockCritical, 0),
            Outcome::FullBlock
        );
        assert_eq!(
            Outcome::from_physical(PhysicalAttackResult::BlockCritical, 10),
            Outcome::PartialBlockCrit
        );
        assert_eq!(
            Outcome::from_physical(PhysicalAttackResult::Critical, 10),
            Outcome::Crit
        );
        assert_eq!(
            Outcome::from_physical(PhysicalAttackResult::Hit, 0),
            Outcome::Hit
        );
    }

    #[test]
    fn outcome_from_magic_maps_resists() {
        assert_eq!(
            Outcome::from_magic(MagicAttackResult::Miss, MagicResistResult::Partial50),
            Outcome::Miss
        );
        assert_eq!(
            Outcome::from_magic(MagicAttackResult::Hit, MagicResistResult::FullResist),
            Outcome::FullResist
        );
        assert_eq!(
            Outcome::from_magic(MagicAttackResult::Hit, MagicResistResult::Partial25),
            Outcome::PartialResist25
        );
        assert_eq!(
            Outcome::from_magic(MagicAttackResult::Critical, MagicResistResult::Partial75),
            Outcome::PartialResistCrit75
        );
        assert_eq!(
            Outcome::from_magic(MagicAttackResult::Critical, MagicResistResult::NoResist),
            Outcome::Crit
        );
    }

    #[test]
    fn records_attempts_damage_and_threat_by_outcome() {
        let mut stats = SpellStatistics::new("Bloodthirst", 4);
        stats.record_attack(&hit(100), 30.0);
        stats.record_attack(&hit(300), 30.0);
        stats.record_attack(
            &AttackOutcome {
                result: PhysicalAttackResult::Critical,
                spell: None,
                damage: 500,
                threat: 750.0,
                execution_time: 1.5,
            },
            30.0,
        );
        stats.record_attack(
            &AttackOutcome {
                result: PhysicalAttackResult::Dodge,
                spell: None,
                damage: 0,
                threat: 0.0,
                execution_time: 1.5,
            },
            30.0,
        );

        assert_eq!(stats.display_name(), "Bloodthirst (rank 4)");
        assert_eq!(stats.total_attempts(), 4);
        assert_eq!(stats.hits(), 2);
        assert_eq!(stats.crits(), 1);
        assert_eq!(stats.dodges(), 1);
        assert_eq!(stats.damage(Outcome::Hit).total(), 400);
        assert_eq!(stats.damage(Outcome::Hit).min(), 100);
        assert_eq!(stats.damage(Outcome::Hit).max(), 300);
        assert_eq!(stats.damage(Outcome::Hit).avg(), 200.0);
        assert_eq!(stats.damage(Outcome::Crit).total(), 500);
        assert_eq!(stats.damage(Outcome::Dodge).total(), 0);
        assert_eq!(stats.total_damage(), 900);
        assert_eq!(stats.threat(Outcome::Hit).total(), 600);
        assert_eq!(stats.total_threat(), 1350);
        assert!((stats.outcome_rate(Outcome::Crit) - 0.25).abs() < 1e-12);
        assert!((stats.damage_share(1800) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn ratios_track_min_max_and_mean_per_success() {
        let mut stats = SpellStatistics::new("Heroic Strike", 1);
        stats.record_attack(&hit(150), 15.0);
        stats.record_attack(&hit(300), 15.0);
        // Damage per rage: 10 and 20.
        assert!(stats.dpr().is_set());
        assert_eq!(stats.dpr().min(), 10.0);
        assert_eq!(stats.dpr().max(), 20.0);
        assert_eq!(stats.dpr().avg(), 15.0);
        // Damage per execution time: 100 and 200.
        assert_eq!(stats.dpet().min(), 100.0);
        assert_eq!(stats.dpet().max(), 200.0);
        assert_eq!(stats.dpet().avg(), 150.0);
        // Threat per rage: 15 and 30.
        assert_eq!(stats.tpr().avg(), 22.5);
        assert_eq!(stats.tpet().avg(), 225.0);
    }

    #[test]
    fn free_attacks_without_execution_time_set_no_ratios() {
        let mut stats = SpellStatistics::new("Mainhand Attack", 1);
        stats.record_attack(
            &AttackOutcome {
                result: PhysicalAttackResult::Glancing,
                spell: None,
                damage: 80,
                threat: 80.0,
                execution_time: 0.0,
            },
            0.0,
        );
        assert_eq!(stats.glances(), 1);
        assert!(!stats.dpr().is_set());
        assert!(!stats.dpet().is_set());
        assert_eq!(stats.dpr().min(), 0.0);
        assert_eq!(stats.dpet().avg(), 0.0);
    }

    #[test]
    fn ticks_count_as_hits() {
        let mut stats = SpellStatistics::new("Rend", 7);
        stats.record_tick(37, 37.0, 10.0 / 7.0, 1.5 / 7.0, MagicResistResult::NoResist);
        stats.record_tick(37, 37.0, 10.0 / 7.0, 1.5 / 7.0, MagicResistResult::NoResist);
        assert_eq!(stats.hits(), 2);
        assert_eq!(stats.total_damage(), 74);
        assert!((stats.dpr().avg() - 25.9).abs() < 1e-9);
    }

    #[test]
    fn merge_sums_counts_and_keeps_extremes() {
        let mut a = SpellStatistics::new("Whirlwind", 1);
        a.record_attack(&hit(100), 25.0);
        a.record_attack(&hit(200), 25.0);
        let mut b = SpellStatistics::new("Whirlwind", 1);
        b.record_attack(&hit(50), 25.0);
        b.record_attack(&hit(400), 25.0);
        b.record_attack(&hit(600), 25.0);
        b.increment(Outcome::Miss);

        a.add(&b);
        assert_eq!(a.total_attempts(), 6);
        assert_eq!(a.hits(), 5);
        assert_eq!(a.misses(), 1);
        assert_eq!(a.damage(Outcome::Hit).total(), 1350);
        assert_eq!(a.damage(Outcome::Hit).min(), 50);
        assert_eq!(a.damage(Outcome::Hit).max(), 600);
        assert_eq!(a.damage(Outcome::Hit).avg(), 270.0);
        // Damage per rage: 4, 8 | 2, 16, 24 → mean 10.8 over five samples.
        assert_eq!(a.dpr().samples(), 5);
        assert_eq!(a.dpr().min(), 2.0);
        assert_eq!(a.dpr().max(), 24.0);
        assert!((a.dpr().avg() - 10.8).abs() < 1e-9);

        let mut empty = SpellStatistics::new("Whirlwind", 1);
        empty.add(&a);
        assert_eq!(empty, a);
    }

    #[test]
    fn reset_clears_everything_but_the_identity() {
        let mut stats = SpellStatistics::new("Execute", 1);
        stats.record_attack(&hit(100), 15.0);
        stats.reset();
        assert_eq!(stats, SpellStatistics::new("Execute", 1));
        assert_eq!(stats.name(), "Execute");
    }
}
