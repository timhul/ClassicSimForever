//! `Attack-table.md`: the white attack table against mobs.
//!
//! Formulas (the wiki's conclusions) are checked exactly; the measured breakdowns are checked
//! against the reported value ± its 95 % confidence interval. Every chance below is in percent
//! unless it goes straight into a `Mechanics` call (fractions).

use approx::assert_abs_diff_eq;

use crate::combat_roll::{
    chance_to_range, CombatRoll, IncludedOutcomes, PhysicalAttackResult, RollContext, ROLL_RANGE,
};
use crate::mechanics::Mechanics;
use crate::rng::Random;

/// Tolerance for the closed-form formulas (fractions).
const EPS: f64 = 1e-6;

#[track_caller]
fn assert_close(expected: f64, actual: f64, what: &str) {
    assert!(
        (expected - actual).abs() <= EPS,
        "{what}: expected {expected}, got {actual}"
    );
    assert_abs_diff_eq!(expected, actual, epsilon = EPS);
}

fn ctx(clvl: u32, hit_percent: f64, dual_wielding: bool, from_behind: bool) -> RollContext {
    RollContext {
        clvl,
        melee_hit_chance: chance_to_range(hit_percent / 100.0),
        dual_wielding,
        attacking_from_behind: from_behind,
        glancing_blows: true,
    }
}

/// Share of each outcome in the white table, in rolls out of [`ROLL_RANGE`] (100 = 1 %),
/// found by feeding every roll once. `spellbook_crit_percent` goes through the level-based crit
/// suppression the roll applies (`get_suppressed_crit`); the aura crit suppression lives in
/// `CharacterStats` and is covered by `crit_aura_suppression`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Distribution {
    miss: u32,
    dodge: u32,
    parry: u32,
    glancing: u32,
    block: u32,
    crit: u32,
    hit: u32,
}

impl Distribution {
    fn percent(rolls: u32) -> f64 {
        f64::from(rolls) / 100.0
    }
}

fn white_distribution(
    target_level: u32,
    ctx: &RollContext,
    wpn_skill: u32,
    spellbook_crit_percent: f64,
) -> Distribution {
    let mut roll = CombatRoll::new(target_level);
    let table = roll.get_melee_white_table(ctx, wpn_skill).clone();
    let crit = roll.get_suppressed_crit(ctx.clvl, chance_to_range(spellbook_crit_percent / 100.0));
    let mut random = Random::new(0, ROLL_RANGE);
    let mut d = Distribution::default();
    for r in 0..ROLL_RANGE {
        match table.get_outcome(&mut random, r, crit, IncludedOutcomes::ALL) {
            PhysicalAttackResult::Miss => d.miss += 1,
            PhysicalAttackResult::Dodge => d.dodge += 1,
            PhysicalAttackResult::Parry => d.parry += 1,
            PhysicalAttackResult::Glancing => d.glancing += 1,
            PhysicalAttackResult::Block | PhysicalAttackResult::BlockCritical => d.block += 1,
            PhysicalAttackResult::Critical => d.crit += 1,
            PhysicalAttackResult::Hit => d.hit += 1,
        }
    }
    d
}

/// Miss share of the white table in percent for a level-60 attacker with `hit_percent` +hit.
fn white_miss_percent(target_level: u32, wpn_skill: u32, hit_percent: f64, dual: bool) -> f64 {
    let d = white_distribution(
        target_level,
        &ctx(60, hit_percent, dual, false),
        wpn_skill,
        0.0,
    );
    Distribution::percent(d.miss)
}

/// `expected` percent against the rolls of one outcome.
#[track_caller]
fn assert_share(expected_percent: f64, rolls: u32, what: &str) {
    let expected = (expected_percent * 100.0).round() as u32;
    assert_eq!(
        rolls,
        expected,
        "{what}: expected {expected_percent:.2} %, table has {:.2} %",
        Distribution::percent(rolls)
    );
}

// ------------------------------------------------------------------ officially confirmed

/// "Creatures at your level have a 5% chance to Dodge your attacks. Each additional level the
/// target has over the player grants them 0.5% additional chance to dodge."
#[test]
fn dodge_is_5_percent_at_level_plus_half_a_percent_per_level() {
    for (target_level, expected) in [(60, 0.05), (61, 0.055), (62, 0.06), (63, 0.065)] {
        assert_close(
            expected,
            Mechanics::new(target_level).dodge_chance(300),
            "dodge",
        );
    }
}

/// "Creatures that are 3 levels above the player have a 14% Parry chance."
#[test]
fn parry_is_14_percent_three_levels_above() {
    assert_close(0.14, Mechanics::new(63).parry_chance(300), "parry");
    let d = white_distribution(63, &ctx(60, 0.0, false, false), 300, 0.0);
    assert_share(14.0, d.parry, "parry vs +3 in the white table");
}

/// "Players have an 8% chance to miss a creature that is 3 levels above them."
#[test]
fn miss_is_8_percent_three_levels_above() {
    assert_close(
        0.08,
        Mechanics::new(63).two_hand_white_miss_chance(300),
        "white miss",
    );
    assert_close(
        0.08,
        Mechanics::new(63).yellow_miss_chance(300),
        "yellow miss",
    );
    assert_eq!(white_miss_percent(63, 300, 0.0, false), 8.0);
}

/// "Critical Strike chance is reduced by 1% per each additional level the target has over the
/// player. (So if you have a 4% chance to crit an at-level target, you have a 1% chance to crit
/// a +3-level target.)"
#[test]
fn crit_is_reduced_by_1_percent_per_level_above() {
    for (target_level, expected) in [(60, 400), (61, 300), (62, 200), (63, 100)] {
        let roll = CombatRoll::new(target_level);
        assert_eq!(
            roll.get_suppressed_crit(60, 400),
            expected,
            "4 % crit vs level {target_level}"
        );
    }
    let d = white_distribution(63, &ctx(60, 0.0, false, false), 300, 4.0);
    assert_share(1.0, d.crit, "4 % spellbook crit vs +3");
}

/// "There is some code in 1.12 that explicitly adds a modifier that causes the first 1% of +hit
/// gained from talents or gear to be ignored against monsters with more than 10 Defense Skill
/// above the attacking player's Weapon Skill. This means that the so-called 'hit cap' is in
/// effect 9% rather than 8% for a player with 300 Weapon Skill fighting a level 63 monster with
/// a Defense Skill of 315. With a Weapon Skill of 305 ... this hit modifier is no longer in
/// place."
#[test]
fn first_1_percent_of_hit_is_ignored_when_defense_exceeds_skill_by_more_than_10() {
    // 300 skill: 8 % miss, the first 1 % of hit does nothing.
    assert_eq!(white_miss_percent(63, 300, 0.0, false), 8.0, "no hit");
    assert_eq!(
        white_miss_percent(63, 300, 1.0, false),
        8.0,
        "1 % hit is ignored"
    );
    assert_eq!(
        white_miss_percent(63, 300, 2.0, false),
        7.0,
        "2 % hit removes 1 %"
    );
    assert_eq!(
        white_miss_percent(63, 300, 8.0, false),
        1.0,
        "8 % hit leaves 1 % miss"
    );
    assert_eq!(
        white_miss_percent(63, 300, 9.0, false),
        0.0,
        "9 % hit is the cap"
    );
    // 305 skill: 6 % miss, every point of hit counts.
    assert_eq!(white_miss_percent(63, 305, 0.0, false), 6.0, "305: no hit");
    assert_eq!(
        white_miss_percent(63, 305, 1.0, false),
        5.0,
        "305: 1 % hit counts"
    );
    assert_eq!(white_miss_percent(63, 305, 5.0, false), 1.0, "305: 5 % hit");
    assert_eq!(
        white_miss_percent(63, 305, 6.0, false),
        0.0,
        "305: 6 % hit is the cap"
    );
}

// ------------------------------------------------------------------ miss

/// ```text
/// defense - skill >= 11: MissChance = 5% + (TargetLevel*5 - AttackerSkill) * 0.2%
///                         HitSuppression = (TargetLevel*5 - AttackerSkill - 10) * 0.2%
/// defense - skill <= 10: MissChance = 5% + (TargetLevel*5 - AttackerSkill) * 0.1%
/// ```
/// The "Miss chance" column of the weapon skill table.
#[test]
fn miss_chance_follows_the_defense_minus_skill_difference() {
    let expected_miss = [
        (15, 8.0),
        (14, 7.8),
        (13, 7.6),
        (12, 7.4),
        (11, 7.2),
        (10, 6.0),
        (9, 5.9),
        (8, 5.8),
        (7, 5.7),
        (6, 5.6),
        (5, 5.5),
        (4, 5.4),
        (3, 5.3),
        (2, 5.2),
        (1, 5.1),
        (0, 5.0),
    ];
    let mechanics = Mechanics::new(63);
    let mut failures = Vec::new();
    for (diff, miss) in expected_miss {
        let skill = 315 - diff;
        let actual = mechanics.two_hand_white_miss_chance(skill) * 100.0;
        if (actual - miss).abs() > 1e-6 {
            failures.push(format!(
                "Δ{diff} (skill {skill}): expected {miss:.1} %, got {actual:.1} %"
            ));
        }
    }
    assert!(failures.is_empty(), "miss chance:\n{}", failures.join("\n"));
}

/// The "Hit cap" column of the weapon skill table: the +hit that removes the last miss, which
/// is the miss chance plus the hit suppression above a difference of 10.
#[test]
fn hit_cap_follows_the_defense_minus_skill_difference() {
    let expected_cap = [
        (15, 9.0),
        (14, 8.6),
        (13, 8.2),
        (12, 7.8),
        (11, 7.4),
        (10, 6.0),
        (9, 5.9),
        (8, 5.8),
        (7, 5.7),
        (6, 5.6),
        (5, 5.5),
        (4, 5.4),
        (3, 5.3),
        (2, 5.2),
        (1, 5.1),
        (0, 5.0),
    ];
    let mut failures = Vec::new();
    for (diff, cap) in expected_cap {
        let skill = 315 - diff;
        let at_cap = white_miss_percent(63, skill, cap, false);
        let below_cap = white_miss_percent(63, skill, cap - 0.1, false);
        if at_cap != 0.0 {
            failures.push(format!(
                "Δ{diff} (skill {skill}): {cap:.1} % hit should remove every miss, {at_cap:.2} % left"
            ));
        }
        if below_cap <= 0.0 {
            failures.push(format!(
                "Δ{diff} (skill {skill}): {:.1} % hit should still leave misses",
                cap - 0.1
            ));
        }
    }
    assert!(failures.is_empty(), "hit cap:\n{}", failures.join("\n"));
}

/// "If the target is a mob below level 10: MissChance = NormalMissChance * (TargetLevel / 10)"
#[test]
fn mobs_below_level_10_are_missed_less_often() {
    // A level 5 character with 25 weapon skill vs a level 5 mob: 5 % × 5 / 10.
    assert_close(
        0.025,
        Mechanics::new(5).two_hand_white_miss_chance(25),
        "level 5 mob",
    );
    // Level 9 vs 9: 5 % × 9 / 10.
    assert_close(
        0.045,
        Mechanics::new(9).two_hand_white_miss_chance(45),
        "level 9 mob",
    );
    // Level 10 mobs are missed normally.
    assert_close(
        0.05,
        Mechanics::new(10).two_hand_white_miss_chance(50),
        "level 10 mob",
    );
}

/// "DualWieldMissChance = NormalMissChance + 19%" — a flat penalty, not the old
/// `80 % × miss + 20 %`.
#[test]
fn dual_wield_adds_a_flat_19_percent_miss() {
    assert_close(
        0.27,
        Mechanics::new(63).dual_wield_white_miss_chance(300),
        "300 skill vs +3",
    );
    assert_close(
        0.25,
        Mechanics::new(63).dual_wield_white_miss_chance(305),
        "305 skill vs +3",
    );
    assert_close(
        0.24,
        Mechanics::new(60).dual_wield_white_miss_chance(300),
        "300 skill vs +0",
    );
}

/// Bimmy's PTR data: dual wielding with 300 skill and +27 % hit vs a +3 level mob missed a flat
/// 1 % (8 % + 19 % − (27 % − 1 % suppressed)); with +28 % hit nothing missed.
#[test]
fn dual_wield_misses_1_percent_with_27_percent_hit_and_nothing_with_28() {
    assert_eq!(white_miss_percent(63, 300, 27.0, true), 1.0, "+27 % hit");
    assert_eq!(white_miss_percent(63, 300, 28.0, true), 0.0, "+28 % hit");
}

// ------------------------------------------------------------------ glancing blows

/// `GlancingChance = 10% + (TargetLevel*5 - MIN(AttackerLevel*5, AttackerSkill)) * 2%`
#[test]
fn glancing_chance_is_10_percent_plus_20_per_level() {
    for (target_level, expected) in [(60, 10.0), (61, 20.0), (62, 30.0), (63, 40.0)] {
        assert_close(
            expected / 100.0,
            Mechanics::new(target_level).glancing_blow_chance(60),
            "glancing chance",
        );
        let d = white_distribution(target_level, &ctx(60, 0.0, false, false), 300, 0.0);
        assert_share(expected, d.glancing, "glancing share in the white table");
    }
}

/// The `MIN(AttackerLevel*5, AttackerSkill)` term: weapon skill above the level cap does not
/// lower the glancing chance ...
#[test]
fn weapon_skill_above_the_level_cap_does_not_reduce_glancing_chance() {
    for skill in [300, 305, 310, 315] {
        let d = white_distribution(63, &ctx(60, 0.0, false, false), skill, 0.0);
        assert_share(
            40.0,
            d.glancing,
            &format!("glancing with {skill} skill vs +3"),
        );
    }
}

/// ... but weapon skill below the level cap raises it.
#[test]
fn weapon_skill_below_the_level_cap_raises_glancing_chance() {
    // 295 skill vs 315 defense: 10 % + 20 × 2 %.
    let d = white_distribution(63, &ctx(60, 0.0, false, false), 295, 0.0);
    assert_share(50.0, d.glancing, "glancing with 295 skill vs +3");
    // 290 skill vs an equal-level mob: 10 % + 10 × 2 %.
    let d = white_distribution(60, &ctx(60, 0.0, false, false), 290, 0.0);
    assert_share(30.0, d.glancing, "glancing with 290 skill vs +0");
}

/// ```text
/// Low end:  1.3 - 0.05*(defense-skill) capped at 0.91
/// High end: 1.2 - 0.03*(defense-skill) min of 0.2 and capped at 0.99
/// Average Reduction: (high + low) / 2
/// ```
/// The "Glancing penalty" column of the weapon skill table.
#[test]
fn glancing_penalty_follows_the_defense_minus_skill_difference() {
    let expected_penalty = [
        (15, 35.0),
        (14, 31.0),
        (13, 27.0),
        (12, 23.0),
        (11, 19.0),
        (10, 15.0),
        (9, 11.0),
        (8, 7.0),
        (7, 5.0),
        (6, 5.0),
        (5, 5.0),
        (4, 5.0),
        (3, 5.0),
        (2, 5.0),
        (1, 5.0),
        (0, 5.0),
    ];
    let mechanics = Mechanics::new(63);
    let mut failures = Vec::new();
    for (diff, penalty) in expected_penalty {
        let skill = 315 - diff;
        let low = (1.3 - 0.05 * f64::from(diff)).min(0.91);
        let high = (1.2 - 0.03 * f64::from(diff)).clamp(0.2, 0.99);
        let actual_low = mechanics.glancing_blow_dmg_penalty_min(60, skill);
        let actual_high = mechanics.glancing_blow_dmg_penalty_max(60, skill);
        if (actual_low - low).abs() > 1e-6 || (actual_high - high).abs() > 1e-6 {
            failures.push(format!(
                "Δ{diff} (skill {skill}): expected [{low:.2}, {high:.2}], got [{actual_low:.2}, {actual_high:.2}]"
            ));
        }
        let average = (1.0 - (actual_low + actual_high) / 2.0) * 100.0;
        if (average - penalty).abs() > 1e-6 {
            failures.push(format!(
                "Δ{diff} (skill {skill}): expected {penalty:.0} % average penalty, got {average:.2} %"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "glancing penalty:\n{}",
        failures.join("\n")
    );
}

/// The low end is only capped at 0.91 and the high end only floored at 0.2: beyond a difference
/// of 15 the penalty keeps growing.
#[test]
fn glancing_penalty_keeps_growing_beyond_a_difference_of_15() {
    let mechanics = Mechanics::new(63);
    // Δ16 (299 skill): 0.50 / 0.72.
    assert_close(
        0.50,
        mechanics.glancing_blow_dmg_penalty_min(60, 299),
        "Δ16 low",
    );
    assert_close(
        0.72,
        mechanics.glancing_blow_dmg_penalty_max(60, 299),
        "Δ16 high",
    );
    // Δ20 (295 skill): 0.30 / 0.60.
    assert_close(
        0.30,
        mechanics.glancing_blow_dmg_penalty_min(60, 295),
        "Δ20 low",
    );
    assert_close(
        0.60,
        mechanics.glancing_blow_dmg_penalty_max(60, 295),
        "Δ20 high",
    );
}

/// The rolled glancing multiplier stays inside `[low, high]` and averages their midpoint
/// (35 % vs +3 with 300 skill — measured ~35.12 %).
#[test]
fn glancing_damage_roll_averages_the_midpoint() {
    let mut roll = CombatRoll::from_seed(63, 7);
    let (low, high) = (0.55, 0.75);
    let samples = 20_000;
    let mut sum = 0.0;
    for _ in 0..samples {
        let penalty = roll.get_glancing_blow_dmg_penalty(60, 300);
        assert!(
            (low - 1e-9..=high + 1e-9).contains(&penalty),
            "glancing multiplier {penalty} outside [{low}, {high}]"
        );
        sum += penalty;
    }
    let mean = sum / f64::from(samples);
    assert!(
        (mean - 0.65).abs() < 0.005,
        "mean glancing multiplier {mean:.4}, expected 0.65 (35 % reduction)"
    );
}

// ------------------------------------------------------------------ dodge / parry / block

/// `DodgeChance = 5% + (TargetLevel*5 - AttackerSkill) * 0.1%` — "with +5 weapon skill vs. a
/// +3 level mob your chance to be dodged will decrease from 6.5% to 6%".
#[test]
fn dodge_follows_the_defense_minus_skill_difference() {
    let mechanics = Mechanics::new(63);
    assert_close(0.065, mechanics.dodge_chance(300), "300 skill");
    assert_close(0.063, mechanics.dodge_chance(302), "302 skill");
    assert_close(0.06, mechanics.dodge_chance(305), "305 skill");
    assert_close(0.05, mechanics.dodge_chance(315), "315 skill");
    assert_close(0.045, mechanics.dodge_chance(320), "320 skill");
    assert_close(0.05, Mechanics::new(60).dodge_chance(300), "equal level");
}

/// `BlockChance = MIN(5%, 5% + (TargetLevel*5 - AttackerSkill) * 0.1%)` — "mobs cannot block
/// more than 5% of attacks regardless of rating difference"; the tests vs +3 level mobs showed
/// ~5 % block, not 6.5 %.
#[test]
fn block_is_5_percent_capped_against_mobs() {
    assert_close(0.05, Mechanics::new(63).block_chance(), "block vs +3");
    let d = white_distribution(63, &ctx(60, 0.0, false, false), 300, 0.0);
    assert_share(5.0, d.block, "block vs +3 with 300 skill");
    let d = white_distribution(60, &ctx(60, 0.0, false, false), 300, 0.0);
    assert_share(5.0, d.block, "block vs +0 with 300 skill");
    // Weapon skill above the defense lowers it: 310 vs 300 defense is 4 %.
    let d = white_distribution(60, &ctx(60, 0.0, false, false), 310, 0.0);
    assert_share(4.0, d.block, "block vs +0 with 310 skill");
}

/// Attacking from behind removes parries (and blocks): the dual-wield boss dummy tests saw
/// 0 % of both.
#[test]
fn attacking_from_behind_removes_parry_and_block() {
    let d = white_distribution(63, &ctx(60, 0.0, false, true), 300, 0.0);
    assert_share(0.0, d.parry, "parry from behind");
    assert_share(0.0, d.block, "block from behind");
}

// ------------------------------------------------------------------ critical strike

/// ```text
/// BaseAttackRating = MIN(PlayerLevel*5, AttackRating)
/// mob, BaseAttackRating - TargetDefense < 0:  CritChance = AttackerCrit + diff * 0.2%
/// mob, BaseAttackRating - TargetDefense >= 0: CritChance = AttackerCrit + diff * 0.04%
/// ```
/// "This means you still have -3% suppression `(300-315)*0.2%` to your critical strike chance
/// even with +5 weapon skill vs. +3 levels mobs."
#[test]
fn crit_suppression_ignores_weapon_skill_above_the_level_cap() {
    for skill in [300, 305, 310, 315] {
        let d = white_distribution(63, &ctx(60, 0.0, false, false), skill, 10.0);
        assert_share(7.0, d.crit, &format!("10 % crit with {skill} skill vs +3"));
    }
    for (target_level, expected) in [(63, 700), (62, 800), (61, 900), (60, 1000)] {
        assert_eq!(
            CombatRoll::new(target_level).get_suppressed_crit(60, 1000),
            expected,
            "10 % crit vs level {target_level}"
        );
    }
}

/// Against mobs whose defense is below the attack rating the crit chance rises by 0.04 % per
/// point of difference.
#[test]
fn crit_rises_by_0_04_percent_per_point_of_rating_above_defense() {
    // 300 rating vs a level 59 mob (295 defense): +0.2 %; vs level 58 (290): +0.4 %.
    assert_eq!(
        CombatRoll::new(59).get_suppressed_crit(60, 1000),
        1020,
        "vs level 59"
    );
    assert_eq!(
        CombatRoll::new(58).get_suppressed_crit(60, 1000),
        1040,
        "vs level 58"
    );
    assert_eq!(
        CombatRoll::new(60).get_suppressed_crit(60, 1000),
        1000,
        "vs level 60"
    );
}

// ------------------------------------------------------------------ measured breakdowns

/// One measured attack breakdown (white hits only). Values in percent, `(value, ±95 % CI)`.
struct Breakdown {
    name: &'static str,
    clvl: u32,
    target_level: u32,
    wpn_skill: u32,
    hit_percent: f64,
    dual_wielding: bool,
    from_behind: bool,
    /// The spellbook crit after the wiki's correction for the aura crit suppression, i.e. the
    /// crit the roll sees before the per-level suppression.
    crit_percent: f64,
    hit: (f64, f64),
    crit: (f64, f64),
    miss: (f64, f64),
    parry: (f64, f64),
    dodge: (f64, f64),
    block: (f64, f64),
    glancing: (f64, f64),
}

fn check_breakdown(b: &Breakdown) {
    let d = white_distribution(
        b.target_level,
        &ctx(b.clvl, b.hit_percent, b.dual_wielding, b.from_behind),
        b.wpn_skill,
        b.crit_percent,
    );
    let mut failures = Vec::new();
    for (what, (measured, ci), rolls) in [
        ("hit", b.hit, d.hit),
        ("crit", b.crit, d.crit),
        ("miss", b.miss, d.miss),
        ("parry", b.parry, d.parry),
        ("dodge", b.dodge, d.dodge),
        ("block", b.block, d.block),
        ("glancing", b.glancing, d.glancing),
    ] {
        let actual = Distribution::percent(rolls);
        if (actual - measured).abs() > ci + 1e-9 {
            failures.push(format!(
                "{what}: simulator {actual:.2} %, measured {measured:.2} % ±{ci:.2}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} (level {} vs {}, {} skill, +{} % hit):\n{}",
        b.name,
        b.clvl,
        b.target_level,
        b.wpn_skill,
        b.hit_percent,
        failures.join("\n")
    );
}

/// Equal level target (n=7044, 8.46 % crit). The formulas do not depend on the level, so the
/// level 20/30/40 logs are checked as a level 60 attacker.
#[test]
fn breakdown_equal_level_target() {
    check_breakdown(&Breakdown {
        name: "Equal level target (n=7044)",
        clvl: 60,
        target_level: 60,
        wpn_skill: 300,
        hit_percent: 0.0,
        dual_wielding: false,
        from_behind: false,
        crit_percent: 8.46,
        hit: (61.13, 1.14),
        crit: (8.21, 0.64),
        miss: (5.20, 0.52),
        parry: (4.97, 0.51),
        dodge: (5.30, 0.52),
        block: (5.11, 0.51),
        glancing: (10.09, 0.70),
    });
}

/// Target at +1 level (n=2000, 3.88 % crit).
#[test]
fn breakdown_target_plus_one_level() {
    check_breakdown(&Breakdown {
        name: "Target at +1 level (n=2000)",
        clvl: 60,
        target_level: 61,
        wpn_skill: 300,
        hit_percent: 0.0,
        dual_wielding: false,
        from_behind: false,
        crit_percent: 3.88,
        hit: (55.20, 2.22),
        crit: (2.25, 0.66),
        miss: (6.00, 1.06),
        parry: (5.75, 1.04),
        dodge: (5.95, 1.06),
        block: (4.55, 0.93),
        glancing: (20.30, 1.80),
    });
}

/// Target at +2 levels (n=1999, 3.88 % crit).
#[test]
fn breakdown_target_plus_two_levels() {
    check_breakdown(&Breakdown {
        name: "Target at +2 levels (n=1999)",
        clvl: 60,
        target_level: 62,
        wpn_skill: 300,
        hit_percent: 0.0,
        dual_wielding: false,
        from_behind: false,
        crit_percent: 3.88,
        hit: (45.27, 2.23),
        crit: (1.75, 0.59),
        miss: (6.20, 1.08),
        parry: (6.55, 1.11),
        dodge: (5.70, 1.04),
        block: (5.30, 1.00),
        glancing: (29.21, 2.03),
    });
}

/// Target at +3 levels (n=31779, 7.46 % crit after the aura correction).
#[test]
fn breakdown_target_plus_three_levels() {
    check_breakdown(&Breakdown {
        name: "Target at +3 levels (n=31779)",
        clvl: 60,
        target_level: 63,
        wpn_skill: 300,
        hit_percent: 0.0,
        dual_wielding: false,
        from_behind: false,
        crit_percent: 7.46,
        hit: (22.20, 0.46),
        crit: (4.42, 0.23),
        miss: (7.81, 0.30),
        parry: (13.97, 0.38),
        dodge: (6.29, 0.27),
        block: (4.94, 0.24),
        glancing: (40.36, 0.54),
    });
}

/// Target at +3 levels, +5 weapon skill (n=28063, 11.08 % crit after the aura correction).
///
/// The +2 weapon skill log (n=1200) is left out: its own glancing rate (43.17 % ±2.80) and
/// block rate (3.42 % ±1.03) lie outside the wiki's formulas, so it cannot serve as a check.
#[test]
fn breakdown_target_plus_three_levels_plus_five_weapon_skill() {
    check_breakdown(&Breakdown {
        name: "Target at +3 levels, +5 weapon skill (n=28063)",
        clvl: 60,
        target_level: 63,
        wpn_skill: 305,
        hit_percent: 0.0,
        dual_wielding: false,
        from_behind: false,
        crit_percent: 11.08,
        hit: (21.27, 0.48),
        crit: (7.80, 0.31),
        miss: (5.99, 0.28),
        parry: (13.49, 0.40),
        dodge: (6.04, 0.28),
        block: (5.13, 0.26),
        glancing: (40.28, 0.57),
    });
}

/// Dual wield, +3 levels, 300 skill, +27 % hit, from behind (n=48445). The rogue's 26.90 %
/// spellbook crit is corrected by the 1.8 % aura suppression the wiki concluded (the log's
/// "crit difference" is ~5.10 %).
#[test]
fn breakdown_dual_wield_plus_three_levels_27_percent_hit() {
    check_breakdown(&Breakdown {
        name: "Dual wield, +3 levels, +27 % hit (n=48445)",
        clvl: 60,
        target_level: 63,
        wpn_skill: 300,
        hit_percent: 27.0,
        dual_wielding: true,
        from_behind: true,
        crit_percent: 26.90 - 1.8,
        hit: (30.60, 0.41),
        crit: (21.81, 0.37),
        miss: (1.01, 0.09),
        parry: (0.0, 0.0),
        dodge: (6.43, 0.22),
        block: (0.0, 0.0),
        glancing: (40.15, 0.44),
    });
}

/// Dual wield, +3 levels, 300 skill, +28 % hit, from behind (n=5398).
#[test]
fn breakdown_dual_wield_plus_three_levels_28_percent_hit() {
    check_breakdown(&Breakdown {
        name: "Dual wield, +3 levels, +28 % hit (n=5398)",
        clvl: 60,
        target_level: 63,
        wpn_skill: 300,
        hit_percent: 28.0,
        dual_wielding: true,
        from_behind: true,
        crit_percent: 26.90 - 1.8,
        hit: (31.70, 1.24),
        crit: (22.05, 1.11),
        miss: (0.0, 0.0),
        parry: (0.0, 0.0),
        dodge: (6.58, 0.66),
        block: (0.0, 0.0),
        glancing: (39.68, 1.31),
    });
}
