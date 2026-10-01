//! Character loader tests against the shipped data and the example setups of
//! `data/characters/`.

use std::sync::OnceLock;

use super::*;
use crate::character::RegenReactions;
use crate::ids::CharId;
use crate::sim_control::{SimControl, run_logged_iteration};
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
    assert!(!setups.is_empty());
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
        character.equipment().temp_enchants(EquipmentSlot::Mainhand),
        [
            EnchantName::ElementalSharpeningStone,
            EnchantName::WindfuryTotem
        ]
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
  MAINHAND: { item: 18828, enchant: EnchantBootsGreaterAgility }
  OFFHAND: { item: 18877 }
  HEAD: { item: 18404 }
  RING1: { item: 999999 }
  BACK: { item: 20068, temp_enchant: WindfuryTotem }
buffs: [Sunder Armor, Juju Power, Elixir of Giants, Nothing]
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
        "equipment.RING1",
        "equipment.BACK.temp_enchants",
        "buffs.Sunder Armor",
        "buffs.Elixir of Giants",
        "buffs.Nothing",
    ] {
        assert!(
            found.contains(&expected),
            "{expected} missing from {issues:#?}"
        );
    }
    assert_eq!(found.len(), 15, "{issues:#?}");

    let message = setup
        .build_raid(data(), &settings())
        .unwrap_err()
        .to_string();
    assert!(
        message.starts_with("character setup \"Minimal\" is invalid:\n  "),
        "{message}"
    );
    assert!(
        message.contains("equipment.RING1: no item 999999"),
        "{message}"
    );
}

/// A Rogue setup is checked like a Warrior's: class, race, talents, rotation, weapons,
/// enchants, poisons; and the armor and weapons the class can use.
#[test]
fn a_rogue_setup_is_checked() {
    let text = r#"
name: Rogue
class: ROGUE
race: UNDEAD
rotation: Combat
phase: 3
talents:
  Combat:
    Improved Eviscerate: 3
    Improved Sinister Strike: 2
    Precision: 3
equipment:
  MAINHAND: { item: 18866, enchant: Crusader, temp_enchants: [InstantPoison] }
  OFFHAND: { item: 18866, temp_enchants: [InstantPoison] }
  CHEST: { item: 16563 }
consumables: [Thistle Tea]
"#;
    let setup: CharacterSetup = serde_yaml::from_str(text).unwrap();
    setup
        .build_raid(data(), &settings())
        .unwrap_or_else(|error| panic!("{error}"));

    let mut tauren = setup.clone();
    tauren.race = Race::Tauren;
    assert!(contexts(&issues(&tauren)).contains(&"race"));

    // Lionheart Helm, Force Reactive Disk.
    let mut plate = setup.clone();
    let equip = |item| EquippedSetup {
        item,
        enchant: None,
        temp_enchants: Vec::new(),
    };
    plate.equipment.insert(EquipmentSlot::Head, equip(12640));
    let found = issues(&plate);
    assert_eq!(contexts(&found), ["equipment.HEAD"], "{found:#?}");
    assert!(found[0].message.contains("cannot wear"), "{found:#?}");

    let mut shield = setup.clone();
    shield
        .equipment
        .insert(EquipmentSlot::Offhand, equip(18168));
    let found = issues(&shield);
    assert_eq!(contexts(&found), ["equipment.OFFHAND"], "{found:#?}");
    assert!(found[0].message.contains("cannot wield"), "{found:#?}");
}

/// The item uses and racials the Rogue rotations name, which only link when equipped or of the
/// right race.
const ROGUE_ITEM_AND_RACIAL_LINES: [&str; 9] = [
    "Burst of Energy",
    "Kiss of the Spider",
    "Jom Gabbar",
    "Badge of the Swarmguard",
    "Slayer's Crest",
    "Earthstrike",
    "Restless Strength",
    "Blood Fury",
    "Berserking",
];

/// Every Rogue setup runs its rotation: the opener once per fight, the builder and a
/// finisher, Slice and Dice kept up; the only lines left unlinked are items not equipped and
/// other races' racials.
#[test]
fn the_rogue_setups_run_their_rotations() {
    let cases = [
        (
            "combat_swords_human.yaml",
            "Garrote",
            "Sinister Strike",
            "Eviscerate",
        ),
        (
            "combat_axes_orc.yaml",
            "Garrote",
            "Sinister Strike",
            "Eviscerate",
        ),
        (
            "combat_daggers_night_elf.yaml",
            "Ambush",
            "Backstab",
            "Eviscerate",
        ),
        ("mutilate_undead.yaml", "Ambush", "Mutilate", "Eviscerate"),
        (
            "mutilate_ea_gnome.yaml",
            "Ambush",
            "Mutilate",
            "Expose Armor",
        ),
        ("hemorrhage_troll.yaml", "Ambush", "Hemorrhage", "Rupture"),
    ];
    for (file, opener, builder, finisher) in cases {
        let setup = shipped(file);
        let settings = SimSettings {
            combat_length: 120,
            ..setup.sim_settings(&settings())
        };
        let mut raid = setup.build_raid(data(), &settings).unwrap();
        let mut cruncher = NumberCruncher::new();
        SimControl::new(settings, 1).run_quick_sim(&mut raid, &mut cruncher);
        let stats = cruncher.merged(None).unwrap();
        assert!(stats.personal_dps() > 300.0, "{file}");

        let casts = |name: &str| -> u64 {
            stats
                .executors()
                .iter()
                .filter(|e| e.spell_name() == name)
                .map(|e| e.successful_casts())
                .sum()
        };
        assert_eq!(casts(opener), stats.iterations(), "{file}: {opener}");
        assert!(
            casts(builder) > 20 * stats.iterations(),
            "{file}: {builder}"
        );
        assert!(casts(finisher) > 0, "{file}: {finisher}");
        let slice_and_dice = stats.buff_statistics("Slice and Dice").unwrap();
        assert!(slice_and_dice.avg_uptime() > 0.9, "{file}");
        for skipped in stats.skipped_executors() {
            assert!(
                ROGUE_ITEM_AND_RACIAL_LINES.contains(&skipped.spell_name.as_str())
                    && skipped.reason == "no spell of this name",
                "{file}: {skipped:?}"
            );
        }
    }
    let rupture = |file: &str| {
        let setup = shipped(file);
        let mut raid = setup.build_raid(data(), &settings()).unwrap();
        let mut cruncher = NumberCruncher::new();
        SimControl::new(setup.sim_settings(&settings()), 1).run_quick_sim(&mut raid, &mut cruncher);
        let stats = cruncher.merged(None).unwrap();
        (
            stats.buff_statistics("Rupture").map(|b| b.avg_uptime()),
            stats
                .buff_statistics("Expose Armor")
                .map(|b| b.avg_uptime()),
        )
    };
    let (hemorrhage_rupture, _) = rupture("hemorrhage_troll.yaml");
    assert!(hemorrhage_rupture.unwrap() > 0.4, "{hemorrhage_rupture:?}");
    let (_, expose_armor) = rupture("mutilate_ea_gnome.yaml");
    assert!(expose_armor.unwrap() > 0.5, "{expose_armor:?}");
}

/// With every shipped Rogue setup, reacting to energy regeneration only after the ticks that
/// can change the rotation's outcome (the default) fights the same fights, event for event, as
/// reacting after every tick (the C++).
#[test]
fn the_rogue_setups_fight_the_same_reacting_to_every_tick() {
    for file in [
        "combat_swords_human.yaml",
        "combat_axes_orc.yaml",
        "combat_daggers_night_elf.yaml",
        "mutilate_undead.yaml",
        "mutilate_ea_gnome.yaml",
        "hemorrhage_troll.yaml",
    ] {
        let setup = shipped(file);
        let settings = SimSettings {
            combat_length: 300,
            ..setup.sim_settings(&settings())
        };
        let fight = |mode: RegenReactions, seed: u64| {
            let mut raid = setup.build_raid(data(), &settings).unwrap();
            raid.character_mut(CharId(0)).set_regen_reactions(mode);
            run_logged_iteration(&settings, seed, &mut raid)
        };
        for seed in [1, 2] {
            let reference = fight(RegenReactions::EveryTick, seed);
            let log = fight(RegenReactions::Thresholds, seed);
            assert!(reference.len() > 500, "{file}: {}", reference.len());
            if let Some(index) = log
                .entries()
                .iter()
                .zip(reference.entries())
                .position(|(a, b)| a != b)
            {
                panic!(
                    "{file}, seed {seed}: entry {index} differs:\n{:?}\n{:?}",
                    log.entries()[index],
                    reference.entries()[index]
                );
            }
            assert_eq!(log.len(), reference.len(), "{file}, seed {seed}");
        }
    }
}

/// A consumable must be in the registry, offered to the class, and listed once.
#[test]
fn consumables_are_checked() {
    let mut setup = minimal();
    setup.consumables = vec![
        "Thistle Tea".to_string(),
        "Nothing".to_string(),
        "Nothing".to_string(),
    ];
    let found = issues(&setup);
    assert_eq!(
        contexts(&found),
        [
            "consumables.Thistle Tea",
            "consumables.Nothing",
            "consumables.Nothing"
        ],
        "{found:#?}"
    );
    assert!(found[0].message.contains("WARRIOR") || found[0].message.contains("Warrior"));
}

#[test]
fn a_two_hander_taking_the_offhand_away_is_reported() {
    let mut setup = minimal();
    setup.equipment.insert(
        EquipmentSlot::Mainhand,
        EquippedSetup {
            item: 18877,
            enchant: None,
            temp_enchants: Vec::new(),
        },
    );
    setup.equipment.insert(
        EquipmentSlot::Offhand,
        EquippedSetup {
            item: 18828,
            enchant: None,
            temp_enchants: Vec::new(),
        },
    );
    let issues = issues(&setup);
    assert_eq!(contexts(&issues), ["equipment.MAINHAND"], "{issues:?}");
}

#[test]
fn temp_enchants_take_a_list_or_a_single_name() {
    let parse = |text: &str| {
        serde_yaml::from_str::<EquippedSetup>(text)
            .unwrap()
            .temp_enchants
    };
    assert_eq!(
        parse("{ item: 1, temp_enchants: [WindfuryTotem, DenseSharpeningStone] }"),
        [
            EnchantName::WindfuryTotem,
            EnchantName::DenseSharpeningStone
        ]
    );
    assert_eq!(
        parse("{ item: 1, temp_enchants: WindfuryTotem }"),
        [EnchantName::WindfuryTotem]
    );
    assert_eq!(
        parse("{ item: 1, temp_enchant: WindfuryTotem }"),
        [EnchantName::WindfuryTotem]
    );
    assert_eq!(parse("{ item: 1 }"), []);
}

#[test]
fn two_temp_enchants_of_one_group_are_reported() {
    let mut setup = minimal();
    setup.equipment.insert(
        EquipmentSlot::Mainhand,
        EquippedSetup {
            item: 18828,
            enchant: None,
            temp_enchants: vec![
                EnchantName::WindfuryTotem,
                EnchantName::DenseSharpeningStone,
                EnchantName::ElementalSharpeningStone,
            ],
        },
    );
    let issues = issues(&setup);
    assert_eq!(
        contexts(&issues),
        ["equipment.MAINHAND.temp_enchants"],
        "{issues:?}"
    );
    assert!(
        issues[0].message.contains("DenseSharpeningStone")
            && issues[0].message.contains("ElementalSharpeningStone"),
        "{issues:?}"
    );
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

/// A fresh directory holding `files` (relative path → contents).
fn setup_dir(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "csim-character-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    for (file, text) in files {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

const BASE: &str = r#"
name: Base
class: WARRIOR
race: ORC
level: 58
rotation: DW Fury High Rage
equipment:
  MAINHAND: { item: 18828 }
  OFFHAND: { item: 18828 }
buffs: [Battle Squawk]
"#;

#[test]
fn include_is_applied_in_place() {
    let dir = setup_dir(
        "in-place",
        &[
            ("common/base.yaml", BASE),
            (
                "troll.yaml",
                "level: 50\nname: Early\ninclude: common/base.yaml\nname: Troll\nrace: TROLL\n",
            ),
        ],
    );
    let setup = CharacterSetup::load(&dir.join("troll.yaml")).unwrap();
    // Keys before the include are overwritten by it, keys after it overwrite it.
    assert_eq!(setup.level, 58);
    assert_eq!(setup.name, "Troll");
    assert_eq!(setup.race, Race::Troll);
    assert_eq!(setup.buffs, ["Battle Squawk"]);
    assert_eq!(
        setup.path.as_deref(),
        Some(dir.join("troll.yaml").as_path())
    );
}

#[test]
fn include_merges_equipment_by_slot_and_null_removes() {
    let dir = setup_dir(
        "equipment",
        &[
            ("base.yaml", BASE),
            (
                "swap.yaml",
                "include: base.yaml\nequipment:\n  MAINHAND: { item: 17075 }\n  OFFHAND: null\n  HEAD: { item: 12640 }\nbuffs: null\n",
            ),
        ],
    );
    let setup = CharacterSetup::load(&dir.join("swap.yaml")).unwrap();
    let items: Vec<(EquipmentSlot, u32)> = setup
        .equipment
        .iter()
        .map(|(slot, equipped)| (*slot, equipped.item))
        .collect();
    assert_eq!(
        items,
        [
            (EquipmentSlot::Mainhand, 17075),
            (EquipmentSlot::Head, 12640)
        ]
    );
    assert!(setup.buffs.is_empty());
}

#[test]
fn include_may_repeat_list_and_nest() {
    let dir = setup_dir(
        "multiple",
        &[
            ("parts/base.yaml", BASE),
            ("parts/level.yaml", "level: 55\n"),
            ("parts/nested.yaml", "include: level.yaml\nrace: TAUREN\n"),
            (
                "list.yaml",
                "include: [parts/base.yaml, parts/nested.yaml]\nname: List\n",
            ),
            (
                "repeated.yaml",
                "include: parts/base.yaml\nlevel: 40\ninclude: parts/nested.yaml\nrace: DWARF\n",
            ),
        ],
    );
    let list = CharacterSetup::load(&dir.join("list.yaml")).unwrap();
    assert_eq!((list.name.as_str(), list.level), ("List", 55));
    assert_eq!(list.race, Race::Tauren);

    let repeated = CharacterSetup::load(&dir.join("repeated.yaml")).unwrap();
    assert_eq!((repeated.name.as_str(), repeated.level), ("Base", 55));
    assert_eq!(repeated.race, Race::Dwarf);
}

#[test]
fn normalize_folds_parent_directories() {
    assert_eq!(
        normalize(Path::new("data/sweeps/../characters/./a.yaml")),
        Path::new("data/characters/a.yaml")
    );
    assert_eq!(
        normalize(Path::new("../a/../b.yaml")),
        Path::new("../b.yaml")
    );
}

#[test]
fn include_errors_are_reported() {
    let dir = setup_dir(
        "errors",
        &[
            ("a.yaml", "include: b.yaml\n"),
            ("b.yaml", "include: a.yaml\n"),
            ("number.yaml", "include: 3\n"),
            ("missing.yaml", "include: nowhere.yaml\n"),
            ("unknown.yaml", "include: base.yaml\nnot_a_key: 1\n"),
            ("base.yaml", BASE),
        ],
    );
    let load = |file: &str| CharacterSetup::load(&dir.join(file)).unwrap_err();
    assert!(
        matches!(load("a.yaml"), CharacterSetupError::Include { ref path, .. } if path.ends_with("a.yaml")),
    );
    assert!(matches!(
        load("number.yaml"),
        CharacterSetupError::Include { .. }
    ));
    // A missing include names both the including and the included file.
    let error = load("missing.yaml");
    assert!(
        matches!(error, CharacterSetupError::Include { ref path, .. } if path.ends_with("missing.yaml")),
        "{error}"
    );
    assert!(error.to_string().contains("nowhere.yaml"), "{error}");
    let message = load("unknown.yaml").to_string();
    assert!(message.contains("not_a_key"), "{message}");
}
