//! Disk usage per transaction in steady state (undo data already pruned).
//! `cargo test -p thecoin-node --release --test storage_growth -- --ignored --nocapture`
//! Tunables: GROWTH_BLOCKS (default 1500), GROWTH_TXS (default 200 per block).

use std::time::Instant;
use thecoin_core::amount::COIN;
use thecoin_core::crypto::SecretKey;
use thecoin_core::{Address, Network, Transaction};
use thecoin_node::chain::{Chain, ChainOptions, ProcessResult};
use thecoin_wallet::builder::{build_tx, transfer, FeePolicy};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Builds `blocks` blocks with `per_block` transfers to new accounts, then
/// enough empty blocks to prune undo data. Returns (bytes used, file size).
fn build(opts: ChainOptions, blocks: u64, per_block: u64, memo: &str) -> (u64, u64) {
    let params = Network::Regtest.params();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chain.redb");
    let chain = Chain::open(params, &path, opts).unwrap();
    let funder = SecretKey::from_bytes(&[7; 32]);
    let funder_addr = Address::from_public_key(&funder.public_key());
    let mut ts = params.genesis_timestamp;
    let mut mine = |txs: Vec<Transaction>| {
        let mut b = chain.build_template(&funder_addr, [0u8; 32], &[], &txs).unwrap();
        assert_eq!(b.txs.len(), txs.len(), "all txs must fit");
        ts += 1;
        b.header.timestamp = ts;
        assert!(matches!(chain.submit_block(b, true).unwrap(), ProcessResult::NewTip { .. }));
    };
    for _ in 0..(params.coinbase_maturity + 40) {
        mine(vec![]);
    }
    let mut nonce = 0u64;
    for i in 0..blocks {
        let txs = (0..per_block)
            .map(|j| {
                let mut seed = [3u8; 32];
                seed[..8].copy_from_slice(&(i * per_block + j).to_le_bytes());
                let to = Address::from_public_key(&SecretKey::from_bytes(&seed).public_key());
                let tx = build_tx(
                    &funder,
                    Network::Regtest,
                    nonce,
                    &FeePolicy { base_fee: 100, fee_per_kb: 1_000, fee_per_kfuel: 100, congestion_bp: 10_000, priority_bp: 20_000 },
                    0,
                    0,
                    transfer(to, COIN / 1000, memo),
                );
                nonce += 1;
                tx
            })
            .collect();
        mine(txs);
    }
    for _ in 0..(params.max_reorg_depth + 20) {
        mine(vec![]);
    }
    let (stored, meta, _) = chain.space_stats().unwrap();
    drop(chain);
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::metadata(&path).unwrap();
    println!(
        "    apparent file size {:.1} MB, blocks actually allocated on disk {:.1} MB",
        md.len() as f64 / 1e6,
        (md.blocks() * 512) as f64 / 1e6
    );
    (stored + meta, md.blocks() * 512)
}

#[test]
#[ignore]
fn storage_growth() {
    let blocks = env("GROWTH_BLOCKS", 1500);
    let per_block = env("GROWTH_TXS", 200);
    let txs = (blocks * per_block) as f64;
    let (empty_used, empty_file) = build(ChainOptions::default(), blocks, 0, "");
    println!("baseline ({} empty blocks): used {:.1} MB, file {:.1} MB", blocks + 876, empty_used as f64 / 1e6, empty_file as f64 / 1e6);
    for (label, opts, memo) in [
        ("archive + address index", ChainOptions::default(), ""),
        ("archive, no address index", ChainOptions { address_index: false, ..Default::default() }, ""),
        ("archive + index, 32-byte memo", ChainOptions::default(), "pedido-000000000000000000000001"),
    ] {
        let t = Instant::now();
        let (used, file) = build(opts, blocks, per_block, memo);
        println!(
            "{label}: {} txs in {:.0}s — used {:.1} MB (file {:.1} MB) → {:.0} bytes/tx of data, {:.0} bytes/tx of real disk",
            blocks * per_block,
            t.elapsed().as_secs_f64(),
            used as f64 / 1e6,
            file as f64 / 1e6,
            (used - empty_used) as f64 / txs,
            (file - empty_file) as f64 / txs
        );
    }
}
