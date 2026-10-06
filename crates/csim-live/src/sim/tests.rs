//! Sim job tests on the shipped data and character setups.

use csim_engine::faction::PlayerClass;
use csim_engine::race::Race;
use csim_engine::sim_control::{SimMode, run_threaded};

use super::*;
use crate::app::{App, Bare};
use crate::session::tests::empty_app;

fn by_name(setup: &str, seed: &str, iterations: u32) -> SimRequest {
    SimRequest {
        load: LoadRequest {
            setup: Some(setup.to_owned()),
            seed: Some(seed.to_owned()),
            ..LoadRequest::default()
        },
        iterations,
    }
}

/// Starts `request` on `app` and runs it in chunks of `chunk`.
fn run(app: &mut App, request: SimRequest, chunk: u32) -> SimResults {
    app.start_sim(request, || 42).unwrap();
    let job = app.sim_mut().unwrap();
    while !job.is_done() {
        job.step(chunk);
    }
    job.results(Duration::from_secs(1)).unwrap()
}

/// What `csim run -n <iterations> -t 1 --seed <seed>` reports for the shipped setup `file`
/// (taking a second).
fn one_thread_run(app: &App, file: &str, seed: u64, iterations: u32) -> Results {
    let setup = CharacterSetup::load(
        &DataBundle::repository_dir()
            .join("characters")
            .join(format!("{file}.yaml")),
    )
    .unwrap();
    let mut settings = setup.sim_settings(&SimSettings::default());
    settings.iterations_quick_sim = iterations;
    settings.threads = 1;
    let data = app.data();
    let cruncher = run_threaded(&settings, SimMode::Quick, seed, None, || {
        setup.build_raid(data, &settings)
    })
    .unwrap();
    Results::collect(&Report {
        setup: &setup,
        settings: &settings,
        seed,
        elapsed: Duration::from_secs(1),
        cruncher: &cruncher,
        raid: None,
    })
}

#[test]
fn a_sim_has_the_results_of_the_one_thread_run_of_its_seed() {
    let mut app = empty_app();
    let expected = one_thread_run(&app, "warrior_fury_dw_orc", 7, 40);
    for chunk in [1, 13, 40, 1000] {
        let results = run(&mut app, by_name("warrior_fury_dw_orc", "7", 40), chunk);
        assert_eq!(results.results, expected, "chunks of {chunk}");
    }
    assert_eq!(expected.run.threads, 1);
    let rogue = run(&mut app, by_name("rogue_combat_swords_human", "7", 20), 7);
    assert_eq!(
        rogue.results,
        one_thread_run(&app, "rogue_combat_swords_human", 7, 20)
    );
}

#[test]
fn a_sim_counts_its_iterations_and_collects_them_after_the_last() {
    let mut app = empty_app();
    let started = app
        .start_sim(by_name("warrior_fury_dw_orc", "", 25), || 99)
        .unwrap();
    assert_eq!(
        started,
        SimProgress {
            name: "DW Fury Orc".into(),
            seed: 99,
            done: 0,
            total: 25,
        },
        "a new seed without one"
    );
    let job = app.sim_mut().unwrap();
    assert_eq!(job.step(10).done, 10);
    assert!(!job.is_done());
    assert_eq!(job.results(Duration::ZERO), None);
    assert_eq!(job.step(10).done, 20);
    assert_eq!(job.step(10).done, 25, "only what is left");
    assert!(job.is_done());
    assert_eq!(job.step(10).done, 25);
    let results = job.results(Duration::ZERO).unwrap();
    assert_eq!(results.results.run.iterations, 25);
    assert_eq!(results.results.run.seed, 99);

    app.stop_sim();
    assert!(app.sim().is_none());
}

#[test]
fn the_results_carry_the_icons_of_their_rows() {
    let mut app = empty_app();
    let results = run(&mut app, by_name("warrior_fury_dw_orc", "1", 10), 10);
    let icons = &results.icons;
    let icon = |map: &BTreeMap<String, Icon>, name: &str| {
        map.get(name)
            .and_then(|icon| icon.name.clone())
            .unwrap_or_else(|| panic!("no icon for {name}: {map:?}"))
    };
    assert_eq!(
        icon(&icons.spells, "Bloodthirst (rank 4)"),
        "spell_nature_bloodlust"
    );
    // The weapons for the swings; a spell's for its off-hand strike.
    icon(&icons.spells, "Mainhand Attack");
    icon(&icons.spells, "Offhand Attack");
    assert_eq!(
        icon(&icons.spells, "Whirlwind Off-Hand"),
        icon(&icons.spells, "Whirlwind")
    );
    icon(&icons.buffs, "Flurry");
    icon(&icons.procs, "Flurry");
    icon(&icons.resources, "Bloodrage");
    assert_eq!(
        icon(&icons.rotation, "(12) Bloodthirst"),
        "spell_nature_bloodlust"
    );
    // Every spell row of a weapon or a game spell has one.
    for row in &results.results.spells {
        assert!(icons.spells.contains_key(&row.name), "{}", row.name);
    }

    let json = serde_json::to_value(&results).unwrap();
    assert!(json["spells"].is_array(), "the results, flattened");
    assert!(json["icons"]["spells"].is_object());
}

#[test]
fn a_sim_plays_the_rotation_of_any_setup_a_load_takes() {
    let mut app = empty_app();
    // Keybinds are ignored: the rotation plays.
    let mut request = by_name("warrior_fury_dw_orc", "3", 5);
    request.load.keybinds = Some("dw_fury".into());
    let results = run(&mut app, request, 5);
    assert!(!results.results.rotation.is_empty());

    // As another race.
    let mut request = by_name("warrior_fury_dw_orc", "3", 5);
    request.load.race = Some(Race::Human);
    assert_eq!(run(&mut app, request, 5).results.setup.race, "Human");

    // A bare character, whose rotation lacks its prerequisite: the lines are skipped.
    let request = SimRequest {
        load: LoadRequest {
            bare: Some(Bare {
                class: PlayerClass::Warrior,
                race: Race::Human,
                rotation: "DW Fury".into(),
            }),
            ..LoadRequest::default()
        },
        iterations: 5,
    };
    let results = run(&mut app, request, 5).results;
    assert_eq!(results.setup.name, "Human Warrior");
    assert!(
        results
            .skipped_rotation_lines
            .iter()
            .any(|line| line.spell == "Bloodthirst"),
        "{:?}",
        results.skipped_rotation_lines
    );
}

#[test]
fn a_failed_start_keeps_the_sim() {
    let mut app = empty_app();
    app.start_sim(by_name("warrior_fury_dw_orc", "1", 5), || 42)
        .unwrap();
    for (request, message) in [
        (by_name("warrior_fury_dw_orc", "1", 0), "from 1 to 1000000"),
        (
            by_name("warrior_fury_dw_orc", "1", MAX_ITERATIONS + 1),
            "from 1 to",
        ),
        (by_name("no_such_setup", "1", 5), "no_such_setup"),
        (by_name("warrior_fury_dw_orc", "soon", 5), "invalid seed"),
    ] {
        let error = app.start_sim(request, || 42).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    assert_eq!(app.sim().unwrap().progress().total, 5);
}

#[test]
fn a_sim_and_a_session_are_apart() {
    let mut app = empty_app();
    app.load(
        LoadRequest {
            setup: Some("rogue_combat_swords_human".into()),
            ..LoadRequest::default()
        },
        || 1,
    )
    .unwrap();
    run(&mut app, by_name("warrior_fury_dw_orc", "1", 3), 3);
    assert_eq!(app.loaded().unwrap().info.name, "Combat Swords Human");
    assert_eq!(app.sim().unwrap().progress().name, "DW Fury Orc");
}
