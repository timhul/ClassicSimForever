//! `csim rank-items`: ranks items by their stats times the stat weights of a weights file
//! (`csim run --scale --weights-file`).
//!
//! An approximation that hints at which items are good: only the static stats are scored.
//! Weapon damage, equip and proc effects, set bonuses and random suffixes are not, so the
//! ranking lists them as not scored.

use std::path::PathBuf;

use clap::Args;
use csim_engine::data_bundle::DataBundle;
use csim_engine::faction::PlayerClass;
use csim_engine::item::{EquipmentSlot, Item};
use csim_engine::phase::Phase;

use crate::list::{matches, parse_phase, print_or_none};
use crate::table::Table;
use crate::weights::StatWeights;
use crate::{parse_serde_name, serde_name, Result};

#[derive(Debug, Args)]
pub struct RankArgs {
    /// The stat weights file written by `csim run --scale --weights-file`.
    #[arg(long, value_name = "PATH")]
    weights: PathBuf,
    /// Only items that fit this slot (MAINHAND, OFFHAND, HEAD, RING1, ...).
    #[arg(long, value_parser = parse_serde_name::<EquipmentSlot>)]
    slot: Option<EquipmentSlot>,
    /// Only items available in this content phase (1-6), in their version of that phase
    /// (default: the phase of the weights).
    #[arg(long, value_parser = parse_phase)]
    phase: Option<Phase>,
    /// Only items whose name contains this text (case-insensitive).
    #[arg(long)]
    search: Option<String>,
    /// Only items this class can use (default: the class of the weights).
    #[arg(long, value_parser = parse_serde_name::<PlayerClass>)]
    class: Option<PlayerClass>,
    /// Shows this many items; 0 shows all.
    #[arg(long, short = 'n', default_value_t = 20)]
    limit: usize,
    /// Ranks by threat (TPS) instead of DPS.
    #[arg(long)]
    tps: bool,
}

pub fn items(data: &DataBundle, args: &RankArgs) -> Result<()> {
    let weights = StatWeights::read(&args.weights)?;
    let phase = args.phase.unwrap_or(weights.phase);
    let class = args.class.unwrap_or(weights.class);
    let db = &data.equipment;
    let items: Vec<_> = db
        .item_ids()
        .into_iter()
        .filter_map(|id| db.get_item(id, phase))
        .filter(|item| {
            item.available_for_class(class)
                && matches(item.name(), args.search.as_deref())
                && args.slot.is_none_or(|slot| item.fits(slot))
        })
        .collect();
    let ranked = rank(items.iter().map(|item| item.as_ref()), &weights, args.tps);

    println!(
        "Stat weights of {} ({} {}, phase {}, {:.1} DPS); {} per item",
        weights.setup,
        class.name(),
        weights.rotation,
        phase.number(),
        weights.dps,
        if args.tps { "TPS" } else { "DPS" },
    );
    let mut table = Table::new([
        "#",
        "Id",
        "Name",
        "Slot",
        "Type",
        "Phase",
        "Score",
        "Weapon dps",
        "Not scored",
    ])
    .left(2)
    .left(3)
    .left(4)
    .left(8);
    let limit = if args.limit == 0 {
        usize::MAX
    } else {
        args.limit
    };
    for (rank, (item, score)) in ranked.into_iter().take(limit).enumerate() {
        table.row(vec![
            (rank + 1).to_string(),
            item.id().to_string(),
            item.name().to_string(),
            serde_name(&item.slot()),
            serde_name(&item.item_type()),
            item.phase().number().to_string(),
            format!("{score:.2}"),
            item.weapon()
                .map_or_else(String::new, |weapon| format!("{:.1}", weapon.dps())),
            not_scored(item).join(", "),
        ]);
    }
    print_or_none(&table, "items");
    Ok(())
}

/// The items with their scores, best first; ties by id.
fn rank<'a>(
    items: impl IntoIterator<Item = &'a Item>,
    weights: &StatWeights,
    tps: bool,
) -> Vec<(&'a Item, f64)> {
    let mut ranked: Vec<_> = items
        .into_iter()
        .map(|item| (item, score(item, weights, tps)))
        .collect();
    ranked
        .sort_by(|(a, a_score), (b, b_score)| b_score.total_cmp(a_score).then(a.id().cmp(&b.id())));
    ranked
}

/// The item's static stats times their weights; stats without a weight count nothing.
fn score(item: &Item, weights: &StatWeights, tps: bool) -> f64 {
    item.spec()
        .stats
        .iter()
        .filter_map(|(stat, value)| {
            let weight = weights.get(*stat)?;
            Some(value * if tps { weight.tps } else { weight.dps })
        })
        .sum()
}

/// What the item has that its score leaves out.
fn not_scored(item: &Item) -> Vec<&'static str> {
    let mut parts = Vec::new();
    if !item.effects().is_empty() {
        parts.push("effects");
    }
    if item.set_id().is_some() {
        parts.push("set");
    }
    if !item.suffixes().is_empty() {
        parts.push("suffixes");
    }
    parts
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use csim_engine::item::{ItemSpec, ItemStat};

    use super::*;
    use crate::weights::StatWeight;

    fn item(yaml: &str) -> Item {
        let spec: ItemSpec = serde_yaml::from_str(yaml).unwrap();
        Item::from_spec(spec).unwrap()
    }

    fn weights() -> StatWeights {
        let weight = |dps, tps| StatWeight { dps, tps };
        StatWeights {
            setup: "Test".into(),
            class: PlayerClass::Warrior,
            rotation: "Fury".into(),
            phase: Phase::MoltenCore,
            iterations: 1,
            seed: 1,
            dps: 1000.0,
            tps: 800.0,
            weights: BTreeMap::from([
                (ItemStat::Strength, weight(2.0, 1.5)),
                (ItemStat::HitRating, weight(3.0, 2.0)),
                (ItemStat::HitChance, weight(3000.0, 2000.0)),
            ]),
        }
    }

    const BELT: &str = "{ id: 2, name: Belt, phase: 1, slot: BELT, type: PLATE, quality: EPIC,
        stats: { STRENGTH: 10, HIT_RATING: 5, STAMINA: 20 }, set: 7 }";
    const GLOVES: &str = "{ id: 3, name: Gloves, phase: 1, slot: GLOVES, type: PLATE,
        quality: EPIC, stats: { HIT_CHANCE: 0.01 } }";
    const BRACERS: &str = "{ id: 1, name: Bracers, phase: 1, slot: WRIST, type: PLATE,
        quality: EPIC, stats: { STRENGTH: 15 } }";

    #[test]
    fn score_sums_the_weighted_stats() {
        let belt = item(BELT);
        // 10 * 2 + 5 * 3; stamina has no weight.
        assert_eq!(score(&belt, &weights(), false), 35.0);
        assert_eq!(score(&belt, &weights(), true), 25.0);
        assert_eq!(score(&item(GLOVES), &weights(), false), 30.0);
    }

    #[test]
    fn rank_is_best_first_then_by_id() {
        let items = [item(BELT), item(GLOVES), item(BRACERS)];
        let ranked: Vec<_> = rank(&items, &weights(), false)
            .into_iter()
            .map(|(item, score)| (item.id(), score))
            .collect();
        // Bracers and gloves tie at 30.
        assert_eq!(ranked, [(2, 35.0), (1, 30.0), (3, 30.0)]);
    }

    #[test]
    fn not_scored_lists_what_the_score_leaves_out() {
        assert_eq!(not_scored(&item(BELT)), ["set"]);
        assert!(not_scored(&item(BRACERS)).is_empty());
    }
}
