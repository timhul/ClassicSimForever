//! The character's talents: three setups of one [`TalentTree`] per tab, a point budget per
//! setup and the current setup. Port of `Talent/CharacterTalents.*`.
//!
//! Like the trees this is a pure value: every mutation returns the [`RankChange`]s it caused,
//! and the character context applies them to the spells
//! ([`CharacterContext::apply_talent_changes`](crate::character::context::CharacterContext)).
//! Talents are addressed by node id, which is unique across the tree.

use std::sync::Arc;

use crate::talent::spec::{TalentFile, TalentSpec};
use crate::talent::tree::{RankChange, TalentState, TalentTree};

/// Talent setups a character keeps (the C++ had three).
pub const SETUP_COUNT: usize = 3;

/// One setup: a tree per tab and the points left.
#[derive(Debug, Clone, PartialEq)]
struct Setup {
    trees: Vec<TalentTree>,
    points_remaining: u32,
}

/// The talents of one character. Port of `CharacterTalents`.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterTalents {
    file: Arc<TalentFile>,
    setups: Vec<Setup>,
    current: usize,
}

impl CharacterTalents {
    /// [`SETUP_COUNT`] empty setups of `file`'s tree.
    pub fn new(file: Arc<TalentFile>) -> Self {
        let setup = Setup {
            trees: file
                .tabs
                .iter()
                .map(|tab| TalentTree::new(&file, tab.skill_line))
                .collect(),
            points_remaining: file.points,
        };
        CharacterTalents {
            setups: vec![setup; SETUP_COUNT],
            file,
            current: 0,
        }
    }

    pub fn file(&self) -> &Arc<TalentFile> {
        &self.file
    }

    /// Talent points per setup.
    pub fn points(&self) -> u32 {
        self.file.points
    }

    // ---------------------------------------------------------------- lookups

    /// The trees of the current setup, in tab order.
    pub fn trees(&self) -> &[TalentTree] {
        &self.setups[self.current].trees
    }

    fn trees_mut(&mut self) -> &mut [TalentTree] {
        &mut self.setups[self.current].trees
    }

    /// The tree of tab `skill_line` in the current setup.
    pub fn tree(&self, skill_line: u32) -> Option<&TalentTree> {
        self.trees().iter().find(|t| t.skill_line() == skill_line)
    }

    /// The tree holding `node` in the current setup.
    pub fn tree_of(&self, node: u32) -> Option<&TalentTree> {
        self.trees().iter().find(|t| t.contains(node))
    }

    fn tree_of_mut(&mut self, node: u32) -> Option<&mut TalentTree> {
        self.trees_mut().iter_mut().find(|t| t.contains(node))
    }

    /// The talent at `node` in the current setup.
    pub fn talent(&self, node: u32) -> Option<&TalentState> {
        self.tree_of(node)?.talent(node)
    }

    /// The talent definition at `node`.
    pub fn spec(&self, node: u32) -> Option<&TalentSpec> {
        self.file.talent(node)
    }

    /// The node of the talent named `name` (in tab `tab` when given). Port of
    /// `get_talent_position`.
    pub fn node_of_name(&self, name: &str, tab: Option<u32>) -> Option<u32> {
        self.file.talent_by_name(name, tab).map(|t| t.node)
    }

    /// The node whose talent spell is `spell`.
    pub fn node_of_spell(&self, spell: u32) -> Option<u32> {
        self.file.talent_by_spell(spell).map(|t| t.node)
    }

    /// Whether `spell` is a talent spell of the tree (granted by a talent, not the trainer).
    pub fn grants(&self, spell: u32) -> bool {
        self.node_of_spell(spell).is_some()
    }

    pub fn rank(&self, node: u32) -> u32 {
        self.tree_of(node).map_or(0, |t| t.rank(node))
    }

    pub fn max_rank(&self, node: u32) -> u32 {
        self.tree_of(node).map_or(0, |t| t.max_rank(node))
    }

    /// The rank of the talent whose spell is `spell` (0 when not a talent or not taken).
    pub fn rank_of_spell(&self, spell: u32) -> u32 {
        self.node_of_spell(spell).map_or(0, |node| self.rank(node))
    }

    pub fn is_active(&self, node: u32) -> bool {
        self.tree_of(node).is_some_and(|t| t.is_active(node))
    }

    pub fn is_maxed(&self, node: u32) -> bool {
        self.tree_of(node).is_some_and(|t| t.is_maxed(node))
    }

    /// Whether a point can go into `node`: the tree allows it and points remain (an active
    /// talent stays "available" for display when none remain). Port of
    /// `CharacterTalents::is_available`.
    pub fn is_available(&self, node: u32) -> bool {
        let Some(tree) = self.tree_of(node) else {
            return false;
        };
        if self.points_remaining() == 0 && !tree.is_active(node) {
            return false;
        }
        tree.is_available(node)
    }

    pub fn has_points_remaining(&self) -> bool {
        self.points_remaining() > 0
    }

    /// Points left in the current setup.
    pub fn points_remaining(&self) -> u32 {
        self.setups[self.current].points_remaining
    }

    /// Points spent in tab `skill_line` of the current setup. Port of `get_tree_points`.
    pub fn tab_points(&self, skill_line: u32) -> u32 {
        self.tree(skill_line).map_or(0, TalentTree::total_points)
    }

    /// Points spent in the current setup.
    pub fn spent_points(&self) -> u32 {
        self.trees().iter().map(TalentTree::total_points).sum()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    /// The active talents of the current setup as `(node, rank)`, tab by tab. Port of
    /// `get_current_talent_setup`.
    pub fn setup(&self) -> Vec<(u32, u32)> {
        self.trees().iter().flat_map(TalentTree::setup).collect()
    }

    /// The spells of the current setup's active talents.
    pub fn active_spells(&self) -> Vec<u32> {
        self.trees()
            .iter()
            .flat_map(|t| t.talents().iter().filter(|t| t.is_active()))
            .map(TalentState::spell)
            .collect()
    }

    // ---------------------------------------------------------------- mutations

    /// Spends a point in `node`. Port of `CharacterTalents::increment_rank`.
    pub fn increment_rank(&mut self, node: u32) -> Option<RankChange> {
        if self.points_remaining() == 0 {
            return None;
        }
        let change = self.tree_of_mut(node)?.increment_rank(node)?;
        self.setups[self.current].points_remaining -= 1;
        Some(change)
    }

    /// Takes a point out of `node`. Port of `CharacterTalents::decrement_rank`.
    pub fn decrement_rank(&mut self, node: u32) -> Option<RankChange> {
        let change = self.tree_of_mut(node)?.decrement_rank(node)?;
        self.setups[self.current].points_remaining += 1;
        Some(change)
    }

    /// Spends points in `node` until it is maxed or none remain. Port of
    /// `increase_to_max_rank`.
    pub fn increase_to_max_rank(&mut self, node: u32) -> Vec<RankChange> {
        let mut changes = Vec::new();
        while let Some(change) = self.increment_rank(node) {
            changes.push(change);
        }
        changes
    }

    /// Takes points out of `node` while the rules allow. Port of `decrease_to_min_rank`.
    pub fn decrease_to_min_rank(&mut self, node: u32) -> Vec<RankChange> {
        let mut changes = Vec::new();
        while let Some(change) = self.decrement_rank(node) {
            changes.push(change);
        }
        changes
    }

    /// Refunds every point of tab `skill_line`. Port of `CharacterTalents::clear_tree`.
    pub fn clear_tab(&mut self, skill_line: u32) -> Vec<RankChange> {
        let Some(tree) = self
            .trees_mut()
            .iter_mut()
            .find(|t| t.skill_line() == skill_line)
        else {
            return Vec::new();
        };
        let refunded = tree.total_points();
        let changes = tree.clear();
        self.setups[self.current].points_remaining += refunded;
        debug_assert!(self.setups[self.current].points_remaining <= self.file.points);
        changes
    }

    /// Refunds every point of the current setup.
    pub fn clear_all(&mut self) -> Vec<RankChange> {
        let lines: Vec<u32> = self.trees().iter().map(TalentTree::skill_line).collect();
        lines
            .into_iter()
            .flat_map(|line| self.clear_tab(line))
            .collect()
    }

    /// Switches to setup `index`: the changes that take the current setup's talents down to
    /// rank 0 followed by the ones that bring the new setup's up. A bad index changes
    /// nothing. Port of `CharacterTalents::set_current_index`.
    pub fn set_current_index(&mut self, index: usize) -> Vec<RankChange> {
        if index >= self.setups.len() || index == self.current {
            return Vec::new();
        }
        let mut changes: Vec<RankChange> = self
            .trees()
            .iter()
            .flat_map(TalentTree::talents)
            .filter(|t| t.is_active())
            .map(|t| RankChange {
                node: t.node(),
                spell: t.spell(),
                from: t.rank(),
                to: 0,
            })
            .collect();
        self.current = index;
        changes.extend(
            self.trees()
                .iter()
                .flat_map(TalentTree::talents)
                .filter(|t| t.is_active())
                .map(|t| RankChange {
                    node: t.node(),
                    spell: t.spell(),
                    from: 0,
                    to: t.rank(),
                }),
        );
        changes
    }

    /// The talent changes that bring every talent of the current setup from rank 0 to its
    /// rank (what a character learning the setup from scratch applies).
    pub fn changes_from_scratch(&self) -> Vec<RankChange> {
        self.trees()
            .iter()
            .flat_map(TalentTree::talents)
            .filter(|t| t.is_active())
            .map(|t| RankChange {
                node: t.node(),
                spell: t.spell(),
                from: 0,
                to: t.rank(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::talent::spec::tests::arms;

    const IMPROVED_REND: u32 = 105956;
    const DEEP_WOUNDS: u32 = 105950;
    const CRUELTY: u32 = 105939;

    fn talents() -> CharacterTalents {
        CharacterTalents::new(Arc::new(arms()))
    }

    #[test]
    fn setups_start_empty_with_the_full_budget() {
        let t = talents();
        assert_eq!(t.points(), 51);
        assert_eq!(t.points_remaining(), 51);
        assert!(t.has_points_remaining());
        assert_eq!(t.current_index(), 0);
        assert_eq!(t.trees().len(), 2);
        assert_eq!(t.tree(26).unwrap().name(), "Arms");
        assert!(t.tree(257).is_none());
        assert_eq!(t.tree_of(CRUELTY).unwrap().skill_line(), 256);
        assert_eq!(t.node_of_name("Cruelty", None), Some(CRUELTY));
        assert_eq!(t.node_of_name("Cruelty", Some(26)), None);
        assert_eq!(t.node_of_spell(12834), Some(DEEP_WOUNDS));
        assert!(t.grants(12294));
        assert!(!t.grants(78));
        assert_eq!(t.spec(DEEP_WOUNDS).unwrap().spell, 12834);
        assert_eq!(t.max_rank(CRUELTY), 5);
        assert!(t.setup().is_empty());
        assert!(t.active_spells().is_empty());
        assert!(t.changes_from_scratch().is_empty());
        assert!(t.talent(1).is_none());
    }

    #[test]
    fn points_are_spent_and_refunded_per_setup() {
        let mut t = talents();
        assert!(t.is_available(CRUELTY));
        assert_eq!(t.increase_to_max_rank(CRUELTY).len(), 5);
        assert_eq!(t.rank(CRUELTY), 5);
        assert_eq!(t.rank_of_spell(12320), 5);
        assert!(t.is_maxed(CRUELTY));
        assert_eq!(t.points_remaining(), 46);
        assert_eq!(t.tab_points(256), 5);
        assert_eq!(t.spent_points(), 5);
        assert_eq!(t.active_spells(), [12320]);
        assert!(t.increment_rank(CRUELTY).is_none());
        assert_eq!(t.increment_rank(IMPROVED_REND).map(|c| c.to), Some(1));
        assert_eq!(t.setup(), [(IMPROVED_REND, 1), (CRUELTY, 5)]);
        assert_eq!(
            t.decrement_rank(CRUELTY).map(|c| (c.from, c.to)),
            Some((5, 4))
        );
        assert_eq!(t.decrease_to_min_rank(CRUELTY).len(), 4);
        assert!(!t.is_active(CRUELTY));
        assert_eq!(t.points_remaining(), 50);
        assert!(t.decrement_rank(CRUELTY).is_none());
        assert!(t.decrement_rank(999).is_none());
        assert!(t.increment_rank(999).is_none());
        assert_eq!(t.clear_tab(256), []);
        assert_eq!(t.clear_tab(26).len(), 1);
        assert_eq!(t.points_remaining(), 51);
        assert_eq!(t.increase_to_max_rank(CRUELTY).len(), 5);
        assert_eq!(t.clear_all().len(), 1);
        assert_eq!(t.points_remaining(), 51);
    }

    #[test]
    fn the_budget_stops_further_points() {
        let mut file = arms();
        file.points = 3;
        let mut t = CharacterTalents::new(Arc::new(file));
        assert_eq!(t.increase_to_max_rank(CRUELTY).len(), 3);
        assert_eq!(t.points_remaining(), 0);
        assert!(!t.has_points_remaining());
        assert!(
            t.is_available(CRUELTY),
            "active talents stay shown as available"
        );
        assert!(!t.is_available(IMPROVED_REND));
        assert!(t.increment_rank(IMPROVED_REND).is_none());
        assert!(t.decrement_rank(CRUELTY).is_some());
        assert!(t.increment_rank(IMPROVED_REND).is_some());
    }

    #[test]
    fn switching_setups_removes_and_reapplies_ranks() {
        let mut t = talents();
        t.increase_to_max_rank(CRUELTY);
        t.increment_rank(IMPROVED_REND);
        let changes = t.set_current_index(1);
        assert_eq!(
            changes,
            [
                RankChange {
                    node: IMPROVED_REND,
                    spell: 12286,
                    from: 1,
                    to: 0
                },
                RankChange {
                    node: CRUELTY,
                    spell: 12320,
                    from: 5,
                    to: 0
                },
            ]
        );
        assert_eq!(t.current_index(), 1);
        assert_eq!(t.points_remaining(), 51);
        assert!(t.setup().is_empty());
        t.increment_rank(CRUELTY);
        t.increment_rank(CRUELTY);
        let changes = t.set_current_index(0);
        assert_eq!(
            changes,
            [
                RankChange {
                    node: CRUELTY,
                    spell: 12320,
                    from: 2,
                    to: 0
                },
                RankChange {
                    node: IMPROVED_REND,
                    spell: 12286,
                    from: 0,
                    to: 1
                },
                RankChange {
                    node: CRUELTY,
                    spell: 12320,
                    from: 0,
                    to: 5
                },
            ]
        );
        assert_eq!(t.points_remaining(), 45);
        assert_eq!(t.changes_from_scratch().len(), 2);
        assert!(t.set_current_index(0).is_empty(), "same setup");
        assert!(t.set_current_index(9).is_empty(), "bad index");
        assert_eq!(t.current_index(), 0);
    }
}
