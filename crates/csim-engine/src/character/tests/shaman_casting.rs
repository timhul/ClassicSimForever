//! A pure caster: an Elemental Shaman's rotation (`attack_mode: magic`) never swings, chains its
//! casts, shortens them with haste and talents, spends mana under the five-second rule and
//! crits for the Elemental Fury bonus.

use super::energy::pull;
use super::paladin::stat;
use super::shaman::with_talents;
use super::*;
use crate::spell::CastReport;
use crate::target::CreatureType;

const LIGHTNING_BOLT: u32 = 15208;

const CONVECTION: u32 = 104773;
const CONCUSSION: u32 = 104772;
const ELEMENTAL_ALACRITY: u32 = 104765;
const CALL_OF_THUNDER: u32 = 104762;
const ELEMENTAL_FURY: u32 = 104766;
const MINDFULNESS: u32 = 104734;

/// A Shaman with `talents` forced in on a magic rotation of `cast_if` lines, seeded the same
/// every time.
fn caster(talents: &[(u32, u32)], cast_if: &str) -> Fixture {
    let mut f = with_talents(talents);
    f.character.set_seed(1);
    let spec = format!("class: SHAMAN\nname: Caster\nattack_mode: magic\ncast_if:\n{cast_if}");
    f.ctx()
        .set_rotation(Arc::new(serde_yaml::from_str(&spec).unwrap()));
    f.ctx().prepare_set_of_combat_iterations();
    f
}

fn bolter(talents: &[(u32, u32)]) -> Fixture {
    caster(talents, "  - name: Lightning Bolt\n")
}

/// Every event up to `until`, with its time, that the character handled.
fn timed(f: &mut Fixture, until: f64) -> Vec<(f64, EventKind)> {
    f.engine
        .add_event(Event::new(until, EventKind::EncounterEnd));
    let mut handled = Vec::new();
    while let Some(event) = f.engine.next_event() {
        if event.kind == EventKind::EncounterEnd {
            break;
        }
        if f.ctx().handle_event(&event) {
            handled.push((event.time, event.kind));
        }
    }
    handled
}

fn cast_ends(events: &[(f64, EventKind)]) -> Vec<f64> {
    events
        .iter()
        .filter(|(_, kind)| matches!(kind, EventKind::CastComplete { .. }))
        .map(|(time, _)| *time)
        .collect()
}

/// Casts end every `period` seconds from the pull.
fn assert_every(ends: &[f64], period: f64) {
    for (index, end) in ends.iter().enumerate() {
        let expected = period * (index + 1) as f64;
        assert!((end - expected).abs() < 1e-6, "{period}: {ends:?}");
    }
}

fn mana(f: &Fixture) -> u32 {
    f.character.resource_level(ResourceType::Mana, 0.0)
}

fn regenerated(f: &Fixture) -> f64 {
    f.character
        .statistics
        .resource_statistics(crate::character::context::REGENERATION, 1)
        .map_or(0.0, |s| s.gain(ResourceType::Mana))
}

/// Four 2.5 s Lightning Bolts in 10 s, each started as the last ends (the spell queue), and no
/// swing: a magic rotation never starts the auto attack.
#[test]
fn a_caster_chains_its_casts_without_swinging() {
    let mut f = bolter(&[]);
    pull(&mut f);
    let events = timed(&mut f, 10.01);
    assert!(
        !events.iter().any(|(_, kind)| matches!(
            kind,
            EventKind::MainhandMeleeHit { .. } | EventKind::OffhandMeleeHit { .. }
        )),
        "no swings"
    );
    assert!(!f.character.spells().is_melee_attacking());
    let ends = cast_ends(&events);
    assert_eq!(ends.len(), 4, "{ends:?}");
    assert_every(&ends, 2.5);
    let bolts = f
        .character
        .statistics
        .spell_statistics("Lightning Bolt", 10)
        .unwrap();
    assert_eq!(bolts.total_attempts(), 4);
}

/// 25 % casting speed: 2.0 s bolts. Elemental Alacrity 3/3: 0.5 s off, 2.0 s bolts.
#[test]
fn haste_and_flat_reductions_shorten_the_cast() {
    let mut hasted = bolter(&[]);
    hasted.character.stats_mut().increase_casting_speed_mod(25);
    pull(&mut hasted);
    assert_every(&cast_ends(&timed(&mut hasted, 8.01)), 2.0);

    let mut quick = bolter(&[(CONVECTION, 5), (CONCUSSION, 5), (ELEMENTAL_ALACRITY, 3)]);
    pull(&mut quick);
    let ends = cast_ends(&timed(&mut quick, 8.01));
    assert_eq!(ends.len(), 4, "{ends:?}");
    assert_every(&ends, 2.0);
}

/// Casting without pause keeps the five-second rule up: spirit regenerates nothing, and with
/// Mindfulness 3/3 half of it (17 + spirit / 5 mana per 5 s, 2 s ticks).
#[test]
fn constant_casting_regenerates_only_through_mindfulness() {
    let mut f = bolter(&[]);
    pull(&mut f);
    f.advance_to(20.0);
    assert_eq!(regenerated(&f), 0.0);

    let mut mindful = bolter(&[(MINDFULNESS, 3)]);
    pull(&mut mindful);
    let spirit = stat(&mindful, |s, ctx| s.get_spirit(ctx));
    // The ticks at 4 .. 20 s; at 2 s the mana is still full (the first bolt spends at 2.5 s).
    mindful.advance_to(20.01);
    let per_tick = (17.0 + f64::from(spirit) / 5.0) / 5.0 * 2.0;
    let expected = 9.0 * per_tick * 0.5;
    assert!(
        (regenerated(&mindful) - expected).abs() <= 2.0,
        "{} vs {expected}",
        regenerated(&mindful)
    );
}

/// A caster out of mana waits: it casts again once spirit regeneration (back five seconds
/// after the last cast) pays for a bolt, rather than stalling for the rest of the fight.
#[test]
fn a_caster_out_of_mana_casts_again_when_it_can() {
    let mut f = bolter(&[]);
    pull(&mut f);
    let full = mana(&f);
    let events = timed(&mut f, 180.0);
    let ends = cast_ends(&events);
    let affordable = (full / 220) as usize;
    assert!(ends.len() > affordable + 2, "{} casts", ends.len());
    let dry = ends[affordable - 1];
    assert!(
        ends[affordable] - dry > 5.0,
        "a pause after running dry at {dry}: {ends:?}"
    );
    assert!(regenerated(&f) > 440.0, "{}", regenerated(&f));
}

/// Elemental Fury 5/5 doubles the crit bonus: a Lightning Bolt crit deals 200 % instead of 150 %.
#[test]
fn elemental_fury_doubles_the_spell_crit_bonus() {
    let damage = |talents: &[(u32, u32)], crit: bool| {
        let mut f = with_talents(talents);
        f.target.set_level(60);
        f.target.set_creature_type(CreatureType::Undead);
        if crit {
            f.character.stats_mut().increase_spell_crit(10_000);
        }
        f.character.set_seed(1);
        pull(&mut f);
        f.character
            .roll_mut()
            .random_mut()
            .set_new_range(5000, 5001);
        let report: CastReport = f
            .ctx()
            .trigger_spell(LIGHTNING_BOLT, None)
            .expect("Lightning Bolt is learned");
        let attack = report.attack.expect("the bolt dealt damage");
        assert_eq!(
            attack.result == PhysicalAttackResult::Critical,
            crit,
            "{talents:?}"
        );
        attack.damage
    };
    let fury = [
        (CONVECTION, 5),
        (CONCUSSION, 5),
        (ELEMENTAL_ALACRITY, 3),
        (CALL_OF_THUNDER, 1),
        (ELEMENTAL_FURY, 5),
    ];
    for (talents, multiplier) in [(&fury[..0], 1.5), (&fury[..], 2.0)] {
        let ratio = f64::from(damage(talents, true)) / f64::from(damage(talents, false));
        assert!((ratio - multiplier).abs() < 0.01, "{talents:?}: {ratio}");
    }
}

/// A caster's report: its spells with the magic resist breakdown, no auto attack rows.
#[test]
fn a_caster_reports_its_spells_by_resist() {
    let mut f = bolter(&[]);
    f.character.set_seed(7);
    pull(&mut f);
    f.advance_to(25.01);
    let rows = crate::statistics::report::spell_rows(&f.character.statistics, 1, 25.01);
    let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["Lightning Bolt (rank 10)"], "{names:?}");
    let bolt = &rows[0];
    assert_eq!(bolt.casts, 10.0);
    let parts: Vec<_> = bolt.breakdown.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(
        parts,
        [
            "100% resisted",
            "75% resisted",
            "50% resisted",
            "25% resisted",
            "0% resisted"
        ]
    );
}

/// Queued spells follow each other without a gap: an instant's global cooldown, a cast's end
/// and a cooldown's end each start the next spell at once.
#[test]
fn queued_spells_follow_each_other_at_once() {
    let mut f = caster(
        &[],
        "  - name: Flame Shock
    condition: buff_duration \"Flame Shock\" less 1
  - name: Lightning Bolt
",
    );
    pull(&mut f);
    // Flame Shock at 0 (its GCD to 1.5), bolts from 1.5; Flame Shock again at 11.5 (0.5 s
    // left), the next bolt as its GCD ends at 13.
    let ends = cast_ends(&timed(&mut f, 16.0));
    assert_eq!(ends, [4.0, 6.5, 9.0, 11.5, 15.5]);

    let mut f = caster(
        &[],
        "  - name: Earth Shock
",
    );
    pull(&mut f);
    timed(&mut f, 18.01);
    let shocks = f
        .character
        .statistics
        .spell_statistics("Earth Shock", 7)
        .unwrap();
    assert_eq!(shocks.total_attempts(), 4, "at 0, 6, 12 and 18");
}
