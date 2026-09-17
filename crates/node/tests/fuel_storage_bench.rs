//! Wall-clock cost of contract storage fuel on the real database (redb +
//! state-root updates), with a large contract state.
//! `cargo test -p thecoin-node --release --test fuel_storage_bench -- --ignored --nocapture`

use std::time::Instant;
use thecoin_core::amount::COIN;
use thecoin_core::crypto::SecretKey;
use thecoin_core::programs::program_address;
use thecoin_core::tccl::Value;
use thecoin_core::tx::TxAction;
use thecoin_core::{Address, Network, Transaction};
use thecoin_node::chain::{Chain, ChainOptions, ProcessResult};
use thecoin_storage::DbRead;
use thecoin_wallet::builder::{build_tx, FeePolicy};

const SRC: &str = "contract Store
state m: map[int, bytes]

action fill(start: int, count: int, d: bytes):
    for i in range(start, start + count):
        m[i] = d

action read_all(start: int):
    let i: int = start
    let n: int = 0
    while true:
        n += len(m[(i * 7919) % 400000])
        i += 1

action rewrite(start: int, d: bytes):
    let i: int = start
    while true:
        m[(i * 7919) % 400000] = d
        i += 1

action miss(start: int):
    let i: int = start
    while true:
        let x: bool = m.has(i * 1000003 + 5000000)
        i += 1
";

#[test]
#[ignore]
fn storage_fuel_cost() {
    let params = Network::Regtest.params();
    let dir = tempfile::tempdir().unwrap();
    let chain = Chain::open(params, &dir.path().join("chain.redb"), ChainOptions::default()).unwrap();
    let key = SecretKey::from_bytes(&[7; 32]);
    let addr = Address::from_public_key(&key.public_key());
    let policy = FeePolicy { base_fee: 100, fee_per_kb: 1_000, fee_per_kfuel: 100, congestion_bp: 10_000, priority_bp: 1_000_000 };
    let mut ts = params.genesis_timestamp;
    let mut nonce = 0u64;
    let mut mine = |txs: Vec<Transaction>| -> std::time::Duration {
        {
            let (reader, h) = chain.read_at_tip().unwrap();
            let mut ov = thecoin_core::state::Overlay::new(&reader);
            for t in &txs {
                if let Err(e) = thecoin_core::execution::apply_tx(params, &mut ov, h + 1, t, t.size()) {
                    println!("tx rejected: {e} (congestion {} bp, height {h})", chain.global().unwrap().congestion_bp);
                }
            }
        }
        let mut b = chain.build_template(&addr, [0u8; 32], &[], &txs).unwrap();
        assert_eq!(b.txs.len(), txs.len(), "all txs must be included");
        ts += 1;
        b.header.timestamp = ts;
        let t = Instant::now();
        assert!(matches!(chain.submit_block(b, true).unwrap(), ProcessResult::NewTip { .. }));
        t.elapsed()
    };
    for _ in 0..40 {
        mine(vec![]);
    }
    let mut tx = |action: TxAction| {
        let t = build_tx(&key, Network::Regtest, nonce, &policy, 0, 0, action);
        nonce += 1;
        t
    };
    let contract = program_address(&addr, 0);
    mine(vec![tx(TxAction::Deploy { source: SRC.into(), init_args: vec![], value: 0, max_fuel: 1_000_000, max_deposit: 1_000 * COIN })]);
    // 400 000 entries of 64 bytes.
    let t = Instant::now();
    for chunk in 0..40 {
        let txs: Vec<_> = (0..4)
            .map(|k| {
                tx(TxAction::Invoke {
                    contract,
                    function: "fill".into(),
                    args: vec![Value::Int((chunk * 4 + k) as i128 * 2_500), Value::Int(2_500), Value::Bytes(vec![1; 64])],
                    value: 0,
                    max_fuel: 10_000_000,
                    max_deposit: 1_000 * COIN,
                })
            })
            .collect();
        mine(txs);
    }
    println!("filled 400k entries in {:.1}s", t.elapsed().as_secs_f64());
    for _ in 0..20 {
        mine(vec![]);
    }
    for (label, function, args) in [
        ("random reads (existing)", "read_all", vec![Value::Int(11)]),
        ("reads of missing keys", "miss", vec![Value::Int(3)]),
        ("random overwrites 64 B", "rewrite", vec![Value::Int(5), Value::Bytes(vec![2; 64])]),
        ("random overwrites 1 kB", "rewrite", vec![Value::Int(99), Value::Bytes(vec![3; 1000])]),
    ] {
        let txs: Vec<_> = (0..5)
            .map(|k| {
                let mut a = args.clone();
                if let Value::Int(s) = &mut a[0] {
                    *s += k * 100_000;
                }
                tx(TxAction::Invoke {
                    contract,
                    function: function.into(),
                    args: a,
                    value: 0,
                    max_fuel: 10_000_000,
                    max_deposit: 1_000 * COIN,
                })
            })
            .collect();
        let ids: Vec<_> = txs.iter().map(|t| t.txid()).collect();
        let el = mine(txs);
        let r = chain.read().unwrap();
        let fuel: u64 = ids.iter().map(|id| r.receipt(id).unwrap().unwrap().fuel_used).sum();
        drop(r);
        let ns = el.as_nanos() as f64 / fuel.max(1) as f64;
        println!("{label:<26} block of {fuel} fuel validated+committed in {:.2}s → {ns:.1} ns/fuel", el.as_secs_f64());
    }
}
