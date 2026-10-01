//! Session tests on the shipped data and character setups.

use std::sync::{Arc, OnceLock};

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
    Session::new(Arc::clone(data()), setup, settings, seed).unwrap()
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
    for (file, seed) in [("dw_fury_orc.yaml", 3), ("combat_swords_human.yaml", 4)] {
        let mut session = session_of(file, seed);
        let frames = play(&mut session, 0.37);
        let numbers: Vec<&DamageNumber> = frames.iter().flat_map(|f| &f.damage).collect();

        let setup = setup(file);
        let settings = settings(&setup);
        let mut raid = setup.build_raid(data(), &settings).unwrap();
        let log = run_logged_iteration(&settings, seed, &mut raid);
        let logged: Vec<DamageNumber> = log
            .entries()
            .iter()
            .filter_map(|entry| damage_number(entry.time, &entry.event))
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
fn advancing_runs_nothing_past_the_time_shown() {
    let mut session = session_of("dw_fury_orc.yaml", 1);
    let mut shown = f64::NEG_INFINITY;
    for frame in play(&mut session, 0.5) {
        assert!(frame.time >= shown);
        assert!(frame.damage.iter().all(|hit| hit.time > shown - 1e-9));
        assert!(frame.damage.iter().all(|hit| hit.time <= frame.time));
        assert!(frame.damage.is_sorted_by(|a, b| a.time <= b.time));
        shown = frame.time;
    }
    // Going back shows the same time and nothing new.
    let mut fresh = session_of("dw_fury_orc.yaml", 1);
    let ahead = fresh.advance(10.0);
    let back = fresh.advance(5.0);
    assert_eq!(back.time, ahead.time);
    assert!(back.damage.is_empty());
}

#[test]
fn white_swings_are_auto_attacks_and_spells_are_not() {
    let mut session = session_of("dw_fury_orc.yaml", 2);
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
    let mut warrior = session_of("dw_fury_orc.yaml", 1);
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
        state.cooldowns.iter().any(|cd| cd.name == "Bloodthirst"),
        "{:?}",
        state.cooldowns
    );
    assert_eq!(state.gcd, 1.5);

    // Before anything was used, every cooldown is ready (none counts down to the pull).
    let fresh = session_of("dw_fury_orc.yaml", 1).advance(-1.0).state;
    let bloodthirst = fresh.cooldowns.iter().find(|cd| cd.name == "Bloodthirst");
    assert_eq!(bloodthirst.unwrap().ready_at, None);
    let bloodrage = fresh.cooldowns.iter().find(|cd| cd.name == "Bloodrage");
    assert_eq!(
        bloodrage.unwrap().ready_at,
        Some(-1.5 + 60.0),
        "cast before the pull"
    );

    let mut rogue = session_of("combat_swords_human.yaml", 1);
    let state = rogue.advance(20.0).state;
    assert_eq!(state.resource.kind, "Energy");
    assert_eq!(state.resource.max, 100);
    assert!(state.combo_points.is_some());
    assert!(state.buffs.iter().any(|buff| buff.name == "Slice and Dice"));
}

#[test]
fn restarting_with_the_same_seed_shows_the_same_iteration() {
    let mut session = session_of("combat_swords_human.yaml", 6);
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
    let mut session = session_of("dw_fury_orc.yaml", 1);
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
