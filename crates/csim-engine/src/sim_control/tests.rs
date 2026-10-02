//! Sim control tests: raids of Orc Warriors built from the shipped class, spell and talent
//! data with a sword and a dagger and a Fury rotation.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::character::tests::{equipment_db, race};
use crate::character::{Character, ClassDb, ClassSpec};
use crate::character_loader::CharacterSetup;
use crate::combat_log::{CombatLogEntry, CombatLogEvent, LogUnit};
use crate::data_bundle::DataBundle;
use crate::engine::EventType;
use crate::faction::PlayerClass;
use crate::ids::SpellId;
use crate::item::EquipmentSlot;
use crate::phase::Phase;
use crate::race::Race;
use crate::rotation::RotationSpec;
use crate::spell::SpellStatus;
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
    let setup = CharacterSetup::load(
        &DataBundle::repository_dir().join("characters/warrior_fury_dw_orc.yaml"),
    )
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
fn a_stepped_iteration_is_the_logged_iteration_of_its_seed() {
    let data = Data::load();
    let settings = settings(1);
    let mut logged_raid = data.raid(&settings, 2, true);
    let logged = run_logged_iteration(&settings, 9, &mut logged_raid);

    let mut raid = data.raid(&settings, 2, true);
    let mut stepper = IterationStepper::new(&settings, 9, &mut raid);
    assert!(stepper.start_at() < 0.0);
    assert_eq!(raid.engine().current_time(), stepper.start_at());
    let mut steps = 0;
    while let Some(event) = stepper.step(&mut raid) {
        assert_eq!(event.time, raid.engine().current_time());
        steps += 1;
    }
    assert!(steps > 100, "{steps}");
    assert!(stepper.is_done(&raid));
    assert_eq!(stepper.next_event_time(&raid), None);
    let log = stepper.finish(&mut raid);

    assert_eq!(log, logged);
    let results = |raid: &mut RaidControl| -> Vec<_> {
        raid.take_statistics()
            .iter()
            .map(|s| s.personal_result())
            .collect()
    };
    assert_eq!(results(&mut raid), results(&mut logged_raid));
    assert!(!raid.engine().is_logging());
}

/// The rotation's decision trace: one decision per executor cast, and recording it changes
/// nothing about the iteration.
#[test]
fn the_rotation_trace_names_every_cast_and_changes_nothing() {
    use crate::rotation::DecidedBy;

    let data = Data::load();
    let settings = settings(1);
    let mut untraced = data.raid(&settings, 2, false);
    let untraced_log = run_logged_iteration(&settings, 8, &mut untraced);

    let mut raid = data.raid(&settings, 2, false);
    for id in raid.char_ids().collect::<Vec<_>>() {
        raid.character_mut(id)
            .rotation_mut()
            .unwrap()
            .enable_trace();
    }
    let log = run_logged_iteration(&settings, 8, &mut raid);
    assert_eq!(log, untraced_log);

    for character in raid.characters() {
        let rotation = character.rotation().unwrap();
        let trace = rotation.trace();
        assert!(trace.is_sorted_by(|a, b| a.time <= b.time));
        let precombat: Vec<_> = trace
            .iter()
            .filter(|d| d.by == DecidedBy::Precombat)
            .map(|d| d.spell)
            .collect();
        assert_eq!(
            precombat,
            rotation.precombat_spells(),
            "all available at the start"
        );
        assert!(
            trace
                .iter()
                .all(|d| d.by != DecidedBy::Precombat || d.time < 0.0)
        );

        let mut casts = 0;
        for (index, executor) in rotation.executors().iter().enumerate() {
            let decisions: Vec<_> = trace
                .iter()
                .filter(|d| d.by == DecidedBy::Executor(index))
                .collect();
            assert_eq!(
                decisions.len() as u64,
                executor.statistics().successful_casts,
                "{}",
                executor.spell_name()
            );
            if let Some(linked) = executor.linked() {
                assert!(decisions.iter().all(|d| d.spell == linked.spell));
            }
            casts += decisions.len();
        }
        // The untalented test rotation casts Execute, Whirlwind, the stances and the like.
        assert!(casts >= 10, "{casts}");
    }
}

#[test]
fn stepping_until_a_time_runs_no_later_event() {
    let data = Data::load();
    let settings = settings(1);
    let mut raid = data.raid(&settings, 1, false);
    let mut stepper = IterationStepper::new(&settings, 3, &mut raid);
    let mut total = 0;
    for time in [-0.5, 0.0, 0.25, 7.3, 7.3, 30.0] {
        total += stepper.step_until(&mut raid, time);
        assert!(raid.engine().current_time() <= time);
        assert!(
            stepper
                .next_event_time(&raid)
                .is_some_and(|next| next > time)
        );
    }
    assert!(total > 10, "{total}");
    assert_eq!(
        stepper.step_until(&mut raid, 7.3),
        0,
        "nothing left before 7.3"
    );
    stepper.step_until(&mut raid, f64::INFINITY);
    assert!(stepper.is_done(&raid));
}

/// A step only adds entries after the log's length at its start: what a viewer read of the log
/// before a step stays as it was.
#[test]
fn a_step_leaves_the_log_before_it_untouched() {
    let data = Data::load();
    let settings = settings(1);
    let mut raid = data.raid(&settings, 2, false);
    let mut stepper = IterationStepper::new(&settings, 4, &mut raid);
    let mut seen: Vec<CombatLogEntry> = Vec::new();
    loop {
        let log = raid.engine().combat_log().unwrap().entries();
        assert_eq!(&log[..seen.len()], &seen[..]);
        seen = log.to_vec();
        if stepper.step(&mut raid).is_none() {
            break;
        }
    }
    assert!(seen.len() > 100, "{}", seen.len());
    assert_eq!(stepper.finish(&mut raid).entries(), &seen[..]);
}

#[test]
fn the_encounter_length_varies_within_the_length_variance() {
    let mut control = SimControl::new(settings(1), 1);
    let lengths: Vec<f64> = (0..1000)
        .map(|_| control.draw_combat_length(60.0))
        .collect();
    assert!(lengths.iter().all(|l| (54.0..=66.0).contains(l)));
    let below = |limit: f64| lengths.iter().filter(|&&l| l < limit).count();
    // Uniform: about a tenth of the draws in each tenth of the range.
    assert!((60..=140).contains(&below(55.2)), "{}", below(55.2));
    assert!((440..=560).contains(&below(60.0)), "{}", below(60.0));
    assert!((860..=940).contains(&below(64.8)), "{}", below(64.8));
}

/// The lengths come from a generator of their own, reset for every set of iterations: the same
/// seed gives the same lengths whatever the raid and however often it runs.
#[test]
fn the_encounter_lengths_depend_only_on_the_seed() {
    let data = Data::load();
    let settings = settings(20);
    let time = |size: usize, sets: usize, seed: u64| {
        let mut raid = data.raid(&settings, size, false);
        let mut control = SimControl::new(settings.clone(), seed);
        (0..sets)
            .map(|_| {
                control.run_sim(&mut raid, 60, 20);
                raid.take_statistics()[0].time_in_combat()
            })
            .collect::<Vec<_>>()
    };
    let solo = time(1, 2, 7);
    assert_eq!(
        solo[0], solo[1],
        "every set of iterations gets the same lengths"
    );
    assert_ne!(solo[0], 20.0 * 60.0);
    assert_eq!(
        time(3, 1, 7)[0],
        solo[0],
        "the raid shuffle draws nothing from it"
    );
    assert_ne!(time(1, 1, 8)[0], solo[0]);
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

/// The player's casts in `log` as `(time, spell name)`.
fn player_casts(log: &CombatLog) -> Vec<(f64, String)> {
    log.entries()
        .iter()
        .filter_map(|entry| match &entry.event {
            CombatLogEvent::SpellCastSuccess { spell, .. }
                if entry.source == LogUnit::Character(CharId(0)) =>
            {
                Some((entry.time, spell.name.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_character_played_by_input_casts_nothing_by_itself() {
    let data = Data::load();
    let settings = settings(1);
    let mut rotation_raid = data.raid(&settings, 1, false);
    let rotation_log = run_logged_iteration(&settings, 5, &mut rotation_raid);
    assert!(player_casts(&rotation_log).len() > 10);

    let mut raid = data.raid(&settings, 1, false);
    raid.character_mut(CharId(0)).enable_manual_input();
    let log = run_logged_iteration(&settings, 5, &mut raid);
    assert_eq!(player_casts(&log), [], "no precombat actions, no rotation");
    let swings = log
        .entries()
        .iter()
        .filter(|entry| matches!(entry.event, CombatLogEvent::SwingDamage { .. }))
        .count();
    assert!(swings > 20, "the auto attacks still run: {swings}");
}

#[test]
fn queued_input_is_cast_when_it_can_be_within_its_window() {
    use crate::rotation::DecidedBy;

    let data = Data::load();
    let settings = settings(1);
    let mut raid = data.raid(&settings, 1, false);
    let me = CharId(0);
    raid.character_mut(me).enable_manual_input();
    raid.character_mut(me)
        .rotation_mut()
        .unwrap()
        .enable_trace();
    let learned = |raid: &RaidControl, name: &str| {
        let spells = raid.character(me).spells();
        let group = spells.rank_group(name).unwrap();
        group
            .get_max_available_spell_rank(|id| spells.spell(id).is_enabled())
            .unwrap()
    };
    let hamstring = learned(&raid, "Hamstring");
    let whirlwind = learned(&raid, "Whirlwind");
    let mut stepper = IterationStepper::new(&settings, 5, &mut raid);
    // A press at `at`, queued for 0.4 s: the caller wakes the character then.
    let press = |raid: &mut RaidControl, stepper: &mut IterationStepper, spell, at: f64| {
        stepper.step_until(raid, at);
        raid.character_mut(me).queue_input(spell, at + 0.4);
        let wake = Event::new(at, EventKind::PlayerAction { character: me });
        raid.engine_mut().add_event(wake);
        stepper.step_until(raid, at);
    };

    // Not in Berserker Stance (no precombat actions: Battle Stance): waiting cannot help,
    // dropped at once.
    press(&mut raid, &mut stepper, whirlwind, 9.0);
    assert_eq!(raid.character(me).queued_input(), None);
    let failure = raid.character_mut(me).take_input_failure();
    assert_eq!(failure, Some((whirlwind, SpellStatus::InBattleStance)));
    assert_eq!(
        raid.character_mut(me).take_input_failure(),
        None,
        "reported once"
    );
    // Castable: cast at the press. Hamstring triggers the global cooldown (1.5 s).
    press(&mut raid, &mut stepper, hamstring, 10.0);
    // Pressed 1 s into the GCD: it ends after the window, so the press is dropped.
    press(&mut raid, &mut stepper, hamstring, 10.5);
    assert_eq!(
        raid.character_mut(me).take_input_failure(),
        None,
        "still waiting"
    );
    stepper.step_until(&mut raid, 12.0);
    assert_eq!(raid.character(me).queued_input(), None, "dropped");
    let failure = raid.character_mut(me).take_input_failure();
    assert_eq!(failure, Some((hamstring, SpellStatus::OnGcd)));
    // Castable again.
    press(&mut raid, &mut stepper, hamstring, 13.0);
    // Pressed 0.2 s before the GCD ends: cast when it does.
    press(&mut raid, &mut stepper, hamstring, 14.3);
    stepper.step_until(&mut raid, 16.0);
    assert_eq!(raid.character_mut(me).take_input_failure(), None);

    let casts = player_casts(raid.engine().combat_log().unwrap());
    let expected = [
        (10.0, "Hamstring"),
        (13.0, "Hamstring"),
        (14.5, "Hamstring"),
    ];
    let casts: Vec<(f64, &str)> = casts.iter().map(|(t, n)| (*t, n.as_str())).collect();
    assert_eq!(casts.len(), expected.len(), "{casts:?}");
    for ((time, name), (expected_time, expected_name)) in casts.iter().zip(expected) {
        assert!((time - expected_time).abs() < 1e-9, "{casts:?}");
        assert_eq!(*name, expected_name);
    }

    let trace = raid.character(me).rotation().unwrap().trace();
    let inputs: Vec<f64> = trace
        .iter()
        .inspect(|decision| assert_eq!(decision.by, DecidedBy::Input))
        .map(|decision| decision.time)
        .collect();
    assert_eq!(inputs.len(), 3, "{inputs:?}");
}

#[test]
fn a_macro_casts_in_order_and_ends_at_the_global_cooldown() {
    let data = Data::load();
    let settings = settings(1);
    let mut raid = data.raid(&settings, 1, false);
    let me = CharId(0);
    raid.character_mut(me).enable_manual_input();
    raid.character_mut(me)
        .rotation_mut()
        .unwrap()
        .enable_trace();
    let learned = |raid: &RaidControl, name: &str| {
        let spells = raid.character(me).spells();
        let group = spells.rank_group(name).unwrap();
        group
            .get_max_available_spell_rank(|id| spells.spell(id).is_enabled())
            .unwrap()
    };
    let [bloodrage, hamstring, heroic_strike, battle_shout] =
        ["Bloodrage", "Hamstring", "Heroic Strike", "Battle Shout"]
            .map(|name| learned(&raid, name));
    let mut stepper = IterationStepper::new(&settings, 5, &mut raid);
    let press = |raid: &mut RaidControl, stepper: &mut IterationStepper, spells: &[SpellId], at| {
        stepper.step_until(raid, at);
        raid.character_mut(me)
            .queue_macro(spells.to_vec(), at + 0.4);
        let wake = Event::new(at, EventKind::PlayerAction { character: me });
        raid.engine_mut().add_event(wake);
        stepper.step_until(raid, at);
    };
    // The input's casts, as the trace has them when cast (the log shows Heroic Strike's when
    // its swing lands).
    let casts_between = |raid: &RaidControl, from: f64, to: f64| -> Vec<(f64, String)> {
        let character = raid.character(me);
        character
            .rotation()
            .unwrap()
            .trace()
            .iter()
            .filter(|decision| (from..to).contains(&decision.time))
            .map(|d| (d.time, character.spells().spell(d.spell).name().to_owned()))
            .collect()
    };

    // Off the GCD, on it, off it: the third never fires, the second's GCD ends the macro.
    press(
        &mut raid,
        &mut stepper,
        &[bloodrage, hamstring, heroic_strike],
        10.0,
    );
    stepper.step_until(&mut raid, 11.0);
    let casts = casts_between(&raid, 10.0, 11.0);
    let names: Vec<&str> = casts.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names, ["Bloodrage", "Hamstring"], "{casts:?}");
    assert!(casts.iter().all(|(time, _)| *time == 10.0));
    assert_eq!(raid.character(me).queued_input(), None);

    // Pressed during the GCD (to 11.5): the first entry fires, the macro waits at the
    // second and goes on when the GCD ends.
    press(&mut raid, &mut stepper, &[heroic_strike, hamstring], 11.2);
    stepper.step_until(&mut raid, 12.0);
    let casts = casts_between(&raid, 11.0, 12.0);
    let expected = [(11.2, "Heroic Strike"), (11.5, "Hamstring")];
    assert_eq!(casts.len(), 2, "{casts:?}");
    for ((time, name), (expected_time, expected_name)) in casts.iter().zip(expected) {
        assert!((time - expected_time).abs() < 1e-9, "{casts:?}");
        assert_eq!(name, expected_name);
    }

    // Bloodrage on its cooldown is skipped.
    press(&mut raid, &mut stepper, &[bloodrage, battle_shout], 13.0);
    let casts = casts_between(&raid, 12.0, 13.5);
    let names: Vec<&str> = casts.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names, ["Battle Shout"]);
    assert_eq!(
        raid.character_mut(me).take_input_failure(),
        None,
        "cast something"
    );

    // Casting nothing reports its first entry's failure.
    press(&mut raid, &mut stepper, &[bloodrage], 16.0);
    assert_eq!(
        raid.character_mut(me).take_input_failure(),
        Some((bloodrage, SpellStatus::OnCooldown))
    );
}
