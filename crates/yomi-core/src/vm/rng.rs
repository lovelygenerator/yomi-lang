//! Random numbers. xoshiro256++ (`rand_xoshiro`). The state is 4 `u64`s, and it returns the same sequence on every
//! platform.
//!
//! The OS random source is not used. It is made from a seed (`VmConfig::seed`), and the state is included in saves.

use rand_core::{Rng as _, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

/// xoshiro256++. A random generator with serializable state, used by `random` in scripts and by package commands
/// ([`Ctx::rng`](crate::Ctx::rng)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    inner: Xoshiro256PlusPlus,
}

impl Rng {
    /// Makes one from a seed (the state is made with splitmix64). The same seed gives the same sequence.
    pub(crate) fn from_seed(seed: u64) -> Self {
        Self {
            inner: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }

    /// Makes one from a saved state. An all-zero state is rejected, because its sequence would be all zeros.
    pub(crate) fn from_state(s: [u64; 4]) -> Option<Self> {
        if s == [0; 4] {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, word) in s.iter().enumerate() {
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&word.to_le_bytes());
        }
        Some(Self {
            inner: Xoshiro256PlusPlus::from_seed(bytes),
        })
    }

    /// The state (put into saves).
    pub(crate) fn state(&self) -> [u64; 4] {
        let bytes = self.inner.state();
        std::array::from_fn(|i| {
            u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().expect("8 バイトずつ"))
        })
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// A decimal in [0.0, 1.0) (uses the upper 53 bits; the same state gives the same value on every platform).
    pub fn unit_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// An integer in `min..=max` (both ends included). If `min > max`, returns `min` (the generator does not advance).
    ///
    /// Maps into the range with Lemire's method (multiplication with rejection) to avoid bias. On every platform, the same
    /// state gives the same value and consumes the same number of outputs.
    pub fn range_inclusive(&mut self, min: i64, max: i64) -> i64 {
        if min > max {
            return min;
        }
        // The size of the range (when it cannot be represented in `u64`, it is 2^64, which becomes 0)
        let span = (max as u64).wrapping_sub(min as u64).wrapping_add(1);
        if span == 0 {
            return self.next_u64() as i64;
        }
        let mut m = u128::from(self.next_u64()) * u128::from(span);
        let mut low = m as u64;
        if low < span {
            let threshold = span.wrapping_neg() % span;
            while low < threshold {
                m = u128::from(self.next_u64()) * u128::from(span);
                low = m as u64;
            }
        }
        min.wrapping_add((m >> 64) as u64 as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_returns_same_sequence_and_different_seed_different_one() {
        let mut a = Rng::from_seed(1);
        let mut b = Rng::from_seed(1);
        let mut c = Rng::from_seed(2);
        let xs: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let zs: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
    }

    #[test]
    fn state_is_never_zero_even_with_seed_0() {
        let r = Rng::from_seed(0);
        assert_ne!(r.state(), [0; 4]);
        assert!(Rng::from_state([0; 4]).is_none());
    }

    #[test]
    fn rebuilding_from_state_gives_same_continuing_sequence() {
        let mut a = Rng::from_seed(5);
        a.next_u64();
        let mut b = Rng::from_state(a.state()).expect("0 でない状態");
        let xs: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_eq!(xs, ys);
    }

    #[test]
    fn range_random_includes_both_ends_and_stays_in_range() {
        let mut r = Rng::from_seed(7);
        let mut seen = [false; 6];
        for _ in 0..2_000 {
            let v = r.range_inclusive(1, 6);
            assert!((1..=6).contains(&v));
            seen[(v - 1) as usize] = true;
        }
        assert!(
            seen.iter().all(|&s| s),
            "両端を含む 1 から 6 のすべてが出る"
        );
    }

    #[test]
    fn single_value_range_returns_that_value() {
        let mut r = Rng::from_seed(3);
        for _ in 0..10 {
            assert_eq!(r.range_inclusive(5, 5), 5);
            assert_eq!(r.range_inclusive(i64::MIN, i64::MIN), i64::MIN);
            assert_eq!(r.range_inclusive(i64::MAX, i64::MAX), i64::MAX);
        }
    }

    #[test]
    fn works_over_full_and_negative_integer_ranges() {
        let mut r = Rng::from_seed(11);
        for _ in 0..200 {
            let _ = r.range_inclusive(i64::MIN, i64::MAX);
            let v = r.range_inclusive(-10, -3);
            assert!((-10..=-3).contains(&v));
            let w = r.range_inclusive(i64::MIN, i64::MIN + 2);
            assert!((i64::MIN..=i64::MIN + 2).contains(&w));
        }
    }

    #[test]
    fn sequence_from_fixed_seed_does_not_change() {
        // The sequence must be the same across platforms and versions (fixed for each format version).
        let mut r = Rng::from_seed(0);
        let first: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        let mut again = Rng::from_seed(0);
        let second: Vec<u64> = (0..3).map(|_| again.next_u64()).collect();
        assert_eq!(first, second);
        assert_eq!(
            first,
            vec![FIXED[0], FIXED[1], FIXED[2]],
            "実測値: {first:x?}"
        );
    }

    /// The first 3 outputs of `Rng::from_seed(0)` (matching the reference implementation; if the implementation changes,
    /// increase the format version).
    const FIXED: [u64; 3] = [
        0x5317_5d61_490b_23df,
        0x61da_6f3d_c380_d507,
        0x5c0f_df91_ec9a_7bfc,
    ];
}
