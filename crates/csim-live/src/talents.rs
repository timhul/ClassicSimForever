//! The talent calculator behind the page's Talents view: a class's tree as the page draws it
//! ([`Layout`]), and the point rules of [`CharacterTalents`] applied to the ranks the page
//! holds ([`edit`]). The page keeps its draft of ranks; each click is one [`EditRequest`]
//! answered with the new [`State`], so the rules stay in one place.
//!
//! A build travels in links as a [`code`]: Wowhead's classic talent string, one digit per
//! talent of each tab in tier then column order, trailing zeros left out, the tabs joined by
//! `-` (`30532...-05050...`).

use std::collections::BTreeMap;
use std::sync::Arc;

use csim_engine::data_bundle::DataBundle;
use csim_engine::faction::PlayerClass;
use csim_engine::talent::{CharacterTalents, TalentFile, TalentSpec};
use serde::{Deserialize, Serialize};

use crate::session::Icon;

/// A setup's talents: tab name → talent name → rank (as a setup file has them).
pub type Talents = BTreeMap<String, BTreeMap<String, u32>>;

/// A class's tree, to draw: the answer of `api/talents` with the session's [`State`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layout {
    pub class: PlayerClass,
    /// Talent points to spend.
    pub points: u32,
    /// Points spent in a tab per tier a tier requires.
    pub points_per_tier: u32,
    /// In the game's order (Arms, Fury, Protection).
    pub tabs: Vec<TabLayout>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TabLayout {
    pub name: String,
    pub skill_line: u32,
    /// `TalentTab.ID`, by which Wowhead serves the tab's background (0 = none).
    pub talent_tab: u32,
    pub icon: Option<Icon>,
    /// By tier, then column.
    pub talents: Vec<TalentLayout>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TalentLayout {
    pub node: u32,
    pub name: String,
    pub tier: u32,
    pub column: u32,
    pub max_ranks: u32,
    /// The node that must be maxed first.
    pub requires: Option<u32>,
    pub icon: Option<Icon>,
    /// The talent's text at each rank: `descriptions[r − 1]` is rank `r`'s.
    pub descriptions: Vec<String>,
}

/// Ranks spent, and what can take a point: the answer of `api/talents/edit`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct State {
    /// The talents with points, by node.
    pub ranks: BTreeMap<u32, u32>,
    /// Points spent per tab, in tab order.
    pub tab_points: Vec<u32>,
    pub points_left: u32,
    /// The character level the points need (the first point at level 10).
    pub required_level: u32,
    /// The talents a point can go into now (or that have points while none are left), by
    /// node, as the game shows them lit.
    pub available: Vec<u32>,
    /// The build as a link carries it ([`code`]).
    pub code: String,
}

/// A click in the calculator: `op` applied to the talents of `class` at `ranks`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    pub class: PlayerClass,
    #[serde(default)]
    pub ranks: BTreeMap<u32, u32>,
    pub op: Op,
    /// The talent of `increment`, `decrement`, `max` and `min`.
    pub node: Option<u32>,
    /// The tab (skill line) of `clear_tab`.
    pub tab: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// No change: the state of `ranks`.
    None,
    Increment,
    Decrement,
    /// As many points as the rules let in.
    Max,
    /// As many points out as the rules let go.
    Min,
    ClearTab,
    ClearAll,
}

impl Layout {
    pub fn of(file: &TalentFile) -> Layout {
        let tabs = file
            .tabs
            .iter()
            .map(|tab| TabLayout {
                name: tab.name.clone(),
                skill_line: tab.skill_line,
                talent_tab: tab.talent_tab,
                icon: Icon::new(tab.icon, tab.icon_name.as_deref()),
                talents: file
                    .talents
                    .iter()
                    .filter(|talent| talent.tab == tab.skill_line)
                    .map(|talent| TalentLayout {
                        node: talent.node,
                        name: talent.name.clone(),
                        tier: talent.tier,
                        column: talent.column,
                        max_ranks: talent.max_ranks,
                        requires: talent.requires,
                        icon: Icon::new(talent.icon, talent.icon_name.as_deref()),
                        descriptions: talent.descriptions.clone(),
                    })
                    .collect(),
            })
            .collect();
        Layout {
            class: file.class,
            points: file.points,
            points_per_tier: file.points_per_tier,
            tabs,
        }
    }
}

impl State {
    pub fn of(talents: &CharacterTalents) -> State {
        let file = talents.file();
        let spent = talents.spent_points();
        let ranks = talents.setup().into_iter().collect();
        State {
            code: code(file, &ranks),
            ranks,
            tab_points: file
                .tabs
                .iter()
                .map(|tab| talents.tab_points(tab.skill_line))
                .collect(),
            points_left: talents.points_remaining(),
            required_level: if spent == 0 { 1 } else { 9 + spent },
            available: file
                .talents
                .iter()
                .map(|talent| talent.node)
                .filter(|&node| talents.is_available(node))
                .collect(),
        }
    }
}

/// The build `ranks` (by node) of `file` as a link carries it: per tab one digit per talent,
/// in tier then column order, trailing zeros left out; the tabs joined by `-`, trailing empty
/// ones left out (`""` for no points).
pub fn code(file: &TalentFile, ranks: &BTreeMap<u32, u32>) -> String {
    let mut tabs: Vec<String> = file
        .tabs
        .iter()
        .map(|tab| {
            let digits: String = in_order(file, tab.skill_line)
                .map(|talent| {
                    let rank = ranks.get(&talent.node).copied().unwrap_or(0);
                    char::from_digit(rank.min(9), 10).unwrap_or('0')
                })
                .collect();
            digits.trim_end_matches('0').to_owned()
        })
        .collect();
    while tabs.last().is_some_and(String::is_empty) {
        tabs.pop();
    }
    tabs.join("-")
}

/// The talents a [`code`] names, by tab and talent name (no check of the rules: the setup's
/// build does that).
///
/// # Errors
/// More tabs or digits than `file` has, or a character that is not a digit.
pub fn from_code(file: &TalentFile, code: &str) -> Result<Talents, String> {
    let parts: Vec<&str> = code.trim().split('-').collect();
    if parts.len() > file.tabs.len() {
        return Err(format!(
            "talents '{code}': {} tabs, the {:?} has {}",
            parts.len(),
            file.class,
            file.tabs.len()
        ));
    }
    let mut talents = Talents::new();
    for (tab, part) in file.tabs.iter().zip(parts) {
        let order: Vec<_> = in_order(file, tab.skill_line).collect();
        if part.len() > order.len() {
            return Err(format!(
                "talents '{code}': {} has {} talents, not {}",
                tab.name,
                order.len(),
                part.len()
            ));
        }
        let mut ranks = BTreeMap::new();
        for (talent, digit) in order.into_iter().zip(part.chars()) {
            let rank = digit
                .to_digit(10)
                .ok_or_else(|| format!("talents '{code}': '{digit}' is not a rank"))?;
            if rank > 0 {
                ranks.insert(talent.name.clone(), rank);
            }
        }
        if !ranks.is_empty() {
            talents.insert(tab.name.clone(), ranks);
        }
    }
    Ok(talents)
}

/// The talents of tab `skill_line` in tier then column order.
fn in_order(file: &TalentFile, skill_line: u32) -> impl Iterator<Item = &TalentSpec> {
    let mut talents: Vec<&TalentSpec> = file
        .talents
        .iter()
        .filter(|talent| talent.tab == skill_line)
        .collect();
    talents.sort_by_key(|talent| (talent.tier, talent.column));
    talents.into_iter()
}

/// The talents of `file` with `ranks` spent, tier by tier as the game lets them in.
///
/// # Errors
/// A node not in the tree, or ranks the rules do not allow (above the maximum, a tier not
/// unlocked, a prerequisite not maxed, more points than there are).
pub fn spent(
    file: &Arc<TalentFile>,
    ranks: &BTreeMap<u32, u32>,
) -> Result<CharacterTalents, String> {
    let mut wanted = Vec::new();
    for (&node, &rank) in ranks {
        let spec = file
            .talent(node)
            .ok_or_else(|| format!("no talent {node} in the {:?} tree", file.class))?;
        if rank > 0 {
            wanted.push((spec, rank));
        }
    }
    wanted.sort_by_key(|(spec, _)| (spec.tier, spec.tab, spec.column));
    let mut talents = CharacterTalents::new(Arc::clone(file));
    for (spec, rank) in wanted {
        for _ in 0..rank {
            if talents.increment_rank(spec.node).is_none() {
                return Err(format!(
                    "{} cannot have rank {rank} (points, tier or prerequisite)",
                    spec.name
                ));
            }
        }
    }
    Ok(talents)
}

/// Applies `request` to its ranks with the game's rules. A click the rules refuse changes
/// nothing (the same state comes back).
///
/// # Errors
/// The class has no tree, the ranks are not a legal tree ([`spent`]), or the op lacks its
/// node or tab.
pub fn edit(data: &DataBundle, request: &EditRequest) -> Result<State, String> {
    let file = data
        .talents
        .get(request.class)
        .ok_or_else(|| format!("no talent tree for the {:?}", request.class))?;
    let mut talents = spent(file, &request.ranks)?;
    let node = || {
        request
            .node
            .ok_or_else(|| "this op needs a `node`".to_owned())
    };
    match request.op {
        Op::None => {}
        Op::Increment => {
            talents.increment_rank(node()?);
        }
        Op::Decrement => {
            talents.decrement_rank(node()?);
        }
        Op::Max => {
            talents.increase_to_max_rank(node()?);
        }
        Op::Min => {
            talents.decrease_to_min_rank(node()?);
        }
        Op::ClearTab => {
            let tab = request
                .tab
                .ok_or_else(|| "clear_tab needs a `tab`".to_owned())?;
            talents.clear_tab(tab);
        }
        Op::ClearAll => {
            talents.clear_all();
        }
    }
    Ok(State::of(&talents))
}

#[cfg(test)]
mod tests;
