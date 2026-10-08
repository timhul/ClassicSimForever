//! Spell power and holy damage on the shipped Paladin data (TASKS.md P.5): the spell power
//! coefficient of direct damage and of holy weapon strikes, no armor and the holy damage
//! modifiers for holy, Judgement of the Crusader's damage taken through the coefficient,
//! Champion of the Light, the crit multiplier of each table and spell hit.
//!
//! Most tests compare two Paladins built and pulled the same way: their rolls and damage
//! ranges are the same, so the difference of one cast is the formula's term alone.

use super::energy::pull;
use super::paladin::{stat, with_talents};
use super::*;
use crate::effect::EffectHost;
use crate::magic_school::MagicSchool;
use crate::spell::CastReport;
use crate::target::CreatureType;

const EXORCISM: u32 = 10314;
const HOLY_STRIKE: u32 = 10333;
const HAMMER_OF_WRATH: u32 = 24275;
const SEAL_OF_COMMAND_PROC: u32 = 20424;
const JUDGEMENT_OF_COMMAND: u32 = 20966;
const JUDGEMENT_OF_THE_CRUSADER: u32 = 20303;
const VENGEANCE_BUFF: u32 = 20050;

const SEAL_OF_COMMAND: u32 = 105696;
const SANCTIFIED_JUDGEMENT: u32 = 105701;
const VENGEANCE: u32 = 105693;
const CHAMPION_OF_THE_LIGHT: u32 = 110882;
const PRECISION: u32 = 105638;

const VENGEANCE_TALENTS: [(u32, u32); 3] = [
    (SEAL_OF_COMMAND, 1),
    (SANCTIFIED_JUDGEMENT, 3),
    (VENGEANCE, 3),
];

/// A Human Paladin with the Seal of Command talent at the pull, against a level 60 undead
/// target (no resistance from levels), `prepare`d, seeded the same every time and every roll
/// rigged to a hit.
fn pulled(prepare: impl FnOnce(&mut Fixture)) -> Fixture {
    pulled_with(&VENGEANCE_TALENTS[..1], prepare)
}

/// [`pulled`] with `talents`. The pull's events are not run: no auto attack lands (and procs
/// Vengeance) before the test's casts.
fn pulled_with(talents: &[(u32, u32)], prepare: impl FnOnce(&mut Fixture)) -> Fixture {
    let mut f = with_talents(talents);
    f.target.set_level(60);
    f.target.set_creature_type(CreatureType::Undead);
    prepare(&mut f);
    f.character.set_seed(1);
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    f
}

/// Casts `game_id` as a trigger (no cost, cooldown or global cooldown).
fn cast(f: &mut Fixture, game_id: u32) -> CastReport {
    f.ctx()
        .trigger_spell(game_id, None)
        .unwrap_or_else(|| panic!("{game_id} is learned"))
}

/// The damage of one cast of `game_id` by a Paladin `prepare`d.
fn damage(game_id: u32, prepare: impl FnOnce(&mut Fixture)) -> u32 {
    let mut f = pulled(prepare);
    cast(&mut f, game_id)
        .attack
        .expect("the spell dealt damage")
        .damage
}

fn spell_damage(amount: u32) -> impl FnOnce(&mut Fixture) {
    move |f: &mut Fixture| f.character.stats_mut().increase_base_spell_damage(amount)
}

fn nothing(_: &mut Fixture) {}

/// `actual` is `expected` up to the rounding of the two damages compared.
fn close_to(actual: i64, expected: f64) {
    assert!(
        (actual as f64 - expected).abs() <= 1.0,
        "{actual}, expected {expected}"
    );
}

/// What `prepare` adds to one cast of `game_id`.
fn difference(game_id: u32, prepare: impl FnOnce(&mut Fixture)) -> i64 {
    i64::from(damage(game_id, prepare)) - i64::from(damage(game_id, nothing))
}

/// Exorcism adds 0.429 of the spell damage, on the magic table.
#[test]
fn exorcism_takes_its_spell_power_coefficient() {
    close_to(difference(EXORCISM, spell_damage(1000)), 429.0);
    let mut f = pulled(nothing);
    let attack = cast(&mut f, EXORCISM).attack.unwrap();
    assert!(attack.spell.is_some(), "on the magic table");
}

/// Holy Strike: 50 % of the normalized weapon damage, plus the whole 93 ± 25 % and 0.429 of
/// the spell damage (the tooltip's reading); holy, so armor does not reduce it.
#[test]
fn holy_strike_adds_its_flat_damage_and_spell_power_whole() {
    close_to(difference(HOLY_STRIKE, spell_damage(1000)), 429.0);
    assert_eq!(
        damage(HOLY_STRIKE, |f| f.target.set_base_armor(20_000)),
        damage(HOLY_STRIKE, |f| f.target.set_base_armor(0)),
    );
    // The two-handed axe (200-300, normalized to 3.3 s) with the attack power.
    let mut f = pulled(nothing);
    let ap = f64::from(f.ctx().melee_ap());
    let weapon = |base: f64| base + ap / 14.0 * 3.3;
    let (min, max) = (
        (weapon(200.0) * 0.5 + 93.0 * 0.75).floor(),
        (weapon(300.0) * 0.5 + 93.0 * 1.25).ceil(),
    );
    for _ in 0..100 {
        let attack = cast(&mut f, HOLY_STRIKE).attack.unwrap();
        assert!(attack.spell.is_none(), "on the melee table");
        let damage = f64::from(attack.damage);
        assert!(
            (min..=max).contains(&damage),
            "{damage} not in {min}..{max}"
        );
    }
}

/// Seal of Command's strike: 70 % of the weapon damage plus 0.29 of the spell damage, holy on
/// the melee table, through no armor.
#[test]
fn seal_of_command_strikes_holy_with_its_coefficient() {
    close_to(difference(SEAL_OF_COMMAND_PROC, spell_damage(1000)), 290.0);
    assert_eq!(
        damage(SEAL_OF_COMMAND_PROC, |f| f.target.set_base_armor(20_000)),
        damage(SEAL_OF_COMMAND_PROC, |f| f.target.set_base_armor(0)),
    );
    let mut f = pulled(nothing);
    let attack = cast(&mut f, SEAL_OF_COMMAND_PROC).attack.unwrap();
    assert!(attack.spell.is_none(), "on the melee table");
}

/// Judgement of the Crusader: the holy damage the target takes (+161) counts as spell damage of
/// every holy attack, through its coefficient: Judgement of Command (0.429) and the Seal of
/// Command strike (0.29) here.
#[test]
fn judgement_of_the_crusader_adds_holy_damage_through_the_coefficient() {
    let mut f = pulled(nothing);
    cast(&mut f, JUDGEMENT_OF_THE_CRUSADER);
    assert_eq!(
        stat(&f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Holy)),
        161
    );
    assert_eq!(
        stat(&f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Fire)),
        0
    );

    let crusader = |f: &mut Fixture| {
        f.target
            .stats_mut()
            .increase_spell_damage_vs_school(161, MagicSchool::Holy)
    };
    close_to(difference(JUDGEMENT_OF_COMMAND, crusader), 161.0 * 0.429);
    close_to(difference(SEAL_OF_COMMAND_PROC, crusader), 161.0 * 0.29);
}

/// Champion of the Light: spell damage of every magic school of 20 % of the intellect per rank.
#[test]
fn champion_of_the_light_turns_intellect_into_spell_damage() {
    for rank in 1..=3 {
        let f = with_talents(&[(CHAMPION_OF_THE_LIGHT, rank)]);
        let intellect = stat(&f, |s, ctx| s.get_intellect(ctx));
        for school in MagicSchool::MAGIC {
            assert_eq!(
                stat(&f, |s, ctx| s.get_spell_damage(ctx, school)),
                intellect * 20 * rank / 100,
                "rank {rank} {school:?}"
            );
        }
        assert_eq!(
            stat(&f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Physical)),
            0
        );
    }
}

/// Vengeance's buff raises physical and holy damage alike (`MOD_DAMAGE_PERCENT_DONE`, mask
/// physical + holy): a Holy Strike hits harder by the same factor.
#[test]
fn vengeance_raises_holy_damage_too() {
    let pulled = |vengeance: bool| {
        let mut f = pulled_with(&VENGEANCE_TALENTS, nothing);
        if vengeance {
            cast(&mut f, VENGEANCE_BUFF);
        }
        f
    };
    let (mut f, mut base) = (pulled(true), pulled(false));
    let holy = stat(&f, |s, ctx| {
        s.get_magic_school_damage_mod(ctx, MagicSchool::Holy)
    });
    let fire = stat(&f, |s, ctx| {
        s.get_magic_school_damage_mod(ctx, MagicSchool::Fire)
    });
    assert!(holy > 1.0 && fire == 1.0, "{holy} {fire}");
    let physical = |f: &Fixture| stat(f, |s, ctx| s.get_total_physical_damage_mod(ctx));
    assert!((physical(&f) / physical(&base) - holy).abs() < 1e-9);

    // The buff's cast rolled nothing: the strikes roll the same.
    let with = f64::from(cast(&mut f, HOLY_STRIKE).attack.unwrap().damage);
    let without = f64::from(cast(&mut base, HOLY_STRIKE).attack.unwrap().damage);
    assert!(
        (with / without - holy).abs() < 0.005,
        "{with} / {without} vs {holy}"
    );
}

/// A crit on the melee table doubles (Holy Strike, the Seal of Command strike, Hammer of Wrath
/// on the ranged table); on the magic table it is 1.5 times (Exorcism).
#[test]
fn crits_multiply_by_the_table_they_roll_on() {
    // Every crit chance at 100 %, the rolls in the middle of the tables: past the misses,
    // dodges and parries, inside the crits.
    let certain_crit = |f: &mut Fixture| {
        let stats = f.character.stats_mut();
        stats.increase_melee_aura_crit(10_000);
        stats.increase_spell_crit(10_000);
    };
    let crit_damage = |game_id| {
        let mut f = pulled(certain_crit);
        f.character
            .roll_mut()
            .random_mut()
            .set_new_range(5000, 5001);
        let attack = cast(&mut f, game_id).attack.unwrap();
        assert_eq!(attack.result, PhysicalAttackResult::Critical, "{game_id}");
        attack.damage
    };
    for (game_id, multiplier) in [
        (HOLY_STRIKE, 2.0),
        (SEAL_OF_COMMAND_PROC, 2.0),
        (EXORCISM, 1.5),
        (HAMMER_OF_WRATH, 2.0),
    ] {
        let crit = crit_damage(game_id);
        let hit = damage(game_id, nothing);
        let ratio = f64::from(crit) / f64::from(hit);
        assert!(
            (ratio - multiplier).abs() < 0.01,
            "{game_id}: {crit} / {hit} = {ratio}"
        );
    }
}

/// Precision 3/3 adds 3 % spell hit: Exorcism misses a level 63 target 3 % less often.
#[test]
fn precision_raises_spell_hit_on_the_magic_table() {
    let miss_rate = |talents: &[(u32, u32)]| {
        let mut f = with_talents(talents);
        f.target.set_creature_type(CreatureType::Undead);
        pull(&mut f);
        f.advance_to(0.01);
        let casts = 20_000;
        let misses = (0..casts)
            .filter(|_| {
                let attack = cast(&mut f, EXORCISM).attack.unwrap();
                attack.result == PhysicalAttackResult::Miss
            })
            .count();
        misses as f64 / f64::from(casts)
    };
    let f = with_talents(&[(PRECISION, 3)]);
    assert_eq!(
        stat(&f, |s, ctx| s.get_spell_hit_chance(ctx, MagicSchool::Holy)),
        300
    );
    let (base, precise) = (miss_rate(&[]), miss_rate(&[(PRECISION, 3)]));
    assert!((base - precise - 0.03).abs() < 0.01, "{base} -> {precise}");
}
