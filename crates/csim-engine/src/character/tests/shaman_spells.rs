//! The Shaman's spells on the shipped data (TASKS.md SH.5): Lightning Bolt, Chain Lightning,
//! the shocks and their shared cooldown, Lava Burst and its Flame Shock bonus, Stormstrike and
//! its debuff, and the swing a Lightning Bolt weave restarts.
//!
//! The damage tests compare two Shamans built and pulled the same way: their rolls and damage
//! ranges are the same, so the difference of one cast is the formula's term alone.

use super::energy::pull;
use super::paladin::stat;
use super::shaman::with_talents;
use super::*;
use crate::combat_roll::MagicResistResult;
use crate::magic_school::MagicSchool;
use crate::spell::CastReport;

const LIGHTNING_BOLT: u32 = 15208;
const CHAIN_LIGHTNING: u32 = 10605;
const EARTH_SHOCK: u32 = 10414;
const FLAME_SHOCK: u32 = 29228;
const FROST_SHOCK: u32 = 10473;
const LAVA_BURST: u32 = 1238300;
const STORMSTRIKE: u32 = 17364;

const REVERBERATION: u32 = 104767;
const LIGHTNING_OVERLOAD: u32 = 104759;
const LAVA_BURST_TALENT: u32 = 104758;
const STORMSTRIKE_TALENT: u32 = 104743;

/// Lava Burst needs Lightning Overload; its copies are casts of their own, apart from the
/// reports compared here.
const TALENTS: [(u32, u32); 3] = [
    (LIGHTNING_OVERLOAD, 3),
    (LAVA_BURST_TALENT, 1),
    (STORMSTRIKE_TALENT, 1),
];

/// An Orc Shaman with Lava Burst and Stormstrike, `talents` forced in, against a level 60
/// target without armor (no resistance from levels), `prepare`d, seeded the same every time and
/// every roll rigged to a hit.
fn pulled(talents: &[(u32, u32)], prepare: impl FnOnce(&mut Fixture)) -> Fixture {
    let mut all = TALENTS.to_vec();
    all.extend_from_slice(talents);
    let mut f = with_talents(&all);
    f.target.set_level(60);
    f.target.set_base_armor(0);
    prepare(&mut f);
    f.character.set_seed(1);
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    f.advance_to(0.01);
    f
}

fn now(f: &Fixture) -> f64 {
    f.engine.current_time()
}

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

/// Casts `game_id` as a trigger (no cost, cast time, cooldown or global cooldown).
fn trigger(f: &mut Fixture, game_id: u32) -> CastReport {
    f.ctx()
        .trigger_spell(game_id, None)
        .unwrap_or_else(|| panic!("{game_id} is learned"))
}

fn damage_of(report: CastReport) -> u32 {
    report.attack.expect("the spell dealt damage").damage
}

/// The damage of one triggered `game_id` by a Shaman `prepare`d.
fn damage(game_id: u32, prepare: impl FnOnce(&mut Fixture)) -> u32 {
    let mut f = pulled(&[], prepare);
    damage_of(trigger(&mut f, game_id))
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

fn difference(game_id: u32, prepare: impl FnOnce(&mut Fixture)) -> i64 {
    i64::from(damage(game_id, prepare)) - i64::from(damage(game_id, nothing))
}

fn cost(f: &mut Fixture, game_id: u32) -> u32 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).resource_cost(&ctx)
}

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

/// The spell power coefficients: 0.714 Lightning Bolt and Lava Burst, 0.571 Chain Lightning,
/// 0.386 Earth and Frost Shock, 0.214 Flame Shock's hit.
#[test]
fn spell_power_coefficients() {
    for (game_id, coefficient) in [
        (LIGHTNING_BOLT, 0.714),
        (LAVA_BURST, 0.714),
        (CHAIN_LIGHTNING, 0.571),
        (EARTH_SHOCK, 0.386),
        (FROST_SHOCK, 0.386),
        (FLAME_SHOCK, 0.214),
    ] {
        close_to(
            difference(game_id, spell_damage(1000)),
            1000.0 * coefficient,
        );
    }
}

/// The totems' attacks and Fire Nova take the tables' coefficients (TASKS.md SH.0): 0.017
/// Searing Totem, 0.033 Magma Totem, 0.143 Fire Nova rank 5.
#[test]
fn totem_and_fire_nova_coefficients() {
    for (game_id, coefficient) in [(10436, 0.017), (10581, 0.033), (11307, 0.143)] {
        close_to(
            difference(game_id, spell_damage(1000)),
            1000.0 * coefficient,
        );
    }
}

/// The level 60 damage ranges of the highest ranks, without spell power: Forever's numbers,
/// about half of 1.12's (Lightning Bolt 196 + 1.2 per level above 56, a range 10.8 % wide).
#[test]
fn base_damage_ranges() {
    for (game_id, base, variance) in [
        (LIGHTNING_BOLT, 196.0_f64 + 4.0 * 1.2, 0.108_352_15),
        (CHAIN_LIGHTNING, 123.0 + 4.0 * 0.8, 0.111_111_11),
        (EARTH_SHOCK, 301.0, 0.052_730_7),
        (LAVA_BURST, 220.0, 0.253_258_85),
        (FROST_SHOCK, 283.0 + 2.0 * 1.8, 0.056),
        (FLAME_SHOCK, 166.0, 0.0),
    ] {
        let mut f = pulled(&[], nothing);
        // The tables' variance is the width of the range.
        let (low, high) = (base * (1.0 - variance / 2.0), base * (1.0 + variance / 2.0));
        let mut seen = (u32::MAX, 0);
        // Unrigged rolls (the rigged ones fix the damage roll too); the plain hits only.
        f.character.set_seed(3);
        for _ in 0..1000 {
            let attack = trigger(&mut f, game_id).attack.unwrap();
            let resisted = attack
                .spell
                .is_some_and(|spell| spell.roll.resist != MagicResistResult::NoResist);
            if attack.result == PhysicalAttackResult::Hit && !resisted {
                seen = (seen.0.min(attack.damage), seen.1.max(attack.damage));
            }
        }
        assert!(
            f64::from(seen.0) >= low.floor() && f64::from(seen.1) <= high.ceil(),
            "{game_id}: {seen:?} outside {low}..{high}"
        );
        assert!(
            f64::from(seen.1 - seen.0) >= (high - low) * 0.8,
            "{game_id}: {seen:?} covers {low}..{high}"
        );
    }
}

/// Mana costs, cast times and cooldowns of the tables.
#[test]
fn costs_cast_times_and_cooldowns() {
    let mut f = pulled(&[], nothing);
    for (game_id, mana, seconds) in [
        (LIGHTNING_BOLT, 220, 2.5),
        (CHAIN_LIGHTNING, 485, 2.0),
        (LAVA_BURST, 265, 2.5),
        (EARTH_SHOCK, 450, 0.0),
        (FLAME_SHOCK, 410, 0.0),
        (FROST_SHOCK, 430, 0.0),
        (STORMSTRIKE, 125, 0.0),
    ] {
        assert_eq!(cost(&mut f, game_id), mana, "{game_id}");
        assert_eq!(cast_time(&mut f, game_id), seconds, "{game_id}");
    }
    let school = |f: &mut Fixture, game_id| {
        let id = f.spell_id(game_id);
        f.character.spells().spell(id).effects()[0].school()
    };
    assert_eq!(school(&mut f, LIGHTNING_BOLT), MagicSchool::Nature);
    assert_eq!(school(&mut f, LAVA_BURST), MagicSchool::Fire);
    assert_eq!(school(&mut f, FROST_SHOCK), MagicSchool::Frost);
    assert_eq!(school(&mut f, STORMSTRIKE), MagicSchool::Physical);

    cast(&mut f, STORMSTRIKE);
    assert_eq!(cooldown_left(&mut f, STORMSTRIKE), 8.0);
    let t = now(&f) + 1.5;
    f.advance_to(t);
    cast(&mut f, LAVA_BURST);
    let t = now(&f) + 2.5;
    f.advance_to(t);
    assert!((cooldown_left(&mut f, LAVA_BURST) - 7.5).abs() < 1e-6);
    cast(&mut f, CHAIN_LIGHTNING);
    let t = now(&f) + 2.0;
    f.advance_to(t);
    assert!((cooldown_left(&mut f, CHAIN_LIGHTNING) - 4.0).abs() < 1e-6);
}

/// The shocks share one 6 s cooldown; Reverberation 5/5 takes 1 s off it.
#[test]
fn the_shocks_share_their_cooldown() {
    for (talents, lockout) in [(&[][..], 6.0), (&[(REVERBERATION, 5)][..], 5.0)] {
        let mut f = pulled(talents, nothing);
        cast(&mut f, EARTH_SHOCK);
        let t = now(&f) + 1.5;
        f.advance_to(t);
        for shock in [EARTH_SHOCK, FLAME_SHOCK, FROST_SHOCK] {
            assert_eq!(status(&mut f, shock), SpellStatus::OnCooldown, "{shock}");
            assert!(
                (cooldown_left(&mut f, shock) - (lockout - 1.5)).abs() < 1e-6,
                "{shock}: {}",
                cooldown_left(&mut f, shock)
            );
        }
        let t = now(&f) + lockout - 1.5;
        f.advance_to(t);
        assert_eq!(status(&mut f, FROST_SHOCK), SpellStatus::Available);
    }
}

/// Flame Shock: the hit, then 44 fire every 3 s for 12 s, each tick with 0.1 of spell power.
#[test]
fn flame_shock_burns_for_twelve_seconds() {
    let ticks = |power| {
        let mut f = pulled(&[], spell_damage(power));
        trigger(&mut f, FLAME_SHOCK);
        f.advance_to(12.5);
        let stats = &f.character.statistics;
        stats
            .spells()
            .filter(|(key, _)| key.name == "Flame Shock")
            .map(|(_, spell)| spell.total_damage())
            .sum::<u64>()
            - u64::from(166 + power * 214 / 1000)
    };
    let plain = ticks(0);
    assert_eq!(plain, 4 * 44);
    close_to(ticks(1000) as i64 - plain as i64, 4.0 * 100.0);
}

/// Casts `game_id` as soon as it is castable and lets its cast time pass.
fn cast_when_ready(f: &mut Fixture, game_id: u32) {
    while status(f, game_id) != SpellStatus::Available {
        let t = now(f) + 0.1;
        f.advance_to(t);
    }
    let seconds = cast_time(f, game_id);
    cast(f, game_id);
    let t = now(f) + seconds + 0.01;
    f.advance_to(t);
}

/// The damage multiplier of `game_id` now (the comparisons of two casts would differ by the
/// rolls the cast between them used).
fn damage_mod(f: &mut Fixture, game_id: u32) -> f64 {
    let id = f.spell_id(game_id);
    let ctx = f.ctx();
    ctx.character.spells().spell(id).damage_mod(&ctx)
}

/// Lava Burst deals 20 % more while the Shaman's Flame Shock is on the target, which stays;
/// another shock does not count.
#[test]
fn lava_burst_with_flame_shock() {
    let mut f = pulled(&[], nothing);
    assert_eq!(damage_mod(&mut f, LAVA_BURST), 1.0);
    trigger(&mut f, FROST_SHOCK);
    assert_eq!(damage_mod(&mut f, LAVA_BURST), 1.0);
    trigger(&mut f, FLAME_SHOCK);
    assert!((damage_mod(&mut f, LAVA_BURST) - 1.2).abs() < 1e-9);
    assert_eq!(damage_mod(&mut f, LIGHTNING_BOLT), 1.0);
    trigger(&mut f, LAVA_BURST);
    assert!((damage_mod(&mut f, LAVA_BURST) - 1.2).abs() < 1e-9);
    f.advance_to(12.5);
    assert_eq!(damage_mod(&mut f, LAVA_BURST), 1.0, "Flame Shock ran out");
}

/// Stormstrike: a normalized weapon strike (2.4 s for a one-hander) that makes the next
/// Lightning Bolt, Chain Lightning or Earth Shock deal 20 % more; one charge.
#[test]
fn stormstrike_empowers_the_next_lightning_or_earth_shock() {
    let strike = |power| {
        let mut f = pulled(&[], |f| f.character.stats_mut().increase_melee_ap(power));
        damage_of(trigger(&mut f, STORMSTRIKE))
    };
    close_to(
        i64::from(strike(1400)) - i64::from(strike(0)),
        1400.0 / 14.0 * 2.4,
    );

    // Real casts: only those use charges (a triggered spell does not).
    let empowered = [LIGHTNING_BOLT, CHAIN_LIGHTNING, EARTH_SHOCK];
    for user in empowered {
        let mut f = pulled(&[], nothing);
        trigger(&mut f, STORMSTRIKE);
        for game_id in empowered {
            assert!(
                (damage_mod(&mut f, game_id) - 1.2).abs() < 1e-9,
                "{game_id}"
            );
        }
        for game_id in [FROST_SHOCK, LAVA_BURST] {
            cast_when_ready(&mut f, game_id);
            assert_eq!(damage_mod(&mut f, game_id), 1.0, "{game_id}");
        }
        assert!(
            (damage_mod(&mut f, user) - 1.2).abs() < 1e-9,
            "kept by the others"
        );
        cast_when_ready(&mut f, user);
        for game_id in empowered {
            assert_eq!(damage_mod(&mut f, game_id), 1.0, "{user} used the charge");
        }
    }
    let mut f = pulled(&[], nothing);
    trigger(&mut f, STORMSTRIKE);
    f.advance_to(12.5);
    assert_eq!(damage_mod(&mut f, LIGHTNING_BOLT), 1.0, "12 s");
}

/// Lightning Bolt and Lava Burst are nature and fire spells on the magic table: spell hit and
/// crit, not the melee ones.
#[test]
fn spells_roll_the_magic_table() {
    let f = pulled(&[], nothing);
    let nature = stat(&f, |s, ctx| {
        s.get_spell_crit_chance(ctx, MagicSchool::Nature)
    });
    assert!(nature > 0);
    let mut f = pulled(&[], nothing);
    let report = trigger(&mut f, LIGHTNING_BOLT);
    assert!(report.attack.unwrap().spell.is_some(), "on the magic table");
}

/// A Lightning Bolt woven between swings pauses them and restarts the swing timer as it
/// completes: the next swing comes a full weapon speed after the cast (TASKS.md SH.0 Q7).
#[test]
fn a_lightning_bolt_weave_restarts_the_swing() {
    let mut f = pulled(&[], nothing);
    f.ctx().start_attack();
    let swings = |f: &mut Fixture, until: f64| {
        f.engine
            .add_event(Event::new(until, EventKind::EncounterEnd));
        let mut times = Vec::new();
        while let Some(event) = f.engine.next_event() {
            if event.kind == EventKind::EncounterEnd {
                break;
            }
            // A re-timed swing leaves a stale event behind; none lands during a cast.
            let swing = matches!(
                event.kind,
                EventKind::MainhandMeleeHit { iteration, .. }
                    if f.character.spells().mh_attack().attack_is_valid(iteration)
                        && f.character.spells().is_melee_attacking()
            );
            if f.ctx().handle_event(&event) && swing {
                times.push(event.time);
            }
        }
        times
    };
    let before = swings(&mut f, 6.0);
    assert!(before.len() >= 2, "{before:?}");
    let speed = before[1] - before[0];
    let start = now(&f);
    assert!(cast(&mut f, LIGHTNING_BOLT).cast_started);
    let after = swings(&mut f, start + 2.5 + 2.0 * speed);
    let end = start + 2.5;
    assert!(
        (after[0] - (end + speed)).abs() < 1e-6,
        "cast {start}..{end}, speed {speed}: {after:?}"
    );
}
