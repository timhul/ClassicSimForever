//! The shipped `data/spells/*.yaml` and `data/talents/*.yaml` files are what `export-spells` /
//! `export-talents` produce from the table dump.
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

/// The classes whose spell and talent files are exported (`csim-tables export-all`).
const EXPORTED_CLASSES: [PlayerClass; 2] = [PlayerClass::Warrior, PlayerClass::Rogue];

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

    for class in EXPORTED_CLASSES {
        let name = class.name().to_lowercase();
        let file = export::export_class(&tables, class, &overrides).unwrap();
        let rendered = export::render(&file, &format!("export-spells --class {name}")).unwrap();
        let shipped = std::fs::read_to_string(spells_dir.join(format!("{name}.yaml"))).unwrap();
        assert!(
            rendered == shipped.replace("\r\n", "\n"),
            "data/spells/{name}.yaml is stale: re-run `csim-tables export-spells --class {name}`"
        );
    }

    let racials = export::export_racials(&tables, &overrides).unwrap();
    let rendered = export::render(&racials, "export-spells --racials").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("racials.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/racials.yaml is stale: re-run `csim-tables export-spells --racials`"
    );

    for class in EXPORTED_CLASSES {
        let name = class.name().to_lowercase();
        let talents = export::export_talents(&tables, class).unwrap();
        let rendered =
            export::render_talents(&talents, &format!("export-talents --class {name}")).unwrap();
        let shipped =
            std::fs::read_to_string(root.join(format!("data/talents/{name}.yaml"))).unwrap();
        assert!(
            rendered == shipped.replace("\r\n", "\n"),
            "data/talents/{name}.yaml is stale: re-run `csim-tables export-talents --class {name}`"
        );
    }

    let registry = ExternalBuffDb::load(&root.join("data/external_buffs.yaml")).unwrap();
    let exclude = export::spell_ids_in_dir(&spells_dir, "externals.yaml").unwrap();
    let externals = export::export_externals(
        &tables,
        &export::external_seeds(&registry),
        &exclude,
        &overrides,
    )
    .unwrap();
    let rendered = export::render(&externals, "export-spells --externals").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("externals.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/externals.yaml is stale: re-run `csim-tables export-spells --externals`"
    );

    let enchants = EnchantDb::load(&root.join("data/enchants.yaml")).unwrap();
    let exclude = export::spell_ids_in_dir(&spells_dir, "enchants.yaml").unwrap();
    let (enchants_file, _) = export::export_enchants(
        &tables,
        &enchants.spell_ids(),
        &enchants.enchantment_ids(),
        &exclude,
        &overrides,
    )
    .unwrap();
    let rendered = export::render(&enchants_file, "export-spells --enchants").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("enchants.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/enchants.yaml is stale: re-run `csim-tables export-spells --enchants`"
    );

    let items = export::items::read_item_specs(&root.join("data/items")).unwrap();
    let sets: csim_engine::item::ItemSetFile =
        serde_yaml::from_str(&std::fs::read_to_string(root.join("data/item_sets.yaml")).unwrap())
            .unwrap();
    let exclude = export::spell_ids_in_dir(&spells_dir, "items.yaml").unwrap();
    let (items_file, _, missing) = export::export_items(
        &tables,
        &export::item_seeds(&items, &sets),
        &exclude,
        &overrides,
    )
    .unwrap();
    assert_eq!(missing, [469141], "item spells the dump lacks");
    let rendered = export::render(&items_file, "export-spells --items").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("items.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/items.yaml is stale: re-run `csim-tables export-spells --items`"
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

/// `data/classes/<class>.yaml` agrees with `ChrClasses` (attack power per stat),
/// `PlayerExpectedStat` (crit per agility at 60) and `CharBaseInfo` (the playable races).
#[test]
fn shipped_classes_match_the_tables() {
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
    for class in EXPORTED_CLASSES {
        let spec = classes.get(class).unwrap();
        let class_row = tables
            .chr_classes()
            .find(|row| row.filename == class.name().to_uppercase())
            .unwrap_or_else(|| panic!("ChrClasses has the {}", class.name()));
        let rules = spec.stat_rules.rules();
        assert_eq!(
            rules.melee_ap_per_strength as f32, class_row.attack_power_per_strength,
            "{class:?}"
        );
        assert_eq!(
            rules.melee_ap_per_agility as f32, class_row.attack_power_per_agility,
            "{class:?}"
        );
        assert_eq!(
            rules.ranged_ap_per_agility as f32, class_row.ranged_attack_power_per_agility,
            "{class:?}"
        );
        assert_eq!(
            u32::try_from(spec.resource.power_type().id()).unwrap(),
            class_row.display_power,
            "{class:?}: DisplayPower is the class resource"
        );

        let expected = tables
            .player_expected_stat(class_row.id, 60)
            .expect("PlayerExpectedStat has level 60");
        let agility_per_percent_crit = 1.0 / (f64::from(expected.crit_per_agility) * 100.0);
        assert!(
            (agility_per_percent_crit - rules.agility_per_percent_crit).abs() < 1e-3,
            "{class:?}: {agility_per_percent_crit} vs {}",
            rules.agility_per_percent_crit
        );
        assert_eq!(expected.spell_crit_per_intellect, 0.0, "{class:?}");
        assert_eq!(
            rules.intellect_per_percent_spell_crit,
            f64::MAX,
            "{class:?}"
        );
        assert_eq!(expected.base_mana, spec.base_stats.mana, "{class:?}");

        let races: Vec<u32> = tables
            .races_of_class(class_row.id)
            .into_iter()
            .filter(|id| Race::from_id(*id).is_some())
            .collect();
        let mut listed: Vec<u32> = spec.available_races.iter().map(|r| r.id()).collect();
        listed.sort_unstable();
        assert_eq!(listed, races, "CharBaseInfo races of {class:?}");
    }
}

/// The item tables of the real dump load and join as ITEM_INSTRUCTIONS §1.1–1.9 describes.
#[test]
fn item_tables_of_the_dump_join() {
    let tables_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tables");
    let Ok(dir) = TableDir::open(&tables_dir) else {
        eprintln!("data/tables/ not present, skipping the item table check");
        return;
    };
    let tables = Tables::load(&dir).unwrap();
    // `Item` is complete, `ItemSparse` is not (§1.1).
    assert!(tables.items().count() > tables.item_sparse_rows().count());
    assert!(tables.item(11815).is_some(), "Hand of Justice");
    let equippable_without_sparse = tables
        .items()
        .filter(|i| matches!(i.class_id, 2 | 4) && tables.item_sparse(i.id).is_none())
        .count();
    assert!(equippable_without_sparse > 0);
    // Every sparse row has its Item row and an in-range budget / armor / damage row.
    for sparse in tables.item_sparse_rows() {
        assert!(tables.item(sparse.id).is_some(), "item {}", sparse.id);
    }
    assert_eq!(tables.rand_prop_points(80).unwrap().epic[3], 23);
    for level in 1..=100 {
        assert!(
            tables.item_armor_total(level).is_some(),
            "armor total {level}"
        );
        assert!(
            tables.item_armor_quality(level).is_some(),
            "armor quality {level}"
        );
        assert!(tables.item_armor_shield(level).is_some(), "shield {level}");
        for table in csim_tables::tables::ItemDamageTable::ALL {
            assert!(
                tables.item_damage(table, level).is_some(),
                "{table:?} {level}"
            );
        }
    }
    // Battlegear of Might (§1.8) and a Classic suffix pool (§1.9).
    let bonuses: Vec<(u32, u32)> = tables
        .item_set_spells(209)
        .iter()
        .map(|b| (b.threshold, b.spell_id))
        .collect();
    assert_eq!(bonuses, [(3, 23562), (5, 21838), (8, 23561)]);
    assert!(tables.item_bonus_tree_nodes(5654).len() >= 24);
}

/// Every in-scope item of the real dump derives without unresolved stats or rows.
#[test]
fn items_of_the_dump_derive() {
    use csim_engine::item::{ItemSlot, ItemStat};
    use csim_tables::export::items::{Skip, derive_items};

    let tables_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tables");
    let Ok(dir) = TableDir::open(&tables_dir) else {
        eprintln!("data/tables/ not present, skipping the item derivation check");
        return;
    };
    let tables = Tables::load(&dir).unwrap();
    let report = derive_items(&tables);
    assert!(report.items.len() > 2500, "{} items", report.items.len());
    // The only unresolved references are item spells missing from the dump.
    for issue in &report.issues {
        assert!(
            issue.message.contains("names missing spell"),
            "{}: {}",
            issue.item_id,
            issue.message
        );
    }
    assert!(report.skipped[&Skip::NoSparseRow].contains(&11815));
    for item in &report.items {
        assert_eq!(
            item.damage.is_some(),
            item.slot.weapon_slot().is_some(),
            "{} {}",
            item.id,
            item.name
        );
    }
    let lionheart = report.items.iter().find(|i| i.id == 12640).unwrap();
    assert_eq!(lionheart.slot, ItemSlot::Head);
    assert_eq!(lionheart.stats[&ItemStat::CritRating], 28.0);
    assert_eq!(lionheart.stats[&ItemStat::Armor], 565.0);
}

/// The tables of the build the shipped item files name, or `None` (with a note) without a dump.
fn item_tables(root: &Path) -> Option<Tables> {
    let build = std::fs::read_to_string(root.join("data/items/one_hand.yaml"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("build: "))
        .expect("the shipped item file names its build")
        .trim()
        .to_owned();
    let Ok(dir) = TableDir::open_build(root.join("data/tables"), &build) else {
        eprintln!("data/tables/*.{build}.csv not present, skipping the item checks");
        return None;
    };
    Some(Tables::load(&dir).unwrap())
}

/// `data/items/*.yaml` and `data/item_sets.yaml` are what `export-items` writes, and nothing
/// else sits next to the generated item files.
#[test]
fn shipped_item_files_match_a_fresh_export() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(tables) = item_tables(&root) else {
        return;
    };
    let report = export::items::derive_items(&tables);
    let files = export::item_files(&tables, &report.items);
    let items_dir = root.join("data/items");
    for (name, file) in &files {
        let rendered = export::render_items(file, "export-items").unwrap();
        let shipped = std::fs::read_to_string(items_dir.join(format!("{name}.yaml"))).unwrap();
        assert!(
            rendered == shipped.replace("\r\n", "\n"),
            "data/items/{name}.yaml is stale: re-run `csim-tables export-items`"
        );
    }
    for entry in std::fs::read_dir(&items_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let stem = path.file_stem().unwrap().to_str().unwrap();
            assert!(
                files.contains_key(stem),
                "{} is not written by `csim-tables export-items`",
                path.display()
            );
        }
    }
    let sets = export::item_set_file(&tables, &report.items);
    let rendered = export::render_item_sets(&sets, "export-items").unwrap();
    let shipped = std::fs::read_to_string(root.join("data/item_sets.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/item_sets.yaml is stale: re-run `csim-tables export-items`"
    );
}

/// The exported items differ from their hand-authored Classic version exactly as reviewed in
/// `tests/fixtures/classic_item_differences.txt`: a change to the item formulas shows up here.
#[test]
fn exported_items_differ_from_classic_only_as_reviewed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(tables) = item_tables(&root) else {
        return;
    };
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let exported: Vec<_> = export::items::derive_items(&tables)
        .items
        .iter()
        .map(|item| item.to_spec())
        .collect();
    let classic = export::items::read_item_specs(&fixtures.join("classic_items.yaml")).unwrap();
    let lines = export::compare_items(&exported, &export::newest_versions(classic));
    let reviewed: Vec<String> =
        std::fs::read_to_string(fixtures.join("classic_item_differences.txt"))
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(str::to_owned)
            .collect();
    let new: Vec<&String> = lines.iter().filter(|l| !reviewed.contains(l)).collect();
    let gone: Vec<&String> = reviewed.iter().filter(|l| !lines.contains(l)).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "differences to review:\nnew: {new:#?}\nno longer different: {gone:#?}"
    );
}
