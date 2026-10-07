//! The Paladin's talents on the shipped data: the deep Retribution build spends its 51 points,
//! and the stat talents the tables carry completely work as they load. The scripted talents
//! (seals, judgements, Vengeance, Vindication, Champion of the Light, ...) come with their
//! mechanics (TASKS.md P.4-P.9).

use super::paladin::{paladin, stat, with_talents};
use super::*;
use crate::magic_school::MagicSchool;

const DIVINE_STRENGTH: u32 = 105639;
const DIVINE_INTELLECT: u32 = 105332;
const IMPROVED_SEALS: u32 = 105334;
const TOUGHNESS: u32 = 105630;
const PRECISION: u32 = 105638;
const BENEDICTION: u32 = 105706;
const IMPROVED_JUDGEMENT: u32 = 105705;
const CONVICTION: u32 = 105703;
const VINDICATION: u32 = 105702;
const SANCTIFIED_JUDGEMENT: u32 = 105701;
const SEAL_OF_COMMAND: u32 = 105696;
const SACRED_ARBITER: u32 = 105700;
const EYE_FOR_AN_EYE: u32 = 105698;
const TWO_HANDED_WEAPON_SPECIALIZATION: u32 = 105697;
const VENGEANCE: u32 = 105693;
const CHAMPION_OF_THE_LIGHT: u32 = 110882;
const TWIST_OF_LIGHT: u32 = 105692;

/// 32 Retribution points up to Twist of Light (Vengeance after Sanctified Judgement), 11 Holy
/// and 8 Protection: a valid build, not a recommendation.
const DEEP_RETRIBUTION: [(u32, u32); 17] = [
    (BENEDICTION, 5),
    (CONVICTION, 5),
    (IMPROVED_JUDGEMENT, 2),
    (VINDICATION, 3),
    (SANCTIFIED_JUDGEMENT, 3),
    (SEAL_OF_COMMAND, 1),
    (SACRED_ARBITER, 1),
    (EYE_FOR_AN_EYE, 2),
    (TWO_HANDED_WEAPON_SPECIALIZATION, 3),
    (VENGEANCE, 3),
    (CHAMPION_OF_THE_LIGHT, 3),
    (TWIST_OF_LIGHT, 1),
    (DIVINE_STRENGTH, 5),
    (DIVINE_INTELLECT, 3),
    (IMPROVED_SEALS, 3),
    (TOUGHNESS, 5),
    (PRECISION, 3),
];

#[test]
fn the_deep_retribution_build_spends_51_points() {
    let f = paladin(Race::Human, &DEEP_RETRIBUTION);
    let talents = f.character.talents().unwrap();
    assert_eq!(talents.spent_points(), 51);
    assert_eq!(talents.tab_points(184), 32, "Retribution");
}

/// Vengeance needs Sanctified Judgement.
#[test]
fn vengeance_needs_sanctified_judgement() {
    let mut f = paladin(Race::Human, &[]);
    let without: Vec<_> = DEEP_RETRIBUTION
        .into_iter()
        .filter(|&(node, _)| node != SANCTIFIED_JUDGEMENT)
        .collect();
    assert_ne!(f.ctx().spend_talent_points(&without), []);
}

/// Divine Strength 5/5: +10 % strength (and its attack power).
#[test]
fn divine_strength_raises_strength() {
    let strength = |f: &Fixture| stat(f, |s, ctx| s.get_strength(ctx));
    let base = paladin(Race::Human, &[]);
    let f = with_talents(&[(DIVINE_STRENGTH, 5)]);
    assert_eq!(
        strength(&f),
        (f64::from(strength(&base)) * 1.1).round() as u32
    );
}

/// Divine Intellect 5/5: +10 % intellect, and the maximum mana follows at the reset.
#[test]
fn divine_intellect_raises_intellect_and_mana() {
    let intellect = |f: &Fixture| stat(f, |s, ctx| s.get_intellect(ctx));
    let base = paladin(Race::Human, &[]);
    let f = with_talents(&[(DIVINE_INTELLECT, 5)]);
    assert_eq!(intellect(&base), 70, "The Human Spirit raises spirit only");
    assert_eq!(intellect(&f), 77);
    assert_eq!(
        f.character.max_resource_level(ResourceType::Mana),
        base.character.max_resource_level(ResourceType::Mana) + 7 * 15
    );
}

/// Conviction 5/5: +5 % melee crit, an aura crit (suppressed against a boss like any other).
#[test]
fn conviction_raises_melee_crit() {
    let crit = |f: &Fixture| stat(f, |s, ctx| s.get_mh_crit_chance(ctx));
    let mut base = paladin(Race::Human, &[]);
    let f = with_talents(&[(CONVICTION, 5)]);
    assert!(crit(&f) > crit(&base));
    base.character.stats_mut().increase_melee_aura_crit(500);
    assert_eq!(crit(&f), crit(&base));
}

/// Precision 3/3: +3 % melee and spell hit.
#[test]
fn precision_raises_melee_and_spell_hit() {
    let melee = |f: &Fixture| stat(f, |s, ctx| s.get_melee_hit_chance(ctx));
    let spell = |f: &Fixture| stat(f, |s, ctx| s.get_spell_hit_chance(ctx, MagicSchool::Holy));
    let base = paladin(Race::Human, &[]);
    let f = with_talents(&[(PRECISION, 3)]);
    assert_eq!(melee(&f), melee(&base) + 300);
    assert_eq!(spell(&f), spell(&base) + 300);
}

/// Two-Handed Weapon Specialization 3/3: +6 % physical damage with a two-hander.
#[test]
fn two_handed_weapon_specialization_raises_physical_damage() {
    let damage = |f: &Fixture| stat(f, |s, ctx| s.get_total_physical_damage_mod(ctx));
    let base = paladin(Race::Human, &[]);
    let f = with_talents(&[(TWO_HANDED_WEAPON_SPECIALIZATION, 3)]);
    assert!(
        (damage(&f) - damage(&base) * 1.06).abs() < 1e-9,
        "{} vs {}",
        damage(&f),
        damage(&base)
    );
}
