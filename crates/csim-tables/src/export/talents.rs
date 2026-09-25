//! `export-talents`: the talent tree of a class as `data/talents/<class>.yaml`.
//!
//! The walk follows `data/TALENT_INSTRUCTIONS.md` §1.2–1.5: the class skill lines lead to the
//! Trait tree (`SkillLineXTraitTree`), its nodes are the talents, each node's entry gives the
//! rank count and its definition the talent spell, `TraitNodeGroupDisplayInfo` puts the node in
//! a tab, the node position gives tier and column, `TraitEdge` the prerequisite and
//! `TraitDefinitionEffectPoints` → `CurvePoint` the value of each effect per rank. The tier
//! gate (`TraitCond`) is checked against the `points_per_tier × tier` rule the runtime applies
//! and reported when it differs.

use std::collections::{BTreeMap, BTreeSet};

use csim_engine::faction::PlayerClass;
use csim_engine::talent::{TalentFile, TalentSpec, TalentTab};

use crate::db::Tables;
use crate::export::spells::{class_skill_lines, ExportError};
use crate::tables::TraitNodeRow;

/// `TraitEdge.Type` of a prerequisite edge.
const PREREQUISITE_EDGE: u32 = 2;
/// `TraitCond.CondType` of a "points spent" condition.
const SPENT_POINTS_COND: u32 = 0;
/// The layout grid step of `TraitNode.PosX` / `PosY`.
const GRID: i32 = 600;

/// What the exporter could not take at face value; printed by the CLI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TalentReport {
    /// Nodes whose `TraitCond` gate is not `points_per_tier × tier` (`(node, required)`).
    pub odd_gates: Vec<(u32, u32)>,
    /// Nodes without a tab group (skipped).
    pub without_tab: Vec<u32>,
    /// Nodes whose spell is not in the tables (skipped).
    pub without_spell: Vec<u32>,
}

impl TalentReport {
    pub fn is_empty(&self) -> bool {
        self.odd_gates.is_empty() && self.without_tab.is_empty() && self.without_spell.is_empty()
    }
}

/// The Trait tree of `class` (`SkillLineXTraitTree` of one of its skill lines).
pub fn trait_tree(tables: &Tables, class: PlayerClass) -> Result<u32, ExportError> {
    let lines = class_skill_lines(tables, class)?;
    tables
        .skill_line_x_trait_tree()
        .iter()
        .find(|link| lines.contains(&link.skill_line_id))
        .map(|link| link.trait_tree_id)
        .ok_or(ExportError::NoTraitTree(class.name().to_owned()))
}

/// Builds the talent file of `class`.
pub fn export_talents(tables: &Tables, class: PlayerClass) -> Result<TalentFile, ExportError> {
    export_talents_with_report(tables, class).map(|(file, _)| file)
}

/// [`export_talents`] plus what did not fit the model.
pub fn export_talents_with_report(
    tables: &Tables,
    class: PlayerClass,
) -> Result<(TalentFile, TalentReport), ExportError> {
    let tree = trait_tree(tables, class)?;
    let nodes = tables.trait_nodes_of_tree(tree);
    let mut report = TalentReport::default();

    // Tabs: the groups with display info, in `OrderIndex` order.
    let mut tab_groups: BTreeMap<u32, (u32, u32)> = BTreeMap::new(); // order → (group, line)
    for &node in nodes {
        for &group in tables.trait_groups_of_node(node) {
            if let Some(info) = tables.trait_group_display_info(group) {
                tab_groups.insert(info.order_index, (group, info.skill_line_id));
            }
        }
    }
    if tab_groups.is_empty() {
        return Err(ExportError::NoTraitTree(class.name().to_owned()));
    }
    let tabs: Vec<TalentTab> = tab_groups
        .values()
        .map(|&(_, line)| TalentTab {
            skill_line: line,
            name: tables
                .skill_line(line)
                .map(|l| l.display_name.clone())
                .unwrap_or_default(),
        })
        .collect();
    let tab_of_group: BTreeMap<u32, u32> = tab_groups.values().copied().collect();

    // Layout: tiers from the tree's top row, columns from each tab's left column.
    let positioned: Vec<(&TraitNodeRow, u32)> = nodes
        .iter()
        .filter_map(|&id| {
            let node = tables.trait_node(id)?;
            let tab = tables
                .trait_groups_of_node(id)
                .iter()
                .find_map(|g| tab_of_group.get(g).copied());
            match tab {
                Some(tab) => Some((node, tab)),
                None => {
                    report.without_tab.push(id);
                    None
                }
            }
        })
        .collect();
    let top = positioned.iter().map(|(n, _)| n.pos_y).min().unwrap_or(0);
    let mut left: BTreeMap<u32, i32> = BTreeMap::new();
    for (node, tab) in &positioned {
        left.entry(*tab)
            .and_modify(|x| *x = (*x).min(node.pos_x))
            .or_insert(node.pos_x);
    }

    let points = tables
        .trait_currency(talent_currency(tables, nodes))
        .map_or(csim_engine::talent::spec::DEFAULT_POINTS, |c| c.sourced_max);
    let points_per_tier = points_per_tier(tables, &positioned, top);

    let mut talents = Vec::new();
    for (node, tab) in &positioned {
        let tier = u32::try_from((node.pos_y - top) / GRID).unwrap_or(0);
        let column = u32::try_from((node.pos_x - left[tab]) / GRID).unwrap_or(0);
        let Some((entry, definition)) =
            tables
                .trait_node_entries_of_node(node.id)
                .iter()
                .find_map(|&e| {
                    let entry = tables.trait_node_entry(e)?;
                    let definition = tables.trait_definition(entry.trait_definition_id)?;
                    Some((entry, definition))
                })
        else {
            report.without_spell.push(node.id);
            continue;
        };
        if !tables.spell_exists(definition.spell_id) {
            report.without_spell.push(node.id);
            continue;
        }
        let mut requires: Vec<u32> = tables
            .trait_edges()
            .iter()
            .filter(|e| e.right_trait_node_id == node.id && e.edge_type == PREREQUISITE_EDGE)
            .map(|e| e.left_trait_node_id)
            .collect();
        requires.sort_unstable();
        requires.dedup();
        if requires.len() > 1 {
            return Err(ExportError::SeveralPrerequisites(node.id, requires));
        }
        let mut rank_values = BTreeMap::new();
        for points in tables.trait_definition_effect_points(definition.id) {
            let values: Vec<f64> = tables
                .curve_points(points.curve_id)
                .iter()
                .take(entry.max_ranks as usize)
                .map(|p| f64::from(p.pos[1]))
                .collect();
            if values.len() == entry.max_ranks as usize {
                rank_values.insert(points.effect_index, values);
            }
        }
        for (required, _) in gates(tables, node.id) {
            if required != points_per_tier * tier {
                report.odd_gates.push((node.id, required));
            }
        }
        talents.push(TalentSpec {
            node: node.id,
            spell: definition.spell_id,
            name: tables
                .spell_name(definition.spell_id)
                .unwrap_or("")
                .to_owned(),
            tab: *tab,
            tier,
            column,
            max_ranks: entry.max_ranks,
            requires: requires.first().copied(),
            rank_values,
        });
    }
    let order: BTreeMap<u32, usize> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| (tab.skill_line, i))
        .collect();
    talents.sort_by_key(|t| (order[&t.tab], t.tier, t.column));
    report.odd_gates.sort_unstable();
    report.without_tab.sort_unstable();
    report.without_spell.sort_unstable();

    let file = TalentFile {
        build: tables.build().to_owned(),
        class,
        tree,
        points,
        points_per_tier,
        tabs,
        talents,
    };
    file.validate()
        .map_err(|source| ExportError::InvalidTalents(class.name().to_owned(), source))?;
    Ok((file, report))
}

/// The "points spent" conditions gating `node`: `(required, counted group)`.
fn gates(tables: &Tables, node: u32) -> Vec<(u32, u32)> {
    let mut gates: Vec<(u32, u32)> = tables
        .trait_groups_of_node(node)
        .iter()
        .flat_map(|&g| tables.trait_conds_of_group(g))
        .filter_map(|&c| tables.trait_cond(c))
        .filter(|c| c.cond_type == SPENT_POINTS_COND && c.spent_amount_required > 0)
        .map(|c| (c.spent_amount_required, c.trait_node_group_id))
        .collect();
    gates.sort_unstable();
    gates.dedup();
    gates
}

/// The talent currency: what the tree's gates count (3820 for the classes).
fn talent_currency(tables: &Tables, nodes: &[u32]) -> u32 {
    nodes
        .iter()
        .flat_map(|&n| tables.trait_groups_of_node(n))
        .flat_map(|&g| tables.trait_conds_of_group(g))
        .filter_map(|&c| tables.trait_cond(c))
        .map(|c| c.trait_currency_id)
        .find(|&id| id != 0)
        .unwrap_or(0)
}

/// Points per tier: the smallest gate of a tier-1 node (5 in this build), falling back to the
/// engine default when no node is gated.
fn points_per_tier(tables: &Tables, nodes: &[(&TraitNodeRow, u32)], top: i32) -> u32 {
    let gated: BTreeSet<u32> = nodes
        .iter()
        .filter(|(node, _)| (node.pos_y - top) / GRID == 1)
        .flat_map(|(node, _)| gates(tables, node.id))
        .map(|(required, _)| required)
        .collect();
    gated
        .into_iter()
        .next()
        .unwrap_or(csim_engine::talent::spec::DEFAULT_POINTS_PER_TIER)
}

/// Renders a talent file as YAML with a header naming its origin.
pub fn render_talents(file: &TalentFile, command: &str) -> Result<String, serde_yaml::Error> {
    let body = crate::export::spells::flow_scalar_sequences(&serde_yaml::to_string(file)?);
    Ok(format!(
        "# Generated by `csim-tables {command}`.\n\
         # Do not edit: re-export from a new table dump instead. What a talent does is in its\n\
         # spell (data/spells/<class>.yaml); the rank values here replace the spell's effect\n\
         # base points at each rank (see data/TALENT_INSTRUCTIONS.md).\n\
         {body}"
    ))
}
