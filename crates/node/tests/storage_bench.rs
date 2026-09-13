//! Storage/processing measurements with the real database (regtest PoW).
//! `cargo test -p thecoin-node --release --test storage_bench -- --ignored --nocapture`

use std::time::Instant;
use thecoin_core::amount::COIN;
use thecoin_core::crypto::SecretKey;
use thecoin_core::{Address, Network};
use thecoin_node::chain::{Chain, ChainOptions, ProcessResult};
use thecoin_wallet::builder::{build_tx, transfer};

fn dir_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum()
}

#[test]
#[ignore]
fn bench_chain_growth() {
    let params = Network::Regtest.params();
    let dir = tempfile::tempdir().unwrap();
    let chain = Chain::open(params, &dir.path().join("chain.redb"), ChainOptions::default()).unwrap();
    let key = SecretKey::from_bytes(&[7; 32]);
    let miner = Address::from_public_key(&key.public_key());

    // Phase 1: empty blocks.
    let n_empty = 10_000u64;
    let t = Instant::now();
    for i in 0..n_empty {
        let mut b = chain.build_template(&miner, &[], &[]).unwrap();
        b.header.timestamp = params.genesis_timestamp + if i < 20 { i + 1 } else { 21 + (i - 20) / 4 };
        match chain.submit_block(b, true).unwrap() {
            ProcessResult::NewTip { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    let el = t.elapsed();
    let size = dir_size(dir.path());
    println!(
        "{n_empty} empty blocks: {:?} ({:.2} ms/block), db {} KB → {:.0} bytes/block → {:.1} MB/year",
        el,
        el.as_secs_f64() * 1000.0 / n_empty as f64,
        size / 1024,
        size as f64 / n_empty as f64,
        size as f64 / n_empty as f64 * 525_960.0 / 1e6
    );

    drop(chain);
    let mut db = thecoin_storage::ChainDb::open(&dir.path().join("chain.redb"), &Default::default()).unwrap();
    db.compact().unwrap();
    drop(db);
    let compacted = dir_size(dir.path());
    println!(
        "after compaction: {} KB → {:.0} bytes/block → {:.1} MB/year",
        compacted / 1024,
        compacted as f64 / n_empty as f64,
        compacted as f64 / n_empty as f64 * 525_960.0 / 1e6
    );
    let chain = Chain::open(params, &dir.path().join("chain.redb"), ChainOptions::default()).unwrap();

    // Phase 2: blocks with 500 transfers each to many new accounts.
    let blocks = 20u64;
    let per_block = 500u64;
    let mut nonce = 0u64;
    let before = dir_size(dir.path());
    let t = Instant::now();
    for i in 0..blocks {
        let txs: Vec<_> = (0..per_block)
            .map(|j| {
                let to = Address::from_public_key(
                    &SecretKey::from_bytes(&{
                        let mut s = [1u8; 32];
                        s[..8].copy_from_slice(&(i * per_block + j).to_le_bytes());
                        s
                    })
                    .public_key(),
                );
                let tx = build_tx(&key, Network::Regtest, nonce, 1, 0, transfer(to, COIN / 100, ""));
                nonce += 1;
                tx
            })
            .collect();
        let mut b = chain.build_template(&miner, &[], &txs).unwrap();
        assert_eq!(b.txs.len() as u64, per_block);
        b.header.timestamp = params.genesis_timestamp + 22 + n_empty / 4 + i;
        assert!(matches!(chain.submit_block(b, true).unwrap(), ProcessResult::NewTip { .. }));
    }
    let el = t.elapsed();
    let grown = dir_size(dir.path()) - before;
    println!(
        "{blocks} blocks × {per_block} transfers: {:?} total incl. template building ({:.0} ms/block), db growth {} KB ({:.0} bytes/tx)",
        el,
        el.as_secs_f64() * 1000.0 / blocks as f64,
        grown / 1024,
        grown as f64 / (blocks * per_block) as f64
    );
}
