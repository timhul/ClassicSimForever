//! Energy on the character: the regeneration grid read through the character, the auras that
//! change it (Adrenaline Rush, Vigor), the reactions to gains and the regeneration reactions of
//! the rotation, checked against the per-tick reference mode.

use super::*;
use crate::character::RegenReactions;
use crate::combat_log::{CombatLog, CombatLogEvent};
use crate::effect::EffectHost;
use crate::talent::{CharacterTalents, TalentDb};

const ADRENALINE_RUSH: u32 = 13750;

/// Talent nodes, tier by tier up to Adrenaline Rush (31 points in Combat).
const COMBAT_TO_ADRENALINE_RUSH: [(u32, u32); 11] = [
    (105708, 3),
    (105741, 2),
    (113398, 5),
    (105737, 3),
    (105738, 3),
    (105736, 2),
    (105732, 2),
    (105740, 5),
    (105728, 1),
    (105727, 5),
    (105724, 1),
];

/// Talent nodes, tier by tier up to Vigor 2/2 (Assassination).
const ASSASSINATION_TO_VIGOR: [(u32, u32); 9] = [
    (105742, 3),
    (105723, 2),
    (105722, 5),
    (105721, 3),
    (105720, 2),
    (105739, 3),
    (105759, 1),
    (105717, 2),
    (105718, 2),
];

pub(super) fn data() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// An Orc Rogue with the shipped data, a sword in the main hand, `talents` spent and
/// `rotation` (YAML body after the class and name) set.
pub(super) fn rogue(talents: &[(u32, u32)], rotation: &str) -> Fixture {
    let classes = crate::character::ClassDb::load(&data().join("classes"), None).unwrap();
    let class = Arc::clone(classes.get(crate::faction::PlayerClass::Rogue).unwrap());
    let mut f = Fixture::orc(class);
    f.db = SpellDb::load(&data().join("spells")).expect("shipped spell data loads");
    let trees = TalentDb::load(&data().join("talents")).unwrap();
    let tree = Arc::clone(trees.get(crate::faction::PlayerClass::Rogue).unwrap());
    f.ctx().set_talents(CharacterTalents::new(tree));
    f.equip(EquipmentSlot::Mainhand, SWORD);
    let db = std::mem::take(&mut f.db);
    f.ctx().learn_all(&db);
    f.db = db;
    assert_eq!(f.ctx().spend_talent_points(talents), []);
    let spec = format!("class: ROGUE\nname: Energy test\n{rotation}");
    f.ctx()
        .set_rotation(Arc::new(serde_yaml::from_str(&spec).unwrap()));
    f.ctx().prepare_set_of_combat_iterations();
    f
}

/// Starts an iteration at 0 with the pull.
pub(super) fn pull(f: &mut Fixture) {
    f.ctx().reset();
    f.engine.prepare_iteration(0.0);
    f.engine.add_event(Event::new(
        0.0,
        EventKind::EncounterStart {
            character: CharId(0),
        },
    ));
}

/// Runs every event up to `until`, returning the times and kinds of those the character
/// handled.
fn run_timed(f: &mut Fixture, until: f64) -> Vec<(f64, EventKind)> {
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

pub(super) fn energy(f: &Fixture, at: f64) -> u32 {
    f.character.resource_level(ResourceType::Energy, at)
}

fn count(handled: &[(f64, EventKind)], wanted: fn(&EventKind) -> bool) -> usize {
    handled.iter().filter(|(_, kind)| wanted(kind)).count()
}

fn is_regen_reaction(kind: &EventKind) -> bool {
    matches!(kind, EventKind::RegenReaction { .. })
}

const SINISTER_STRIKE_ONLY: &str = "attack_mode: magic\ncast_if:\n  - name: Sinister Strike\n";

/// A rotation that never casts.
pub(super) const NOTHING: &str = "attack_mode: magic\ncast_if:\n  - name: Sinister Strike\n    condition: variable \"combo_points\" greater 5\n";

/// The rotation acts 0.1 s after the tick that makes a 45 energy Sinister Strike affordable,
/// not before and not on every tick.
#[test]
fn the_rotation_reacts_to_the_tick_that_affords_the_spell() {
    let mut f = rogue(&[], SINISTER_STRIKE_ONLY);
    f.rig_rolls(PhysicalAttackResult::Hit);
    assert_eq!(f.character.max_resource_level(ResourceType::Energy), 100);
    pull(&mut f);
    // 100 → 55 at the pull, 65 → 20 at the end of the global cooldown.
    let handled = run_timed(&mut f, 1.05);
    assert_eq!(energy(&f, 1.05), 20);
    assert_eq!(f.character.combo_points(1.05), 2);
    // 25 ticks later, at 3.5, the energy is back to 45; the player notices at 3.6.
    let handled = [handled, run_timed(&mut f, 3.59)].concat();
    assert_eq!(energy(&f, 3.59), 45);
    assert_eq!(f.character.combo_points(3.59), 2);
    let handled = [handled, run_timed(&mut f, 3.61)].concat();
    assert_eq!(energy(&f, 3.61), 1, "46 at 3.6, spent");
    assert_eq!(f.character.combo_points(3.61), 3);
    // One regeneration reaction was needed.
    assert_eq!(count(&handled, is_regen_reaction), 1, "{handled:?}");
}

/// An energy condition is checked again when the energy crosses its threshold.
#[test]
fn an_energy_condition_is_rechecked_at_its_threshold() {
    let mut f = rogue(
        &[],
        "attack_mode: magic\ncast_if:\n  - name: Sinister Strike\n    condition: resource \"Energy\" greater 90\n",
    );
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    // 55 after the pull: the condition waits for 91, reached by the tick at 3.6.
    let handled = run_timed(&mut f, 3.69);
    assert_eq!(energy(&f, 3.69), 91);
    assert_eq!(f.character.combo_points(3.69), 1);
    let handled = [handled, run_timed(&mut f, 3.71)].concat();
    assert_eq!(energy(&f, 3.71), 47, "92 at 3.7, spent");
    assert_eq!(f.character.combo_points(3.71), 2);
    assert_eq!(count(&handled, is_regen_reaction), 1, "{handled:?}");
}

/// Gains of any source wake the player 0.1 s later: an `ENERGIZE` and a refund on a miss.
#[test]
fn energy_gains_and_refunds_wake_the_rotation() {
    let mut f = rogue(&[], SINISTER_STRIKE_ONLY);
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    run_timed(&mut f, 2.25);
    assert_eq!(energy(&f, 2.25), 32);
    assert_eq!(f.ctx().gain_resource(ResourceType::Energy, 30), 30);
    run_timed(&mut f, 2.34);
    assert_eq!(energy(&f, 2.34), 63);
    run_timed(&mut f, 2.36);
    assert_eq!(energy(&f, 2.36), 18, "63 at 2.35, spent");

    // A missed Sinister Strike gives 80 % back, and the player reacts to that.
    let mut f = rogue(&[], SINISTER_STRIKE_ONLY);
    f.rig_rolls(PhysicalAttackResult::Miss);
    pull(&mut f);
    let handled = run_timed(&mut f, 0.5);
    assert_eq!(energy(&f, 0.0), 91);
    assert!(
        handled.iter().any(|(time, kind)| {
            (*time - 0.1).abs() < 1e-9 && matches!(kind, EventKind::PlayerAction { .. })
        }),
        "{handled:?}"
    );
}

/// Nothing is castable (the only executor waits for combo points to drop): no regeneration
/// reaction at all, where the per-tick mode reacts to every one of the 45 ticks until the cap.
#[test]
fn no_regeneration_reactions_when_nothing_can_be_cast() {
    let rotation = "attack_mode: magic\ncast_if:\n  - name: Sinister Strike\n    condition: variable \"combo_points\" less 1\n";
    for (mode, reactions) in [
        (RegenReactions::Thresholds, 0),
        (RegenReactions::EveryTick, 45),
    ] {
        let mut f = rogue(&[], rotation);
        f.character.set_regen_reactions(mode);
        f.rig_rolls(PhysicalAttackResult::Hit);
        pull(&mut f);
        let handled = run_timed(&mut f, 20.0);
        assert_eq!(energy(&f, 20.0), 100);
        assert_eq!(f.character.combo_points(20.0), 1);
        assert_eq!(count(&handled, is_regen_reaction), reactions, "{mode:?}");
    }
}

/// Adrenaline Rush doubles the rate for its 15 s, keeping the running tick; Vigor raises the
/// maximum.
#[test]
fn adrenaline_rush_and_vigor_change_the_energy() {
    let mut f = rogue(&COMBAT_TO_ADRENALINE_RUSH, NOTHING);
    pull(&mut f);
    run_timed(&mut f, 0.5);
    f.ctx().lose_resource(ResourceType::Energy, 100);
    run_timed(&mut f, 1.04);
    // Spent at 0.5, 5 at 1.04; Adrenaline Rush 40 % into the tick due at 1.1.
    assert_eq!(energy(&f, 1.04), 5);
    let rush = f.spell_id(ADRENALINE_RUSH);
    assert_eq!(f.ctx().cast(rush).result, SpellResult::Success);
    let interval = |f: &Fixture| {
        f.character
            .resource()
            .as_energy()
            .map(|energy| energy.interval())
            .unwrap()
    };
    assert_eq!(interval(&f), 0.05);
    // The rest of the running tick at the doubled rate, then 20 per second.
    assert_eq!(energy(&f, 1.069), 5);
    assert_eq!(energy(&f, 1.07), 6);
    assert_eq!(energy(&f, 1.47), 14);
    // Back to 10 per second when the buff runs out at 16.04, capped long before.
    run_timed(&mut f, 16.1);
    assert_eq!(interval(&f), 0.1);
    assert_eq!(energy(&f, 16.1), 100);
    assert!(!f.ctx().aura_active(ADRENALINE_RUSH));

    let f = rogue(&ASSASSINATION_TO_VIGOR, NOTHING);
    assert_eq!(f.character.max_resource_level(ResourceType::Energy), 110);
    assert_eq!(energy(&f, 0.0), 110, "full at the pull");
}

/// The regeneration statistics: energy from ticks and ticks lost at the cap, from the pull.
#[test]
fn regeneration_is_recorded() {
    let rotation = "attack_mode: magic\ncast_if:\n  - name: Sinister Strike\n    condition: variable \"combo_points\" less 1\n";
    let mut f = rogue(&[], rotation);
    f.rig_rolls(PhysicalAttackResult::Hit);
    pull(&mut f);
    run_timed(&mut f, 20.0);
    f.ctx().reset();
    let statistics = f.character.statistics();
    let regeneration = statistics
        .resource_statistics(super::super::context::REGENERATION, 1)
        .expect("regeneration is a resource source");
    // 45 ticks refill the Sinister Strike at the pull; the other 155 of 20 s are lost.
    assert_eq!(regeneration.gain(ResourceType::Energy), 45.0);
    assert_eq!(statistics.lost_at_cap(ResourceType::Energy), 155.0);
}

/// Fights `rotation` for three iterations of 120 s with each seed, reacting to every
/// regeneration tick and after the thresholds only: the thresholds only drop passes that cannot
/// cast anything, so the fights are the same, event for event in the combat log. Returns the
/// reference logs.
fn assert_threshold_reactions_match_every_tick(rotation: &str, seeds: &[u64]) -> Vec<CombatLog> {
    let fight = |mode: RegenReactions, seed: u64| {
        let mut f = rogue(&COMBAT_TO_ADRENALINE_RUSH, rotation);
        f.equip(EquipmentSlot::Offhand, DAGGER);
        f.character.set_regen_reactions(mode);
        f.character.set_seed(seed);
        f.engine.enable_combat_log();
        let mut reactions = 0;
        for _ in 0..3 {
            pull(&mut f);
            let handled = run_timed(&mut f, 120.0);
            reactions += count(&handled, is_regen_reaction);
            f.ctx().reset();
        }
        let log = f.engine.take_combat_log().unwrap();
        let spells: Vec<_> = f
            .character
            .statistics()
            .spells()
            .map(|(key, stats)| (key.clone(), stats.clone()))
            .collect();
        (log, spells, reactions)
    };
    let mut logs = Vec::new();
    for &seed in seeds {
        let (reference_log, reference_spells, every_tick) = fight(RegenReactions::EveryTick, seed);
        let (log, spells, thresholds) = fight(RegenReactions::Thresholds, seed);
        assert!(
            reference_log.len() > 500,
            "seed {seed}: {}",
            reference_log.len()
        );
        if let Some(index) = log
            .entries()
            .iter()
            .zip(reference_log.entries())
            .position(|(a, b)| a != b)
        {
            panic!(
                "seed {seed}: entry {index} differs:\n{:?}\n{:?}",
                log.entries()[index],
                reference_log.entries()[index]
            );
        }
        assert_eq!(log.len(), reference_log.len(), "seed {seed}");
        assert_eq!(spells, reference_spells, "seed {seed}");
        assert!(
            thresholds * 5 < every_tick,
            "seed {seed}: {thresholds} reactions, {every_tick} every tick"
        );
        logs.push(reference_log);
    }
    logs
}

fn casts(log: &CombatLog, name: &str) -> usize {
    log.entries()
        .iter()
        .filter(|entry| {
            matches!(&entry.event, CombatLogEvent::SpellCastSuccess { spell, .. }
                if spell.name == name)
        })
        .count()
}

/// Energy, combo point and duration conditions, a finisher and Adrenaline Rush.
#[test]
fn threshold_reactions_match_the_per_tick_reference() {
    let rotation = r#"attack_mode: melee
cast_if:
  - name: Adrenaline Rush
    condition: variable "time_remaining_encounter" less 250
  - name: Slice and Dice
    condition: |
      buff_duration "Slice and Dice" less 2
      and variable "combo_points" greater 1
      and variable "time_remaining_encounter" greater 240
  - name: Eviscerate
    condition: |
      variable "combo_points" geq 3
      and buff_duration "Slice and Dice" less 3
  - name: Sinister Strike
    condition: |
      variable "combo_points" less 5
      or resource "Energy" greater 70
"#;
    for log in assert_threshold_reactions_match_every_tick(rotation, &[1, 2, 3]) {
        assert_eq!(casts(&log, "Adrenaline Rush"), 3);
        assert!(casts(&log, "Eviscerate") > 10);
        assert!(casts(&log, "Slice and Dice") > 10);
    }
}

/// Swing timer, target health and cooldown conditions: a swing landing on the instant of a
/// regeneration reaction is seen by it whichever way the reactions are planned.
#[test]
fn threshold_reactions_match_the_per_tick_reference_with_timers() {
    let rotation = r#"attack_mode: melee
cast_if:
  - name: Adrenaline Rush
    condition: |
      variable "target_health" less 0.9
      and resource "Energy" less 50
  - name: Slice and Dice
    condition: |
      buff_duration "Slice and Dice" less 1.5
      and variable "combo_points" greater 0
      and variable "time_remaining_swing" greater 0.4
  - name: Eviscerate
    condition: |
      variable "combo_points" geq 2
      and resource "Energy" greater 55
      or variable "combo_points" geq 4
      and variable "time_since_swing" less 1
  - name: Sinister Strike
    condition: |
      variable "combo_points" less 5
      and resource "Energy" greater 62
      or buff_duration "Adrenaline Rush" greater 0
      or spell "Adrenaline Rush" less 30
"#;
    for log in assert_threshold_reactions_match_every_tick(rotation, &[1, 2, 3]) {
        assert!(casts(&log, "Slice and Dice") > 10);
    }
}
