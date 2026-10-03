//! Session tests on the shipped data and character setups.

use std::sync::{Arc, OnceLock};

use csim_engine::proc::Proc;
use csim_engine::sim_control::run_logged_iteration;

use super::*;

fn data() -> &'static Arc<DataBundle> {
    static DATA: OnceLock<Arc<DataBundle>> = OnceLock::new();
    DATA.get_or_init(|| Arc::new(DataBundle::load(&DataBundle::repository_dir()).unwrap()))
}

fn setup(file: &str) -> CharacterSetup {
    let path = DataBundle::repository_dir().join("characters").join(file);
    CharacterSetup::load(&path).unwrap()
}

fn settings(setup: &CharacterSetup) -> SimSettings {
    setup.sim_settings(&SimSettings {
        combat_length: 60,
        ..SimSettings::default()
    })
}

pub(crate) fn session_of(file: &str, seed: u64) -> Session {
    let setup = setup(file);
    let settings = settings(&setup);
    Session::new(Arc::clone(data()), setup, settings, seed, Vec::new()).unwrap()
}

/// `file` played from the keyboard with the keybinds of the YAML `keybinds`.
pub(crate) fn manual_session_of(file: &str, seed: u64, keybinds: &str) -> Session {
    let setup = setup(file);
    let settings = settings(&setup);
    let keybinds = crate::keybinds::parse(keybinds).unwrap();
    Session::new(Arc::clone(data()), setup, settings, seed, keybinds).unwrap()
}

/// Advances in steps of `dt` to the end and returns every frame.
fn play(session: &mut Session, dt: f64) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut time = session.info().start_at;
    loop {
        let frame = session.advance(time);
        let done = frame.done;
        frames.push(frame);
        if done {
            return frames;
        }
        time += dt;
    }
}

#[test]
fn the_session_shows_the_iteration_the_cli_logs() {
    for (file, seed) in [
        ("warrior_fury_dw_orc.yaml", 3),
        ("rogue_combat_swords_human.yaml", 4),
    ] {
        let mut session = session_of(file, seed);
        let frames = play(&mut session, 0.37);
        let numbers: Vec<&DamageNumber> = frames
            .iter()
            .flat_map(|f| &f.damage)
            .filter(|hit| !hit.proc)
            .collect();

        let setup = setup(file);
        let settings = settings(&setup);
        let mut raid = setup.build_raid(data(), &settings).unwrap();
        let log = run_logged_iteration(&settings, seed, &mut raid);
        let logged: Vec<DamageNumber> = log
            .entries()
            .iter()
            .filter_map(|entry| session.damage_number(entry.time, &entry.event))
            .collect();

        assert!(logged.len() > 50, "{file}: {}", logged.len());
        assert_eq!(numbers, logged.iter().collect::<Vec<_>>(), "{file}");
        let last = frames.last().unwrap();
        assert_eq!(last.total_damage, log.total_damage(), "{file}");
        assert_eq!(last.time, session.info().end_at, "{file}");
        assert!(last.dps > 0.0);
    }
}

#[test]
fn the_buff_uptimes_at_the_end_are_the_ones_csim_run_reports() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 3);
    assert!(
        session.advance(0.0).buff_uptimes.is_empty(),
        "before the pull"
    );
    let early = session.advance(20.0).buff_uptimes;
    assert!(!early.is_empty());
    assert!(
        early
            .iter()
            .all(|uptime| uptime.row.uptime > 0.0 && uptime.row.uptime <= 1.0),
        "{early:?}"
    );
    let last = play(&mut session, 7.0).pop().unwrap();

    let setup = setup("warrior_fury_dw_orc.yaml");
    let settings = settings(&setup);
    let mut raid = setup.build_raid(data(), &settings).unwrap();
    run_logged_iteration(&settings, 3, &mut raid);
    let mut reported =
        csim_engine::statistics::report::buff_rows(raid.character(PLAYER).statistics());
    // Applied before the pull, they count from it.
    for row in &mut reported {
        row.uptime = row.uptime.min(1.0);
    }
    let uptimes = |rows: &[BuffRow]| -> Vec<(String, bool, f64)> {
        let mut rows: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.name.clone(),
                    row.debuff,
                    (row.uptime * 1e6).round() / 1e6,
                )
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    };
    assert!(reported.len() > 3, "{reported:?}");
    let shown: Vec<BuffRow> = last
        .buff_uptimes
        .iter()
        .map(|uptime| uptime.row.clone())
        .collect();
    assert_eq!(uptimes(&shown), uptimes(&reported));
    assert!(last.buff_uptimes.iter().any(|uptime| uptime.icon.is_some()));
}

#[test]
fn the_procs_at_the_end_are_the_ones_csim_run_reports() {
    for (file, seed) in [
        ("warrior_fury_dw_orc.yaml", 3),
        ("rogue_combat_swords_human.yaml", 4),
    ] {
        let mut session = session_of(file, seed);
        assert!(session.advance(0.0).procs.is_empty(), "before the pull");
        let last = play(&mut session, 7.0).pop().unwrap();

        let setup = setup(file);
        let settings = settings(&setup);
        let mut raid = setup.build_raid(data(), &settings).unwrap();
        run_logged_iteration(&settings, seed, &mut raid);
        let statistics = raid.take_statistics();
        let reported = csim_engine::statistics::report::proc_rows(&statistics[0]);
        let rounded = |rows: &[ProcRow]| -> Vec<(String, f64, f64, f64)> {
            let round = |value: f64| (value * 1e6).round() / 1e6;
            rows.iter()
                .map(|row| {
                    (
                        row.name.clone(),
                        row.per_fight,
                        round(row.proc_rate),
                        round(row.ppm),
                    )
                })
                .collect()
        };
        assert!(reported.len() > 1, "{file}: {reported:?}");
        let shown: Vec<ProcRow> = last.procs.iter().map(|count| count.row.clone()).collect();
        assert_eq!(rounded(&shown), rounded(&reported), "{file}");
        assert!(
            last.procs.iter().any(|count| count.icon.is_some()),
            "{file}"
        );
    }
}

#[test]
fn the_resources_at_the_end_are_the_ones_csim_run_reports() {
    use csim_engine::statistics::report::{ResourceRow, resource_rows, resource_totals};
    for (file, seed) in [
        ("warrior_fury_dw_orc.yaml", 3),
        ("rogue_combat_swords_human.yaml", 4),
    ] {
        let mut session = session_of(file, seed);
        let first = session.advance(0.0);
        assert!(first.resources.is_empty(), "before the pull");
        assert!(first.resource_totals.is_empty(), "before the pull");
        let last = play(&mut session, 7.0).pop().unwrap();

        let setup = setup(file);
        let settings = settings(&setup);
        let mut raid = setup.build_raid(data(), &settings).unwrap();
        run_logged_iteration(&settings, seed, &mut raid);
        let statistics = &raid.take_statistics()[0];
        let reported = resource_rows(statistics);
        let rounded = |rows: &[ResourceRow]| -> Vec<(String, String, f64, f64)> {
            let round = |value: f64| (value * 1e6).round() / 1e6;
            rows.iter()
                .map(|row| {
                    let (source, resource) = (row.source.clone(), row.resource.clone());
                    (
                        source,
                        resource,
                        round(row.per_fight),
                        round(row.per_second),
                    )
                })
                .collect()
        };
        assert!(reported.len() > 1, "{file}: {reported:?}");
        let shown: Vec<ResourceRow> = last.resources.iter().map(|gain| gain.row.clone()).collect();
        assert_eq!(rounded(&shown), rounded(&reported), "{file}");
        let totals = resource_totals(
            &reported,
            |kind| statistics.lost_at_cap(kind),
            statistics.iterations(),
            statistics.time_in_combat(),
        );
        let round = |value: f64| (value * 1e6).round() / 1e6;
        for (shown, reported) in last.resource_totals.iter().zip(&totals) {
            assert_eq!(shown.resource, reported.resource, "{file}");
            assert_eq!(round(shown.per_fight), round(reported.per_fight), "{file}");
            assert_eq!(
                round(shown.lost_at_cap_per_fight),
                round(reported.lost_at_cap_per_fight),
                "{file}"
            );
        }
        assert_eq!(last.resource_totals.len(), totals.len(), "{file}");
        assert!(
            last.resources.iter().any(|gain| gain.icon.is_some()),
            "{file}"
        );
    }
}

#[test]
fn a_spell_is_affordable_with_the_rage_it_costs() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 3);
    let mut seen = Vec::new();
    for frame in play(&mut session, 0.25) {
        let state = &frame.state;
        let bloodthirst = state
            .rotation_spells
            .iter()
            .find(|spell| spell.name == "Bloodthirst")
            .expect("in the rotation");
        // Bloodthirst costs 30 rage.
        assert_eq!(
            bloodthirst.affordable,
            state.resource.current >= 30,
            "{}",
            frame.time
        );
        seen.push(bloodthirst.affordable);
    }
    assert!(seen.contains(&true) && seen.contains(&false));
}

#[test]
fn a_spell_is_usable_when_its_demands_beyond_rage_hold() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 3);
    let end = session.info().end_at;
    let mut executable = Vec::new();
    for frame in play(&mut session, 0.25) {
        let state = &frame.state;
        let usable = |name: &str| {
            state
                .rotation_spells
                .iter()
                .find(|spell| spell.name == name)
                .expect("in the rotation")
                .usable
        };
        // In execute range: the last 20 % of the fight (by the engine's clock, at the last event
        // before the time shown: not checked right at the edge).
        let in_range = (end - frame.time) / end <= 0.2;
        if (frame.time - 0.8 * end).abs() > 0.3 {
            assert_eq!(usable("Execute"), in_range, "{}", frame.time);
        }
        executable.push(usable("Execute"));
        // Overpower needs Battle Stance (and a dodge); Whirlwind is fine in Berserker Stance.
        if state.stance == Some("Berserker Stance") {
            assert!(!usable("Overpower"), "{}", frame.time);
            assert!(usable("Whirlwind"), "{}", frame.time);
        }
    }
    assert!(executable.contains(&true) && executable.contains(&false));
}

#[test]
fn the_breakdown_adds_up_to_the_damage_so_far() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 3);
    assert!(session.advance(0.0).breakdown.is_empty(), "before the pull");
    for time in [10.0, 30.0] {
        let frame = session.advance(time);
        let rows = &frame.breakdown;
        assert!(rows.len() > 3, "{rows:?}");
        let damage: f64 = rows.iter().map(|row| row.dps * frame.time).sum();
        assert!(
            (damage - frame.total_damage as f64).abs() < 1e-6 * damage,
            "{damage} {}",
            frame.total_damage
        );
        let share: f64 = rows.iter().map(|row| row.damage_share).sum();
        assert!((share - 1.0).abs() < 1e-9, "{share}");
        assert!(
            rows.iter()
                .any(|row| row.name.starts_with("Bloodthirst") && row.casts >= 1.0)
        );
    }
}

#[test]
fn advancing_runs_nothing_past_the_time_shown() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 1);
    let mut shown = f64::NEG_INFINITY;
    for frame in play(&mut session, 0.5) {
        assert!(frame.time >= shown);
        assert!(frame.damage.iter().all(|hit| hit.time > shown - 1e-9));
        assert!(frame.damage.iter().all(|hit| hit.time <= frame.time));
        assert!(frame.damage.is_sorted_by(|a, b| a.time <= b.time));
        shown = frame.time;
    }
    // Going back shows the same time and nothing new.
    let mut fresh = session_of("warrior_fury_dw_orc.yaml", 1);
    let ahead = fresh.advance(10.0);
    let back = fresh.advance(5.0);
    assert_eq!(back.time, ahead.time);
    assert!(back.damage.is_empty());
}

#[test]
fn white_swings_are_auto_attacks_and_spells_are_not() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 2);
    let numbers: Vec<DamageNumber> = play(&mut session, 1.0)
        .into_iter()
        .flat_map(|frame| frame.damage)
        .collect();
    let names = |auto: bool| -> Vec<&str> {
        numbers
            .iter()
            .filter(|hit| hit.auto == auto)
            .map(|hit| hit.name.as_str())
            .collect()
    };
    let white = names(true);
    assert!(white.contains(&"Main hand") && white.contains(&"Off hand"));
    assert!(
        white
            .iter()
            .all(|&name| name == "Main hand" || name == "Off hand")
    );
    let yellow = names(false);
    assert!(yellow.contains(&"Bloodthirst"), "{yellow:?}");
    assert!(!yellow.contains(&"Main hand"));
}

#[test]
fn a_warrior_has_a_stance_and_a_rogue_combo_points() {
    let mut warrior = session_of("warrior_fury_dw_orc.yaml", 1);
    let state = warrior.advance(20.0).state;
    assert_eq!(state.resource.kind, "Rage");
    assert_eq!(state.combo_points, None);
    assert!(state.stance.is_some());
    assert!(state.offhand.is_some());
    assert!(state.mainhand.next >= 20.0 && state.mainhand.last <= 20.0);
    let shout = state.buffs.iter().find(|buff| buff.name == "Battle Shout");
    let shout = shout.expect("Battle Shout is up");
    let expires_at = shout.expires_at.unwrap();
    assert!(expires_at > 20.0 && expires_at <= shout.duration.unwrap());
    assert!(
        state
            .rotation_spells
            .iter()
            .any(|cd| cd.name == "Bloodthirst"),
        "{:?}",
        state.rotation_spells
    );
    assert_eq!(state.gcd, 1.5);
    let on_gcd = |name: &str| {
        state
            .rotation_spells
            .iter()
            .find(|cd| cd.name == name)
            .unwrap()
            .on_gcd
    };
    assert!(on_gcd("Bloodthirst") && on_gcd("Whirlwind"));
    assert!(!on_gcd("Bloodrage"));

    // Before anything was used, every cooldown is ready (none counts down to the pull).
    let fresh = session_of("warrior_fury_dw_orc.yaml", 1)
        .advance(-1.0)
        .state;
    // The rotation's spells without a cooldown too.
    for name in ["Heroic Strike", "Hamstring"] {
        let spell = fresh.rotation_spells.iter().find(|cd| cd.name == name);
        let spell = spell.unwrap_or_else(|| panic!("{name}: {:?}", fresh.rotation_spells));
        assert_eq!((spell.duration, spell.ready_at), (0.0, None), "{name}");
    }
    let bloodthirst = fresh
        .rotation_spells
        .iter()
        .find(|cd| cd.name == "Bloodthirst");
    assert_eq!(bloodthirst.unwrap().ready_at, None);
    let bloodrage = fresh
        .rotation_spells
        .iter()
        .find(|cd| cd.name == "Bloodrage");
    assert_eq!(
        bloodrage.unwrap().ready_at,
        Some(-1.5 + 60.0),
        "cast before the pull"
    );

    let mut rogue = session_of("rogue_combat_swords_human.yaml", 1);
    let state = rogue.advance(20.0).state;
    assert_eq!(state.resource.kind, "Energy");
    assert_eq!(state.resource.max, 100);
    assert!(state.combo_points.is_some());
    assert!(state.buffs.iter().any(|buff| buff.name == "Slice and Dice"));
}

#[test]
fn energy_regenerates_between_events() {
    let mut session = session_of("rogue_combat_swords_human.yaml", 1);
    let mut regenerated_without_event = 0;
    let mut previous: Option<(f64, u32)> = None;
    let mut time = session.info().start_at;
    loop {
        let frame = session.advance(time);
        if frame.done {
            break;
        }
        time += 0.05;
        let engine_time = session.raid.engine().current_time();
        let energy = frame.state.resource.current;
        assert_eq!(
            energy,
            session
                .raid
                .character(PLAYER)
                .resource()
                .current(frame.time),
            "the energy at the time shown, {}",
            frame.time
        );
        if previous.is_some_and(|(at, before)| at == engine_time && energy > before) {
            regenerated_without_event += 1;
        }
        previous = Some((engine_time, energy));
    }
    assert!(regenerated_without_event > 0);
}

#[test]
fn restarting_with_the_same_seed_shows_the_same_iteration() {
    let mut session = session_of("rogue_combat_swords_human.yaml", 6);
    let first = play(&mut session, 0.25);
    let info = session.info();
    session.restart(7);
    assert_eq!(session.info().seed, 7);
    assert_ne!(play(&mut session, 0.25), first);
    session.restart(6);
    assert_eq!(session.info(), info);
    assert_eq!(play(&mut session, 0.25), first);
}

#[test]
fn stepping_runs_one_event_or_up_to_a_cast() {
    let mut session = session_of("warrior_fury_dw_orc.yaml", 1);
    let frame = session.step_event();
    assert!(frame.event.is_some());
    let mut casts = 0;
    while !session.done() {
        let read = session.read;
        let frame = session.step_cast();
        let cast = session.log()[read..].iter().any(|entry| {
            matches!(entry.event, CombatLogEvent::SpellCastSuccess { .. })
                && entry.source == LogUnit::Character(PLAYER)
        });
        assert!(cast || frame.done);
        casts += usize::from(cast);
    }
    assert!(casts > 10, "{casts}");
    assert_eq!(session.step_event().event, None);
}

#[test]
fn hits_buffs_and_cooldowns_carry_their_icons() {
    let bloodthirst = Icon::new(136012, Some("spell_nature_bloodlust"));
    // High Warlord's Bludgeon, in both hands.
    let bludgeon = Icon::new(133057, Some("inv_hammer_20"));

    let mut session = session_of("warrior_fury_dw_orc.yaml", 1);
    let frames = play(&mut session, 1.0);
    let numbers: Vec<&DamageNumber> = frames.iter().flat_map(|f| &f.damage).collect();
    let icons_of = |name: &str| -> Vec<Option<Icon>> {
        let mut icons: Vec<_> = numbers
            .iter()
            .filter(|hit| hit.name == name)
            .map(|hit| hit.icon.clone())
            .collect();
        icons.dedup();
        icons
    };
    assert_eq!(icons_of("Bloodthirst"), std::slice::from_ref(&bloodthirst));
    assert_eq!(icons_of("Main hand"), std::slice::from_ref(&bludgeon));
    assert_eq!(icons_of("Off hand"), [bludgeon]);
    assert!(
        numbers
            .iter()
            .all(|hit| hit.icon.as_ref().is_some_and(|icon| icon.name.is_some())),
        "every hit has one, named"
    );
    assert_eq!(
        serde_json::to_string(&Icon::new(133057, Some("inv_hammer_20"))).unwrap(),
        r#"{"id":133057,"name":"inv_hammer_20"}"#
    );

    let state = session_of("warrior_fury_dw_orc.yaml", 1)
        .advance(20.0)
        .state;
    let cooldown = state
        .rotation_spells
        .iter()
        .find(|cd| cd.name == "Bloodthirst");
    assert_eq!(cooldown.unwrap().icon, bloodthirst);
    let shout = state.buffs.iter().find(|buff| buff.name == "Battle Shout");
    assert!(shout.unwrap().icon.is_some());

    let state = session_of("rogue_combat_swords_human.yaml", 1)
        .advance(20.0)
        .state;
    let slice = state
        .buffs
        .iter()
        .find(|buff| buff.name == "Slice and Dice");
    assert!(slice.unwrap().icon.is_some());
}

#[test]
fn the_rotation_entries_and_decisions() {
    for file in ["warrior_fury_dw_orc.yaml", "rogue_combat_swords_human.yaml"] {
        let mut session = session_of(file, 2);
        let info = session.info();
        assert!(info.cast_if.len() > 5, "{file}");
        assert!(
            info.cast_if
                .iter()
                .enumerate()
                .all(|(index, entry)| entry.position == index + 1)
        );
        let active = |position: usize| info.cast_if[position - 1].skipped.is_none();

        let frames = play(&mut session, 0.5);
        let mut shown = f64::NEG_INFINITY;
        let mut decisions = Vec::new();
        for frame in &frames {
            for decision in &frame.decisions {
                assert!(
                    decision.time <= frame.time,
                    "{file}: never ahead of the frame"
                );
                assert!(decision.time >= shown - 1e-9, "{file}: in time order");
                shown = decision.time;
            }
            decisions.extend(frame.decisions.iter().cloned());
        }

        let precombat: Vec<&str> = decisions
            .iter()
            .filter(|d| d.by == "precombat")
            .map(|d| d.spell.as_str())
            .collect();
        // Not the stance the character starts in: a stance spell does nothing in its stance.
        let expected: Vec<&str> = info
            .precombat
            .iter()
            .map(String::as_str)
            .filter(|&spell| spell != "Battle Stance")
            .collect();
        assert_eq!(precombat, expected, "{file}");
        let by_entry: Vec<&Decision> = decisions.iter().filter(|d| d.by == "entry").collect();
        // A minute: the rogue is energy bound (~30 casts), the warrior casts more.
        assert!(by_entry.len() > 20, "{file}: {}", by_entry.len());
        for decision in &by_entry {
            let position = decision.entry.unwrap();
            assert!(active(position), "{file}: #{position} is skipped");
            assert_eq!(decision.spell, info.cast_if[position - 1].spell, "{file}");
            assert!(decision.icon.is_some());
        }
        assert!(
            by_entry
                .iter()
                .any(|d| info.cast_if[d.entry.unwrap() - 1].condition.is_some()),
            "{file}: some decisions come from conditional entries"
        );
    }

    // DW Fury: Bloodthirst is decided by its own entry, whose condition is as written.
    let session = session_of("warrior_fury_dw_orc.yaml", 2);
    let info = session.info();
    let bloodrage = info
        .cast_if
        .iter()
        .find(|e| e.spell == "Bloodrage")
        .unwrap();
    assert!(
        bloodrage
            .condition
            .as_deref()
            .unwrap()
            .contains("resource \"Rage\"")
    );
    assert!(bloodrage.icon.is_some());
}

#[test]
fn avoided_attacks_are_in_the_feed() {
    let mut avoided = Vec::new();
    for seed in 1..=3 {
        let mut session = session_of("warrior_fury_dw_orc.yaml", seed);
        let frames = play(&mut session, 1.0);
        let numbers: Vec<DamageNumber> = frames.into_iter().flat_map(|f| f.damage).collect();
        let shown = numbers.iter().filter(|hit| hit.miss.is_some()).count();
        let logged = session
            .log()
            .iter()
            .filter(|entry| entry.source == LogUnit::Character(PLAYER))
            .filter(|entry| {
                matches!(
                    entry.event,
                    CombatLogEvent::SwingMissed { .. } | CombatLogEvent::SpellMissed { .. }
                )
            })
            .count();
        assert_eq!(shown, logged, "seed {seed}");
        avoided.extend(numbers.into_iter().filter(|hit| hit.miss.is_some()));
    }
    for hit in &avoided {
        assert_eq!(hit.amount, 0);
        assert!(!hit.critical && !hit.glancing);
        assert!(hit.icon.is_some(), "{}", hit.name);
    }
    let kinds = |auto: bool| -> Vec<&str> {
        let mut kinds: Vec<&str> = avoided
            .iter()
            .filter(|hit| hit.auto == auto)
            .map(|hit| hit.miss.unwrap())
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    };
    let white = kinds(true);
    assert!(
        white.contains(&"Miss") && white.contains(&"Dodge"),
        "{white:?}"
    );
    assert!(!kinds(false).is_empty(), "some spell is avoided");
}

#[test]
fn procs_are_in_the_feed() {
    for (file, seed) in [
        ("warrior_fury_dw_orc.yaml", 3),
        ("rogue_combat_swords_human.yaml", 4),
    ] {
        let mut session = session_of(file, seed);
        let frames = play(&mut session, 0.37);
        let feed: Vec<DamageNumber> = frames.into_iter().flat_map(|f| f.damage).collect();
        let shown = |name: &str| {
            feed.iter()
                .filter(|hit| hit.proc && hit.name == name)
                .count()
        };

        let procs = session.raid.character(PLAYER).spells().procs().procs();
        let fired: Vec<&Proc> = procs.iter().filter(|proc| proc.procs() > 0).collect();
        assert!(fired.len() > 1, "{file}");
        // Procs of the same name (a poison on each weapon) add up.
        for proc in &fired {
            let count: u32 = fired
                .iter()
                .filter(|other| other.name() == proc.name())
                .map(|other| other.procs())
                .sum();
            assert_eq!(
                shown(proc.name()),
                count as usize,
                "{file}: {}",
                proc.name()
            );
        }
        for hit in feed.iter().filter(|hit| hit.proc) {
            assert_eq!(hit.amount, 0);
            assert!(hit.miss.is_none() && !hit.critical && !hit.auto);
        }
    }

    // Windfury Totem: between the hit that procced it (a swing or a spell like Bloodthirst)
    // and the extra main-hand swing (a queued Heroic Strike when one is queued).
    let mut session = session_of("warrior_fury_dw_orc.yaml", 3);
    let feed: Vec<DamageNumber> = play(&mut session, 0.37)
        .into_iter()
        .flat_map(|f| f.damage)
        .collect();
    let windfury: Vec<usize> = feed
        .iter()
        .enumerate()
        .filter(|(_, hit)| hit.proc && hit.name == "Windfury Totem")
        .map(|(index, _)| index)
        .collect();
    assert!(windfury.len() > 5, "{}", windfury.len());
    for index in windfury {
        let at = feed[index].time;
        let before = feed[..index].iter().rev().find(|hit| !hit.proc).unwrap();
        assert_eq!(before.time, at, "{before:?}");
        let after = feed[index + 1..].iter().find(|hit| !hit.proc).unwrap();
        assert_eq!(after.time, at, "{:?}", &feed[index..index + 4]);
    }
}

#[test]
fn the_target_debuffs_are_the_sims_and_the_setups() {
    let state = session_of("warrior_fury_dw_orc.yaml", 1)
        .advance(20.0)
        .state;
    let names: Vec<&str> = state.debuffs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Deep Wound", "Sunder Armor", "Faerie Fire"]);
    let deep_wound = &state.debuffs[0];
    assert!(deep_wound.expires_at.unwrap() > 20.0);
    let sunder = &state.debuffs[1];
    assert_eq!(
        (sunder.stacks, sunder.expires_at),
        (5, None),
        "external: permanent"
    );
    assert!(state.debuffs.iter().all(|debuff| debuff.icon.is_some()));
    assert!(
        !state
            .buffs
            .iter()
            .any(|buff| names.contains(&buff.name.as_str()))
    );

    let rogue = session_of("rogue_combat_swords_human.yaml", 1)
        .advance(20.0)
        .state;
    let names: Vec<&str> = rogue.debuffs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Deadly Poison V", "Sunder Armor", "Faerie Fire"]);
}

const KEYBINDS: &str = "Bloodthirst: 1\nHamstring: Shift+2\nBattle Shout: Ctrl+Alt+B\n";

#[test]
fn played_from_the_keyboard_nothing_is_cast_without_input() {
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, KEYBINDS);
    let info = session.info();
    assert!(info.manual);
    assert_eq!(info.precombat, Vec::<String>::new());
    let bound: Vec<(&str, &str)> = info
        .keybinds
        .iter()
        .map(|keybind| (keybind.name.as_str(), keybind.binding.as_str()))
        .collect();
    assert_eq!(
        bound,
        [
            ("Bloodthirst", "1"),
            ("Hamstring", "Shift+2"),
            ("Battle Shout", "Ctrl+Alt+B")
        ]
    );
    assert!(info.keybinds.iter().all(|keybind| keybind.icon.is_some()));

    let frames = play(&mut session, 0.5);
    assert!(frames.iter().all(|frame| frame.decisions.is_empty()));
    let state = &frames.last().unwrap().state;
    let names: Vec<&str> = state
        .rotation_spells
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Bloodthirst", "Hamstring", "Battle Shout"],
        "the bound spells"
    );
    let hits = frames.iter().flat_map(|frame| &frame.damage);
    assert!(hits.clone().any(|hit| hit.auto), "the auto attacks run");
    assert!(
        !hits
            .clone()
            .any(|hit| !hit.auto && !hit.proc && hit.name == "Bloodthirst")
    );
}

#[test]
fn a_key_press_casts_its_spell_now_or_within_the_queue_window() {
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, KEYBINDS);
    session.advance(9.0);
    let frame = session.cast("Hamstring", 10.0).unwrap();
    assert_eq!(frame.time, 10.0);
    let inputs = |frame: &Frame| -> Vec<(f64, String)> {
        frame
            .decisions
            .iter()
            .inspect(|decision| assert_eq!(decision.by, "input"))
            .map(|decision| (decision.time, decision.spell.clone()))
            .collect()
    };
    assert_eq!(inputs(&frame), [(10.0, "Hamstring".to_owned())]);
    // During its global cooldown (to 11.5): queued, cast once the GCD ends.
    let queued = session.cast("Hamstring", 11.2).unwrap();
    assert_eq!(inputs(&queued), []);
    let later = session.advance(12.0);
    assert_eq!(inputs(&later), [(11.5, "Hamstring".to_owned())]);

    let error = session.cast("Whirlwind", 13.0).unwrap_err();
    assert!(error.contains("not bound"), "{error}");
    let error = session_of("warrior_fury_dw_orc.yaml", 3).cast("Hamstring", 1.0);
    assert!(error.is_err(), "played by the rotation");
}

#[test]
fn an_unlearned_bound_spell_is_refused() {
    let setup = setup("warrior_fury_dw_orc.yaml");
    let settings = settings(&setup);
    let keybinds = crate::keybinds::parse("Mortal Strike: 1\n").unwrap();
    let error = Session::new(Arc::clone(data()), setup, settings, 1, keybinds)
        .err()
        .unwrap();
    assert!(error.contains("Mortal Strike"), "{error}");
}

#[test]
fn the_shipped_keybinds_bind_their_characters_spells() {
    for (file, keybinds) in [
        ("warrior_fury_dw_orc.yaml", "dw_fury.yaml"),
        ("rogue_combat_swords_human.yaml", "combat.yaml"),
    ] {
        let path = DataBundle::repository_dir().join("keybinds").join(keybinds);
        let text = std::fs::read_to_string(&path).unwrap();
        let session = manual_session_of(file, 1, &text);
        let info = session.info();
        assert!(info.keybinds.len() > 5, "{keybinds}");
        assert!(
            info.keybinds.iter().all(|keybind| keybind.icon.is_some()),
            "{keybinds}"
        );
    }
}

#[test]
fn a_press_that_cannot_cast_says_why() {
    let mut session = manual_session_of(
        "warrior_fury_dw_orc.yaml",
        3,
        "Whirlwind: 2\nHamstring: 4\n",
    );
    // No precombat actions from the keyboard: still in Battle Stance.
    let frame = session.cast("Whirlwind", 3.0).unwrap();
    let error = frame.input_error.expect("dropped at once");
    assert_eq!(error.spell, "Whirlwind");
    assert_eq!(error.reason, "Can't do that in Battle Stance");
    assert_eq!(session.advance(3.5).input_error, None, "reported once");

    session.cast("Hamstring", 10.0).unwrap();
    // 1 s into the GCD: waits, and is dropped when its window closes before the GCD ends.
    let waiting = session.cast("Hamstring", 10.5).unwrap();
    assert_eq!(waiting.input_error, None);
    let dropped = session.advance(11.0).input_error.unwrap();
    assert_eq!(dropped.reason, "Ability is not ready yet");
}

#[test]
fn the_queue_window_covers_the_stance_cooldown_as_the_gcd() {
    let keybinds = "Whirlwind: 3\nBerserker Stance: Shift+2\n";
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, keybinds);
    let swap = session.cast("Berserker Stance", 4.0).unwrap();
    assert_eq!(swap.decisions[0].spell, "Berserker Stance");
    // The swap's stance cooldown ends at 5.0: 0.8 s after this press, beyond its window.
    let early = session.cast("Whirlwind", 4.2).unwrap();
    assert!(early.decisions.is_empty());
    let dropped = session.advance(4.7).input_error.expect("dropped");
    assert_eq!(dropped.reason, "Ability is not ready yet");
    // 0.3 s before it ends: cast when it does.
    let pressed = session.cast("Whirlwind", 4.7).unwrap();
    assert!(pressed.decisions.is_empty());
    let later = session.advance(6.0);
    assert_eq!(later.input_error, None);
    let cast: Vec<(f64, &str)> = later
        .decisions
        .iter()
        .map(|decision| (decision.time, decision.spell.as_str()))
        .collect();
    assert_eq!(cast, [(5.0, "Whirlwind")]);
}

#[test]
fn a_macro_press_casts_its_entries_up_to_the_gcd() {
    let keybinds = "Burst:\n  hotkey: T\n  cast: [Bloodrage, Bloodthirst, Heroic Strike]\n";
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, keybinds);
    let info = session.info();
    let burst = &info.keybinds[0];
    assert!(burst.is_macro);
    assert_eq!(burst.spells, ["Bloodrage", "Bloodthirst", "Heroic Strike"]);
    let bloodthirst_icon = Icon::new(136012, Some("spell_nature_bloodlust"));
    assert_eq!(burst.icon, bloodthirst_icon, "its first GCD spell's");

    let frame = session.cast("Burst", 10.0).unwrap();
    let cast: Vec<(f64, &str)> = frame
        .decisions
        .iter()
        .map(|decision| (decision.time, decision.spell.as_str()))
        .collect();
    assert_eq!(cast, [(10.0, "Bloodrage"), (10.0, "Bloodthirst")]);
    let later = session.advance(12.0);
    assert!(later.decisions.is_empty(), "Heroic Strike never fires");
    let tile = &later.state.rotation_spells[0];
    assert_eq!(tile.name, "Burst");
    assert_eq!(tile.duration, 6.0, "Bloodthirst's cooldown");
}

const PULL_KEYBINDS: &str = "Bloodrage: 1
Charge: 2
";
const CHARGE: u32 = 11578;

/// The decisions of `frame` as (time, spell).
fn casts(frame: &Frame) -> Vec<(f64, &str)> {
    frame
        .decisions
        .iter()
        .map(|decision| (decision.time, decision.spell.as_str()))
        .collect()
}

#[test]
fn from_the_keyboard_the_player_has_minutes_to_pull() {
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, PULL_KEYBINDS);
    assert_eq!(session.info().start_at, -MANUAL_PRE_PULL);
    // Bloodrage does not pull.
    let bloodrage = session.cast("Bloodrage", -595.0).unwrap();
    assert_eq!(casts(&bloodrage), [(-595.0, "Bloodrage")]);
    let later = session.advance(-500.0);
    assert_eq!(later.rebased_by, None);
    assert_eq!(later.time, -500.0);
    assert_eq!(session.info().start_at, -MANUAL_PRE_PULL);
}

#[test]
fn an_offensive_press_pulls_when_it_lands() {
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, PULL_KEYBINDS);
    session.cast("Bloodrage", -595.0).unwrap();
    // Charge's 1 s cast lands at -589: the pull, which moves every time by 589 s.
    let charge = session.cast("Charge", -590.0).unwrap();
    assert_eq!(charge.rebased_by, Some(-589.0));
    assert_eq!(charge.time, -1.0);
    assert_eq!(session.info().start_at, -11.0);
    // The frame holds the iteration since its start again: the replayed presses.
    assert_eq!(casts(&charge), [(-6.0, "Bloodrage"), (-1.0, "Charge")]);

    let pulled = session.advance(0.5);
    assert_eq!(pulled.rebased_by, None, "shown once");
    let charge_lands = session.log().iter().find(|entry| {
        matches!(entry.event, CombatLogEvent::SpellCastSuccess { .. })
            && logged_spell(&entry.event) == Some(CHARGE)
    });
    assert_eq!(charge_lands.unwrap().time, 0.0, "Charge lands at the pull");
    let first_swing = pulled.damage.iter().find(|hit| hit.auto).unwrap();
    assert_eq!(first_swing.time, 0.0, "the auto attacks start at the pull");
    // Bloodrage's 10 s, from 6 s before the pull.
    let bloodrage = pulled
        .state
        .buffs
        .iter()
        .find(|buff| buff.name == "Bloodrage")
        .unwrap();
    assert_eq!(bloodrage.expires_at, Some(4.0));

    // A press in combat does not pull again.
    let again = session.cast("Bloodrage", 2.0).unwrap();
    assert_eq!(again.rebased_by, None);
}

#[test]
fn restarting_brings_the_pre_pull_back() {
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, PULL_KEYBINDS);
    // Charge pressed 300 s after the start lands at -299: 301 s after it.
    session.cast("Charge", -300.0).unwrap();
    assert_eq!(session.info().start_at, -301.0);
    session.restart(3);
    assert_eq!(session.info().start_at, -MANUAL_PRE_PULL);
    let charge = session.cast("Charge", -100.0).unwrap();
    assert_eq!(charge.rebased_by, Some(-99.0));
    assert_eq!(casts(&charge), [(-1.0, "Charge")]);
}

#[test]
fn before_the_pull_nothing_waits_for_it() {
    let keybinds = "Bloodrage: 1
Battle Shout: 2
Blood Fury: 3
";
    let mut session = manual_session_of("warrior_fury_dw_orc.yaml", 3, keybinds);
    // Bloodrage's rage pays for Battle Shout minutes before the pull: no cooldown, global
    // cooldown or trinket cooldown waits for it.
    session.cast("Bloodrage", -595.0).unwrap();
    let shout = session.cast("Battle Shout", -594.0).unwrap();
    assert_eq!(shout.input_error, None);
    assert_eq!(casts(&shout), [(-594.0, "Battle Shout")]);
    let fury = session.cast("Blood Fury", -593.0).unwrap();
    assert_eq!(casts(&fury), [(-593.0, "Blood Fury")]);
    assert!(fury.state.gcd_end.is_finite());
}

/// A named setting (`--setting`) holds across the keyboard pull's rebuild and a restart, and
/// the page's info names it.
#[test]
fn named_settings_hold_across_the_pull_and_a_restart() {
    use csim_engine::named_settings::parse_setting_pairs;

    let setup = setup("warrior_fury_dw_orc.yaml");
    let mut settings = settings(&setup);
    settings
        .apply_settings(&parse_setting_pairs("rage_formula:marrow_sigmoid").unwrap())
        .unwrap();
    let keybinds = crate::keybinds::parse(PULL_KEYBINDS).unwrap();
    let mut session = Session::new(Arc::clone(data()), setup, settings, 3, keybinds).unwrap();
    let scaled = |session: &Session| session.raid.character(PLAYER).swing_rage_factor() > 1.0;
    assert!(scaled(&session));
    assert_eq!(
        session.info().settings.as_deref(),
        Some(
            "rage_formula:marrow_sigmoid,sigmoid_floor:0,sigmoid_ceiling:46,\
             sigmoid_midpoint:58,sigmoid_width:3.8"
        )
    );

    let charge = session.cast("Charge", -590.0).unwrap();
    assert_eq!(charge.rebased_by, Some(-589.0));
    assert!(scaled(&session), "after the pull's rebuild");
    session.restart(3);
    assert!(scaled(&session), "after a restart");

    let forever = session_of("warrior_fury_dw_orc.yaml", 3);
    assert_eq!(forever.raid.character(PLAYER).swing_rage_factor(), 1.0);
    assert_eq!(forever.info().settings, None);
}
