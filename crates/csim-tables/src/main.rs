//! `csim-tables`: inspect the client table dumps and export them to the YAML data files under
//! `data/`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use csim_engine::buff::external::ExternalBuffDb;
use csim_engine::enchant::EnchantDb;
use csim_engine::faction::PlayerClass;
use csim_engine::item::{ItemFile, ItemSetFile};
use csim_engine::spell::overrides::Overrides;
use csim_engine::spell::record::{SpellDb, OVERRIDES_DIR};
use csim_tables::export::{self, ExportError};
use csim_tables::tables::ALL_TABLES;
use csim_tables::{dir::missing_tables, TableDir, TableError, Tables};

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Table(#[from] TableError),
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error(transparent)]
    Overrides(#[from] csim_engine::spell::overrides::OverrideError),
    #[error(transparent)]
    ExternalBuffs(#[from] csim_engine::buff::external::ExternalBuffError),
    #[error(transparent)]
    Enchants(#[from] csim_engine::enchant::EnchantDbError),
    #[error(transparent)]
    SpellDb(#[from] csim_engine::spell::record::SpellDbError),
    #[error(transparent)]
    ReadItems(#[from] export::items::ReadItemsError),
    #[error(transparent)]
    EquipmentDb(#[from] csim_engine::item::EquipmentDbError),
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot render YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("{0:?} is not a class (use e.g. warrior)")]
    UnknownClass(String),
    #[error("{0} unsupported effects (see above)")]
    Unsupported(usize),
}

/// The file `export-spells --externals` writes under the spell data directory.
const EXTERNALS_FILE: &str = "externals.yaml";
/// The file `export-spells --enchants` writes under the spell data directory.
const ENCHANTS_FILE: &str = "enchants.yaml";
/// The file `export-spells --items` writes under the spell data directory.
const ITEMS_FILE: &str = "items.yaml";
/// The classes whose spells and talents `export-all` writes.
const EXPORTED_CLASSES: &[&str] = &["warrior"];

#[derive(Parser)]
#[command(name = "csim-tables", version, about)]
struct Cli {
    /// Directory holding the `<Table>.<build>.csv` files.
    #[arg(long, default_value = "data/tables", global = true)]
    tables: PathBuf,
    /// Build to use when the directory holds several (e.g. `1.60.1.70009`).
    #[arg(long, global = true)]
    build: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lists the builds and tables found in the directory with their row counts.
    Info {
        /// Also count the tables the exporters do not use.
        #[arg(long)]
        all: bool,
    },
    /// Prints everything the tables know about one spell, following triggered spells.
    Spell {
        /// Spell id (`SpellName.ID`).
        id: u32,
    },
    /// Prints the joined item rows of one item: identity, stats, spells, set, unique group and
    /// random-suffix pools.
    Item {
        /// Item id (`Item.ID`).
        id: u32,
    },
    /// Writes the spellbook of a class (or the racials, or the external buffs) as an engine
    /// data file.
    ExportSpells {
        /// The class to export (`warrior`, `rogue`, ...).
        #[arg(
            long,
            conflicts_with_all = ["racials", "externals", "enchants", "items"],
            required_unless_present_any = ["racials", "externals", "enchants", "items"]
        )]
        class: Option<String>,
        /// Export the racial abilities instead of a class.
        #[arg(long, conflicts_with_all = ["externals", "enchants", "items"])]
        racials: bool,
        /// Export the aura spells of the external buff registry and the rulesets instead of a
        /// class.
        #[arg(long, conflicts_with_all = ["enchants", "items"])]
        externals: bool,
        /// Export the spells the enchant procs name instead of a class.
        #[arg(long, conflicts_with = "items")]
        enchants: bool,
        /// Export the spells the exported items grant and the set bonus spells instead of a
        /// class. Run it after the other exports: spells they already carry are not repeated.
        #[arg(long)]
        items: bool,
        /// The external buff registry (`--externals`).
        #[arg(long, default_value = "data/external_buffs.yaml")]
        external_buffs: PathBuf,
        /// The enchant data file (`--enchants`).
        #[arg(long, default_value = "data/enchants.yaml")]
        enchant_data: PathBuf,
        /// The item data directory (`--items`).
        #[arg(long, default_value = "data/items")]
        item_data: PathBuf,
        /// The exported item sets (`--items`).
        #[arg(long, default_value = "data/item_sets.yaml")]
        item_sets: PathBuf,
        /// The spell data directory; the overrides in `<spells>/overrides/` extend the walk and
        /// the file is written to `<spells>/<class>.yaml` unless `--out` is given.
        #[arg(long, default_value = "data/spells")]
        spells: PathBuf,
        /// Output file (`-` for stdout).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Writes the talent tree of a class as an engine data file.
    ExportTalents {
        /// The class to export (`warrior`, `rogue`, ...).
        #[arg(long)]
        class: String,
        /// The talent data directory; the file is written to `<talents>/<class>.yaml` unless
        /// `--out` is given.
        #[arg(long, default_value = "data/talents")]
        talents: PathBuf,
        /// Output file (`-` for stdout).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Writes the weapons and armor of quality Rare and above as engine item files (one per
    /// slot) and the item sets they belong to.
    ExportItems {
        /// The item data directory; one `<slot>.yaml` per slot is written there.
        #[arg(long, default_value = "data/items")]
        items: PathBuf,
        /// The item set file.
        #[arg(long, default_value = "data/item_sets.yaml")]
        sets: PathBuf,
    },
    /// Runs every export in dependency order: the class spells (`warrior`), the racials, the
    /// external buff and enchant spells, the class talents, the items and item sets, then the
    /// item spells, and finally `check`.
    ExportAll {
        /// The spell data directory.
        #[arg(long, default_value = "data/spells")]
        spells: PathBuf,
        /// The talent data directory.
        #[arg(long, default_value = "data/talents")]
        talents: PathBuf,
        /// The item data directory.
        #[arg(long, default_value = "data/items")]
        items: PathBuf,
        /// The item set file.
        #[arg(long, default_value = "data/item_sets.yaml")]
        item_sets: PathBuf,
        /// The external buff registry.
        #[arg(long, default_value = "data/external_buffs.yaml")]
        external_buffs: PathBuf,
        /// The enchant data file.
        #[arg(long, default_value = "data/enchants.yaml")]
        enchant_data: PathBuf,
        /// Fail when the final check finds unsupported effects.
        #[arg(long)]
        strict: bool,
    },
    /// Compares the exported items with hand-authored Classic items of the same id and prints
    /// the differences (ratings through the interim level-60 factors).
    CompareItems {
        /// The hand-authored Classic item files (a directory or one file).
        #[arg(
            long,
            default_value = "crates/csim-tables/tests/fixtures/classic_items.yaml"
        )]
        legacy: PathBuf,
        /// Also write the compared legacy items (their newest phase) to this file.
        #[arg(long)]
        snapshot: Option<PathBuf>,
    },
    /// Loads the exported spell files with the engine and reports what the sim cannot use.
    Check {
        /// The spell data directory.
        #[arg(long, default_value = "data/spells")]
        spells: PathBuf,
        /// Fail when there are unsupported effects.
        #[arg(long)]
        strict: bool,
    },
}

fn parse_class(name: &str) -> Result<PlayerClass, CliError> {
    serde_yaml::from_str(&name.trim().to_uppercase())
        .map_err(|_| CliError::UnknownClass(name.to_owned()))
}

/// What `export-spells` writes.
enum ExportTarget {
    Class(String),
    Racials,
    Externals(PathBuf),
    Enchants(PathBuf),
    Items { items: PathBuf, sets: PathBuf },
}

fn export_spells(
    tables: &Tables,
    target: ExportTarget,
    spells_dir: &Path,
    out: Option<PathBuf>,
) -> Result<(), CliError> {
    let overrides = Overrides::load(&spells_dir.join(OVERRIDES_DIR))?;
    let ((file, pruned), command, default_name) = match target {
        ExportTarget::Racials => (
            export::export_racials_with_report(tables, &overrides)?,
            "export-spells --racials".to_owned(),
            "racials.yaml".to_owned(),
        ),
        ExportTarget::Externals(registry) => {
            let registry = ExternalBuffDb::load(&registry)?;
            let exclude = export::spell_ids_in_dir(spells_dir, EXTERNALS_FILE)?;
            let seeds = export::external_seeds(&registry);
            let repeated: Vec<u32> = seeds.intersection(&exclude).copied().collect();
            if !repeated.is_empty() {
                eprintln!("already in another spell file, not repeated: {repeated:?}");
            }
            (
                export::export_externals_with_report(tables, &seeds, &exclude, &overrides)?,
                "export-spells --externals".to_owned(),
                EXTERNALS_FILE.to_owned(),
            )
        }
        ExportTarget::Enchants(enchants) => {
            let enchants = EnchantDb::load(&enchants)?;
            let exclude = export::spell_ids_in_dir(spells_dir, ENCHANTS_FILE)?;
            let seeds = enchants.spell_ids();
            let repeated: Vec<u32> = seeds.intersection(&exclude).copied().collect();
            if !repeated.is_empty() {
                eprintln!("already in another spell file, not repeated: {repeated:?}");
            }
            (
                export::export_enchants(tables, &seeds, &exclude, &overrides)?,
                "export-spells --enchants".to_owned(),
                ENCHANTS_FILE.to_owned(),
            )
        }
        ExportTarget::Items { items, sets } => {
            let specs = export::items::read_item_specs(&items)?;
            let sets_text = std::fs::read_to_string(&sets).map_err(|source| CliError::Read {
                path: sets.clone(),
                source,
            })?;
            let sets: ItemSetFile = serde_yaml::from_str(&sets_text)?;
            let exclude = export::spell_ids_in_dir(spells_dir, ITEMS_FILE)?;
            let seeds = export::item_seeds(&specs, &sets);
            let repeated: Vec<u32> = seeds.intersection(&exclude).copied().collect();
            if !repeated.is_empty() {
                eprintln!("already in another spell file, not repeated: {repeated:?}");
            }
            let (file, pruned, missing) =
                export::export_items(tables, &seeds, &exclude, &overrides)?;
            if !missing.is_empty() {
                eprintln!("warning: item spells missing from the dump, left out: {missing:?}");
            }
            (
                (file, pruned),
                "export-spells --items".to_owned(),
                ITEMS_FILE.to_owned(),
            )
        }
        ExportTarget::Class(name) => {
            let class = parse_class(&name)?;
            (
                export::export_class_with_report(tables, class, &overrides)?,
                format!("export-spells --class {}", class.name().to_lowercase()),
                format!("{}.yaml", class.name().to_lowercase()),
            )
        }
    };
    let text = export::render(&file, &command)?;
    eprintln!(
        "pruned {} effects and {} spells: {}",
        pruned.effects.len(),
        pruned.spells.len(),
        pruned.spell_names().join(", ")
    );
    for spell in &pruned.spells {
        if let Some(spell_override) = overrides.get(spell.id) {
            eprintln!(
                "warning: the overrides mention dropped spell {} ({}): {}",
                spell.name, spell.id, spell_override.note
            );
        }
    }
    let out = out.unwrap_or_else(|| spells_dir.join(default_name));
    if out == Path::new("-") {
        print!("{text}");
    } else {
        std::fs::write(&out, text).map_err(|source| CliError::Write {
            path: out.clone(),
            source,
        })?;
        eprintln!(
            "wrote {} spells (build {}) to {}",
            file.spells.len(),
            file.build,
            out.display()
        );
    }
    Ok(())
}

fn export_talents(
    tables: &Tables,
    class: &str,
    talents_dir: &Path,
    out: Option<PathBuf>,
) -> Result<(), CliError> {
    let class = parse_class(class)?;
    let (file, report) = export::export_talents_with_report(tables, class)?;
    let command = format!("export-talents --class {}", class.name().to_lowercase());
    let text = export::render_talents(&file, &command)?;
    for (node, required) in &report.odd_gates {
        eprintln!(
            "warning: node {node} is gated on {required} points, not {} per tier",
            file.points_per_tier
        );
    }
    if !report.without_tab.is_empty() {
        eprintln!(
            "warning: nodes without a tab, skipped: {:?}",
            report.without_tab
        );
    }
    if !report.without_spell.is_empty() {
        eprintln!(
            "warning: nodes without a spell, skipped: {:?}",
            report.without_spell
        );
    }
    let out =
        out.unwrap_or_else(|| talents_dir.join(format!("{}.yaml", class.name().to_lowercase())));
    if out == Path::new("-") {
        print!("{text}");
    } else {
        std::fs::write(&out, text).map_err(|source| CliError::Write {
            path: out.clone(),
            source,
        })?;
        eprintln!(
            "wrote {} talents in {} tabs (build {}) to {}",
            file.talents.len(),
            file.tabs.len(),
            file.build,
            out.display()
        );
    }
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<(), CliError> {
    std::fs::write(path, text).map_err(|source| CliError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn export_items(tables: &Tables, items_dir: &Path, sets_path: &Path) -> Result<(), CliError> {
    let report = export::items::derive_items(tables);
    for (file_name, file) in export::item_files(tables, &report.items) {
        let path = items_dir.join(format!("{file_name}.yaml"));
        write(&path, &export::render_items(&file, "export-items")?)?;
        eprintln!("wrote {} items to {}", file.items.len(), path.display());
    }
    let sets = export::item_set_file(tables, &report.items);
    write(sets_path, &export::render_item_sets(&sets, "export-items")?)?;
    eprintln!(
        "wrote {} item sets to {}",
        sets.sets.len(),
        sets_path.display()
    );

    for (skip, ids) in &report.skipped {
        eprintln!("skipped {} items: {skip:?}", ids.len());
    }
    for issue in &report.issues {
        eprintln!("warning: item {}: {}", issue.item_id, issue.message);
    }
    // The written files must load.
    let db = csim_engine::item::EquipmentDb::load(items_dir, None, None)?;
    eprintln!("{} items exported", db.len());
    Ok(())
}

/// The paths `export-all` reads and writes.
struct ExportAllPaths<'a> {
    spells: &'a Path,
    talents: &'a Path,
    items: &'a Path,
    item_sets: &'a Path,
    external_buffs: &'a Path,
    enchant_data: &'a Path,
}

/// Runs every export in the order their inputs need: the item spells exclude what the other
/// spell files carry and read the exported items and sets, so they come last; `check` loads
/// the result.
fn export_all(dir: &TableDir, paths: &ExportAllPaths, strict: bool) -> Result<(), CliError> {
    let tables = Tables::load(dir)?;
    for &class in EXPORTED_CLASSES {
        eprintln!("== export-spells --class {class}");
        export_spells(
            &tables,
            ExportTarget::Class(class.to_owned()),
            paths.spells,
            None,
        )?;
    }
    eprintln!("== export-spells --racials");
    export_spells(&tables, ExportTarget::Racials, paths.spells, None)?;
    eprintln!("== export-spells --externals");
    export_spells(
        &tables,
        ExportTarget::Externals(paths.external_buffs.to_path_buf()),
        paths.spells,
        None,
    )?;
    eprintln!("== export-spells --enchants");
    export_spells(
        &tables,
        ExportTarget::Enchants(paths.enchant_data.to_path_buf()),
        paths.spells,
        None,
    )?;
    for &class in EXPORTED_CLASSES {
        eprintln!("== export-talents --class {class}");
        export_talents(&tables, class, paths.talents, None)?;
    }
    eprintln!("== export-items");
    export_items(&tables, paths.items, paths.item_sets)?;
    eprintln!("== export-spells --items");
    export_spells(
        &tables,
        ExportTarget::Items {
            items: paths.items.to_path_buf(),
            sets: paths.item_sets.to_path_buf(),
        },
        paths.spells,
        None,
    )?;
    eprintln!("== check");
    check(paths.spells, strict)
}

fn compare_items(
    dir: &TableDir,
    legacy_path: &Path,
    snapshot: Option<&Path>,
) -> Result<(), CliError> {
    let tables = Tables::load(dir)?;
    let exported: Vec<_> = export::items::derive_items(&tables)
        .items
        .iter()
        .map(|item| item.to_spec())
        .collect();
    let legacy = export::newest_versions(export::items::read_item_specs(legacy_path)?);
    let lines = export::compare_items(&exported, &legacy);
    for line in &lines {
        println!("{line}");
    }
    let compared: Vec<_> = exported
        .iter()
        .filter_map(|item| legacy.get(&item.id).cloned())
        .collect();
    let differing: std::collections::BTreeSet<&str> = lines
        .iter()
        .filter_map(|line| line.split(' ').next())
        .collect();
    eprintln!(
        "{} exported items have a legacy version, {} of them differ",
        compared.len(),
        differing.len()
    );
    if let Some(path) = snapshot {
        let count = compared.len();
        let text = format!(
            "# The hand-authored Classic version (newest phase) of every item `csim-tables\n\
             # export-items` produces, written by `csim-tables compare-items --snapshot`. Frozen\n\
             # reference data for the comparison test; not loaded by the engine.\n{}",
            serde_yaml::to_string(&ItemFile {
                build: String::new(),
                items: compared,
            })?
        );
        write(path, &text)?;
        eprintln!("wrote {count} legacy items to {}", path.display());
    }
    Ok(())
}

fn check(spells_dir: &Path, strict: bool) -> Result<(), CliError> {
    let db = SpellDb::load(spells_dir)?;
    println!(
        "{} spells, {} overrides, build {}",
        db.len(),
        db.overrides().len(),
        db.build().unwrap_or("unknown")
    );
    let unsupported = db.unsupported();
    for item in &unsupported {
        let name = db.get(item.spell).map_or("?", |r| r.name.as_str());
        println!("unsupported: {item} ({name})");
    }
    println!("{} unsupported effects", unsupported.len());
    if strict && !unsupported.is_empty() {
        return Err(CliError::Unsupported(unsupported.len()));
    }
    Ok(())
}

fn open(cli: &Cli) -> Result<TableDir, TableError> {
    match &cli.build {
        Some(build) => TableDir::open_build(&cli.tables, build),
        None => TableDir::open(&cli.tables),
    }
}

fn info(dir: &TableDir, all: bool) -> Result<(), TableError> {
    println!("build {} in {}", dir.build(), dir.dir().display());
    let names: Vec<&str> = if all {
        dir.table_names().collect()
    } else {
        ALL_TABLES.to_vec()
    };
    for name in names {
        if dir.has(name) {
            println!("{:<32} {:>8} rows", name, dir.row_count(name)?);
        } else {
            println!("{name:<32}  missing");
        }
    }
    let missing = missing_tables(dir, ALL_TABLES.iter().copied());
    if !missing.is_empty() {
        println!(
            "missing tables needed by the exporters: {}",
            missing.join(", ")
        );
    }
    Ok(())
}

fn describe_spell(tables: &Tables, id: u32, depth: usize, seen: &mut Vec<u32>) {
    if id == 0 || seen.contains(&id) {
        return;
    }
    seen.push(id);
    let indent = "  ".repeat(depth);
    let Some(name) = tables.spell_name(id) else {
        println!("{indent}{id}: no such spell");
        return;
    };
    let subtext = tables
        .spell(id)
        .map(|s| s.name_subtext.as_str())
        .unwrap_or("");
    println!("{indent}{id} {name} {subtext}");
    if let Some(misc) = tables.spell_misc(id) {
        let duration = tables
            .spell_duration(misc.duration_index)
            .map(|d| d.duration);
        let cast = tables
            .spell_cast_times(misc.casting_time_index)
            .map(|c| c.base);
        let range = tables.spell_range(misc.range_index).map(|r| r.range_max[0]);
        println!(
            "{indent}  attributes {:#x} school {} cast {cast:?} ms duration {duration:?} ms range {range:?} yd",
            misc.attributes[0], misc.school_mask
        );
    }
    for power in tables.spell_power(id) {
        println!(
            "{indent}  power type {} cost {} pct {}",
            power.power_type, power.mana_cost, power.power_cost_pct
        );
    }
    if let Some(cd) = tables.spell_cooldowns(id) {
        println!(
            "{indent}  cooldown {} ms category {} ms gcd {} ms",
            cd.recovery_time, cd.category_recovery_time, cd.start_recovery_time
        );
    }
    if let Some(cat) = tables.spell_categories(id) {
        let category_name = tables
            .spell_category(cat.category)
            .map(|c| c.name.as_str())
            .unwrap_or("");
        println!(
            "{indent}  category {} {category_name:?} defense {} mechanic {} start recovery category {}",
            cat.category, cat.defense_type, cat.mechanic, cat.start_recovery_category
        );
    }
    if let Some(shape) = tables.spell_shapeshift(id) {
        println!("{indent}  shapeshift mask {:#x}", shape.shapeshift_mask[0]);
    }
    if let Some(aura) = tables.spell_aura_options(id) {
        let ppm = tables
            .spell_procs_per_minute(aura.spell_procs_per_minute_id)
            .map(|p| p.base_proc_rate);
        println!(
            "{indent}  proc chance {} charges {} icd {} ms mask {:#x} ppm {ppm:?} max stacks {}",
            aura.proc_chance,
            aura.proc_charges,
            aura.proc_category_recovery,
            aura.proc_type_mask[0],
            aura.cumulative_aura
        );
    }
    if let Some(levels) = tables.spell_levels(id) {
        println!(
            "{indent}  levels base {} spell {} max {}",
            levels.base_level, levels.spell_level, levels.max_level
        );
    }
    if let Some(class) = tables.spell_class_options(id) {
        println!(
            "{indent}  family {} mask {:?}",
            class.spell_class_set, class.spell_class_mask
        );
    }
    if let Some(equip) = tables.spell_equipped_items(id) {
        println!(
            "{indent}  equipped item class {} subclass mask {:#x} inv types {:#x}",
            equip.equipped_item_class, equip.equipped_item_subclass, equip.equipped_item_inv_types
        );
    }
    if let Some(aura) = tables.spell_aura_restrictions(id) {
        println!(
            "{indent}  aura state caster {} target {} caster spell {}",
            aura.caster_aura_state, aura.target_aura_state, aura.caster_aura_spell
        );
    }
    if let Some(target) = tables.spell_target_restrictions(id) {
        if target.max_targets > 0 {
            println!("{indent}  max targets {}", target.max_targets);
        }
    }
    for ability in tables.skill_line_abilities_of_spell(id) {
        println!(
            "{indent}  skill line {} class mask {:#x} race mask {:#x} supercedes {} acquire {}",
            ability.skill_line,
            ability.class_mask,
            ability.race_masks[0],
            ability.supercedes_spell,
            ability.acquire_method
        );
    }
    if let Some(spell) = tables.spell(id) {
        if !spell.description.is_empty() {
            println!(
                "{indent}  description: {}",
                spell.description.replace('\n', " ")
            );
        }
    }
    let mut triggered = Vec::new();
    for effect in tables.spell_effects(id) {
        println!(
            "{indent}  E{}: effect {} aura {} points {} per level {} variance {} period {} ms misc {:?} trigger {} target {:?} class mask {:?}",
            effect.effect_index,
            effect.effect,
            effect.effect_aura,
            effect.effect_base_points,
            effect.effect_real_points_per_level,
            effect.variance,
            effect.effect_aura_period,
            effect.effect_misc_value,
            effect.effect_trigger_spell,
            effect.implicit_target,
            effect.effect_spell_class_mask
        );
        if effect.effect_trigger_spell != 0 {
            triggered.push(effect.effect_trigger_spell);
        }
    }
    for trigger in triggered {
        describe_spell(tables, trigger, depth + 1, seen);
    }
}

fn describe_item(tables: &Tables, id: u32) {
    let Some(item) = tables.item(id) else {
        println!("{id}: no such item");
        return;
    };
    let sub_class = tables
        .item_sub_class(item.class_id, item.subclass_id)
        .map(|s| s.display_name.as_str())
        .unwrap_or("");
    println!(
        "{id} class {} subclass {} {sub_class:?} inventory type {} material {} icon {}",
        item.class_id, item.subclass_id, item.inventory_type, item.material, item.icon_file_data_id
    );
    let Some(sparse) = tables.item_sparse(id) else {
        println!("  no ItemSparse row (not in this dump)");
        return;
    };
    println!("  {:?}", sparse.name);
    if !sparse.description.is_empty() {
        println!("  description: {:?}", sparse.description);
    }
    println!(
        "  item level {} required level {} quality {} bonding {} max count {} flags (hex) {:x?}",
        sparse.item_level,
        sparse.required_level,
        sparse.overall_quality_id,
        sparse.bonding,
        sparse.max_count,
        sparse.flags
    );
    println!(
        "  allowable class {} race (hex) {:x?} faction {} reputation {} skill {} rank {}",
        sparse.allowable_class,
        sparse.allowable_race,
        sparse.min_faction_id,
        sparse.min_reputation,
        sparse.required_skill,
        sparse.required_skill_rank
    );
    if sparse.item_delay > 0 {
        println!(
            "  delay {} ms variance {} damage type {}",
            sparse.item_delay, sparse.dmg_variance, sparse.damage_type
        );
    }
    if sparse.quality_modifier != 0.0 {
        println!("  bonus armor {}", sparse.quality_modifier);
    }
    for (stat, pct) in sparse
        .stat_modifier_bonus_stat
        .iter()
        .zip(&sparse.stat_percent_editor)
        .filter(|(stat, _)| **stat >= 0)
    {
        println!("  stat {stat} budget share {pct}");
    }
    for effect in tables.item_effects_of_item(id) {
        let name = tables.spell_name(effect.spell_id).unwrap_or("?");
        println!(
            "  effect {} trigger {} spell {} {name:?} charges {} cooldown {} ms category {} ({} ms)",
            effect.id,
            effect.trigger_type,
            effect.spell_id,
            effect.charges,
            effect.cooldown_ms,
            effect.spell_category_id,
            effect.category_cooldown_ms
        );
    }
    if let Some(set) = tables.item_set(sparse.item_set) {
        let members: Vec<u32> = set.item_ids.iter().copied().filter(|&i| i != 0).collect();
        println!("  set {} {:?} items {members:?}", set.id, set.name);
        for bonus in tables.item_set_spells(set.id) {
            let name = tables.spell_name(bonus.spell_id).unwrap_or("?");
            println!(
                "    ({}) spell {} {name:?}",
                bonus.threshold, bonus.spell_id
            );
        }
    }
    if let Some(limit) = tables.item_limit_category(sparse.limit_category) {
        println!(
            "  limit category {} {:?} quantity {}",
            limit.id, limit.name, limit.quantity
        );
    }
    if let Some(subtitle) = tables.item_name_description(sparse.item_name_description_id) {
        println!("  subtitle {:?}", subtitle.description);
    }
    for &tree in tables.item_bonus_trees(id) {
        let nodes = tables.item_bonus_tree_nodes(tree);
        println!("  bonus tree {tree}: {} nodes", nodes.len());
        for node in nodes {
            let bonuses: Vec<String> = tables
                .item_bonuses(node.child_item_bonus_list_id)
                .iter()
                .map(|b| match b.bonus_type {
                    5 => format!(
                        "{:?}",
                        tables
                            .item_name_description(b.value[0] as u32)
                            .map(|d| d.description.as_str())
                            .unwrap_or("?")
                    ),
                    2 => format!("stat {} share {}", b.value[0], b.value[1]),
                    other => format!("type {other} {:?}", b.value),
                })
                .collect();
            println!(
                "    context {} list {}: {}",
                node.item_context,
                node.child_item_bonus_list_id,
                bonuses.join(", ")
            );
        }
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    match &cli.command {
        Command::Info { all } => Ok(info(&open(&cli)?, *all)?),
        Command::Spell { id } => {
            let tables = Tables::load(&open(&cli)?)?;
            describe_spell(&tables, *id, 0, &mut Vec::new());
            Ok(())
        }
        Command::Item { id } => {
            let tables = Tables::load(&open(&cli)?)?;
            describe_item(&tables, *id);
            Ok(())
        }
        Command::ExportSpells {
            class,
            racials,
            externals,
            enchants,
            external_buffs,
            enchant_data,
            items,
            item_data,
            item_sets,
            spells,
            out,
        } => {
            let target = match class {
                Some(class) => ExportTarget::Class(class.clone()),
                None if *racials => ExportTarget::Racials,
                None if *externals => ExportTarget::Externals(external_buffs.clone()),
                None if *enchants => ExportTarget::Enchants(enchant_data.clone()),
                None if *items => ExportTarget::Items {
                    items: item_data.clone(),
                    sets: item_sets.clone(),
                },
                None => unreachable!(
                    "clap requires --class, --racials, --externals, --enchants or --items"
                ),
            };
            export_spells(&Tables::load(&open(&cli)?)?, target, spells, out.clone())
        }
        Command::ExportTalents {
            class,
            talents,
            out,
        } => export_talents(&Tables::load(&open(&cli)?)?, class, talents, out.clone()),
        Command::ExportItems { items, sets } => {
            export_items(&Tables::load(&open(&cli)?)?, items, sets)
        }
        Command::ExportAll {
            spells,
            talents,
            items,
            item_sets,
            external_buffs,
            enchant_data,
            strict,
        } => export_all(
            &open(&cli)?,
            &ExportAllPaths {
                spells,
                talents,
                items,
                item_sets,
                external_buffs,
                enchant_data,
            },
            *strict,
        ),
        Command::CompareItems { legacy, snapshot } => {
            compare_items(&open(&cli)?, legacy, snapshot.as_deref())
        }
        Command::Check { spells, strict } => check(spells, *strict),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
