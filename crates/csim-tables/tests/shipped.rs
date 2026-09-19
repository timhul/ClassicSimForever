//! The shipped `data/spells/*.yaml` files are what `export-spells` produces from the table dump.
//! Needs `data/tables/` (gitignored); without it the test passes trivially with a note.

use std::path::Path;

use csim_engine::buff::external::ExternalBuffDb;
use csim_engine::character::ClassDb;
use csim_engine::enchant::EnchantDb;
use csim_engine::faction::{Faction, PlayerClass};
use csim_engine::race::{Race, RaceDb};
use csim_engine::spell::overrides::Overrides;
use csim_engine::spell::record::OVERRIDES_DIR;
use csim_tables::export;
use csim_tables::{TableDir, Tables};

#[test]
fn shipped_spell_files_match_a_fresh_export() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tables_dir = root.join("data/tables");
    let spells_dir = root.join("data/spells");
    let build = std::fs::read_to_string(spells_dir.join("warrior.yaml"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("build: "))
        .expect("the shipped file names its build")
        .trim()
        .to_owned();
    let Ok(dir) = TableDir::open_build(&tables_dir, &build) else {
        eprintln!("data/tables/*.{build}.csv not present, skipping the export parity check");
        return;
    };
    let tables = Tables::load(&dir).unwrap();
    let overrides = Overrides::load(&spells_dir.join(OVERRIDES_DIR)).unwrap();

    let warrior = export::export_class(&tables, PlayerClass::Warrior, &overrides).unwrap();
    let rendered = export::render(&warrior, "export-spells --class warrior").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("warrior.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/warrior.yaml is stale: re-run `csim-tables export-spells --class warrior`"
    );

    let racials = export::export_racials(&tables, &overrides).unwrap();
    let rendered = export::render(&racials, "export-spells --racials").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("racials.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/racials.yaml is stale: re-run `csim-tables export-spells --racials`"
    );

    let registry = ExternalBuffDb::load(&root.join("data/external_buffs.yaml")).unwrap();
    let exclude = export::spell_ids_in_dir(&spells_dir, "externals.yaml").unwrap();
    let externals =
        export::export_externals(&tables, &registry.spell_ids(), &exclude, &overrides).unwrap();
    let rendered = export::render(&externals, "export-spells --externals").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("externals.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/externals.yaml is stale: re-run `csim-tables export-spells --externals`"
    );
}

/// `data/races.yaml` ids, names and factions agree with `ChrRaces`, and each race's bit matches
/// the `race_mask` the racial exporter writes.
#[test]
fn shipped_races_match_chr_races() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tables_dir = root.join("data/tables");
    let build = std::fs::read_to_string(root.join("data/spells/racials.yaml"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("build: "))
        .expect("the shipped file names its build")
        .trim()
        .to_owned();
    let Ok(dir) = TableDir::open_build(&tables_dir, &build) else {
        eprintln!("data/tables/*.{build}.csv not present, skipping the race parity check");
        return;
    };
    let tables = Tables::load(&dir).unwrap();
    let races = RaceDb::load(&root.join("data/races.yaml")).unwrap();

    for spec in races.specs() {
        let row = tables
            .chr_race(spec.id)
            .unwrap_or_else(|| panic!("{:?}: ChrRaces has no id {}", spec.race, spec.id));
        assert_eq!(row.name, spec.race.name(), "{:?}", spec.race);
        let faction = match row.alliance {
            0 => Faction::Alliance,
            _ => Faction::Horde,
        };
        assert_eq!(faction, spec.faction, "{:?}", spec.race);
        assert_eq!(
            row.playable_race_bit,
            i32::try_from(spec.id - 1).unwrap(),
            "{:?}: race_mask bit",
            spec.race
        );
    }
}

/// `data/classes/warrior.yaml` agrees with `ChrClasses` (attack power per stat),
/// `PlayerExpectedStat` (crit per agility at 60) and `CharBaseInfo` (the playable races).
#[test]
fn shipped_warrior_class_matches_the_tables() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tables_dir = root.join("data/tables");
    let build = std::fs::read_to_string(root.join("data/spells/warrior.yaml"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("build: "))
        .expect("the shipped file names its build")
        .trim()
        .to_owned();
    let Ok(dir) = TableDir::open_build(&tables_dir, &build) else {
        eprintln!("data/tables/*.{build}.csv not present, skipping the class parity check");
        return;
    };
    let tables = Tables::load(&dir).unwrap();
    let enchants = EnchantDb::load(&root.join("data/enchants.yaml")).unwrap();
    let classes = ClassDb::load(&root.join("data/classes"), Some(&enchants)).unwrap();
    let warrior = classes.get(PlayerClass::Warrior).unwrap();

    let class_row = tables
        .chr_classes()
        .find(|row| row.filename == "WARRIOR")
        .expect("ChrClasses has the Warrior");
    let rules = warrior.stat_rules.rules();
    assert_eq!(
        rules.melee_ap_per_strength as f32,
        class_row.attack_power_per_strength
    );
    assert_eq!(
        rules.melee_ap_per_agility as f32,
        class_row.attack_power_per_agility
    );
    assert_eq!(
        rules.ranged_ap_per_agility as f32,
        class_row.ranged_attack_power_per_agility
    );
    assert_eq!(
        u32::try_from(warrior.resource.power_type().id()).unwrap(),
        class_row.display_power,
        "DisplayPower is the class resource"
    );

    let expected = tables
        .player_expected_stat(class_row.id, 60)
        .expect("PlayerExpectedStat has level 60");
    let agility_per_percent_crit = 1.0 / (f64::from(expected.crit_per_agility) * 100.0);
    assert!(
        (agility_per_percent_crit - rules.agility_per_percent_crit).abs() < 1e-3,
        "{agility_per_percent_crit} vs {}",
        rules.agility_per_percent_crit
    );
    assert_eq!(expected.spell_crit_per_intellect, 0.0);
    assert_eq!(rules.intellect_per_percent_spell_crit, f64::MAX);
    assert_eq!(expected.base_mana, warrior.base_stats.mana);

    let races: Vec<u32> = tables
        .races_of_class(class_row.id)
        .into_iter()
        .filter(|id| Race::from_id(*id).is_some())
        .collect();
    let mut listed: Vec<u32> = warrior.available_races.iter().map(|r| r.id()).collect();
    listed.sort_unstable();
    assert_eq!(listed, races, "CharBaseInfo races of class 1");
}
