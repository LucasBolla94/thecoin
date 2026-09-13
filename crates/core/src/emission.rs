//! Emission schedule ("Bitcoin-style" halvings).
//!
//! * Block time: 60 s  (≈ 525 960 blocks per year)
//! * Initial reward: 40 TCN per block
//! * Halving every 625 000 blocks (≈ 434 days ≈ 1.19 years)
//! * Total: 40 × 625 000 × (1 + 1/2 + 1/4 + ...) < 50 000 000 TCN
//!
//! The first four eras (≈ 4.76 years — the *stabilization cycle*) emit 93.75%
//! of the supply. Rewards then keep halving until they reach zero; after that
//! miners are paid only by fees. The genesis block has no reward.

use crate::params::ChainParams;

/// Reward (in motes) created by the block at `height`.
pub fn block_subsidy(p: &ChainParams, height: u64) -> u64 {
    if height == 0 {
        return 0;
    }
    let halvings = (height - 1) / p.halving_interval;
    if halvings >= 64 {
        0
    } else {
        p.initial_reward >> halvings
    }
}

/// Era number (0-based) of `height`.
pub fn era(p: &ChainParams, height: u64) -> u64 {
    if height == 0 {
        0
    } else {
        (height - 1) / p.halving_interval
    }
}

/// Total motes created by blocks `0..=height`.
pub fn cumulative_emission(p: &ChainParams, height: u64) -> u64 {
    let mut total: u64 = 0;
    let mut remaining = height; // heights 1..=height
    let mut reward = p.initial_reward;
    while remaining > 0 && reward > 0 {
        let blocks = remaining.min(p.halving_interval);
        total += blocks * reward;
        remaining -= blocks;
        reward >>= 1;
    }
    total
}

/// Total motes that will ever be created.
pub fn total_emission(p: &ChainParams) -> u64 {
    cumulative_emission(p, u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amount::COIN;
    use crate::params::{MAINNET, MAX_SUPPLY, REGTEST};

    #[test]
    fn supply_is_capped_at_50m() {
        let total = total_emission(&MAINNET);
        assert!(total <= MAX_SUPPLY);
        // Integer halving truncation keeps us just below the cap.
        assert!(MAX_SUPPLY - total < COIN, "total = {total}");
        assert!(total_emission(&REGTEST) <= MAX_SUPPLY);
    }

    #[test]
    fn stabilization_cycle() {
        let four_eras = 4 * MAINNET.halving_interval;
        let emitted = cumulative_emission(&MAINNET, four_eras);
        assert_eq!(emitted, MAX_SUPPLY / 16 * 15); // 93.75%
        let years = four_eras as f64 * 60.0 / (365.25 * 86400.0);
        assert!(years > 4.5 && years < 5.0, "{years}");
    }

    #[test]
    fn subsidy_matches_cumulative() {
        let p = &REGTEST;
        let mut sum = 0;
        for h in 0..=(p.halving_interval * 40) {
            sum += block_subsidy(p, h);
            if h % 97 == 0 {
                assert_eq!(sum, cumulative_emission(p, h));
            }
        }
        assert_eq!(block_subsidy(&MAINNET, 1), 40 * COIN);
        assert_eq!(block_subsidy(&MAINNET, 625_000), 40 * COIN);
        assert_eq!(block_subsidy(&MAINNET, 625_001), 20 * COIN);
    }
}
