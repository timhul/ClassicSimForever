//! The statistics as rows of a report, shared by `csim run` and `csim-live`: one row per
//! spell, split by outcome.

use serde::Serialize;

use super::{ClassStatistics, Outcome, SpellStatistics};

#[derive(Debug, Serialize)]
pub struct SpellRow {
    pub name: String,
    pub dps: f64,
    pub damage_share: f64,
    pub tps: f64,
    pub casts: f64,
    /// The smallest and largest damage of one successful attempt; none without damage.
    pub min_hit: Option<u32>,
    pub max_hit: Option<u32>,
    /// Mean damage per resource point of the successful attempts; none for spells without a
    /// cost.
    pub damage_per_resource: Option<f64>,
    pub hit: f64,
    pub crit: f64,
    pub glance: f64,
    pub miss: f64,
    pub dodge: f64,
    pub parry: f64,
    pub block: f64,
    pub resist: f64,
    /// The attempts split by outcome, each part a row of its own (named after the outcome, its
    /// rates shares of all the spell's attempts, so the parts add up to the spell): a magic
    /// spell's by the share of damage resisted (see [`RESIST_BREAKDOWN`]), a white swing's by
    /// crit, hit and glancing blow, a melee ability's by crit and hit. Empty for other spells
    /// (physical damage over time, such as Deep Wounds).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breakdown: Vec<SpellRow>,
}

/// A magic spell's outcomes by the share of damage resisted: 100 % (a miss or a full resist)
/// down to 0 %.
const RESIST_BREAKDOWN: [(&str, &[Outcome]); 5] = [
    ("100% resisted", &[Outcome::Miss, Outcome::FullResist]),
    (
        "75% resisted",
        &[Outcome::PartialResist75, Outcome::PartialResistCrit75],
    ),
    (
        "50% resisted",
        &[Outcome::PartialResist50, Outcome::PartialResistCrit50],
    ),
    (
        "25% resisted",
        &[Outcome::PartialResist25, Outcome::PartialResistCrit25],
    ),
    ("0% resisted", &[Outcome::Hit, Outcome::Crit]),
];

/// A white swing's connecting outcomes.
const SWING_BREAKDOWN: [(&str, &[Outcome]); 3] = [
    ("Crit", &[Outcome::Crit]),
    ("Hit", &[Outcome::Hit]),
    ("Glancing", &[Outcome::Glancing]),
];

/// A melee ability's connecting outcomes (abilities cannot glance).
const ABILITY_BREAKDOWN: [(&str, &[Outcome]); 2] =
    [("Crit", &[Outcome::Crit]), ("Hit", &[Outcome::Hit])];

/// The spells (auto attacks and procs) that did something, by damage then threat, each with
/// its outcome breakdown, over `iterations` iterations lasting `time` seconds of combat in
/// all (a set's finished ones, [`ClassStatistics::iterations`] and
/// [`ClassStatistics::time_in_combat`]; mid-iteration 1 and the time so far).
pub fn spell_rows(stats: &ClassStatistics, iterations: u64, time: f64) -> Vec<SpellRow> {
    let total_damage = stats.total_damage();
    let mut spells: Vec<_> = stats
        .spells()
        .filter(|(_, spell)| {
            spell.total_attempts() > 0 || spell.total_damage() > 0 || spell.total_threat() > 0
        })
        .collect();
    spells.sort_by(|(a_key, a), (b_key, b)| {
        (b.total_damage(), b.total_threat())
            .cmp(&(a.total_damage(), a.total_threat()))
            .then_with(|| a_key.cmp(b_key))
    });
    spells
        .into_iter()
        .map(|(key, spell)| {
            let row = |name: String, outcomes: &[Outcome]| {
                outcome_row(name, spell, outcomes, iterations, time, total_damage)
            };
            let breakdown: &[(&str, &[Outcome])] = if spell.is_magic() {
                &RESIST_BREAKDOWN
            } else if spell.is_auto_attack() {
                &SWING_BREAKDOWN
            } else if spell.is_melee() {
                &ABILITY_BREAKDOWN
            } else {
                &[]
            };
            SpellRow {
                damage_per_resource: spell.dpr().is_set().then(|| spell.dpr().avg()),
                breakdown: breakdown
                    .iter()
                    .map(|(name, outcomes)| row(name.to_string(), outcomes))
                    .collect(),
                ..row(key.display_name(), &Outcome::ALL)
            }
        })
        .collect()
}

/// The attempts of `spell` that ended with one of `outcomes`, as a row named `name`: their
/// damage and threat, and their rates as shares of all the spell's attempts.
fn outcome_row(
    name: String,
    spell: &SpellStatistics,
    outcomes: &[Outcome],
    iterations: u64,
    time: f64,
    total_damage: u64,
) -> SpellRow {
    let attempts = spell.total_attempts();
    let sum = |value: &dyn Fn(Outcome) -> u64| outcomes.iter().map(|&o| value(o)).sum::<u64>();
    // The share of the attempts that ended with one of `outcomes` in `column`.
    let rate = |column: &[Outcome]| {
        per(
            sum(&|o| {
                if column.contains(&o) {
                    spell.attempts(o)
                } else {
                    0
                }
            }),
            attempts,
        )
    };
    let damage = sum(&|o| spell.damage(o).total());
    let damaging = || {
        outcomes
            .iter()
            .map(|&outcome| spell.damage(outcome))
            .filter(|tally| tally.max() > 0)
    };
    SpellRow {
        name,
        dps: per_second(damage, time),
        damage_share: per(damage, total_damage),
        tps: per_second(sum(&|o| spell.threat(o).total()), time),
        casts: per(sum(&|o| spell.attempts(o)), iterations),
        min_hit: damaging().map(|tally| tally.min()).min(),
        max_hit: damaging().map(|tally| tally.max()).max(),
        damage_per_resource: None,
        hit: rate(&[
            Outcome::Hit,
            Outcome::PartialResist25,
            Outcome::PartialResist50,
            Outcome::PartialResist75,
        ]),
        crit: rate(&[
            Outcome::Crit,
            Outcome::PartialResistCrit25,
            Outcome::PartialResistCrit50,
            Outcome::PartialResistCrit75,
        ]),
        glance: rate(&[Outcome::Glancing]),
        miss: rate(&[Outcome::Miss]),
        dodge: rate(&[Outcome::Dodge]),
        parry: rate(&[Outcome::Parry]),
        block: rate(&[
            Outcome::FullBlock,
            Outcome::PartialBlock,
            Outcome::PartialBlockCrit,
        ]),
        resist: rate(&[Outcome::FullResist]),
        breakdown: Vec::new(),
    }
}

/// `count` per `of` (0 for none).
fn per(count: u64, of: u64) -> f64 {
    if of == 0 {
        0.0
    } else {
        count as f64 / of as f64
    }
}

/// `amount` per second over `time` (0 for no time).
fn per_second(amount: u64, time: f64) -> f64 {
    if time > 0.0 {
        amount as f64 / time
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_roll::PhysicalAttackResult;
    use crate::spell::AttackOutcome;

    #[test]
    fn swings_break_down_by_crit_hit_and_glance() {
        let swing = |result, damage| AttackOutcome {
            result,
            spell: None,
            damage,
            threat: f64::from(damage),
            execution_time: 0.0,
        };
        let mut stats = ClassStatistics::new("Tester", 100.0);
        let mh = stats.spell("Mainhand Attack", 1);
        for (result, damage) in [
            (PhysicalAttackResult::Critical, 800),
            (PhysicalAttackResult::Hit, 400),
            (PhysicalAttackResult::Hit, 420),
            (PhysicalAttackResult::Glancing, 250),
            (PhysicalAttackResult::Dodge, 0),
        ] {
            mh.record_swing(&swing(result, damage));
        }
        stats
            .spell("Bloodthirst", 1)
            .record_attack(&swing(PhysicalAttackResult::Hit, 600), 30.0);
        stats
            .spell("Deep Wounds", 1)
            .record_tick(25, 25.0, 0.0, 0.0, None);

        let rows = spell_rows(&stats, 1, 100.0);
        let names = |name: &str| -> Vec<String> {
            let row = rows.iter().find(|r| r.name == name).unwrap();
            row.breakdown.iter().map(|r| r.name.clone()).collect()
        };
        assert_eq!(names("Bloodthirst"), ["Crit", "Hit"]);
        assert!(names("Deep Wounds").is_empty());
        let mh = rows.iter().find(|r| r.name == "Mainhand Attack").unwrap();
        let parts: Vec<_> = mh
            .breakdown
            .iter()
            .map(|r| {
                (
                    r.name.as_str(),
                    r.min_hit,
                    r.max_hit,
                    r.hit,
                    r.crit,
                    r.glance,
                )
            })
            .collect();
        assert_eq!(
            parts,
            [
                ("Crit", Some(800), Some(800), 0.0, 0.2, 0.0),
                ("Hit", Some(400), Some(420), 0.4, 0.0, 0.0),
                ("Glancing", Some(250), Some(250), 0.0, 0.0, 0.2),
            ]
        );
        let share: f64 = mh.breakdown.iter().map(|r| r.damage_share).sum();
        assert!((share - mh.damage_share).abs() < 1e-12);
        assert_eq!(mh.dodge, 0.2);
    }
}
