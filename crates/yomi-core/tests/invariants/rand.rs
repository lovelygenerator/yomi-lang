//! Random numbers

use super::*;

pub(super) fn case_count() -> usize {
    rng::case_count_or(300)
}

pub(super) fn base_seed() -> u64 {
    rng::base_seed_or(0x5EED_1234)
}
