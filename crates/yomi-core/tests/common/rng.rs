//! The seeded random generator for invariant tests, and the environment variables for case counts and seeds.

/// xorshift64*. A hand-written random generator so as not to add an external crate.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
        if r.0 == 0 {
            r.0 = 0x1234_5678_9ABC_DEF1;
        }
        for _ in 0..4 {
            r.next();
        }
        r
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `0..n` (`n` is 1 or more).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() >> 11) as usize % n
    }

    /// `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as usize) as i64
    }

    /// Probability `num / den`.
    pub fn chance(&mut self, num: usize, den: usize) -> bool {
        self.below(den) < num
    }

    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// The number of cases per test (environment variable `YOMI_INVARIANT_CASES`; `default` if unset).
pub fn case_count_or(default: usize) -> usize {
    std::env::var("YOMI_INVARIANT_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The random seed (environment variable `YOMI_INVARIANT_SEED`; `default` if unset).
pub fn base_seed_or(default: u64) -> u64 {
    std::env::var("YOMI_INVARIANT_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
