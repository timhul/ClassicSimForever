//! Raid loader tests against the shipped data, characters and raids.

use std::sync::OnceLock;

use super::*;
use crate::ids::CharId;
use crate::sim_control::SimControl;
use crate::statistics::NumberCruncher;

fn data() -> &'static DataBundle {
    static DATA: OnceLock<DataBundle> = OnceLock::new();
    DATA.get_or_init(|| DataBundle::load(&DataBundle::repository_dir()).unwrap())
}

fn characters() -> PathBuf {
    DataBundle::repository_dir().join("characters")
}

fn character(file: &str) -> CharacterSetup {
    CharacterSetup::load(&characters().join(file)).unwrap()
}

fn settings() -> SimSettings {
    SimSettings {
        combat_length: 60,
        iterations_quick_sim: 10,
        threads: 1,
        ..SimSettings::default()
    }
}

fn raid(yaml: &str) -> RaidSetup {
    serde_yaml::from_str(yaml).unwrap()
}

/// The issues of a raid that must fail to build with `player`.
fn issues(setup: &RaidSetup, player: Option<&CharacterSetup>) -> Vec<SetupIssue> {
    let members = setup.resolve(&characters()).unwrap();
    match setup.build_raid(player, &members, data(), &settings()) {
        Err(RaidSetupError::Invalid { issues, .. }) => issues,
        Err(other) => panic!("unexpected error {other}"),
        Ok(_) => panic!("the raid built"),
    }
}

fn contexts(issues: &[SetupIssue]) -> Vec<&str> {
    issues.iter().map(|i| i.context.as_str()).collect()
}

#[test]
fn every_shipped_raid_validates() {
    let dir = DataBundle::repository_dir();
    let raids = RaidSetup::load_dir(&dir.join("raids")).unwrap();
    assert!(!raids.is_empty());
    for raid in &raids {
        raid.validate(&dir, data())
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

#[test]
fn the_player_comes_first_and_the_members_fill_their_parties() {
    let setup = raid(
        "name: R\nplayer_party: 2\nparties:\n  - [warrior_fury_2h_orc]\n  - [warrior_fury_dw_orc.yaml, warrior_fury_2h_orc]\n",
    );
    let members = setup.resolve(&characters()).unwrap();
    assert_eq!(
        members
            .iter()
            .map(|m| (m.party, m.setup.name.as_str()))
            .collect::<Vec<_>>(),
        [(0, "2h Fury Orc"), (1, "DW Fury Orc"), (1, "2h Fury Orc")]
    );
    let player = character("warrior_fury_dw_orc.yaml");
    let raid = setup
        .build_raid(Some(&player), &members, data(), &settings())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(raid.len(), 4);
    assert_eq!(raid.character_at(1, 0), Some(CharId(0)));
    assert_eq!(raid.character_at(0, 0), Some(CharId(1)));
    assert_eq!(raid.character_at(1, 1), Some(CharId(2)));
    assert_eq!(raid.character_at(1, 2), Some(CharId(3)));
    assert!(raid.character(CharId(0)).equipment().is_dual_wielding());
    assert!(!raid.character(CharId(1)).equipment().is_dual_wielding());
}

#[test]
fn the_raid_provides_the_raid_buffs_and_debuffs() {
    let player = character("warrior_fury_dw_orc.yaml");
    let setup = raid(
        "name: R
parties:
  - [warrior_fury_dw_orc]
",
    );
    let members = setup.resolve(&characters()).unwrap();
    let raid = setup
        .build_raid(Some(&player), &members, data(), &settings())
        .unwrap();
    assert_eq!(raid.target().armor(), raid.target().base_armor());
    for id in [CharId(0), CharId(1)] {
        let buffs = raid.character(id).external_buffs();
        for from_raid in ["Sunder Armor", "Strength of Earth Totem", "Trueshot Aura"] {
            assert!(!buffs.is_selected(from_raid), "{id:?} {from_raid}");
        }
        for consumable in ["Juju Power", "Elixir of the Mongoose", "Grilled Squid"] {
            assert!(buffs.is_selected(consumable), "{id:?} {consumable}");
        }
    }
    // Alone, the setup keeps them.
    let alone = player.build_raid(data(), &settings()).unwrap();
    assert!(alone.target().armor() < alone.target().base_armor());
    assert!(
        alone
            .character(CharId(0))
            .external_buffs()
            .is_selected("Strength of Earth Totem")
    );
}

#[test]
fn a_raid_simulates_with_a_result_per_member() {
    let player = character("warrior_fury_dw_orc.yaml");
    let setup =
        RaidSetup::load(&DataBundle::repository_dir().join("raids/horde_melee.yaml")).unwrap();
    let members = setup.resolve(&characters()).unwrap();
    let settings = player.sim_settings(&settings());
    let mut raid = setup
        .build_raid(Some(&player), &members, data(), &settings)
        .unwrap();
    raid.set_seed(5);
    let mut control = SimControl::new(settings, 1);
    let mut cruncher = NumberCruncher::new();
    control.run_quick_sim(&mut raid, &mut cruncher);
    let results = cruncher.player_results();
    assert_eq!(results.len(), 1 + members.len());
    assert!(results.iter().all(|result| result.dps > 0.0));
    assert!(cruncher.raid_dps() > cruncher.personal_dps(None));
}

#[test]
fn party_and_raid_sizes_are_checked() {
    let full = "name: R\nparties:\n  - [warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc]\n";
    let player = character("warrior_fury_dw_orc.yaml");
    let found = issues(&raid(full), Some(&player));
    assert_eq!(contexts(&found), ["parties.1"]);
    assert!(
        found[0].message.contains("counting the player"),
        "{found:?}"
    );
    // Without a player five fit.
    let members = raid(full).resolve(&characters()).unwrap();
    assert!(
        raid(full)
            .build_raid(None, &members, data(), &settings())
            .is_ok()
    );

    let nine = format!("name: R\nparties:\n{}", "  - []\n".repeat(9));
    assert_eq!(contexts(&issues(&raid(&nine), Some(&player))), ["parties"]);
    let outside = "name: R\nplayer_party: 9\n";
    assert_eq!(
        contexts(&issues(&raid(outside), Some(&player))),
        ["player_party"]
    );
    assert_eq!(
        contexts(&issues(&raid("name: R\n"), None)),
        ["parties"],
        "an empty raid"
    );
}

#[test]
fn a_full_raid_of_forty_builds() {
    let party = "  - [warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc]\n";
    let setup = raid(&format!(
        "name: R\nplayer_party: 8\nparties:\n{}  - [warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc, warrior_fury_dw_orc]\n",
        party.repeat(7)
    ));
    let members = setup.resolve(&characters()).unwrap();
    let raid = setup
        .build_raid(
            Some(&character("warrior_fury_dw_orc.yaml")),
            &members,
            data(),
            &settings(),
        )
        .unwrap();
    assert_eq!(raid.len(), 40);
    assert_eq!(raid.free_place(), None);
}

#[test]
fn members_must_share_the_players_faction() {
    let setup = raid("name: R\nparties:\n  - [warrior_arms_human_swords, warrior_fury_dw_orc]\n");
    let found = issues(&setup, Some(&character("warrior_fury_dw_orc.yaml")));
    assert_eq!(
        contexts(&found),
        ["parties.1[0] (warrior_arms_human_swords)"]
    );
    assert!(found[0].message.contains("Alliance"), "{found:?}");
}

#[test]
fn unknown_references_are_reported_together() {
    let setup = raid(
        "name: R\nparties:\n  - [nobody, ../characters/warrior_fury_dw_orc]\n  - [warrior_fury_dw_orc]\n",
    );
    let Err(RaidSetupError::Invalid { issues, .. }) = setup.resolve(&characters()) else {
        panic!("the references resolved");
    };
    assert_eq!(
        contexts(&issues),
        [
            "parties.1[0] (nobody)",
            "parties.1[1] (../characters/warrior_fury_dw_orc)"
        ]
    );
}

#[test]
fn a_member_problem_names_the_member_and_its_field() {
    let player = character("warrior_fury_dw_orc.yaml");
    let mut broken = character("warrior_fury_dw_orc.yaml");
    broken.rotation = "Nothing".to_string();
    let setup = raid("name: R\nparties:\n  - [warrior_fury_dw_orc]\n");
    let members = vec![RaidMember {
        party: 0,
        reference: "broken".to_string(),
        setup: broken,
    }];
    let Err(RaidSetupError::Invalid { issues, .. }) =
        setup.build_raid(Some(&player), &members, data(), &settings())
    else {
        panic!("the raid built");
    };
    assert_eq!(contexts(&issues), ["parties.1[0] (broken): rotation"]);
}

#[test]
fn unknown_fields_are_parse_errors() {
    assert!(serde_yaml::from_str::<RaidSetup>("name: R\nparty: []\n").is_err());
}
