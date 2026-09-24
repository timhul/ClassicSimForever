//! Interim rating conversion.
//!
//! Items exported from the tables carry combat ratings (`HIT_RATING`, `CRIT_RATING`, ...) as the
//! tables store them. The engine does not have a rating-based stat system yet, so until it does,
//! ratings are turned into the chance stats the engine understands at fixed level-60 factors
//! (`data/ITEM_INSTRUCTIONS.md` §1.4). Everything that converts goes through this module so the
//! factors live in one place and can be removed together when ratings become first-class.
//!
//! Haste, expertise and armor penetration ratings have no interim factor: they are carried by
//! the data but have no effect yet.

use super::ItemStat;

/// The chance stat a rating converts to and how many rating points make 1 %, or `None` for a
/// stat that is not a convertible rating.
pub fn interim_chance(stat: ItemStat) -> Option<(ItemStat, f64)> {
    Some(match stat {
        ItemStat::HitRating => (ItemStat::HitChance, 10.0),
        ItemStat::CritRating => (ItemStat::CritChance, 14.0),
        ItemStat::DodgeRating => (ItemStat::DodgeChance, 12.0),
        ItemStat::ParryRating => (ItemStat::ParryChance, 15.0),
        ItemStat::BlockRating => (ItemStat::BlockChance, 5.0),
        _ => return None,
    })
}

/// A rating amount in the engine's internal chance units (`100` = 1 %), rounded.
pub fn to_chance_units(rating: f64, rating_per_percent: f64) -> u32 {
    (rating * 100.0 / rating_per_percent).round().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_60_factors() {
        assert_eq!(
            interim_chance(ItemStat::HitRating),
            Some((ItemStat::HitChance, 10.0))
        );
        assert_eq!(
            interim_chance(ItemStat::CritRating),
            Some((ItemStat::CritChance, 14.0))
        );
        assert_eq!(interim_chance(ItemStat::HasteRating), None);
        assert_eq!(interim_chance(ItemStat::Strength), None);
        // Lionheart Helm: 28 crit rating = 2 %, 20 hit rating = 2 %.
        assert_eq!(to_chance_units(28.0, 14.0), 200);
        assert_eq!(to_chance_units(20.0, 10.0), 200);
        // Fractions of a percent are kept: 7 crit rating = 0.5 %.
        assert_eq!(to_chance_units(7.0, 14.0), 50);
    }
}
