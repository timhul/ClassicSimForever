//! `csim-tables`: inspect the client table dumps and (later Phase 3T tasks) export them to the
//! YAML data files under `data/`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use csim_tables::tables::ALL_TABLES;
use csim_tables::{dir::missing_tables, TableDir, TableError, Tables};

#[derive(Parser)]
#[command(name = "csim-tables", version, about)]
struct Cli {
    /// Directory holding the `<Table>.<build>.csv` files.
    #[arg(long, default_value = "data/tables", global = true)]
    tables: PathBuf,
    /// Build to use when the directory holds several (e.g. `1.60.1.69893`).
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

fn run(cli: Cli) -> Result<(), TableError> {
    let dir = open(&cli)?;
    match cli.command {
        Command::Info { all } => info(&dir, all),
        Command::Spell { id } => {
            let tables = Tables::load(&dir)?;
            describe_spell(&tables, id, 0, &mut Vec::new());
            Ok(())
        }
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
