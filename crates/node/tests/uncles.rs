//! Uncles: a block that loses the race is carried by the next block, its miner
//! is paid part of the subsidy and its proof of work counts for the chain.

use thecoin_core::block::{uncle_reward, MAX_UNCLES, MAX_UNCLE_DEPTH};
use thecoin_core::crypto::SecretKey;
use thecoin_core::emission::block_subsidy;
use thecoin_core::state::{account_key, read_typed, Account};
use thecoin_core::{Address, Block, Network};
use thecoin_node::chain::{Chain, ChainOptions, ProcessResult};
use thecoin_storage::DbRead;

fn addr(seed: u8) -> Address {
    Address::from_public_key(&SecretKey::from_bytes(&[seed; 32]).public_key())
}

struct Net {
    chain: Chain,
    ts: u64,
    _dir: tempfile::TempDir,
}

impl Net {
    fn new() -> Net {
        let dir = tempfile::tempdir().unwrap();
        let params = Network::Regtest.params();
        let chain = Chain::open(params, &dir.path().join("chain.redb"), ChainOptions::default()).unwrap();
        Net { chain, ts: params.genesis_timestamp, _dir: dir }
    }

    /// Builds and submits a block; returns it.
    fn mine(&mut self, miner: Address) -> Block {
        let mut b = self.chain.build_template(&miner, [0u8; 32], &[], &[]).unwrap();
        self.ts += 1;
        b.header.timestamp = self.ts;
        let r = self.chain.submit_block(b.clone(), true).unwrap();
        assert!(matches!(r, ProcessResult::NewTip { .. }), "{r:?}");
        b
    }

    /// A competing block for the same parent, mined by someone else.
    fn competitor(&mut self, parent: &Block, miner: Address) -> Block {
        let mut b = parent.clone();
        b.header.miner = miner;
        b.header.timestamp = parent.header.timestamp + 1;
        b.header.uncles_root = thecoin_core::block::uncles_root(&[]);
        b.uncles.clear();
        b
    }

    fn balance(&self, a: &Address) -> u64 {
        let r = self.chain.read().unwrap();
        read_typed::<Account, _>(&r, &account_key(a)).unwrap().unwrap_or_default().balance
    }

    fn chainwork(&self, hash: &thecoin_core::Hash32) -> thecoin_core::U256 {
        let r = self.chain.read().unwrap();
        let rec = r.header(hash).unwrap().unwrap();
        thecoin_core::pow::u256_from_bytes(&rec.chainwork)
    }
}

#[test]
fn losing_block_becomes_an_uncle_and_is_paid() {
    let (main, rival) = (addr(1), addr(2));
    let mut net = Net::new();
    let first = net.mine(main);

    // The rival found a block for the same parent, a moment later: it loses.
    let losing = net.competitor(&first, rival);
    let losing_hash = losing.hash();
    assert!(matches!(net.chain.submit_block(losing, true).unwrap(), ProcessResult::SideChain));

    // The next block carries it as an uncle.
    let next = net.mine(main);
    assert_eq!(next.uncles.len(), 1, "the losing block should be an uncle");
    assert_eq!(next.uncles[0].hash(), losing_hash);
    assert_eq!(next.header.uncles_root, thecoin_core::block::uncles_root(&next.uncles));

    // Its work counts for the chain: more than parent + this block alone.
    let solo = thecoin_core::pow::work_for_target(&next.header.target_u256());
    let added = net.chainwork(&next.hash()) - net.chainwork(&first.hash());
    assert!(added > solo, "uncle work must count: {added} vs {solo}");

    // Rewards appear after the cooldown; the uncle gets (7-1)/24 of the subsidy.
    let subsidy = block_subsidy(Network::Regtest.params(), next.header.height);
    let expected = uncle_reward(subsidy, 1);
    assert!(expected > 0);
    for _ in 0..20 {
        net.mine(main);
    }
    assert_eq!(net.balance(&rival), expected, "uncle miner paid");

    // The same uncle cannot be included twice.
    let again = net.chain.build_template(&main, [0u8; 32], &[], &[]).unwrap();
    assert!(again.uncles.iter().all(|u| u.hash() != losing_hash));
}

#[test]
fn uncle_rules_are_enforced() {
    let (main, rival) = (addr(3), addr(4));
    let mut net = Net::new();
    let first = net.mine(main);
    let losing = net.competitor(&first, rival);

    assert!(matches!(net.chain.submit_block(losing.clone(), true).unwrap(), ProcessResult::SideChain));

    // The template already carries the uncle; hiding it from the header is refused.
    let good = net.chain.build_template(&main, [0u8; 32], &[], &[]).unwrap();
    assert_eq!(good.uncles.len(), 1);
    let mut hidden = good.clone();
    hidden.header.timestamp = net.ts + 1;
    hidden.header.uncles_root = thecoin_core::block::uncles_root(&[]);
    let r = net.chain.submit_block(hidden, true).unwrap();
    assert!(matches!(r, ProcessResult::Invalid(thecoin_core::BlockError::BadUncle(_))), "{r:?}");

    // Too many uncles.
    let mut many = net.chain.build_template(&main, [0u8; 32], &[], &[]).unwrap();
    many.header.timestamp = net.ts + 2;
    many.uncles = (0..=MAX_UNCLES).map(|i| net.competitor(&first, addr(10 + i as u8)).header).collect();
    many.header.uncles_root = thecoin_core::block::uncles_root(&many.uncles);
    let r = net.chain.submit_block(many, true).unwrap();
    assert!(matches!(r, ProcessResult::Invalid(thecoin_core::BlockError::BadUncle(_))), "{r:?}");

    // Too old: mine past the depth limit, then try to include the old loser.
    for _ in 0..(MAX_UNCLE_DEPTH + 1) {
        net.mine(main);
    }
    let mut old = net.chain.build_template(&main, [0u8; 32], &[], &[]).unwrap();
    old.header.timestamp = net.ts + 3;
    old.uncles = vec![losing.header.clone()];
    old.header.uncles_root = thecoin_core::block::uncles_root(&old.uncles);
    let r = net.chain.submit_block(old, true).unwrap();
    assert!(matches!(r, ProcessResult::Invalid(thecoin_core::BlockError::BadUncle(_))), "{r:?}");
}
