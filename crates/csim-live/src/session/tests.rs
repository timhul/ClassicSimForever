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
    let fresh = session_of("dw_fury_orc.yaml", 1).advance(-1.0).state;
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

#[test]
fn hits_buffs_and_cooldowns_carry_their_icons() {
    const BLOODTHIRST: Option<u32> = Some(136012);
    // High Warlord's Bludgeon, in both hands.
    const BLUDGEON: Option<u32> = Some(133057);

    let mut session = session_of("dw_fury_orc.yaml", 1);
    let frames = play(&mut session, 1.0);
    let numbers: Vec<&DamageNumber> = frames.iter().flat_map(|f| &f.damage).collect();
    let icons_of = |name: &str| -> Vec<Option<u32>> {
        let mut icons: Vec<_> = numbers
            .iter()
            .filter(|hit| hit.name == name)
            .map(|hit| hit.icon)
            .collect();
        icons.dedup();
        icons
    };
    assert_eq!(icons_of("Bloodthirst"), [BLOODTHIRST]);
    assert_eq!(icons_of("Main hand"), [BLUDGEON]);
    assert_eq!(icons_of("Off hand"), [BLUDGEON]);
    assert!(
        numbers.iter().all(|hit| hit.icon.is_some()),
        "every hit has one"
    );

    let state = session_of("dw_fury_orc.yaml", 1).advance(20.0).state;
    let cooldown = state
        .rotation_spells
        .iter()
        .find(|cd| cd.name == "Bloodthirst");
    assert_eq!(cooldown.unwrap().icon, BLOODTHIRST);
    let shout = state.buffs.iter().find(|buff| buff.name == "Battle Shout");
    assert!(shout.unwrap().icon.is_some());

    let state = session_of("combat_swords_human.yaml", 1)
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
    for file in ["dw_fury_orc.yaml", "combat_swords_human.yaml"] {
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
        assert_eq!(precombat, info.precombat, "{file}");
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
    let session = session_of("dw_fury_orc.yaml", 2);
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
        let mut session = session_of("dw_fury_orc.yaml", seed);
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
    for (file, seed) in [("dw_fury_orc.yaml", 3), ("combat_swords_human.yaml", 4)] {
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
    let mut session = session_of("dw_fury_orc.yaml", 3);
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
    let state = session_of("dw_fury_orc.yaml", 1).advance(20.0).state;
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

    let rogue = session_of("combat_swords_human.yaml", 1)
        .advance(20.0)
        .state;
    let names: Vec<&str> = rogue.debuffs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Sunder Armor", "Faerie Fire"]);
}
