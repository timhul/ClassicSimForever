//! One tab of a talent tree: the points spent per talent and per tier, and the rules that
//! allow a point to go in or come out.
//! (the bookkeeping; what a rank does to the character is
//! [`CharacterTalents`](crate::talent::CharacterTalents)'s business).
//!
//! Rules (the vanilla ones, as `TraitCond` / `TraitEdge` encode them):
//! - a point can go into a talent when its tier is unlocked (`points_per_tier × tier` points
//!   spent in the tab) and its prerequisite, if any, is maxed;
//! - a point can come out when no talent requiring this one has points, and the tiers above
//!   stay unlocked without it.
//!
//! Talents are addressed by node id. The tree is a pure value: every mutation returns the
//! [`RankChange`]s it caused so the owner can apply them to the character's spells.

use std::sync::Arc;

use crate::talent::spec::{TalentFile, TalentSpec, TalentTab};

/// A talent's rank changed. `from` and `to` differ by one except for a forced clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankChange {
    pub node: u32,
    /// The talent spell.
    pub spell: u32,
    pub from: u32,
    pub to: u32,
}

/// A talent and its points.
#[derive(Debug, Clone, PartialEq)]
pub struct TalentState {
    spec: TalentSpec,
    rank: u32,
}

impl TalentState {
    pub fn spec(&self) -> &TalentSpec {
        &self.spec
    }

    pub fn node(&self) -> u32 {
        self.spec.node
    }

    pub fn spell(&self) -> u32 {
        self.spec.spell
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }

    pub fn rank(&self) -> u32 {
        self.rank
    }

    pub fn max_rank(&self) -> u32 {
        self.spec.max_ranks
    }

    /// Whether any point is spent. Port of `Talent::is_active`.
    pub fn is_active(&self) -> bool {
        self.rank > 0
    }

    /// Port of `Talent::is_maxed`.
    pub fn is_maxed(&self) -> bool {
        self.rank == self.spec.max_ranks
    }

    /// The `(effect index, value)` substitutions of the current rank.
    pub fn values(&self) -> Vec<(u32, f64)> {
        self.spec.values_at(self.rank)
    }
}

/// One tab of the tree with its points. Port of `TalentTree`.
#[derive(Debug, Clone, PartialEq)]
pub struct TalentTree {
    tab: TalentTab,
    points_per_tier: u32,
    /// By tier then column.
    talents: Vec<TalentState>,
    /// Points spent per tier.
    tier_points: Vec<u32>,
    total: u32,
}

impl TalentTree {
    /// The tab `skill_line` of `file`, with no points spent.
    ///
    /// # Panics
    /// Panics if `file` has no such tab.
    pub fn new(file: &Arc<TalentFile>, skill_line: u32) -> Self {
        let tab = file
            .tab(skill_line)
            .unwrap_or_else(|| panic!("{:?} has no talent tab {skill_line}", file.class))
            .clone();
        let talents: Vec<TalentState> = file
            .talents_of_tab(skill_line)
            .into_iter()
            .map(|spec| TalentState {
                spec: spec.clone(),
                rank: 0,
            })
            .collect();
        let tiers = talents.iter().map(|t| t.spec.tier + 1).max().unwrap_or(0);
        TalentTree {
            tab,
            points_per_tier: file.points_per_tier,
            talents,
            tier_points: vec![0; tiers as usize],
            total: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.tab.name
    }

    pub fn skill_line(&self) -> u32 {
        self.tab.skill_line
    }

    pub fn points_per_tier(&self) -> u32 {
        self.points_per_tier
    }

    /// The talents, by tier then column.
    pub fn talents(&self) -> &[TalentState] {
        &self.talents
    }

    pub fn talent(&self, node: u32) -> Option<&TalentState> {
        self.talents.iter().find(|t| t.spec.node == node)
    }

    fn talent_mut(&mut self, node: u32) -> Option<&mut TalentState> {
        self.talents.iter_mut().find(|t| t.spec.node == node)
    }

    pub fn contains(&self, node: u32) -> bool {
        self.talent(node).is_some()
    }

    /// The node of the talent named `name`. Port of `get_position_from_talent_name`.
    pub fn node_of_name(&self, name: &str) -> Option<u32> {
        self.talents
            .iter()
            .find(|t| t.spec.name == name)
            .map(|t| t.spec.node)
    }

    /// The node whose talent spell is `spell`.
    pub fn node_of_spell(&self, spell: u32) -> Option<u32> {
        self.talents
            .iter()
            .find(|t| t.spec.spell == spell)
            .map(|t| t.spec.node)
    }

    /// Points spent in the tab. Port of `get_total_points`.
    pub fn total_points(&self) -> u32 {
        self.total
    }

    /// Points spent in each tier.
    pub fn tier_points(&self) -> &[u32] {
        &self.tier_points
    }

    pub fn rank(&self, node: u32) -> u32 {
        self.talent(node).map_or(0, TalentState::rank)
    }

    pub fn max_rank(&self, node: u32) -> u32 {
        self.talent(node).map_or(0, TalentState::max_rank)
    }

    pub fn is_active(&self, node: u32) -> bool {
        self.talent(node).is_some_and(TalentState::is_active)
    }

    pub fn is_maxed(&self, node: u32) -> bool {
        self.talent(node).is_some_and(TalentState::is_maxed)
    }

    /// The prerequisite of `node`, if any. Port of `has_parent` / `get_parent`.
    pub fn parent(&self, node: u32) -> Option<u32> {
        self.talent(node)?.spec.requires
    }

    pub fn has_parent(&self, node: u32) -> bool {
        self.parent(node).is_some()
    }

    /// The talents that require `node`.
    pub fn children(&self, node: u32) -> Vec<u32> {
        self.talents
            .iter()
            .filter(|t| t.spec.requires == Some(node))
            .map(|t| t.spec.node)
            .collect()
    }

    /// Whether a talent requiring `node` has points. Port of `Talent::any_child_active`.
    pub fn child_is_active(&self, node: u32) -> bool {
        self.talents
            .iter()
            .any(|t| t.spec.requires == Some(node) && t.is_active())
    }

    /// Whether `tier` is unlocked: `points_per_tier × tier` points are spent in the tab.
    pub fn tier_is_unlocked(&self, tier: u32) -> bool {
        self.total >= self.points_per_tier * tier
    }

    /// Whether a point can go into `node`: its tier is unlocked and its prerequisite maxed.
    /// Port of `TalentTree::is_available` (the point budget is the owner's check).
    pub fn is_available(&self, node: u32) -> bool {
        let Some(talent) = self.talent(node) else {
            return false;
        };
        if talent
            .spec
            .requires
            .is_some_and(|parent| !self.is_maxed(parent))
        {
            return false;
        }
        self.tier_is_unlocked(talent.spec.tier)
    }

    /// Whether a child of `node` could take a point once `node` is maxed (its tier is
    /// unlocked). Port of `bottom_child_is_available` / `right_child_is_available`.
    pub fn child_is_available(&self, node: u32, child: u32) -> bool {
        self.is_maxed(node)
            && self.talent(child).is_some_and(|c| {
                c.spec.requires == Some(node) && self.tier_is_unlocked(c.spec.tier)
            })
    }

    /// Spends a point in `node`. Port of `TalentTree::increment_rank`.
    pub fn increment_rank(&mut self, node: u32) -> Option<RankChange> {
        if !self.is_available(node) {
            return None;
        }
        let talent = self.talent_mut(node)?;
        if talent.is_maxed() {
            return None;
        }
        talent.rank += 1;
        let change = RankChange {
            node,
            spell: talent.spec.spell,
            from: talent.rank - 1,
            to: talent.rank,
        };
        let tier = talent.spec.tier as usize;
        self.tier_points[tier] += 1;
        self.total += 1;
        Some(change)
    }

    /// Takes a point out of `node`: refused while a talent requiring it has points, and when
    /// a tier above would lose its unlock. Port of `TalentTree::decrement_rank`.
    pub fn decrement_rank(&mut self, node: u32) -> Option<RankChange> {
        let talent = self.talent(node)?;
        if !talent.is_active() || self.child_is_active(node) {
            return None;
        }
        let tier = talent.spec.tier as usize;
        // Every invested tier above must stay unlocked with one point less below it.
        let mut below = self.tier_points[..=tier].iter().sum::<u32>() - 1;
        for (t, &points) in self.tier_points.iter().enumerate().skip(tier + 1) {
            if points > 0 && below < self.points_per_tier * t as u32 {
                return None;
            }
            below += points;
        }
        let talent = self.talent_mut(node)?;
        talent.rank -= 1;
        let change = RankChange {
            node,
            spell: talent.spec.spell,
            from: talent.rank + 1,
            to: talent.rank,
        };
        self.tier_points[tier] -= 1;
        self.total -= 1;
        Some(change)
    }

    /// Takes every point out, regardless of the rules. Port of `TalentTree::clear_tree`.
    pub fn clear(&mut self) -> Vec<RankChange> {
        let mut changes = Vec::new();
        for talent in &mut self.talents {
            if talent.rank > 0 {
                changes.push(RankChange {
                    node: talent.spec.node,
                    spell: talent.spec.spell,
                    from: talent.rank,
                    to: 0,
                });
                talent.rank = 0;
            }
        }
        self.tier_points.iter_mut().for_each(|p| *p = 0);
        self.total = 0;
        changes
    }

    /// The active talents as `(node, rank)`, by tier then column. Port of
    /// `get_talent_tree_setup`.
    pub fn setup(&self) -> Vec<(u32, u32)> {
        self.talents
            .iter()
            .filter(|t| t.is_active())
            .map(|t| (t.spec.node, t.rank))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::talent::spec::tests::arms;

    const IMPROVED_REND: u32 = 105956;
    const DEEP_WOUNDS: u32 = 105950;
    const SWEEPING_STRIKES: u32 = 105945;
    const MORTAL_STRIKE: u32 = 105941;

    fn tree() -> TalentTree {
        TalentTree::new(&Arc::new(arms()), 26)
    }

    #[test]
    fn a_tab_lists_its_talents_by_position() {
        let tree = tree();
        assert_eq!(tree.name(), "Arms");
        assert_eq!(tree.skill_line(), 26);
        assert_eq!(tree.points_per_tier(), 5);
        assert_eq!(
            tree.talents()
                .iter()
                .map(TalentState::node)
                .collect::<Vec<_>>(),
            [IMPROVED_REND, DEEP_WOUNDS, SWEEPING_STRIKES, MORTAL_STRIKE]
        );
        assert_eq!(tree.tier_points().len(), 7);
        assert_eq!(tree.node_of_name("Deep Wounds"), Some(DEEP_WOUNDS));
        assert_eq!(tree.node_of_spell(12294), Some(MORTAL_STRIKE));
        assert!(tree.node_of_name("Cruelty").is_none(), "another tab");
        assert!(!tree.contains(105939));
        assert_eq!(tree.max_rank(DEEP_WOUNDS), 3);
        assert_eq!(tree.parent(DEEP_WOUNDS), Some(IMPROVED_REND));
        assert!(!tree.has_parent(IMPROVED_REND));
        assert_eq!(tree.children(IMPROVED_REND), [DEEP_WOUNDS]);
        let deep_wounds = tree.talent(DEEP_WOUNDS).unwrap();
        assert_eq!(deep_wounds.name(), "Deep Wounds");
        assert_eq!(deep_wounds.spell(), 12834);
        assert_eq!(deep_wounds.spec().tier, 2);
        assert!(deep_wounds.values().is_empty(), "rank 0");
    }

    #[test]
    #[should_panic(expected = "has no talent tab 999")]
    fn an_unknown_tab_panics() {
        TalentTree::new(&Arc::new(arms()), 999);
    }

    #[test]
    fn points_follow_tiers_and_prerequisites() {
        let mut tree = tree();
        assert!(tree.is_available(IMPROVED_REND));
        assert!(
            !tree.is_available(DEEP_WOUNDS),
            "tier 2 locked, parent not maxed"
        );
        assert!(tree.decrement_rank(IMPROVED_REND).is_none());
        assert_eq!(
            tree.increment_rank(IMPROVED_REND),
            Some(RankChange {
                node: IMPROVED_REND,
                spell: 12286,
                from: 0,
                to: 1
            })
        );
        assert!(tree.is_active(IMPROVED_REND));
        assert_eq!(tree.talent(IMPROVED_REND).unwrap().values(), [(0, 12.0)]);
        assert!(tree.increment_rank(IMPROVED_REND).is_some());
        assert!(tree.increment_rank(IMPROVED_REND).is_some());
        assert!(tree.is_maxed(IMPROVED_REND));
        assert!(tree.increment_rank(IMPROVED_REND).is_none(), "maxed");
        assert_eq!(tree.total_points(), 3);
        // The parent is maxed but tier 2 needs 10 points in the tab (the fixture tab has
        // nothing else in tiers 0 and 1, so Deep Wounds stays locked).
        assert!(!tree.is_available(DEEP_WOUNDS));
        assert!(!tree.child_is_available(IMPROVED_REND, DEEP_WOUNDS));
        assert!(tree.increment_rank(DEEP_WOUNDS).is_none());
        assert_eq!(tree.tier_points(), [3, 0, 0, 0, 0, 0, 0]);
        assert_eq!(tree.setup(), [(IMPROVED_REND, 3)]);
    }

    /// A tree whose tiers can be unlocked: the Fury-less fixture is padded with filler talents.
    fn full_tree() -> TalentTree {
        let mut file = arms();
        for tier in 0..7u32 {
            for column in 0..4u32 {
                if file
                    .talents
                    .iter()
                    .any(|t| t.tab == 26 && t.tier == tier && t.column == column)
                {
                    continue;
                }
                let node = 900_000 + tier * 10 + column;
                file.talents.push(TalentSpec {
                    node,
                    spell: 800_000 + node,
                    name: format!("Filler {tier}.{column}"),
                    tab: 26,
                    tier,
                    column,
                    max_ranks: 5,
                    requires: None,
                    rank_values: Default::default(),
                });
            }
        }
        file.validate().unwrap();
        TalentTree::new(&Arc::new(file), 26)
    }

    fn fill(tree: &mut TalentTree, node: u32, points: u32) {
        for _ in 0..points {
            assert!(tree.increment_rank(node).is_some(), "node {node}");
        }
    }

    #[test]
    fn decrementing_respects_children_and_the_tiers_above() {
        let mut tree = full_tree();
        fill(&mut tree, IMPROVED_REND, 3);
        fill(&mut tree, 900_000, 2); // tier 0 → 5 points
        assert!(tree.tier_is_unlocked(1));
        fill(&mut tree, 900_010, 5); // tier 1 → 10 points
        assert!(tree.is_available(DEEP_WOUNDS));
        assert!(tree.child_is_available(IMPROVED_REND, DEEP_WOUNDS));
        fill(&mut tree, DEEP_WOUNDS, 1);
        // Improved Rend is Deep Wounds' parent: locked while the child has points.
        assert!(tree.child_is_active(IMPROVED_REND));
        assert!(tree.decrement_rank(IMPROVED_REND).is_none());
        // Tier 0 has exactly 5 points and tier 1 is invested: nothing comes out of tier 0.
        assert!(tree.decrement_rank(900_000).is_none());
        // Tier 1 has 5 points and tier 2 is invested: locked too.
        assert!(tree.decrement_rank(900_010).is_none());
        // One more point in tier 0 frees one point below.
        fill(&mut tree, 900_001, 1);
        assert!(tree.decrement_rank(900_000).is_some());
        assert!(tree.decrement_rank(900_000).is_none());
        // Deep Wounds itself can come out (nothing requires it).
        assert_eq!(
            tree.decrement_rank(DEEP_WOUNDS).map(|c| (c.from, c.to)),
            Some((1, 0))
        );
        assert!(
            tree.decrement_rank(IMPROVED_REND).is_none(),
            "tier 0 is at 5 with tier 1 invested"
        );
        assert!(
            tree.decrement_rank(900_010).is_some(),
            "nothing above tier 1"
        );
        assert_eq!(tree.total_points(), 9);
        assert_eq!(tree.tier_points(), [5, 4, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn clearing_refunds_everything() {
        let mut tree = full_tree();
        fill(&mut tree, IMPROVED_REND, 3);
        fill(&mut tree, 900_000, 2);
        fill(&mut tree, 900_010, 5);
        fill(&mut tree, DEEP_WOUNDS, 3);
        let changes = tree.clear();
        assert_eq!(changes.len(), 4);
        assert!(changes.iter().all(|c| c.to == 0));
        assert_eq!(
            changes
                .iter()
                .find(|c| c.node == DEEP_WOUNDS)
                .map(|c| c.from),
            Some(3)
        );
        assert_eq!(tree.total_points(), 0);
        assert!(tree.setup().is_empty());
        assert!(tree.tier_points().iter().all(|&p| p == 0));
        assert!(tree.clear().is_empty());
    }

    #[test]
    fn the_last_tier_needs_thirty_points_and_its_parent() {
        let mut tree = full_tree();
        for tier in 0..6u32 {
            let mut points = 0;
            for column in 0..4u32 {
                let node = 900_000 + tier * 10 + column;
                if tree.contains(node) {
                    let take = (5 - points).min(5);
                    fill(&mut tree, node, take);
                    points += take;
                }
                if points == 5 {
                    break;
                }
            }
        }
        assert_eq!(tree.total_points(), 30);
        assert!(tree.tier_is_unlocked(6));
        assert!(
            !tree.is_available(MORTAL_STRIKE),
            "Sweeping Strikes not taken"
        );
        fill(&mut tree, SWEEPING_STRIKES, 1);
        assert!(tree.is_available(MORTAL_STRIKE));
        fill(&mut tree, MORTAL_STRIKE, 1);
        assert!(tree.decrement_rank(SWEEPING_STRIKES).is_none());
        assert!(tree.decrement_rank(900_000).is_none(), "tier 6 invested");
        assert!(tree.decrement_rank(MORTAL_STRIKE).is_some());
        assert!(tree.decrement_rank(SWEEPING_STRIKES).is_some());
        assert!(
            tree.decrement_rank(900_000).is_none(),
            "tier 1 still invested"
        );
        assert!(
            tree.decrement_rank(900_050).is_some(),
            "nothing above tier 5"
        );
        assert_eq!(tree.total_points(), 29);
    }
}
