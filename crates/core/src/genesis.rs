//! Deterministic genesis block and genesis state.
//!
//! There is **no premine**: the genesis state only contains the chain-global
//! record with the default governance parameters. Every TCN in existence is
//! created by proof of work.

use crate::address::Address;
use crate::block::{Block, BlockHeader};
use crate::hash::{tagged_hash, tags};
use crate::lthash::LtHash;
use crate::merkle::merkle_root;
use crate::params::{ChainParams, BLOCK_VERSION};
use crate::pow::u256_to_bytes;
use crate::state::{global_key, ChainGlobal};

/// Records present in the genesis state.
pub fn genesis_state(p: &ChainParams) -> Vec<(Vec<u8>, Vec<u8>)> {
    let g = ChainGlobal {
        emitted: 0,
        burned: 0,
        params: p.gov_defaults,
        voting: Vec::new(),
        pending_activations: Vec::new(),
        proposal_count: 0,
        contract_count: 0,
        congestion_bp: crate::params::CONGESTION_MIN_BP,
    };
    vec![(global_key(), borsh::to_vec(&g).expect("serializable"))]
}

pub fn genesis_lthash(p: &ChainParams) -> LtHash {
    let mut h = LtHash::default();
    for (k, v) in genesis_state(p) {
        h.insert(&k, &v);
    }
    h
}

pub fn genesis_block(p: &ChainParams) -> Block {
    let header = BlockHeader {
        version: BLOCK_VERSION,
        height: 0,
        prev_hash: tagged_hash(tags::GENESIS, &[p.genesis_message.as_bytes(), &p.chain_id.to_le_bytes()]),
        tx_root: merkle_root(&[]),
        state_root: genesis_lthash(p).root(),
        timestamp: p.genesis_timestamp,
        target: u256_to_bytes(&p.genesis_target()),
        nonce: 0,
        miner: Address::ZERO,
        signal: 0,
    };
    Block { header, txs: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Network;

    #[test]
    fn genesis_hashes_are_distinct_and_stable() {
        let m = genesis_block(Network::Mainnet.params()).hash();
        let t = genesis_block(Network::Testnet.params()).hash();
        let r = genesis_block(Network::Regtest.params()).hash();
        assert_ne!(m, t);
        assert_ne!(t, r);
        assert_eq!(m, genesis_block(Network::Mainnet.params()).hash());
        println!("mainnet genesis {m}\ntestnet genesis {t}\nregtest genesis {r}");
    }
}
