//! `csim list-items`, `list-spells` and `list-rotations`.

use clap::Args;
use csim_engine::data_bundle::DataBundle;
use csim_engine::faction::PlayerClass;
use csim_engine::item::EquipmentSlot;
use csim_engine::phase::Phase;

use crate::table::Table;
use crate::{parse_serde_name, serde_name};

#[derive(Debug, Args)]
pub struct ItemArgs {
    /// Only items that fit this slot (MAINHAND, OFFHAND, HEAD, RING1, ...).
    #[arg(long, value_parser = parse_serde_name::<EquipmentSlot>)]
    slot: Option<EquipmentSlot>,
    /// Only items available in this content phase (1-6), in their version of that phase.
    #[arg(long, value_parser = parse_phase)]
    phase: Option<Phase>,
    /// Only items whose name contains this text (case-insensitive).
    #[arg(long)]
    search: Option<String>,
}

#[derive(Debug, Args)]
pub struct SpellArgs {
    /// Only spells of this class (WARRIOR, ...).
    #[arg(long, value_parser = parse_serde_name::<PlayerClass>)]
    class: Option<PlayerClass>,
    /// Only spells whose name contains this text (case-insensitive).
    #[arg(long)]
    search: Option<String>,
}

#[derive(Debug, Args)]
pub struct RotationArgs {
    /// Only rotations of this class (WARRIOR, ...).
    #[arg(long, value_parser = parse_serde_name::<PlayerClass>)]
    class: Option<PlayerClass>,
}

pub fn parse_phase(text: &str) -> Result<Phase, String> {
    let number: u8 = text
        .parse()
        .map_err(|_| format!("expected a phase number, got {text:?}"))?;
    Phase::try_from(number).map_err(|error| error.to_string())
}

pub(crate) fn matches(name: &str, search: Option<&str>) -> bool {
    search.is_none_or(|search| name.to_lowercase().contains(&search.to_lowercase()))
}

pub fn items(data: &DataBundle, args: &ItemArgs) {
    let db = &data.equipment;
    let mut table = Table::new(["Id", "Name", "Slot", "Type", "Quality", "Phase", "Weapon"])
        .left(1)
        .left(2)
        .left(3)
        .left(4)
        .left(6);
    for id in db.item_ids() {
        let item = match args.phase {
            Some(phase) => db.get_item(id, phase),
            None => db.item(id),
        };
        let Some(item) = item else { continue };
        if !matches(item.name(), args.search.as_deref())
            || args.slot.is_some_and(|slot| !item.fits(slot))
        {
            continue;
        }
        let weapon = item.weapon().map_or_else(String::new, |weapon| {
            format!(
                "{}-{} @ {:.1} ({:.1} dps)",
                weapon.min_dmg,
                weapon.max_dmg,
                weapon.speed,
                weapon.dps()
            )
        });
        table.row(vec![
            id.to_string(),
            item.name().to_string(),
            serde_name(&item.slot()),
            serde_name(&item.item_type()),
            serde_name(&item.quality()),
            item.phase().number().to_string(),
            weapon,
        ]);
    }
    print_or_none(&table, "items");
}

pub fn spells(data: &DataBundle, args: &SpellArgs) {
    let db = &data.spells;
    let mut table = Table::new(["Id", "Name", "Rank", "Class"])
        .left(1)
        .left(2)
        .left(3);
    let mut records: Vec<_> = match args.class {
        Some(class) => db
            .ids_of_class(Some(class))
            .iter()
            .filter_map(|id| db.get(*id))
            .collect(),
        None => db.records(),
    };
    records.sort_by_key(|record| record.id);
    for record in records {
        if !matches(&record.name, args.search.as_deref()) {
            continue;
        }
        let class = db
            .class_of(record.id)
            .flatten()
            .map_or("", PlayerClass::name);
        table.row(vec![
            record.id.to_string(),
            record.name.clone(),
            record.rank_text.clone(),
            class.to_string(),
        ]);
    }
    print_or_none(&table, "spells");
}

pub fn rotations(data: &DataBundle, args: &RotationArgs) {
    let mut table = Table::new(["Class", "Name", "Attack mode", "Description"])
        .left(1)
        .left(2)
        .left(3);
    for rotation in data.rotations.iter() {
        if args.class.is_some_and(|class| class != rotation.class) {
            continue;
        }
        table.row(vec![
            rotation.class.name().to_string(),
            rotation.name.clone(),
            serde_name(&rotation.attack_mode),
            rotation.description.clone(),
        ]);
    }
    print_or_none(&table, "rotations");
}

pub(crate) fn print_or_none(table: &Table, what: &str) {
    if table.is_empty() {
        println!("No {what} found");
    } else {
        print!("{}", table.render());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_parse_by_number() {
        assert_eq!(parse_phase("3"), Ok(Phase::BlackwingLair));
        assert!(parse_phase("7").is_err());
        assert!(parse_phase("BWL").is_err());
    }

    #[test]
    fn slots_and_classes_parse_case_insensitively() {
        assert_eq!(
            parse_serde_name::<EquipmentSlot>("mainhand"),
            Ok(EquipmentSlot::Mainhand)
        );
        assert_eq!(
            parse_serde_name::<PlayerClass>("Warrior"),
            Ok(PlayerClass::Warrior)
        );
        assert!(parse_serde_name::<PlayerClass>("Monk").is_err());
    }

    #[test]
    fn search_is_case_insensitive() {
        assert!(matches("Brutality Blade", Some("brutal")));
        assert!(!matches("Brutality Blade", Some("sword")));
        assert!(matches("Anything", None));
    }
}
