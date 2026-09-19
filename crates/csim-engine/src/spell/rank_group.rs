//! Rank groups. Port of `Spells/SpellRankGroup.*`.
//!
//! A group lists the ranks of one spell name in ascending rank order as [`SpellId`] handles into
//! the character's spell list. Rank lookups take a closure telling whether a rank is learned
//! (level / phase), since that needs the character.

use crate::ids::SpellId;
use crate::spell::MAX_RANK;

/// The ranks of one spell. Port of `SpellRankGroup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellRankGroup {
    name: String,
    /// `(rank, spell)` pairs sorted by rank.
    ranks: Vec<(u32, SpellId)>,
}

impl SpellRankGroup {
    /// # Panics
    /// Panics on an empty group, a duplicate rank or rank `MAX_RANK` (0).
    pub fn new(name: &str, ranks: impl IntoIterator<Item = (u32, SpellId)>) -> Self {
        let mut ranks: Vec<(u32, SpellId)> = ranks.into_iter().collect();
        assert!(
            !ranks.is_empty(),
            "Cannot create empty spell rank group {name}"
        );
        ranks.sort_by_key(|(rank, _)| *rank);
        for pair in ranks.windows(2) {
            assert!(
                pair[0].0 != pair[1].0,
                "{name} has rank {} twice",
                pair[0].0
            );
        }
        assert!(
            ranks[0].0 != MAX_RANK,
            "{name} uses rank {MAX_RANK}, which is reserved for MAX_RANK"
        );
        SpellRankGroup {
            name: name.to_string(),
            ranks,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every rank's spell, lowest rank first.
    pub fn spells(&self) -> impl Iterator<Item = SpellId> + '_ {
        self.ranks.iter().map(|(_, id)| *id)
    }

    /// The rank numbers, ascending.
    pub fn rank_numbers(&self) -> impl Iterator<Item = u32> + '_ {
        self.ranks.iter().map(|(rank, _)| *rank)
    }

    pub fn len(&self) -> usize {
        self.ranks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranks.is_empty()
    }

    /// The highest rank in the group regardless of whether it is learned.
    pub fn max_rank(&self) -> u32 {
        self.ranks.last().map_or(0, |(rank, _)| *rank)
    }

    /// The spell of `rank` if that rank exists and is learned, or the highest learned rank for
    /// `MAX_RANK`. Port of `SpellRankGroup::get_spell_rank` (looking ranks up by number rather
    /// than by position, so groups whose ranks do not start at 1 work).
    pub fn get_spell_rank(
        &self,
        rank: u32,
        is_learned: impl Fn(SpellId) -> bool,
    ) -> Option<SpellId> {
        if rank == MAX_RANK {
            return self.get_max_available_spell_rank(is_learned);
        }
        self.ranks
            .iter()
            .find(|(r, _)| *r == rank)
            .map(|(_, id)| *id)
            .filter(|id| is_learned(*id))
    }

    /// The highest learned rank: ranks are learned in order, so the search stops at the first
    /// unlearned one. Port of `SpellRankGroup::get_max_available_spell_rank`.
    pub fn get_max_available_spell_rank(
        &self,
        is_learned: impl Fn(SpellId) -> bool,
    ) -> Option<SpellId> {
        let mut available = None;
        for (_, id) in &self.ranks {
            if !is_learned(*id) {
                break;
            }
            available = Some(*id);
        }
        available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> SpellRankGroup {
        SpellRankGroup::new(
            "Battle Shout",
            [(7, SpellId(11)), (6, SpellId(10)), (5, SpellId(9))],
        )
    }

    #[test]
    fn ranks_are_sorted_and_looked_up_by_number() {
        let group = group();
        assert_eq!(group.name(), "Battle Shout");
        assert_eq!(group.len(), 3);
        assert_eq!(group.max_rank(), 7);
        assert_eq!(
            group.spells().collect::<Vec<_>>(),
            vec![SpellId(9), SpellId(10), SpellId(11)]
        );
        assert_eq!(group.rank_numbers().collect::<Vec<_>>(), vec![5, 6, 7]);

        let all = |_: SpellId| true;
        assert_eq!(group.get_spell_rank(6, all), Some(SpellId(10)));
        assert_eq!(group.get_spell_rank(8, all), None);
        assert_eq!(group.get_spell_rank(1, all), None);
        assert_eq!(group.get_spell_rank(MAX_RANK, all), Some(SpellId(11)));
    }

    #[test]
    fn unlearned_ranks_are_not_returned() {
        let group = group();
        let up_to_six = |id: SpellId| id.0 <= 10;
        assert_eq!(group.get_spell_rank(7, up_to_six), None);
        assert_eq!(group.get_spell_rank(6, up_to_six), Some(SpellId(10)));
        assert_eq!(
            group.get_max_available_spell_rank(up_to_six),
            Some(SpellId(10))
        );
        assert_eq!(group.get_spell_rank(MAX_RANK, up_to_six), Some(SpellId(10)));

        let none = |_: SpellId| false;
        assert_eq!(group.get_max_available_spell_rank(none), None);

        // Learning stops at the first unlearned rank even if a higher one would qualify.
        let gap = |id: SpellId| id != SpellId(10);
        assert_eq!(group.get_max_available_spell_rank(gap), Some(SpellId(9)));
    }

    #[test]
    #[should_panic(expected = "empty")]
    fn empty_groups_panic() {
        let _ = SpellRankGroup::new("x", []);
    }

    #[test]
    #[should_panic(expected = "twice")]
    fn duplicate_ranks_panic() {
        let _ = SpellRankGroup::new("x", [(1, SpellId(0)), (1, SpellId(1))]);
    }

    #[test]
    #[should_panic(expected = "reserved")]
    fn rank_zero_panics() {
        let _ = SpellRankGroup::new("x", [(0, SpellId(0))]);
    }
}
