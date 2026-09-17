//! Performance measurements (run with `cargo test -p thecoin-core --release --test bench -- --ignored --nocapture`).

use std::collections::BTreeMap;
use std::time::Instant;
use thecoin_core::address::Address;
use thecoin_core::amount::COIN;
use thecoin_core::block::{Block, BlockHeader};
use thecoin_core::crypto::SecretKey;
use thecoin_core::execution::{apply_block, check_tx_stateless};
use thecoin_core::genesis::genesis_state;
use thecoin_core::hash::Hash32;
use thecoin_core::lthash::LtHash;
use thecoin_core::params::MAINNET;
use thecoin_core::state::*;
use thecoin_core::tx::{TxAction, TxBody, TX_VERSION};

#[test]
#[ignore]
fn bench_block_validation() {
    let p = &MAINNET;
    const SENDERS: usize = 5_000;
    let mut state: BTreeMap<Vec<u8>, Vec<u8>> = genesis_state(p).into_iter().collect();
    let keys: Vec<SecretKey> = (0..SENDERS)
        .map(|i| {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&(i as u64).to_le_bytes());
            SecretKey::from_bytes(&b)
        })
        .collect();
    for k in &keys {
        let a = Address::from_public_key(&k.public_key());
        state.insert(account_key(&a), borsh::to_vec(&Account { balance: 1_000 * COIN, nonce: 0, locked: 0, locked_until: 0 }).unwrap());
    }
    let t = Instant::now();
    let txs: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let to = Address::from_public_key(&keys[(i + 1) % SENDERS].public_key());
            let body = TxBody {
                version: TX_VERSION,
                chain_id: p.chain_id,
                flags: 0,
                nonce: 0,
                fee: 10_000,
                expiry_height: 0,
                action: TxAction::Transfer { to, amount: COIN, memo: vec![] },
            };
            body.sign(k)
        })
        .collect();
    println!("signing {SENDERS} txs: {:?}", t.elapsed());
    let size = txs[0].size();
    println!("transfer tx size (no memo): {size} bytes");

    let t = Instant::now();
    for tx in &txs {
        check_tx_stateless(p, tx).unwrap();
    }
    let sig = t.elapsed();
    println!("stateless checks + Ed25519 verify: {:?} total, {:.0} tx/s", sig, SENDERS as f64 / sig.as_secs_f64());

    let header = BlockHeader {
        version: 1,
        height: 1,
        prev_hash: Hash32::ZERO,
        tx_root: Hash32::ZERO,
        state_root: Hash32::ZERO,
        timestamp: 0,
        target: [0xff; 32],
        nonce: 0,
        miner: Address::ZERO,
        signal: 0,
        uncles_root: thecoin_core::block::uncles_root(&[]),
    };
    let block = Block { header, txs, uncles: Vec::new() };
    println!("block: {} txs, {} bytes", block.txs.len(), block.serialized_size());
    let t = Instant::now();
    let mut ov = Overlay::new(&state);
    apply_block(p, &mut ov, &block, false).unwrap();
    let apply = t.elapsed();
    let diff = ov.diff().unwrap();
    let t2 = Instant::now();
    let mut lt = LtHash::default();
    apply_changes_to_lthash(&mut lt, &diff);
    let lth = t2.elapsed();
    println!(
        "apply_block (incl. signatures): {:?} ({:.0} tx/s); state changes: {}",
        apply,
        SENDERS as f64 / apply.as_secs_f64(),
        diff.len()
    );
    println!("LtHash update for {} changes: {:?}", diff.len(), lth);
}
