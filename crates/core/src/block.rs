//! Blocks and block headers.

use crate::address::Address;
use crate::hash::{tagged_hash, tags, Hash32};
use crate::merkle::merkle_root;
use crate::pow::{u256_from_bytes, PowHasher};
use crate::tx::Transaction;
use crate::U256;
use borsh::{BorshDeserialize, BorshSerialize};

/// Fixed-size block header (180 bytes when Borsh-encoded).
///
/// Field order and types are consensus-critical.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BlockHeader {
    /// Header format version (currently 1).
    pub version: u32,
    /// Height of this block (genesis = 0).
    pub height: u64,
    /// Hash of the parent block header.
    pub prev_hash: Hash32,
    /// Merkle root of the transaction ids of this block.
    pub tx_root: Hash32,
    /// State commitment after applying this block (see [`crate::lthash`]).
    pub state_root: Hash32,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    /// PoW target as a big-endian 256-bit integer. `CoinHash(header) <= target`.
    pub target: [u8; 32],
    /// Free field for miners to search the PoW.
    pub nonce: u64,
    /// Receives the block reward and the fees (after maturity).
    pub miner: Address,
    /// Governance signalling bits: bit `i` set = miner supports the proposal
    /// assigned to bit `i`. Unassigned bits must be zero.
    pub signal: u32,
    /// Merkle root of the uncle headers carried by this block (see [`Block::uncles`]).
    pub uncles_root: Hash32,
}

impl BlockHeader {
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("header serialization cannot fail")
    }

    /// Block id: `tagged_hash("block", borsh(header))`. Cheap to compute; the
    /// expensive PoW hash is separate.
    pub fn hash(&self) -> Hash32 {
        tagged_hash(tags::BLOCK, &[&self.to_bytes()])
    }

    pub fn target_u256(&self) -> U256 {
        u256_from_bytes(&self.target)
    }

    /// Computes CoinHash (RandomX with this height's epoch key) and checks it
    /// against the header target.
    pub fn check_pow(&self, hasher: &mut PowHasher) -> bool {
        let h = hasher.hash(self.height, &self.to_bytes());
        crate::pow::meets_target(&h, &self.target_u256())
    }
}

/// Body of a block as stored and relayed: transactions plus uncle headers.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BlockBody {
    pub txs: Vec<Transaction>,
    pub uncles: Vec<BlockHeader>,
}

/// Merkle root of uncle headers (the root of an empty list when there are none).
pub fn uncles_root(uncles: &[BlockHeader]) -> Hash32 {
    let ids: Vec<Hash32> = uncles.iter().map(|u| u.hash()).collect();
    merkle_root(&ids)
}

/// Most uncles a block may carry.
pub const MAX_UNCLES: usize = 2;
/// An uncle may be at most this many blocks older than the block including it.
pub const MAX_UNCLE_DEPTH: u64 = 6;

/// Part of the subsidy paid to the miner of an uncle `depth` blocks old.
/// `(7 - depth) / 24` of the subsidy: 25 % at depth 1 down to 4 % at depth 6.
pub fn uncle_reward(subsidy: u64, depth: u64) -> u64 {
    if depth == 0 || depth > MAX_UNCLE_DEPTH {
        return 0;
    }
    ((subsidy as u128 * (7 - depth) as u128) / 24) as u64
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub txs: Vec<Transaction>,
    /// Headers of recent valid blocks that lost the race ("uncles").
    ///
    /// Including them pays part of the subsidy to the miner who found them and
    /// adds their proof of work to this chain's weight, so a miner with a slow
    /// connection is not punished for losing a race by milliseconds — which is
    /// what would otherwise push mining towards a few big farms.
    pub uncles: Vec<BlockHeader>,
}

impl Block {
    pub fn hash(&self) -> Hash32 {
        self.header.hash()
    }

    pub fn compute_tx_root(&self) -> Hash32 {
        let ids: Vec<Hash32> = self.txs.iter().map(|t| t.txid()).collect();
        merkle_root(&ids)
    }

    pub fn compute_uncles_root(&self) -> Hash32 {
        uncles_root(&self.uncles)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("block serialization cannot fail")
    }

    pub fn serialized_size(&self) -> usize {
        self.to_bytes().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_size_is_fixed() {
        let h = BlockHeader {
            version: 1,
            height: u64::MAX,
            prev_hash: Hash32::ZERO,
            tx_root: Hash32::ZERO,
            state_root: Hash32::ZERO,
            timestamp: 0,
            target: [0xff; 32],
            nonce: 0,
            miner: Address::ZERO,
            signal: 0,
            uncles_root: Hash32::ZERO,
        };
        assert_eq!(h.to_bytes().len(), 212);
    }
}
