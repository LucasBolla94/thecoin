//! Four mining nodes reach finality: once two thirds of the recent miners
//! signed the same block, every node marks it as final.

use std::sync::Arc;
use std::time::{Duration, Instant};
use thecoin_core::crypto::SecretKey;

use thecoin_core::{Address, Network};
use thecoin_node::{Node, NodeConfig};
use thecoin_storage::DbRead;

fn config(dir: &std::path::Path, miner: Address, connect: Vec<String>) -> NodeConfig {
    let mut c = NodeConfig::for_network(Network::Regtest);
    c.data_dir = dir.to_path_buf();
    c.p2p.listen = "127.0.0.1:0".into();
    c.rpc.listen = "127.0.0.1:0".into();
    c.p2p.connect = connect;
    c.mining.enabled = true;
    c.mining.address = miner.encode(Network::Regtest);
    c.mining.threads = 1;
    c.mining.mode = "light".into();
    c
}

async fn wait_until(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        if start.elapsed() > timeout {
            panic!("timeout waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn miners_finalise_blocks_together() {
    let dirs: Vec<tempfile::TempDir> = (0..4).map(|_| tempfile::tempdir().unwrap()).collect();
    let addrs: Vec<Address> = (0..4).map(|i| Address::from_public_key(&SecretKey::from_bytes(&[i as u8 + 1; 32]).public_key())).collect();

    let first = Node::start(config(dirs[0].path(), addrs[0], vec![])).await.unwrap();
    let seed = first.p2p_addr.get().unwrap().to_string();
    let mut nodes: Vec<Arc<Node>> = vec![first];
    for i in 1..4 {
        nodes.push(Node::start(config(dirs[i].path(), addrs[i], vec![seed.clone()])).await.unwrap());
    }

    // Every node must mine, so that the window has four different signers.
    for (i, n) in nodes.iter().enumerate() {
        let n = n.clone();
        wait_until(&format!("node {i} mines"), Duration::from_secs(120), move || {
            n.miner.blocks_found.load(std::sync::atomic::Ordering::Relaxed) > 0
        })
        .await;
    }

    // The window only counts once the chain is long enough.
    let n0 = nodes[0].clone();
    wait_until("the chain passes the finality window", Duration::from_secs(180), move || {
        n0.chain.tip().height > Network::Regtest.params().finality_window + 5
    })
    .await;

    for (i, n) in nodes.iter().enumerate() {
        let tip = n.chain.tip();
        let full = n.params.finality_window;
        let window = n.chain.signer_window(&tip.header.prev_hash, full).unwrap();
        let mut distinct: Vec<[u8; 32]> = Vec::new();
        for k in &window {
            if !distinct.contains(k) {
                distinct.push(*k);
            }
        }
        let weight = n.finality.lock().weight(tip.height, &tip.hash, &window);
        let mut recent = Vec::new();
        {
            let r = n.chain.read().unwrap();
            let tally = n.finality.lock();
            for h in tip.height.saturating_sub(6)..=tip.height {
                if let Some(hash) = r.main_hash(h).unwrap() {
                    let w = tally.weight(h, &hash, &window);
                    recent.push(format!("{h}:{w}"));
                }
            }
        }
        println!(
            "node {i}: height {} peers {} window {} distinct {} weight {} finalized {:?} signer_set {}",
            tip.height,
            n.peers.read().len(),
            window.len(),
            distinct.len(),
            weight,
            n.chain.finalized(),
            tip.header.signer != [0u8; 32],
        );
        println!("   recent weights {} counts {:?}", recent.join(" "), n.finality.lock().counts());
    }

    for (i, n) in nodes.iter().enumerate() {
        let n = n.clone();
        wait_until(&format!("node {i} finalises a block"), Duration::from_secs(60), move || {
            n.chain.finalized().is_some_and(|(h, _)| h > 0)
        })
        .await;
    }

    // Every node finalises the same block, on its own main chain, right behind the tip.
    let (height, hash) = nodes[0].chain.finalized().unwrap();
    for n in &nodes {
        let (h, hh) = n.chain.finalized().unwrap();
        let r = n.chain.read().unwrap();
        assert_eq!(r.main_hash(h).unwrap(), Some(hh), "the final block must be on the main chain");
        if h == height {
            assert_eq!(hh, hash, "nodes disagree on the final block at {h}");
        }
        assert!(n.chain.tip().height - h < 20, "finality is far behind the tip: {h} vs {}", n.chain.tip().height);
    }
    let r = nodes[0].chain.read().unwrap();
    assert_eq!(r.main_hash(height).unwrap(), Some(hash));
    drop(r);
    for n in nodes {
        n.stop();
    }
}
