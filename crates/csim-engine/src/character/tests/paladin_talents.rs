//! The Paladin's talents on the shipped data (TASKS.md P.9): the deep Retribution build spends
//! its 51 points, every DPS talent works through the spell machinery, and the others change
//! nothing. The talents whose mechanics come with a spell are tested with it: Improved Seals,
//! Improved Judgement, Swift Judgement and Sanctified Judgement in `paladin_seals.rs`,
//! Reverence, Benediction, Holy Conduit and Twist of Light's cost in `paladin_mana.rs`,
//! Champion of the Light and Vengeance's holy damage in `paladin_damage.rs`, Purifying Power,
//! Consecrated Ground, Sacred Arbiter and Instrument of Law in `paladin_abilities.rs`, Twist
//! of Light's Echoes in `paladin_echo.rs`.

use super::paladin::{paladin, stat, with_talents};
use super::*;
use crate::effect::EffectHost;
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

const HOLY_POWER: u32 = 105321;
const REVERENCE: u32 = 110871;
const ONE_HANDED_WEAPON_SPECIALIZATION: u32 = 105629;

const HOLY_STRIKE: u32 = 10333;
const EXORCISM: u32 = 10314;
const VINDICATION_BUFF: u32 = 440668;
const VENGEANCE_BUFF: u32 = 20050;

/// The talents that do nothing the sim models (heals, defense, threat, block, stuns, the Holy
/// Shock build), at their maximum rank.
const NOT_SIMULATED: [(u32, u32); 26] = [
    (105333, 3), // Healing Light
    (105335, 2), // Spiritual Focus
    (105331, 2), // Unyielding Faith
    (105330, 1), // Voice of Truth
    (110873, 2), // Infusion of Light
    (105329, 5), // Illumination
    (105325, 1), // Divine Favor
    (105323, 1), // Holy Shock
    (105320, 1), // Light's Vigil
    (TOUGHNESS, 5),
    (105626, 5), // Redoubt
    (105637, 2), // Guardian's Favor
    (105636, 5), // Anticipation
    (110875, 1), // Improved Seal of Fury
    (105634, 3), // Improved Righteous Fury
    (110874, 3), // Shield Specialization
    (105632, 2), // Sacred Duty
    (105633, 3), // Improved Hammer of Justice
    (105625, 1), // Templar's Bulwark
    (105627, 5), // Reckoning
    (110879, 5), // Iron Creed
    (105628, 1), // Holy Shield
    (105707, 5), // Deflection
    (105699, 2), // Pursuit of Justice
    (EYE_FOR_AN_EYE, 2),
    (105694, 1), // Repentance
];

fn buff_of(f: &Fixture, game_id: u32) -> crate::ids::BuffId {
    let id = f.spell_id(game_id);
    f.character.spells().spell(id).marker_buff().unwrap()
}

/// Every roll from now a crit: all crit chances at 100 %, the rolls in the middle of the
/// tables (past the misses, dodges and parries, inside the crits).
fn certain_crits(f: &mut Fixture) {
    let stats = f.character.stats_mut();
    stats.increase_melee_aura_crit(10_000);
    stats.increase_spell_crit(10_000);
    f.character
        .roll_mut()
        .random_mut()
        .set_new_range(5000, 5001);
}

/// Holy Power 5/5: +15 % crit chance on Holy Strike, +5 % on the other spells (Exorcism).
#[test]
fn holy_power_raises_spell_crit() {
    let bonus = |f: &mut Fixture, game_id: u32| {
        let id = f.spell_id(game_id);
        let ctx = f.ctx();
        ctx.character.spells().spell(id).crit_chance_bonus(&ctx)
    };
    let mut base = paladin(Race::Human, &[]);
    let mut f = with_talents(&[(HOLY_POWER, 5)]);
    assert_eq!(
        bonus(&mut f, HOLY_STRIKE),
        bonus(&mut base, HOLY_STRIKE) + 1500
    );
    assert_eq!(bonus(&mut f, EXORCISM), bonus(&mut base, EXORCISM) + 500);
}

/// One-Handed Weapon Specialization 3/3: +10 % physical damage with a one-hander, nothing with
/// a two-hander.
#[test]
fn one_handed_weapon_specialization_raises_physical_damage_with_a_one_hander() {
    let damage = |f: &Fixture| stat(f, |s, ctx| s.get_total_physical_damage_mod(ctx));
    let mut base = paladin(Race::Human, &[]);
    let mut f = with_talents(&[(ONE_HANDED_WEAPON_SPECIALIZATION, 3)]);
    assert!((damage(&f) - damage(&base)).abs() < 1e-9, "a two-hander");
    for f in [&mut base, &mut f] {
        f.equip(EquipmentSlot::Mainhand, SWORD);
        f.ctx().reevaluate_passives();
    }
    assert!(
        (damage(&f) - damage(&base) * 1.1).abs() < 1e-9,
        "{} vs {}",
        damage(&f),
        damage(&base)
    );
}

/// Vindication 3/3: a landed melee attack (the pull's first swing) gives +3 % attack power for
/// 30 s.
#[test]
fn vindication_raises_attack_power_on_a_hit() {
    let mut base = super::paladin_seals::pulled(&[]);
    let mut f = super::paladin_seals::pulled(&[(VINDICATION, 3)]);
    let buff = buff_of(&f, VINDICATION_BUFF);
    let swung_at = 0.0;
    let t = f.engine.current_time();
    assert!(f.ctx().buff_ref(buff).is_active(), "the first swing landed");
    assert!((f.ctx().buff_ref(buff).time_left(t) - (30.0 - (t - swung_at))).abs() < 1e-9);
    let (with, without) = (f.ctx().melee_ap(), base.ctx().melee_ap());
    assert!(
        (f64::from(with) - f64::from(without) * 1.03).abs() <= 1.0,
        "{without} → {with}"
    );
}

/// Vengeance 3/3: a crit of any table applies a stack (a white swing, Holy Strike, Exorcism on
/// the magic table), up to 3.
#[test]
fn vengeance_stacks_on_crits_of_any_kind() {
    let mut f = super::paladin_seals::pulled(&[(SANCTIFIED_JUDGEMENT, 3), (VENGEANCE, 3)]);
    f.target
        .set_creature_type(crate::target::CreatureType::Undead);
    let buff = buff_of(&f, VENGEANCE_BUFF);
    let stacks = |f: &mut Fixture| {
        let buff = f.ctx().buff_ref(buff).clone();
        if buff.is_active() { buff.stacks() } else { 0 }
    };
    assert_eq!(
        stacks(&mut f),
        0,
        "the pull's first swing hit without a crit"
    );
    certain_crits(&mut f);
    // The white table is one roll: past the glancing blows, inside the crits.
    let white_crit = |f: &mut Fixture| {
        f.character
            .roll_mut()
            .random_mut()
            .set_new_range(9000, 9001);
        let swing = f.ctx().perform_swing(Hand::Mainhand);
        f.character
            .roll_mut()
            .random_mut()
            .set_new_range(5000, 5001);
        swing
    };
    let swing = white_crit(&mut f);
    assert_eq!(swing.attack.result, PhysicalAttackResult::Critical);
    assert_eq!(stacks(&mut f), 1, "a white crit");
    let exorcism = f.spell_id(EXORCISM);
    let report = f.ctx().cast(exorcism);
    assert!(report.attack.unwrap().spell.is_some(), "on the magic table");
    assert_eq!(stacks(&mut f), 2, "a spell crit");
    let t = f.engine.current_time() + 1.5;
    f.advance_to(t);
    let holy_strike = f.spell_id(HOLY_STRIKE);
    let report = f.ctx().cast(holy_strike);
    assert_eq!(
        report.attack.unwrap().result,
        PhysicalAttackResult::Critical
    );
    assert_eq!(stacks(&mut f), 3, "a strike crit");
    white_crit(&mut f);
    assert_eq!(stacks(&mut f), 3, "at most 3");
}

/// The talents the sim does not model change nothing: a fight with all of them deals the
/// same damage as one without, roll for roll.
#[test]
fn the_other_talents_do_nothing() {
    let fight = |talents: &[(u32, u32)]| {
        let mut all = vec![(SEAL_OF_COMMAND, 1)];
        all.extend_from_slice(talents);
        let mut f = with_talents(&all);
        f.target.set_level(63);
        f.character.set_seed(3);
        let spec = r#"
class: PALADIN
name: Command
attack_mode: melee
cast_if:
  - name: Seal of Command
    condition: buff_duration "Seal of Command" is false
  - name: Judgement
  - name: Holy Strike
"#;
        f.ctx()
            .set_rotation(std::sync::Arc::new(serde_yaml::from_str(spec).unwrap()));
        f.ctx().prepare_set_of_combat_iterations();
        super::energy::pull(&mut f);
        f.advance_to(60.0);
        f.ctx().take_statistics().total_damage()
    };
    // Illumination needs Reverence, which both fights have.
    let base = fight(&[(REVERENCE, 3)]);
    assert!(base > 0);
    let mut all = vec![(REVERENCE, 3)];
    all.extend_from_slice(&NOT_SIMULATED);
    assert_eq!(fight(&all), base);
}
