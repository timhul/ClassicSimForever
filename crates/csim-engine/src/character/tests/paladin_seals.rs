//! The Paladin's seals and Judgement on the shipped data (TASKS.md P.6): one seal at a time,
//! the seals' swing procs (Command at 7 PPM, Righteousness by weapon speed, Fury), Seal of the
//! Crusader's attack speed and power, Judgement casting the active seal's judgement and
//! leaving the seal up, and the judgement talents.

use super::energy::pull;
use super::paladin::{stat, with_talents};
use super::rogue::highest_rank;
use super::*;
use crate::effect::EffectHost;
use crate::ids::ProcId;
use crate::magic_school::MagicSchool;
use crate::proc::ProcSource;
use crate::spell::CastReport;

const JUDGEMENT: u32 = 20271;
const SEAL_OF_COMMAND_STRIKE: u32 = 20424;
const JUDGEMENT_OF_COMMAND: u32 = 20968;
const JUDGEMENT_OF_COMMAND_DAMAGE: u32 = 20966;
const JUDGEMENT_OF_RIGHTEOUSNESS: u32 = 20286;
const JUDGEMENT_OF_THE_CRUSADER: u32 = 20303;
const SEAL_OF_RIGHTEOUSNESS_SWING: u32 = 25713;
const SEAL_OF_FURY_SWING: u32 = 20418;

const SEAL_OF_COMMAND_TALENT: u32 = 105696;
const IMPROVED_JUDGEMENT: u32 = 105705;
const IMPROVED_SEALS: u32 = 105334;
const SANCTIFIED_JUDGEMENT: u32 = 105701;
const IMPROVED_SEAL_OF_FURY: u32 = 110875;
const SWIFT_JUDGEMENT: u32 = 110878;
const SWIFT_JUDGEMENT_SPELL: u32 = 1310994;

/// A Human Paladin with `talents` (and the Seal of Command talent) at the pull against a
/// level 60 target, seeded the same every time, every roll rigged to a hit; the pull's first
/// swing is done.
fn pulled(talents: &[(u32, u32)]) -> Fixture {
    let mut all = vec![(SEAL_OF_COMMAND_TALENT, 1)];
    all.extend_from_slice(talents);
    let mut f = with_talents(&all);
    f.target.set_level(60);
    f.character.set_seed(1);
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    f.advance_to(0.01);
    f
}

fn now(f: &Fixture) -> f64 {
    f.engine.current_time()
}

/// Casts `id` now, which must be castable.
fn cast(f: &mut Fixture, id: SpellId) -> CastReport {
    let status = {
        let ctx = f.ctx();
        ctx.character.spells().spell(id).status(&ctx)
    };
    assert_eq!(status, SpellStatus::Available, "{id:?} at {}", now(f));
    f.ctx().cast(id)
}

/// Casts the highest rank of the seal `name` after the global cooldown of the last cast.
fn seal(f: &mut Fixture, name: &str) -> SpellId {
    let t = now(f) + 1.5;
    f.advance_to(t);
    let id = highest_rank(f, name);
    assert_eq!(cast(f, id).result, SpellResult::Success);
    id
}

fn is_up(f: &mut Fixture, id: SpellId) -> bool {
    let buff = f.character.spells().spell(id).marker_buff().unwrap();
    f.ctx().buff_ref(buff).is_active()
}

fn judge(f: &mut Fixture) -> CastReport {
    let id = f.spell_id(JUDGEMENT);
    cast(f, id)
}

/// The spells a cast triggered, with what they triggered, depth first.
fn triggered(report: &CastReport) -> Vec<(u32, &CastReport)> {
    let mut all = Vec::new();
    for (id, inner) in &report.triggered {
        all.push((*id, inner));
        all.extend(triggered(inner));
    }
    all
}

fn damage_of(report: &CastReport, game_id: u32) -> Option<u32> {
    triggered(report)
        .into_iter()
        .find(|(id, _)| *id == game_id)
        .and_then(|(_, inner)| inner.attack)
        .map(|attack| attack.damage)
}

/// The proc of the seal `id`'s buff.
fn seal_proc(f: &Fixture, id: SpellId) -> ProcId {
    f.character
        .spells()
        .buff_proc(id)
        .expect("the seal's buff is a proc aura")
}

/// The reports of the procs one landed main-hand swing fires.
fn swing(f: &mut Fixture) -> Vec<CastReport> {
    f.ctx()
        .run_proc_checks(&[ProcSource::MainhandSwing])
        .into_iter()
        .map(|(_, report)| report)
        .collect()
}

fn mana(f: &Fixture) -> u32 {
    f.character
        .resource_level(ResourceType::Mana, f.engine.current_time())
}

/// Only one seal is up: casting another ends it; casting the same one again refreshes it.
#[test]
fn one_seal_at_a_time() {
    let mut f = pulled(&[]);
    let command = seal(&mut f, "Seal of Command");
    assert!(is_up(&mut f, command));
    let righteousness = seal(&mut f, "Seal of Righteousness");
    assert!(is_up(&mut f, righteousness));
    assert!(
        !is_up(&mut f, command),
        "Seal of Righteousness ended Command"
    );
    let crusader = seal(&mut f, "Seal of the Crusader");
    assert!(!is_up(&mut f, righteousness));
    let cast_at = now(&f);
    seal(&mut f, "Seal of the Crusader");
    let buff = f.character.spells().spell(crusader).marker_buff().unwrap();
    let t = now(&f);
    let left = f.ctx().buff_ref(buff).time_left(t);
    assert!(
        (left - 30.0).abs() < 1e-9,
        "refreshed, not {left} after {cast_at}"
    );
}

/// Judgement needs a seal, casts the seal's judgement, leaves the seal up and costs 6 % of
/// the base mana, with a 10 s cooldown.
#[test]
fn judgement_unleashes_the_seal_and_keeps_it() {
    let mut f = pulled(&[]);
    assert_ne!(f.status(JUDGEMENT), SpellStatus::Available, "no seal");
    let righteousness = seal(&mut f, "Seal of Righteousness");
    let before = mana(&f);
    let report = judge(&mut f);
    assert_eq!(report.result, SpellResult::Success);
    assert!(damage_of(&report, JUDGEMENT_OF_RIGHTEOUSNESS).is_some_and(|d| d > 0));
    assert_eq!(before - mana(&f), 90);
    assert!(is_up(&mut f, righteousness), "the seal stays");
    assert_eq!(f.status(JUDGEMENT), SpellStatus::OnCooldown);
    let t = now(&f) + 9.9;
    f.advance_to(t);
    assert_eq!(f.status(JUDGEMENT), SpellStatus::OnCooldown);
    let t = now(&f) + 0.2;
    f.advance_to(t);
    assert_eq!(f.status(JUDGEMENT), SpellStatus::Available);
}

/// Seal of Command: the landed white swings proc its strike at 7 PPM (42 % with the 3.6 s
/// axe), not more often than once a second, and only while the seal is up.
#[test]
fn seal_of_command_procs_at_7_ppm_with_a_1_second_cooldown() {
    let mut f = pulled(&[]);
    let command = highest_rank(&f, "Seal of Command");
    let proc = seal_proc(&f, command);
    let range = {
        let ctx = f.ctx();
        ctx.character
            .spells()
            .procs()
            .get(proc)
            .proc_range(ProcSource::MainhandSwing, &ctx)
    };
    assert_eq!(range, 4200);
    assert!(swing(&mut f).is_empty(), "no seal, no strike");

    seal(&mut f, "Seal of Command");
    let first = (0..100).position(|_| !swing(&mut f).is_empty());
    assert!(first.is_some(), "a strike within 100 swings");
    let at_once: usize = (0..50).map(|_| swing(&mut f).len()).sum();
    assert_eq!(at_once, 0, "the 1 s proc cooldown");
    let t = now(&f) + 1.01;
    f.advance_to(t);
    assert!(
        (0..100).any(|_| !swing(&mut f).is_empty()),
        "after the cooldown"
    );
    let procs = f.character.spells().procs().get(proc).procs();
    assert_eq!(procs, 2);
}

/// A Seal of Command strike: 70 % of the weapon damage, holy.
#[test]
fn seal_of_command_strike_is_the_payload() {
    let mut f = pulled(&[]);
    seal(&mut f, "Seal of Command");
    let report = (0..100)
        .find_map(|_| swing(&mut f).into_iter().next())
        .expect("a strike within 100 swings");
    assert!(damage_of(&report, SEAL_OF_COMMAND_STRIKE).is_some_and(|d| d > 0));
}

/// Judgement of Command: half of its damage spell's damage (a raid boss is never stunned).
#[test]
fn judgement_of_command_deals_half_damage() {
    let judged = {
        let mut f = pulled(&[]);
        seal(&mut f, "Seal of Command");
        let report = judge(&mut f);
        assert!(
            triggered(&report)
                .iter()
                .any(|(id, _)| *id == JUDGEMENT_OF_COMMAND)
        );
        damage_of(&report, JUDGEMENT_OF_COMMAND_DAMAGE).expect("the damage spell")
    };
    let direct = {
        let mut f = pulled(&[]);
        seal(&mut f, "Seal of Command");
        f.ctx()
            .trigger_spell(JUDGEMENT_OF_COMMAND_DAMAGE, None)
            .and_then(|report| report.attack)
            .unwrap()
            .damage
    };
    assert!(
        (i64::from(judged) * 2 - i64::from(direct)).abs() <= 1,
        "{judged} is half of {direct}"
    );
}

/// Seal of Righteousness: every landed white swing deals holy damage by the weapon speed:
/// 1880 / 87 at 1.5 s to 1880 / 25 at 4.0 s, 66.6 with the 3.6 s axe; no armor.
#[test]
fn seal_of_righteousness_deals_damage_by_weapon_speed() {
    assert!((crate::proc::swing_damage_by_speed(1880.0, 1.5) - 21.609).abs() < 0.001);
    assert!((crate::proc::swing_damage_by_speed(1880.0, 4.0) - 75.2).abs() < 1e-9);
    assert!((crate::proc::swing_damage_by_speed(1880.0, 3.5) - 64.48).abs() < 0.01);
    let mut f = pulled(&[]);
    f.target.set_base_armor(20_000);
    seal(&mut f, "Seal of Righteousness");
    for _ in 0..5 {
        let reports = swing(&mut f);
        assert_eq!(reports.len(), 1, "every swing");
        let damage = damage_of(&reports[0], SEAL_OF_RIGHTEOUSNESS_SWING).unwrap();
        assert_eq!(damage, 67, "66.6 rounded, through no armor");
    }
}

/// Seal of Fury: every landed white swing deals 35 holy damage (the shield is not simulated).
#[test]
fn seal_of_fury_deals_holy_damage_each_swing() {
    let mut f = pulled(&[]);
    seal(&mut f, "Seal of Fury");
    let reports = swing(&mut f);
    assert_eq!(reports.len(), 1);
    assert_eq!(damage_of(&reports[0], SEAL_OF_FURY_SWING), Some(35));
}

/// Seal of the Crusader: +325 attack power (306 + 2.4 per level past 52), 40 % faster main-hand
/// swings that each deal 100 / 140 of the damage. Its judgement: +161 holy damage taken for
/// 40 s, which the paladin's melee strikes refresh.
#[test]
fn seal_of_the_crusader_and_its_judgement() {
    let mut f = pulled(&[]);
    let ap = |f: &mut Fixture| f.ctx().melee_ap();
    let speed = |f: &Fixture| f.character.stats().get_melee_attack_speed_mod();
    let before = (ap(&mut f), speed(&f));
    let weapon = |f: &mut Fixture| {
        f.character.set_seed(7);
        f.ctx().random_non_normalized_mh_dmg()
    };
    let plain = weapon(&mut f);
    seal(&mut f, "Seal of the Crusader");
    assert_eq!(ap(&mut f), before.0 + 325);
    assert!((speed(&f) / before.1 - 1.4).abs() < 1e-9);
    let hit = weapon(&mut f);
    let ap_part = |ap: u32| f64::from(ap) / 14.0 * 3.6;
    let crusader_ap = ap(&mut f);
    // The same roll: the weapon part with the new attack power, times 100 / 140.
    let expected = (plain - ap_part(before.0) + ap_part(crusader_ap)) * 100.0 / 140.0;
    assert!((hit - expected).abs() < 1e-6, "{hit} vs {expected}");

    judge(&mut f);
    let holy = |f: &Fixture| stat(f, |s, ctx| s.get_spell_damage(ctx, MagicSchool::Holy));
    assert_eq!(holy(&f), 161);
    // The swings keep starting the 40 s again: still up long after, never far from 40 s left.
    let judged_at = now(&f);
    let buff = {
        let id = f.spell_id(JUDGEMENT_OF_THE_CRUSADER);
        f.character.spells().spell(id).marker_buff().unwrap()
    };
    for t in [20.0, 45.0, 90.0] {
        f.advance_to(judged_at + t);
        let left = f.ctx().buff_ref(buff).time_left(judged_at + t);
        assert!(left > 40.0 - 3.6, "{left} s left at {t} s");
    }
    assert_eq!(holy(&f), 161);
}

/// Improved Judgement 2/2: Judgement's cooldown is 8 s.
#[test]
fn improved_judgement_shortens_the_cooldown() {
    let mut f = pulled(&[(IMPROVED_JUDGEMENT, 2)]);
    seal(&mut f, "Seal of Righteousness");
    judge(&mut f);
    let t = now(&f) + 7.9;
    f.advance_to(t);
    assert_eq!(f.status(JUDGEMENT), SpellStatus::OnCooldown);
    let t = now(&f) + 0.2;
    f.advance_to(t);
    assert_eq!(f.status(JUDGEMENT), SpellStatus::Available);
}

/// Swift Judgement finishes Judgement's cooldown, and the next Judgement is free.
#[test]
fn swift_judgement_resets_the_judgement() {
    let mut f = pulled(&[(IMPROVED_SEAL_OF_FURY, 1), (SWIFT_JUDGEMENT, 1)]);
    seal(&mut f, "Seal of Righteousness");
    judge(&mut f);
    assert_eq!(f.status(JUDGEMENT), SpellStatus::OnCooldown);
    let swift = f.spell_id(SWIFT_JUDGEMENT_SPELL);
    cast(&mut f, swift);
    assert_eq!(f.status(JUDGEMENT), SpellStatus::Available);
    let before = mana(&f);
    judge(&mut f);
    assert_eq!(mana(&f), before, "free");
}

/// Sanctified Judgement 3/3: every Judgement returns 60 % of the judged seal's mana cost
/// (Seal of Righteousness: 200, so 120).
#[test]
fn sanctified_judgement_returns_the_seals_mana() {
    let mut f = pulled(&[(SANCTIFIED_JUDGEMENT, 3)]);
    let righteousness = seal(&mut f, "Seal of Righteousness");
    let seal_cost = {
        let ctx = f.ctx();
        ctx.character
            .spells()
            .spell(righteousness)
            .resource_cost(&ctx)
    };
    assert_eq!(seal_cost, 200);
    let before = mana(&f);
    let report = judge(&mut f);
    assert_eq!(mana(&f), before - 90 + 120);
    assert_eq!(report.resource_gained, [(ResourceType::Mana, 120)]);
}

/// Improved Seals 3/3: the seals' and judgements' damage is 15 % higher.
#[test]
fn improved_seals_raise_seal_and_judgement_damage() {
    let damage = |talents: &[(u32, u32)]| {
        let mut f = pulled(talents);
        seal(&mut f, "Seal of Righteousness");
        let swing = damage_of(&swing(&mut f)[0], SEAL_OF_RIGHTEOUSNESS_SWING).unwrap();
        let judged = damage_of(&judge(&mut f), JUDGEMENT_OF_RIGHTEOUSNESS).unwrap();
        (f64::from(swing), f64::from(judged))
    };
    let (swing, judged) = damage(&[]);
    let (improved_swing, improved_judged) = damage(&[(IMPROVED_SEALS, 3)]);
    assert!(
        (improved_swing / swing - 1.15).abs() < 0.02,
        "{improved_swing} / {swing}"
    );
    assert!(
        (improved_judged / judged - 1.15).abs() < 0.01,
        "{improved_judged} / {judged}"
    );
}

/// Seal of Wisdom: every landed melee strike, white or special, restores 90 mana (rank 3).
#[test]
fn seal_of_wisdom_restores_mana_on_each_strike() {
    let mut f = pulled(&[]);
    seal(&mut f, "Seal of Wisdom");
    let t = now(&f);
    f.character.resource_mut().lose(500, t);
    for source in [ProcSource::MainhandSwing, ProcSource::MainhandSpell] {
        let before = mana(&f);
        let reports = f.ctx().run_proc_checks(&[source]);
        assert_eq!(reports.len(), 1, "{source:?}");
        assert_eq!(mana(&f), before + 90, "{source:?}");
    }
}
