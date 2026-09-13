//! Multi-node integration tests on regtest (in-process nodes on localhost).

use std::sync::Arc;
use std::time::{Duration, Instant};
use thecoin_core::amount::COIN;
use thecoin_core::crypto::SecretKey;
use thecoin_core::{Address, Network};
use thecoin_node::{Node, NodeConfig};
use thecoin_storage::DbRead;
use thecoin_wallet::builder::{build_tx, transfer, FeePolicy};

fn policy() -> FeePolicy {
    FeePolicy { base_fee: 100, fee_per_kb: 1_000, fee_per_kfuel: 100, congestion_bp: 10_000, priority_bp: 20_000 }
}

fn config(dir: &std::path::Path, miner: Option<Address>, connect: Vec<String>) -> NodeConfig {
    let mut c = NodeConfig::for_network(Network::Regtest);
    c.data_dir = dir.to_path_buf();
    c.p2p.listen = "127.0.0.1:0".into();
    c.rpc.listen = "127.0.0.1:0".into();
    c.p2p.connect = connect;
    c.mining.enabled = miner.is_some();
    c.mining.address = miner.map(|a| a.encode(Network::Regtest)).unwrap_or_default();
    c.mining.threads = 1;
    c
}

async fn wait_until(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        if start.elapsed() > timeout {
            panic!("timeout waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn hash_at(node: &Node, h: u64) -> Option<thecoin_core::Hash32> {
    node.chain.read().ok()?.main_hash(h).ok()?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sync_relay_and_mine_transactions() {
    let key = SecretKey::from_bytes(&[42; 32]);
    let miner_addr = Address::from_public_key(&key.public_key());
    let bob = Address::from_public_key(&SecretKey::from_bytes(&[43; 32]).public_key());

    let da = tempfile::tempdir().unwrap();
    let a = Node::start(config(da.path(), Some(miner_addr), vec![])).await.unwrap();
    wait_until("node A mines 12 blocks", Duration::from_secs(60), || a.chain.tip().height >= 12).await;

    let db = tempfile::tempdir().unwrap();
    let a_p2p = a.p2p_addr.get().unwrap().to_string();
    let b = Node::start(config(db.path(), None, vec![a_p2p])).await.unwrap();
    wait_until("node B syncs with A", Duration::from_secs(60), || {
        let hb = b.chain.tip().height;
        hb >= 12 && hash_at(&a, hb) == Some(b.chain.tip().hash)
    })
    .await;

    // Transaction submitted to B is relayed to A, mined, and the block comes back to B.
    let nonce = {
        let r = b.chain.read().unwrap();
        thecoin_core::state::read_typed::<thecoin_core::state::Account, _>(&r, &thecoin_core::state::account_key(&miner_addr))
            .unwrap()
            .unwrap()
            .nonce
    };
    let tx = build_tx(&key, Network::Regtest, nonce, &policy(), 0, 0, transfer(bob, 7 * COIN, "p2p test"));
    let txid = b.submit_tx(tx, None).expect("tx accepted by B");
    wait_until("tx reaches A's mempool or chain", Duration::from_secs(30), || {
        a.mempool.lock().contains(&txid) || a.chain.read().unwrap().tx_location(&txid).unwrap().is_some()
    })
    .await;
    wait_until("tx confirmed on B", Duration::from_secs(60), || b.chain.read().unwrap().tx_location(&txid).unwrap().is_some()).await;
    let bob_balance = {
        let r = b.chain.read().unwrap();
        thecoin_core::state::read_typed::<thecoin_core::state::Account, _>(&r, &thecoin_core::state::account_key(&bob))
            .unwrap()
            .unwrap()
            .balance
    };
    assert_eq!(bob_balance, 7 * COIN);
    assert!(!b.mempool.lock().contains(&txid));

    // Double-spend attempt: A accepts tx1; B already has tx1 relayed and gets a
    // conflicting tx2 with the same nonce → B refuses it and alerts A.
    let nonce = {
        let r = a.chain.read().unwrap();
        thecoin_core::state::read_typed::<thecoin_core::state::Account, _>(&r, &thecoin_core::state::account_key(&miner_addr))
            .unwrap()
            .unwrap()
            .nonce
    };
    let tx1 = build_tx(&key, Network::Regtest, nonce, &policy(), 0, 0, transfer(bob, COIN, "first"));
    let tx2 = build_tx(&key, Network::Regtest, nonce, &policy(), 0, 0, transfer(miner_addr, COIN, "second"));
    let id1 = a.submit_tx(tx1, None).expect("tx1 accepted");
    wait_until("tx1 relayed to B", Duration::from_secs(30), || {
        b.mempool.lock().contains(&id1) || b.chain.read().unwrap().tx_location(&id1).unwrap().is_some()
    })
    .await;
    if b.mempool.lock().contains(&id1) {
        assert!(matches!(b.submit_tx(tx2, None), Err(thecoin_node::mempool::MempoolError::NotReplaceable { .. })));
        wait_until("A receives the double-spend alert", Duration::from_secs(30), || {
            !a.mempool.lock().conflicts(Network::Regtest).is_empty()
        })
        .await;
    }

    // A TCCL contract deployed through A executes identically on B.
    let source =
        "contract Hello\nstate greeting: text = \"hi\"\naction set(g: text):\n    greeting = g\nview get() -> text:\n    return greeting\n";
    let nonce = {
        let r = a.chain.read().unwrap();
        let acc = thecoin_core::state::read_typed::<thecoin_core::state::Account, _>(&r, &thecoin_core::state::account_key(&miner_addr))
            .unwrap()
            .unwrap();
        a.mempool.lock().next_nonce(&miner_addr, acc.nonce)
    };
    let program = thecoin_core::programs::program_address(&miner_addr, nonce);
    let deploy =
        build_tx(&key, Network::Regtest, nonce, &policy(), 0, 0, thecoin_wallet::builder::deploy(source, vec![], 0, 500_000, COIN));
    let set = build_tx(
        &key,
        Network::Regtest,
        nonce + 1,
        &policy(),
        0,
        0,
        thecoin_wallet::builder::invoke(program, "set", vec![thecoin_core::tccl::Value::Text("olá".into())], 0, 200_000, COIN),
    );
    let deploy_id = a.submit_tx(deploy, None).expect("deploy accepted");
    wait_until("deploy confirmed on B", Duration::from_secs(60), || b.chain.read().unwrap().receipt(&deploy_id).unwrap().is_some()).await;
    let set_id = a.submit_tx(set, None).expect("invoke accepted");
    wait_until("invoke confirmed on B", Duration::from_secs(60), || b.chain.read().unwrap().receipt(&set_id).unwrap().is_some()).await;
    let receipt = b.chain.read().unwrap().receipt(&set_id).unwrap().unwrap();
    assert!(receipt.success, "{:?}", receipt.error);
    let r = b.chain.read().unwrap();
    let (value, _) = thecoin_core::programs::view(&r, 0, &program, "get", vec![], 100_000).unwrap();
    assert_eq!(value.unwrap(), thecoin_core::tccl::Value::Text("olá".into()));

    a.stop();
    b.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn competing_miners_converge_via_reorg() {
    let mx = Address::from_public_key(&SecretKey::from_bytes(&[1; 32]).public_key());
    let my = Address::from_public_key(&SecretKey::from_bytes(&[2; 32]).public_key());

    let dx = tempfile::tempdir().unwrap();
    let dy = tempfile::tempdir().unwrap();
    // X gets more hashrate: with trivial regtest PoW both miners would otherwise
    // produce blocks at exactly the same pace and stay tied (first-seen wins).
    let mut cx = config(dx.path(), Some(mx), vec![]);
    cx.mining.threads = 2;
    let x = Node::start(cx).await.unwrap();
    let y = Node::start(config(dy.path(), Some(my), vec![])).await.unwrap();
    // Two independent chains.
    wait_until("both mine separately", Duration::from_secs(60), || x.chain.tip().height >= 6 && y.chain.tip().height >= 6).await;
    assert_ne!(hash_at(&x, 5), hash_at(&y, 5));

    // Z bridges them; the heavier chain must win everywhere.
    let dz = tempfile::tempdir().unwrap();
    let z = Node::start(config(dz.path(), None, vec![x.p2p_addr.get().unwrap().to_string(), y.p2p_addr.get().unwrap().to_string()]))
        .await
        .unwrap();
    let nodes: Vec<Arc<Node>> = vec![x.clone(), y.clone(), z.clone()];
    wait_until("all three nodes agree on a common block", Duration::from_secs(120), || {
        let min_h = nodes.iter().map(|n| n.chain.tip().height).min().unwrap();
        if min_h < 8 {
            return false;
        }
        let h = min_h - 2;
        let hx = hash_at(&x, h);
        hx.is_some() && hx == hash_at(&y, h) && hx == hash_at(&z, h)
    })
    .await;
    // At least one miner must have reorganized away from its own early block 5.
    let final5 = hash_at(&z, 5);
    assert!(final5 == hash_at(&x, 5) && final5 == hash_at(&y, 5));

    for n in nodes {
        n.stop();
    }
}
