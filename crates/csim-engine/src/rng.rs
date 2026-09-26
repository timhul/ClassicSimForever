//! Pseudo random number generation.
//!
//! Port of `CombatRoll/Random.*` and `CombatRoll/xorshift/xoroshiro128plus.*`.
//!
//! The original C++ generator by David Blackman and Sebastiano Vigna is public domain
//! (see <http://vigna.di.unimi.it/xorshift/>). The C++ port seeded the second state word from the
//! CPU tick counter, which made runs non-reproducible even with an explicit seed. The Rust port
//! derives the full state from the seed with SplitMix64 (the seeding procedure recommended by the
//! xoroshiro authors) so that a fixed seed always yields the same roll sequence.

use std::time::{SystemTime, UNIX_EPOCH};

/// Number of outputs discarded after seeding, matching the C++ warm-up loop.
const WARMUP_ROUNDS: usize = 100;

/// xoroshiro128+ generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xoroshiro128Plus {
    state: [u64; 2],
}

impl Xoroshiro128Plus {
    /// Creates a generator with its state fully derived from `seed`.
    pub fn from_seed(seed: u64) -> Self {
        let mut r#gen = Self { state: [0, 0] };
        r#gen.set_state(seed);
        r#gen
    }

    /// Creates a generator seeded from the system clock.
    pub fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::from_seed(nanos)
    }

    /// Re-seeds the generator. Both state words are derived from `seed` via SplitMix64 and the
    /// generator is warmed up by discarding the first outputs.
    pub fn set_state(&mut self, seed: u64) {
        let mut splitmix = seed;
        self.state[0] = splitmix64(&mut splitmix);
        self.state[1] = splitmix64(&mut splitmix);

        for _ in 0..WARMUP_ROUNDS {
            self.next();
        }
    }

    /// Returns the next 64-bit output.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        let s0 = self.state[0];
        let mut s1 = self.state[1];
        let result = s0.wrapping_add(s1);

        s1 ^= s0;
        self.state[0] = s0.rotate_left(24) ^ s1 ^ (s1 << 16);
        self.state[1] = s1.rotate_left(37);

        result
    }
}

/// SplitMix64 step, used only to expand a seed into generator state.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Uniform integer rolls in the half-open range `[min, max)`.
///
/// This keeps the semantics of the C++ `Random(min_range, max_range)`: the roll is
/// `next() % (max - min) + min`, so `max` itself is never produced. When `min == max` every roll
/// is `min`. Attack tables rely on this: `Random::new(0, 10000)` rolls `0..=9999`.
#[derive(Debug, Clone)]
pub struct Random {
    min: u64,
    modulo: u64,
    r#gen: Xoroshiro128Plus,
}

impl Random {
    /// Creates a generator for `[min, max)` seeded from the system clock.
    ///
    /// # Panics
    /// Panics if `min > max`.
    pub fn new(min: u32, max: u32) -> Self {
        Self::with_generator(min, max, Xoroshiro128Plus::from_entropy())
    }

    /// Creates a generator for `[min, max)` with a fixed seed.
    ///
    /// # Panics
    /// Panics if `min > max`.
    pub fn from_seed(min: u32, max: u32, seed: u64) -> Self {
        Self::with_generator(min, max, Xoroshiro128Plus::from_seed(seed))
    }

    fn with_generator(min: u32, max: u32, r#gen: Xoroshiro128Plus) -> Self {
        assert!(min <= max, "Random: min ({min}) must be <= max ({max})");
        Self {
            min: u64::from(min),
            modulo: u64::from(max - min),
            r#gen,
        }
    }

    /// Changes the roll range to `[min, max)`.
    ///
    /// # Panics
    /// Panics if `min > max`.
    pub fn set_new_range(&mut self, min: u32, max: u32) {
        assert!(min <= max, "Random: min ({min}) must be <= max ({max})");
        self.min = u64::from(min);
        self.modulo = u64::from(max - min);
    }

    /// Re-seeds the underlying generator.
    pub fn set_gen_from_seed(&mut self, seed: u64) {
        self.r#gen.set_state(seed);
    }

    /// Lower bound (inclusive) of the roll range.
    pub fn min(&self) -> u32 {
        self.min as u32
    }

    /// Upper bound (exclusive) of the roll range.
    pub fn max(&self) -> u32 {
        (self.min + self.modulo) as u32
    }

    /// Returns a roll in `[min, max)`.
    pub fn get_roll(&mut self) -> u32 {
        if self.modulo == 0 {
            return self.min as u32;
        }
        (self.r#gen.next() % self.modulo + self.min) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values computed independently (big-integer implementation of the xoroshiro128+
    /// reference algorithm) for the raw state `[1, 2]` (no seeding/warm-up involved).
    #[test]
    fn xoroshiro_matches_reference_sequence() {
        let mut r#gen = Xoroshiro128Plus { state: [1, 2] };
        assert_eq!(r#gen.next(), 3);
        assert_eq!(r#gen.next(), 412_333_834_243);
        assert_eq!(r#gen.next(), 2_360_170_716_294_286_339);
        assert_eq!(r#gen.next(), 9_295_852_285_959_843_169);
        assert_eq!(r#gen.next(), 2_797_080_929_874_688_578);
    }

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut a = Xoroshiro128Plus::from_seed(42);
        let mut b = Xoroshiro128Plus::from_seed(42);
        for _ in 0..1000 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        let mut a = Xoroshiro128Plus::from_seed(1);
        let mut b = Xoroshiro128Plus::from_seed(2);
        let differing = (0..100).filter(|_| a.next() != b.next()).count();
        assert!(differing > 90);
    }

    #[test]
    fn set_state_resets_sequence() {
        let mut r#gen = Xoroshiro128Plus::from_seed(7);
        let first: Vec<u64> = (0..10).map(|_| r#gen.next()).collect();
        r#gen.set_state(7);
        let second: Vec<u64> = (0..10).map(|_| r#gen.next()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn zero_seed_does_not_produce_zero_state() {
        let r#gen = Xoroshiro128Plus::from_seed(0);
        assert_ne!(r#gen.state, [0, 0]);
    }

    #[test]
    fn rolls_stay_within_half_open_range() {
        let mut random = Random::from_seed(5, 10, 123);
        let mut seen = [false; 10];
        for _ in 0..10_000 {
            let roll = random.get_roll();
            assert!((5..10).contains(&roll), "roll {roll} outside [5, 10)");
            seen[roll as usize] = true;
        }
        assert_eq!(
            seen,
            [
                false, false, false, false, false, true, true, true, true, true
            ]
        );
    }

    #[test]
    fn attack_table_range_never_reaches_upper_bound() {
        let mut random = Random::from_seed(0, 10_000, 9);
        for _ in 0..100_000 {
            assert!(random.get_roll() < 10_000);
        }
    }

    #[test]
    fn empty_range_always_returns_min() {
        let mut random = Random::from_seed(17, 17, 1);
        for _ in 0..100 {
            assert_eq!(random.get_roll(), 17);
        }
    }

    #[test]
    fn set_new_range_changes_bounds() {
        let mut random = Random::from_seed(0, 10_000, 3);
        random.set_new_range(7_500, 9_900);
        assert_eq!(random.min(), 7_500);
        assert_eq!(random.max(), 9_900);
        for _ in 0..10_000 {
            let roll = random.get_roll();
            assert!(
                (7_500..9_900).contains(&roll),
                "roll {roll} outside [7500, 9900)"
            );
        }
    }

    #[test]
    fn reseeding_reproduces_rolls() {
        let mut random = Random::from_seed(0, 10_000, 99);
        let first: Vec<u32> = (0..50).map(|_| random.get_roll()).collect();
        random.set_gen_from_seed(99);
        let second: Vec<u32> = (0..50).map(|_| random.get_roll()).collect();
        assert_eq!(first, second);
    }

    #[test]
    #[should_panic(expected = "min (5) must be <= max (4)")]
    fn inverted_range_panics() {
        let _ = Random::from_seed(5, 4, 0);
    }
}
