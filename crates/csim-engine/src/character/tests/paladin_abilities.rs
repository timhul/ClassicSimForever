//! The Paladin's abilities on the shipped data (TASKS.md P.8): Holy Strike and Sacred Arbiter,
//! Exorcism and Holy Wrath against Undead and Demons only (Purifying Power), Consecration's
//! ticks and Consecrated Ground, Hammer of Wrath in execute range on the ranged table
//! (Instrument of Law), Blessing of Might on oneself, and the Season of Discovery passives the
//! class learns that do nothing.

use super::energy::data;
use super::paladin_seals::{now, pulled};
use super::*;
use crate::buff::external::ExternalBuffDb;
use crate::effect::EffectHost;
use crate::magic_school::MagicSchool;
use crate::spell::CastReport;
use crate::spell::SpellHost;
use crate::statistics::ClassStatistics;
use crate::target::CreatureType;

const HOLY_STRIKE: u32 = 10333;
const EXORCISM: u32 = 10314;
const HOLY_WRATH: u32 = 10318;
const CONSECRATION: u32 = 20924;
const HAMMER_OF_WRATH: u32 = 24275;
const BLESSING_OF_MIGHT: u32 = 25291;
const JUDGEMENT: u32 = 20271;
const JUDGEMENT_OF_THE_CRUSADER: u32 = 20303;
const SACRED_ARBITER_SPELL: u32 = 1311087;

const SACRED_ARBITER: u32 = 105700;
const PURIFYING_POWER: u32 = 105327;
const CONSECRATED_GROUND: u32 = 110872;
const INSTRUMENT_OF_LAW: u32 = 110880;

fn status(f: &mut Fixture, game_id: u32) -> SpellStatus {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).status(&ctx)
}

/// Casts `game_id` now, which must be castable.
fn cast(f: &mut Fixture, game_id: u32) -> CastReport {
    assert_eq!(status(f, game_id), SpellStatus::Available, "{game_id}");
    let id = f.spell_id(game_id);
    f.ctx().cast(id)
}

/// Waits out the global cooldown of the last cast.
fn after_gcd(f: &mut Fixture) {
    let t = now(f) + 1.5;
    f.advance_to(t);
}

/// The damage of the spells called `name` (any rank) in `stats`.
fn damage_named(stats: &ClassStatistics, name: &str) -> u64 {
    stats
        .spells()
        .filter(|(key, _)| key.name == name)
        .map(|(_, spell)| spell.total_damage())
        .sum()
}

/// The cast time of `game_id` now, in seconds.
fn cast_time(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).cast_time(&ctx)
}

fn cooldown_left(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).cooldown_remaining(&ctx)
}

/// Holy Strike: 10 s cooldown; Sacred Arbiter adds 20 % to it.
#[test]
fn holy_strike_and_sacred_arbiter() {
    let mut f = pulled(&[]);
    let plain = cast(&mut f, HOLY_STRIKE).attack.unwrap().damage;
    assert!((cooldown_left(&mut f, HOLY_STRIKE) - 10.0).abs() < 1e-9);
    after_gcd(&mut f);
    assert_eq!(status(&mut f, HOLY_STRIKE), SpellStatus::OnCooldown);

    let mut arbiter = pulled(&[(SACRED_ARBITER, 1)]);
    let more = cast(&mut arbiter, HOLY_STRIKE).attack.unwrap().damage;
    let ratio = f64::from(more) / f64::from(plain);
    assert!((ratio - 1.2).abs() < 0.01, "{more} / {plain}");
}

/// Sacred Arbiter: the strike starts the duration of the caster's judgements on the target
/// again; without the talent it does not.
#[test]
fn sacred_arbiter_refreshes_the_judgements() {
    for (talents, refreshed) in [(&[(SACRED_ARBITER, 1)][..], true), (&[][..], false)] {
        let mut f = pulled(talents);
        let crusader = super::rogue::highest_rank(&f, "Seal of the Crusader");
        f.ctx().cast(crusader);
        after_gcd(&mut f);
        cast(&mut f, JUDGEMENT);
        let buff = {
            let id = f.spell_id(JUDGEMENT_OF_THE_CRUSADER);
            f.character.spells().spell(id).marker_buff().unwrap()
        };
        // Right before the swing after the next one: the swings refresh it too.
        let until_swing = |f: &Fixture| {
            let t = now(f);
            f.character.spells().mh_attack().time_until_next_swing(t)
        };
        let t = now(&f) + until_swing(&f) + 0.01;
        f.advance_to(t);
        let t = t + until_swing(&f) - 0.05;
        f.advance_to(t);
        let before = f.ctx().buff_ref(buff).time_left(t);
        assert!(before < 40.0 - 1.0, "{before}");
        f.ctx().refresh_seal_judgements(SACRED_ARBITER_SPELL);
        let after = f.ctx().buff_ref(buff).time_left(t);
        assert_eq!(
            (after - 40.0).abs() < 1e-9,
            refreshed,
            "{talents:?}: {after}"
        );
    }
}

/// Exorcism and Holy Wrath hit Undead and Demons only (Holy Wrath, an area around the paladin,
/// hits the target); Purifying Power 2/2 takes a third off their cooldowns.
#[test]
fn exorcism_and_holy_wrath_are_for_undead_and_demons() {
    let mut f = pulled(&[]);
    for creature_type in CreatureType::ALL {
        f.target.set_creature_type(creature_type);
        let usable = matches!(creature_type, CreatureType::Undead | CreatureType::Demon);
        for spell in [EXORCISM, HOLY_WRATH] {
            let expected = if usable {
                SpellStatus::Available
            } else {
                SpellStatus::InvalidTarget
            };
            assert_eq!(
                status(&mut f, spell),
                expected,
                "{spell} vs {creature_type:?}"
            );
        }
    }

    for (talents, factor) in [(&[][..], 1.0), (&[(PURIFYING_POWER, 2)][..], 0.67)] {
        let mut f = pulled(talents);
        f.target.set_creature_type(CreatureType::Undead);
        cast(&mut f, EXORCISM);
        assert!((cooldown_left(&mut f, EXORCISM) - 15.0 * factor).abs() < 1e-6);
        after_gcd(&mut f);
        let id = f.spell_id(HOLY_WRATH);
        f.ctx().take_statistics();
        f.ctx().cast(id);
        f.advance_to(now(&f) + 2.0);
        let stats = f.ctx().take_statistics();
        assert!(
            damage_named(&stats, "Holy Wrath") > 0,
            "the 2 s cast hit the target"
        );
        let left = cooldown_left(&mut f, HOLY_WRATH);
        assert!(
            left > 60.0 * factor - 2.01 && left <= 60.0 * factor,
            "{left}"
        );
    }
}

/// Consecration: 8 ticks, one a second, of 12 + 27 holy (the target is among the first 4
/// enemies), 0.095 of the spell damage each; Consecrated Ground 2/2 raises the paladin's holy
/// damage by 10 % while it is up.
#[test]
fn consecration_ticks_and_consecrated_ground() {
    let consecrate = |talents: &[(u32, u32)], spell_damage: u32| {
        let mut f = pulled(talents);
        f.character
            .stats_mut()
            .increase_base_spell_damage(spell_damage);
        f.ctx().take_statistics();
        cast(&mut f, CONSECRATION);
        let t = now(&f);
        f.advance_to(t + 8.5);
        (f.ctx().take_statistics(), f)
    };
    let (stats, _) = consecrate(&[], 0);
    assert_eq!(damage_named(&stats, "Consecration"), 8 * 39);
    let (stats, _) = consecrate(&[], 100);
    let expected = 8.0 * (39.0 + 9.5);
    let dealt = damage_named(&stats, "Consecration") as f64;
    assert!((dealt - expected).abs() <= 8.0, "{dealt} vs {expected}");

    // The bonus holds while the Consecration is up, and holy damage only.
    let mut f = pulled(&[(CONSECRATED_GROUND, 2)]);
    let holy = |f: &mut Fixture| f.ctx().magic_school_damage_mod(MagicSchool::Holy);
    let fire = |f: &mut Fixture| f.ctx().magic_school_damage_mod(MagicSchool::Fire);
    let before = (holy(&mut f), fire(&mut f));
    cast(&mut f, CONSECRATION);
    assert!((holy(&mut f) / before.0 - 1.1).abs() < 1e-9);
    assert_eq!(fire(&mut f), before.1);
    let t = now(&f);
    f.advance_to(t + 8.5);
    assert!(
        (holy(&mut f) - before.0).abs() < 1e-9,
        "over with the Consecration"
    );
    // All 8 ticks, the last one as the Consecration ends included: 39 × 1.1 = 42.9.
    let (stats, _) = consecrate(&[(CONSECRATED_GROUND, 2)], 0);
    assert_eq!(damage_named(&stats, "Consecration"), 8 * 43);
}

/// Hammer of Wrath: only at or below 20 % health, a 1 s cast (instant with Instrument of Law
/// 2/2), on the ranged table: it misses like a melee attack and crits for double, but is never
/// dodged or parried.
#[test]
fn hammer_of_wrath() {
    let mut f = pulled(&[]);
    assert_eq!(
        status(&mut f, HAMMER_OF_WRATH),
        SpellStatus::NotInExecuteRange
    );
    let in_execute = |talents: &[(u32, u32)]| {
        let mut f = pulled(talents);
        let sim = *f.character.sim();
        f.character.set_sim(SimParams {
            target_start_health: 0.2,
            ..sim
        });
        f
    };
    let mut f = in_execute(&[]);
    assert_eq!(cast_time(&mut f, HAMMER_OF_WRATH), 1.0);
    let id = f.spell_id(HAMMER_OF_WRATH);
    let effect = &f.character.spells().spell(id).effects()[0];
    let included = effect.included_outcomes();
    assert!(included.miss && !included.dodge && !included.parry && !included.block);
    let report = f.ctx().trigger_spell(HAMMER_OF_WRATH, None).unwrap();
    let attack = report.attack.unwrap();
    assert!(attack.spell.is_none(), "not on the magic table");
    assert!(attack.damage > 0);

    f.rig_rolls(PhysicalAttackResult::Miss);
    let report = f.ctx().trigger_spell(HAMMER_OF_WRATH, None).unwrap();
    assert_eq!(report.attack.unwrap().damage, 0, "a miss");

    let mut f = in_execute(&[(INSTRUMENT_OF_LAW, 2)]);
    assert_eq!(cast_time(&mut f, HAMMER_OF_WRATH), 0.0);
}

/// Blessing of Might on oneself: +133 attack power for an hour; not castable under another
/// paladin's Greater Blessing of Might, which gives the same.
#[test]
fn blessing_of_might_on_oneself() {
    let mut f = pulled(&[]);
    let ap = f.ctx().melee_ap();
    cast(&mut f, BLESSING_OF_MIGHT);
    assert_eq!(f.ctx().melee_ap(), ap + 133);

    let mut f = pulled(&[]);
    let registry = ExternalBuffDb::load(&data().join("external_buffs.yaml")).unwrap();
    let db = std::mem::take(&mut f.db);
    f.ctx().add_external_buffs(&registry, &db);
    f.db = db;
    f.ctx()
        .set_external_buff_selected("Greater Blessing of Might", true)
        .unwrap();
    assert_eq!(
        status(&mut f, BLESSING_OF_MIGHT),
        SpellStatus::StrongerAuraActive
    );
}

/// The Season of Discovery passives the class learns do nothing: Exorcism stays Exorcism
/// (Exorcist), Blessing of Might costs its full mana (Enhanced Blessings).
#[test]
fn season_of_discovery_passives_do_nothing() {
    let mut f = pulled(&[]);
    let exorcism = f.spell_id(EXORCISM);
    let group = f.character.spells().rank_group("Exorcism").unwrap();
    assert_eq!(group.get_max_available_spell_rank(|_| true), Some(exorcism));
    let id = f.spell_id(BLESSING_OF_MIGHT);
    let cost = {
        let ctx = f.ctx();
        ctx.character.spells().spell(id).resource_cost(&ctx)
    };
    assert_eq!(cost, 130);
    for passive in [415076, 20254, 20361, 435984] {
        let id = f.spell_id(passive);
        let ctx = f.ctx();
        assert_eq!(
            ctx.character.spells().spell(id).requirements_status(&ctx),
            SpellStatus::NotSupported,
            "{passive}"
        );
    }
}
