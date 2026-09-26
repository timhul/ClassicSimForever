//! Sim control tests: raids of Orc Warriors built from the shipped class, spell and talent
//! data with a sword and a dagger and a Fury rotation.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::character::tests::{equipment_db, race};
use crate::character::{Character, ClassDb, ClassSpec};
use crate::character_loader::CharacterSetup;
use crate::data_bundle::DataBundle;
use crate::engine::EventType;
use crate::faction::PlayerClass;
use crate::item::EquipmentSlot;
use crate::phase::Phase;
use crate::race::Race;
use crate::rotation::RotationSpec;
use crate::spell::record::SpellDb;
use crate::statistics::ClassStatistics;
use crate::talent::{CharacterTalents, TalentDb, TalentFile};
use crate::target::Target;

const SWORD: u32 = 1;
const DAGGER: u32 = 2;

const FURY: &str = r#"
class: WARRIOR
name: Fury (no talents)
precombat_actions: [Bloodrage, Battle Shout, Berserker Stance]
cast_if:
  - name: Bloodrage
    condition: resource "Rage" less 70
  - name: Battle Shout
    condition: buff_duration "Battle Shout" less 3
  - name: Heroic Strike
    condition: resource "Rage" greater 50
  - name: Blood Fury
  - name: Execute
  - name: Whirlwind
  - name: Battle Stance
    condition: variable "combo_points" greater 0
  - name: Berserker Stance
    condition: variable "combo_points" eq 0
"#;

/// The shipped data a raid is built from, loaded once and shared by the threads.
struct Data {
    db: SpellDb,
    class: Arc<ClassSpec>,
    talents: Arc<TalentFile>,
    rotation: Arc<RotationSpec>,
}

impl Data {
    fn load() -> Self {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let classes = ClassDb::load(&data.join("classes"), None).unwrap();
        let talents = TalentDb::load(&data.join("talents")).unwrap();
        Data {
            db: SpellDb::load(&data.join("spells")).unwrap(),
            class: Arc::clone(classes.get(PlayerClass::Warrior).unwrap()),
            talents: Arc::clone(talents.get(PlayerClass::Warrior).unwrap()),
            rotation: Arc::new(serde_yaml::from_str(FURY).unwrap()),
        }
    }

    /// A raid of `size` Warriors (the second one tanking when `tank`) set up for `settings`.
    fn raid(&self, settings: &SimSettings, size: usize, tank: bool) -> RaidControl {
        let mut raid = RaidControl::new(Target::new(63));
        for _ in 0..size {
            let id = raid
                .add_character(|id, party, member| {
                    Character::new(
                        id,
                        Arc::clone(&self.class),
                        &race(Race::Orc),
                        equipment_db(),
                        Phase::MoltenCore,
                        settings.sim_params(),
                        63,
                        party,
                        member,
                    )
                })
                .unwrap();
            let equipment = raid.character_mut(id).equipment_mut();
            equipment.equip(EquipmentSlot::Mainhand, SWORD).unwrap();
            equipment.equip(EquipmentSlot::Offhand, DAGGER).unwrap();
            raid.with_character(id, |ctx| {
                ctx.set_talents(CharacterTalents::new(Arc::clone(&self.talents)));
                ctx.learn_all(&self.db);
                ctx.set_rotation(Arc::clone(&self.rotation));
            });
            raid.character_mut(id).set_tanking(tank && id.0 == 1);
        }
        raid
    }
}

fn settings(iterations: u32) -> SimSettings {
    SimSettings {
        combat_length: 60,
        iterations_quick_sim: iterations,
        iterations_full_sim: iterations,
        threads: 1,
        ..SimSettings::default()
    }
}

fn baseline(cruncher: &NumberCruncher) -> ClassStatistics {
    cruncher.merged(None).unwrap()
}

#[test]
fn run_sim_runs_every_iteration_for_every_character() {
    let data = Data::load();
    let settings = settings(20);
    let mut raid = data.raid(&settings, 2, true);
    let mut control = SimControl::new(settings, 1);
    let mut cruncher = NumberCruncher::new();
    control.run_quick_sim(&mut raid, &mut cruncher);

    let stats = baseline(&cruncher);
    assert_eq!(stats.player_name(), "You");
    assert_eq!(stats.iterations(), 20);
    assert!(stats.personal_dps() > 0.0);

    let engine = stats.engine();
    assert_eq!(engine.event_count(EventType::EncounterStart), 2 * 20);
    assert_eq!(engine.event_count(EventType::EncounterEnd), 20);
    // Only the tank takes incoming damage (which nothing handles yet).
    assert_eq!(engine.event_count(EventType::IncomingDamage), 20);
    assert!(engine.event_count(EventType::PlayerAction) > 20);

    // The precombat Battle Shout is up from the pull.
    let shout = stats.buff_statistics("Battle Shout (party 1)").unwrap();
    assert!(shout.avg_uptime() > 0.9, "{shout:?}");

    // Both members' results, in `CharId` order.
    let results = cruncher.player_results();
    assert_eq!(
        results
            .iter()
            .map(|r| r.player_name.as_str())
            .collect::<Vec<_>>(),
        ["You", "P1M2"]
    );
    assert!(results.iter().all(|r| r.iterations == 20 && r.dps > 0.0));
    assert!((cruncher.raid_dps() - results[0].dps - results[1].dps).abs() < 1e-9);
    assert!((results[0].dps - stats.personal_dps()).abs() < 1e-9);

    // The raid is clean for another set.
    assert!(raid.engine().queue().is_empty());
    let again = &mut NumberCruncher::new();
    control.run_quick_sim(&mut raid, again);
    assert_eq!(baseline(again).iterations(), 20);
}

#[test]
fn a_full_sim_runs_each_option_and_takes_its_stat_back() {
    let data = Data::load();
    let mut settings = settings(10);
    settings.options = [SimOption::ScaleStrength, SimOption::ScaleAttackPower]
        .into_iter()
        .collect();
    let mut raid = data.raid(&settings, 2, false);
    let ap = |raid: &RaidControl| {
        raid.char_ids()
            .map(|id| raid.character(id).melee_ap(&raid.target().stat_view()))
            .collect::<Vec<_>>()
    };
    let before = ap(&raid);

    let mut cruncher = NumberCruncher::new();
    SimControl::new(settings, 1).run_full_sim(&mut raid, &mut cruncher);

    assert_eq!(ap(&raid), before);
    assert_eq!(
        cruncher.options().collect::<Vec<_>>(),
        [
            None,
            Some(SimOption::ScaleStrength),
            Some(SimOption::ScaleAttackPower)
        ]
    );
    for option in cruncher.options() {
        assert_eq!(cruncher.merged(option).unwrap().iterations(), 10);
    }
    assert_eq!(cruncher.stat_weights_dps().len(), 2);
    // The raid results are the baseline's.
    assert_eq!(cruncher.player_results()[0].iterations, 10);
}

/// A set of iterations leaves the character as it found it: the passives' auras (talents,
/// here also a 0 % attack power aura) are neither applied twice nor left behind.
#[test]
fn every_set_of_iterations_starts_from_the_same_stats() {
    let data = DataBundle::load(&DataBundle::repository_dir()).unwrap();
    let setup =
        CharacterSetup::load(&DataBundle::repository_dir().join("characters/dw_fury_orc.yaml"))
            .unwrap();
    let settings = settings(5);
    let mut raid = setup.build_raid(&data, &settings).unwrap();
    let mut control = SimControl::new(settings, 1);
    let stats = |raid: &RaidControl| format!("{:?}", raid.character(CharId(0)).stats());

    control.run_sim(&mut raid, 60, 5);
    let after_first = stats(&raid);
    // Back in caster form: Berserker Stance Passive's -20 % threat is off, not left behind by
    // the first set (its buff was active from the setup).
    let threat = raid.character(CharId(0)).stats().get_total_threat_mod();
    assert!((threat - 1.0).abs() < 1e-9, "threat modifier {threat}");
    let first = raid.take_statistics().remove(0).personal_dps();
    for _ in 0..2 {
        control.run_sim(&mut raid, 60, 5);
        assert_eq!(stats(&raid), after_first);
        let dps = raid.take_statistics().remove(0).personal_dps();
        assert!(
            (dps - first).abs() < first * 0.25,
            "DPS drifted from {first} to {dps}"
        );
    }
}

#[test]
fn a_seed_fixes_the_run() {
    let data = Data::load();
    let mut settings = settings(30);
    settings.threads = 3;
    let run = |seed: u64| {
        run_threaded(&settings, SimMode::Quick, seed, None, || {
            Ok::<_, ()>(data.raid(&settings, 2, false))
        })
        .unwrap()
    };
    let per_iteration = |cruncher: &NumberCruncher| baseline(cruncher).dps_per_iteration().to_vec();
    let events = |cruncher: &NumberCruncher| baseline(cruncher).engine().events().clone();

    let (a, b, c) = (run(7), run(7), run(8));
    assert_eq!(per_iteration(&a).len(), 30);
    assert_eq!(a.class_statistics(None).len(), 3, "one per thread");
    assert_eq!(per_iteration(&a), per_iteration(&b));
    assert_eq!(events(&a), events(&b));
    assert_eq!(a.player_results(), b.player_results());
    assert_ne!(per_iteration(&a), per_iteration(&c));
}

#[test]
fn the_threads_run_every_iteration_and_report_their_progress() {
    let data = Data::load();
    let mut settings = settings(25);
    settings.threads = 4;
    let completed = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&completed);
    let progress: Progress = Arc::new(move |n| {
        counter.fetch_add(n, Ordering::Relaxed);
    });
    let cruncher = run_threaded(&settings, SimMode::Quick, 1, Some(progress), || {
        Ok::<_, ()>(data.raid(&settings, 1, false))
    })
    .unwrap();

    assert_eq!(baseline(&cruncher).iterations(), 25);
    assert_eq!(cruncher.player_results()[0].iterations, 25);
    assert_eq!(completed.load(Ordering::Relaxed), 25);
}

#[test]
fn a_build_error_is_returned() {
    let settings = SimSettings {
        threads: 2,
        ..settings(10)
    };
    let result = run_threaded(&settings, SimMode::Quick, 1, None, || {
        Err::<RaidControl, _>("no raid")
    });
    assert_eq!(result.err(), Some("no raid"));
}

#[test]
fn iterations_are_split_over_the_threads() {
    assert_eq!(split_iterations(10, 3), [4, 3, 3]);
    assert_eq!(split_iterations(9, 3), [3, 3, 3]);
    assert_eq!(split_iterations(2, 4), [1, 1, 0, 0]);
    assert_eq!(split_iterations(5, 1), [5]);
}

#[test]
fn the_shuffle_is_a_permutation() {
    let mut control = SimControl::new(settings(1), 3);
    let mut order: Vec<CharId> = (0..10).map(CharId).collect();
    let mut seen_first = std::collections::BTreeSet::new();
    for _ in 0..200 {
        control.shuffle_order(&mut order);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..10).map(CharId).collect::<Vec<_>>());
        seen_first.insert(order[0]);
    }
    assert_eq!(seen_first.len(), 10, "everyone gets to act first");
}

#[test]
#[should_panic(expected = "was set up for another combat length")]
fn a_character_set_up_for_another_combat_length_panics() {
    let data = Data::load();
    let raid_settings = settings(1);
    let mut raid = data.raid(&raid_settings, 1, false);
    let settings = SimSettings {
        combat_length: 120,
        ..raid_settings
    };
    SimControl::new(settings, 1).run_quick_sim(&mut raid, &mut NumberCruncher::new());
}

#[test]
fn a_logged_iteration_is_the_one_thread_iteration_of_its_seed() {
    let data = Data::load();
    let settings = settings(1);
    let unlogged = run_threaded(&settings, SimMode::Quick, 5, None, || {
        Ok::<_, ()>(data.raid(&settings, 2, false))
    })
    .unwrap();

    let mut raid = data.raid(&settings, 2, false);
    let log = run_logged_iteration(&settings, 5, &mut raid);
    let statistics = raid.take_statistics();

    let damage: u64 = statistics.iter().map(ClassStatistics::total_damage).sum();
    assert!(damage > 0);
    assert_eq!(log.total_damage(), damage);
    let results: Vec<_> = statistics.iter().map(|s| s.personal_result()).collect();
    assert_eq!(unlogged.player_results(), results);
    // Both characters, from the precombat actions on.
    for id in raid.char_ids() {
        let unit = crate::combat_log::LogUnit::Character(id);
        assert!(log.entries().iter().any(|e| e.source == unit));
    }
    // From the precombat actions to the end of the encounter, at most 10 % past the length.
    assert!(log.entries()[0].time < 0.0, "{:#?}", log.entries()[0]);
    let end = f64::from(settings.combat_length) * 1.1;
    assert!(log.entries().iter().all(|e| e.time <= end));
    assert!(!raid.engine().is_logging());
}

#[test]
fn the_encounter_length_varies_within_the_length_variance() {
    let data = Data::load();
    let settings = settings(40);
    let mut raid = data.raid(&settings, 1, false);
    let mut control = SimControl::new(settings, 1);
    let mut lengths = Vec::new();
    for _ in 0..40 {
        control.run_sim(&mut raid, 60, 1);
        lengths.push(raid.take_statistics().remove(0).time_in_combat());
    }
    assert!(
        lengths.iter().all(|l| (54.0..=66.0).contains(l)),
        "{lengths:?}"
    );
    let (min, max) = lengths
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &l| (lo.min(l), hi.max(l)));
    assert!(min < 57.0 && max > 63.0, "{lengths:?}");
    // The character is set up for the nominal length again.
    assert_eq!(raid.character(CharId(0)).sim().combat_length, 60.0);
}

#[test]
fn without_length_variance_every_encounter_lasts_the_combat_length() {
    let data = Data::load();
    let settings = SimSettings {
        length_variance: 0.0,
        ..settings(10)
    };
    let mut raid = data.raid(&settings, 1, false);
    let mut cruncher = NumberCruncher::new();
    SimControl::new(settings, 1).run_quick_sim(&mut raid, &mut cruncher);
    assert_eq!(baseline(&cruncher).time_in_combat(), 600.0);
}
