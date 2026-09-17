//! Difficulty adjustment: **LWMA-1** (linearly weighted moving average, by
//! Zawy), recalculated every block.
//!
//! LWMA reacts quickly to hashrate changes, which matters for a young network
//! of small machines where miners join and leave often, and it resists
//! timestamp manipulation (solve times are clamped to `[1, 6T]` and timestamps
//! must be above the median time past of the last 11 blocks).

use crate::params::ChainParams;
use crate::{U256, U512};

/// `(timestamp, target)` of an ancestor block.
#[derive(Clone, Copy, Debug)]
pub struct BlockTimeInfo {
    pub timestamp: u64,
    pub target: U256,
}

/// Number of ancestors (ending at the parent) that [`next_target`] needs.
pub fn required_ancestors(p: &ChainParams) -> usize {
    (p.lwma_window + 1) as usize
}

/// Target required for the block at `next_height`.
///
/// `ancestors` must contain the last blocks of the branch, oldest first, ending
/// with the parent (height `next_height - 1`). At most
/// [`required_ancestors`] entries are used.
pub fn next_target(p: &ChainParams, next_height: u64, ancestors: &[BlockTimeInfo]) -> U256 {
    if !p.retarget || ancestors.len() < 2 || next_height < 2 {
        return p.genesis_target();
    }
    // Warm-up: use the solve times available (at least LWMA_MIN_WINDOW) until
    // the full window exists, so a young network reacts after a few blocks
    // instead of waiting for `lwma_window` blocks at the genesis difficulty.
    let n = p.lwma_window.min(next_height - 1).min(ancestors.len() as u64 - 1);
    if n < crate::params::LWMA_MIN_WINDOW {
        return p.genesis_target();
    }
    let window = &ancestors[ancestors.len() - (n as usize + 1)..];
    let t = p.target_block_time;
    let k = n * (n + 1) * t / 2;

    let mut prev_ts = window[0].timestamp;
    let mut weighted: u64 = 0;
    let mut sum_targets = U512::zero();
    for (j, b) in window[1..].iter().enumerate() {
        let this_ts = b.timestamp.max(prev_ts + 1);
        let solve_time = (this_ts - prev_ts).min(6 * t);
        prev_ts = this_ts;
        weighted += solve_time * (j as u64 + 1);
        sum_targets += U512::from(b.target);
    }
    // next = (sum_targets / N) * weighted / k   (computed in 512-bit to avoid overflow)
    let mut next = sum_targets * U512::from(weighted) / U512::from(n * k);
    // Never more than 2× easier or harder than the previous block: smooths the
    // warm-up window and bounds the effect of manipulated timestamps.
    let prev = U512::from(window[n as usize].target);
    let (lo, hi) = (prev / U512::from(2u8), prev * U512::from(2u8));
    if next < lo {
        next = lo;
    }
    if next > hi {
        next = hi;
    }
    let limit = U512::from(p.pow_limit());
    let next = if next > limit { limit } else { next };
    let next = U256::try_from(next).expect("clamped below 2^256");
    if next.is_zero() {
        U256::one()
    } else {
        next
    }
}

/// Median of the timestamps of the last (up to) 11 blocks.
pub fn median_time_past(timestamps: &[u64]) -> u64 {
    let start = timestamps.len().saturating_sub(crate::params::MTP_WINDOW);
    let mut v: Vec<u64> = timestamps[start..].to_vec();
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MAINNET;

    fn chain(len: usize, solve: u64, target: U256) -> Vec<BlockTimeInfo> {
        (0..len).map(|i| BlockTimeInfo { timestamp: 1_000_000 + i as u64 * solve, target }).collect()
    }

    #[test]
    fn stable_when_on_target() {
        let p = &MAINNET;
        let target = p.genesis_target();
        let c = chain(61, p.target_block_time, target);
        let next = next_target(p, 1000, &c);
        assert_eq!(next, target);
    }

    #[test]
    fn harder_when_fast_easier_when_slow() {
        let p = &MAINNET;
        let target = p.genesis_target();
        let fast = next_target(p, 1000, &chain(61, p.target_block_time / 2, target));
        assert!(fast < target);
        assert_eq!(fast, target / 2);
        let slow = next_target(p, 1000, &chain(61, p.target_block_time * 3 / 2, target));
        assert!(slow > target);
    }

    #[test]
    fn warm_up_reacts_early_and_is_bounded() {
        let p = &MAINNET;
        let target = p.genesis_target();
        // 5 solve times: not enough, genesis target
        assert_eq!(next_target(p, 6, &chain(6, 2, target)), target);
        // 6 blocks far faster than the target: harder, but at most 2× per block
        let t7 = next_target(p, 7, &chain(7, 2, target));
        assert!(t7 < target);
        assert_eq!(t7, target / 2);
        // very slow blocks: easier, at most 2×
        let slow = next_target(p, 20, &chain(20, 10_000, target));
        assert_eq!(slow, target * 2);
    }

    #[test]
    fn clamped_to_pow_limit() {
        let p = &MAINNET;
        let next = next_target(p, 1000, &chain(61, 100_000, p.pow_limit()));
        assert_eq!(next, p.pow_limit());
    }

    #[test]
    fn mtp() {
        assert_eq!(median_time_past(&[5, 1, 3]), 3);
        assert_eq!(median_time_past(&(0..20).collect::<Vec<_>>()), 14);
    }
}
