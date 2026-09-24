//! Character loader tests against the shipped data and the example setups of
//! `data/characters/`.

use std::sync::OnceLock;

use super::*;
use crate::ids::CharId;
use crate::sim_control::SimControl;
use crate::statistics::NumberCruncher;

fn data() -> &'static DataBundle {
    static DATA: OnceLock<DataBundle> = OnceLock::new();
    DATA.get_or_init(|| DataBundle::load(&DataBundle::repository_dir()).unwrap())
}

fn shipped(file: &str) -> CharacterSetup {
    CharacterSetup::load(&DataBundle::repository_dir().join("characters").join(file)).unwrap()
}

fn settings() -> SimSettings {
    SimSettings {
        combat_length: 60,
        iterations_quick_sim: 10,
        threads: 1,
        ..SimSettings::default()
    }
}

/// The issues of a setup that must fail.
fn issues(setup: &CharacterSetup) -> Vec<SetupIssue> {
    match setup.build_raid(data(), &settings()) {
        Err(CharacterSetupError::Invalid { issues, .. }) => issues,
        Err(other) => panic!("unexpected error {other}"),
        Ok(_) => panic!("the setup built"),
    }
}

fn contexts(issues: &[SetupIssue]) -> Vec<&str> {
    issues.iter().map(|i| i.context.as_str()).collect()
}

const MINIMAL: &str = r#"
name: Minimal
class: WARRIOR
race: ORC
rotation: DW Fury High Rage
"#;

fn minimal() -> CharacterSetup {
    serde_yaml::from_str(MINIMAL).unwrap()
}

#[test]
fn every_shipped_setup_builds() {
    let setups =
        CharacterSetup::load_dir(&DataBundle::repository_dir().join("characters")).unwrap();
    assert_eq!(setups.len(), 4);
    for setup in &setups {
        let raid = setup
            .build_raid(data(), &settings())
            .unwrap_or_else(|error| panic!("{error}"));
        let character = raid.character(CharId(0));
        assert_eq!(character.rotation_name(), setup.rotation);
        let talents = character.talents().unwrap();
        assert_eq!(talents.spent_points(), 51, "{}", setup.name);
        for (&slot, equipped) in &setup.equipment {
            assert_eq!(
                character.equipment().item_id(slot),
                Some(equipped.item),
                "{}: {slot:?}",
                setup.name
            );
        }
        let general = character.external_buffs();
        for name in setup.buffs.iter().chain(&setup.debuffs) {
            assert!(general.is_selected(name), "{}: {name}", setup.name);
        }
    }
}

#[test]
fn the_dw_fury_setup_is_built_as_written() {
    let setup = shipped("dw_fury_orc.yaml");
    let raid = setup.build_raid(data(), &settings()).unwrap();
    let character = raid.character(CharId(0));
    assert_eq!(character.race(), Race::Orc);
    assert_eq!(character.clvl(), 60);
    assert!(!character.is_tanking());
    let talents = character.talents().unwrap();
    let flurry = talents.node_of_name("Flurry", None).unwrap();
    assert_eq!(talents.rank(flurry), 5);
    assert!(character.equipment().is_dual_wielding());
    assert_eq!(
        character.equipment().enchant(EquipmentSlot::Mainhand),
        Some(EnchantName::Crusader)
    );
    assert_eq!(
        character.equipment().temp_enchant(EquipmentSlot::Mainhand),
        Some(EnchantName::WindfuryTotem)
    );
    let target = raid.target();
    assert_eq!(target.level(), 63);
    assert_eq!(target.base_armor(), 3731);
    assert_eq!(target.creature_type(), CreatureType::Dragonkin);
}

#[test]
fn a_shipped_setup_simulates() {
    let setup = shipped("dw_fury_orc.yaml");
    let settings = setup.sim_settings(&settings());
    let mut raid = setup.build_raid(data(), &settings).unwrap();
    let mut control = SimControl::new(settings, 1);
    let mut cruncher = NumberCruncher::new();
    control.run_quick_sim(&mut raid, &mut cruncher);
    let stats = cruncher.merged(None).unwrap();
    assert_eq!(stats.iterations(), 10);
    assert!(stats.personal_dps() > 0.0);
}

#[test]
fn the_prot_setup_tanks_with_a_shield() {
    let raid = shipped("prot_dwarf.yaml")
        .build_raid(data(), &settings())
        .unwrap();
    let character = raid.character(CharId(0));
    assert!(character.is_tanking());
    assert!(!character.equipment().is_dual_wielding());
}

#[test]
fn defaults_fill_what_the_file_leaves_out() {
    let setup = minimal();
    assert_eq!(setup.level, 60);
    assert_eq!(setup.target, TargetSetup::default());
    assert!(setup.phase.is_none() && setup.ruleset.is_none());
    let raid = setup.build_raid(data(), &settings()).unwrap();
    let character = raid.character(CharId(0));
    assert_eq!(character.talents().unwrap().spent_points(), 0);
    assert_eq!(raid.target().base_armor(), Mechanics::BOSS_BASE_ARMOR);
}

#[test]
fn the_setup_overrides_the_phase_and_ruleset_of_the_settings() {
    let mut setup = minimal();
    let base = settings();
    assert_eq!(setup.sim_settings(&base).phase, base.phase);
    setup.phase = Some(Phase::MoltenCore);
    setup.ruleset = Some(Ruleset::Loatheb);
    let merged = setup.sim_settings(&base);
    assert_eq!(merged.phase, Phase::MoltenCore);
    assert_eq!(merged.ruleset, Ruleset::Loatheb);
    assert_eq!(merged.combat_length, base.combat_length);

    let raid = setup.build_raid(data(), &base).unwrap();
    assert_eq!(raid.character(CharId(0)).sim().ruleset, Ruleset::Loatheb);
}

#[test]
fn unknown_fields_are_parse_errors() {
    let text = format!("{MINIMAL}talent:\n  Fury:\n    Cruelty: 5\n");
    let error = serde_yaml::from_str::<CharacterSetup>(&text).unwrap_err();
    assert!(error.to_string().contains("talent"), "{error}");
}

#[test]
fn a_race_the_class_cannot_be_stops_the_build() {
    let mut setup = minimal();
    setup.class = PlayerClass::Paladin;
    setup.race = Race::Orc;
    let issues = issues(&setup);
    // Either the class is not shipped or the race is not available to it.
    assert!(
        contexts(&issues)
            .iter()
            .any(|c| *c == "class" || *c == "race"),
        "{issues:?}"
    );
}

#[test]
fn every_problem_is_reported_with_its_field() {
    let text = format!(
        "{MINIMAL}{}",
        r#"
level: 70
talents:
  Fury:
    Cruelty: 6
    Flurry: 5
    Not A Talent: 1
  Holy:
    Anything: 1
equipment:
  MAINHAND: { item: 18832, enchant: EnchantBootsGreaterAgility }
  OFFHAND: { item: 17076 }
  HEAD: { item: 18404 }
  LEGS: { item: 23068 }
  RING1: { item: 999999 }
  BACK: { item: 13340, temp_enchant: WindfuryTotem }
buffs: [Sunder Armor, Juju Power, Elixir of Giants, Greater Blessing of Kings, Nothing]
target:
  level: 70
"#
    );
    let mut setup: CharacterSetup = serde_yaml::from_str(&text).unwrap();
    setup.phase = Some(Phase::BlackwingLair);
    setup.rotation = "No Such Rotation".to_string();
    let issues = issues(&setup);
    let found = contexts(&issues);
    for expected in [
        "rotation",
        "level",
        "target.level",
        "talents.Fury.Cruelty",
        "talents.Fury.Not A Talent",
        "talents.Holy",
        // Flurry needs Enrage and 25 points in Fury.
        "talents.Fury.Flurry",
        "equipment.MAINHAND.enchant",
        "equipment.OFFHAND",
        "equipment.HEAD",
        "equipment.LEGS",
        "equipment.RING1",
        "equipment.BACK.temp_enchant",
        "buffs.Sunder Armor",
        "buffs.Elixir of Giants",
        "buffs.Greater Blessing of Kings",
        "buffs.Nothing",
    ] {
        assert!(
            found.contains(&expected),
            "{expected} missing from {issues:#?}"
        );
    }
    assert_eq!(found.len(), 17, "{issues:#?}");

    let message = setup
        .build_raid(data(), &settings())
        .unwrap_err()
        .to_string();
    assert!(
        message.starts_with("character setup \"Minimal\" is invalid:\n  "),
        "{message}"
    );
    assert!(message
        .contains("equipment.LEGS: Legplates of Carnage (23068) is not available in phase 3"));
}

#[test]
fn a_two_hander_taking_the_offhand_away_is_reported() {
    let mut setup = minimal();
    setup.equipment.insert(
        EquipmentSlot::Mainhand,
        EquippedSetup {
            item: 17076,
            enchant: None,
            temp_enchant: None,
        },
    );
    setup.equipment.insert(
        EquipmentSlot::Offhand,
        EquippedSetup {
            item: 17075,
            enchant: None,
            temp_enchant: None,
        },
    );
    let issues = issues(&setup);
    assert_eq!(contexts(&issues), ["equipment.MAINHAND"], "{issues:?}");
}

#[test]
fn load_reports_the_file() {
    let path = DataBundle::repository_dir().join("characters/missing.yaml");
    let error = CharacterSetup::load(&path).unwrap_err();
    assert!(matches!(error, CharacterSetupError::Io { .. }));
    assert!(error.to_string().contains("missing.yaml"));

    let mut setup = shipped("dw_fury_orc.yaml");
    setup.rotation = "No Such Rotation".to_string();
    let message = setup.validate(data()).unwrap_err().to_string();
    assert!(message.contains("dw_fury_orc.yaml"), "{message}");
}
