//! Built-in CPU miner.
//!
//! * Worker threads run at the lowest OS priority (`nice 19`) so the node,
//!   the API and the rest of the machine stay responsive.
//! * Default thread count is CPU cores − 1 (at least 1).
//! * Mining pauses while the node is catching up with the network.
//! * Each worker keeps its own 16 MiB CoinHash buffer.

use crate::chain::now_secs;
use crate::node::{BlockJob, Node};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thecoin_core::hash::Hash32;
use thecoin_core::pow::{meets_target, PowHasher};
use thecoin_core::{Address, Block, U256};
use tracing::{debug, info, warn};

pub struct MiningJob {
    pub id: u64,
    pub block: Block,
    pub target: U256,
}

pub struct MinerState {
    pub address: Option<Address>,
    pub threads: usize,
    pub signal: Vec<Hash32>,
    job: crate::chain::NodeRwLock<Option<Arc<MiningJob>>>,
    job_id: AtomicU64,
    pub hashes: AtomicU64,
    hashrate_bits: AtomicU64,
    pub blocks_found: AtomicU64,
}

impl MinerState {
    pub fn new(address: Option<Address>, threads: usize, signal: Vec<Hash32>) -> Self {
        MinerState {
            address,
            threads,
            signal,
            job: crate::chain::NodeRwLock::new(None),
            job_id: AtomicU64::new(0),
            hashes: AtomicU64::new(0),
            hashrate_bits: AtomicU64::new(0f64.to_bits()),
            blocks_found: AtomicU64::new(0),
        }
    }

    pub fn hashrate(&self) -> f64 {
        f64::from_bits(self.hashrate_bits.load(Ordering::Relaxed))
    }

    fn set_job(&self, block: Option<Block>) {
        let id = self.job_id.fetch_add(1, Ordering::SeqCst) + 1;
        *self.job.write() = block.map(|b| Arc::new(MiningJob { id, target: b.header.target_u256(), block: b }));
    }
}

pub fn start(node: Arc<Node>) {
    let Some(address) = node.miner.address else { return };
    info!(threads = node.miner.threads, address = %address.encode(node.params.network), "starting CPU miner");
    for i in 0..node.miner.threads {
        let n = node.clone();
        std::thread::Builder::new().name(format!("miner-{i}")).spawn(move || worker(n)).expect("spawn miner thread");
    }
    let n = node.clone();
    tokio::spawn(async move { coordinator(n, address).await });
    let n = node.clone();
    tokio::spawn(async move {
        let mut last = n.miner.hashes.load(Ordering::Relaxed);
        let mut t = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            if n.is_stopping() {
                return;
            }
            let now = n.miner.hashes.load(Ordering::Relaxed);
            let rate = (now - last) as f64 / t.elapsed().as_secs_f64();
            n.miner.hashrate_bits.store(rate.to_bits(), Ordering::Relaxed);
            last = now;
            t = Instant::now();
        }
    });
}

async fn coordinator(node: Arc<Node>, address: Address) {
    let mut tip_rx = node.tip_tx.subscribe();
    let mut shutdown = node.shutdown.subscribe();
    loop {
        if node.is_syncing() {
            node.miner.set_job(None);
            debug!("miner paused while syncing");
        } else {
            let n = node.clone();
            let built = tokio::task::spawn_blocking(move || {
                let max = n.chain.global().map(|g| g.params.max_block_bytes as usize).unwrap_or(1_000_000);
                let candidates = n.mempool.lock().ordered_for_block(max);
                n.chain.build_template(&address, &n.miner.signal, &candidates)
            })
            .await;
            match built {
                Ok(Ok(block)) => {
                    debug!(height = block.header.height, txs = block.txs.len(), "new mining job");
                    node.miner.set_job(Some(block));
                }
                Ok(Err(e)) => {
                    warn!(error = %e, "failed to build block template");
                    node.miner.set_job(None);
                }
                Err(e) => warn!(error = %e, "template task panicked"),
            }
        }
        tokio::select! {
            _ = tip_rx.changed() => {}
            _ = node.mempool_changed.notified() => {
                // Debounce bursts of transactions.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {}
            _ = shutdown.changed() => {
                node.miner.set_job(None);
                return;
            }
        }
    }
}

#[cfg(unix)]
fn lower_priority() {
    // SAFETY: setpriority only affects scheduling of the calling thread.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }
}

#[cfg(not(unix))]
fn lower_priority() {}

fn worker(node: Arc<Node>) {
    lower_priority();
    let mut hasher = PowHasher::new(node.params.pow);
    loop {
        if node.is_stopping() {
            return;
        }
        let job = node.miner.job.read().clone();
        let Some(job) = job else {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        };
        let mut header = job.block.header.clone();
        let mut nonce: u64 = rand::random();
        let mut last_ts = Instant::now();
        let mut local_hashes = 0u64;
        loop {
            header.nonce = nonce;
            nonce = nonce.wrapping_add(1);
            let hash = hasher.hash(&header.to_bytes());
            local_hashes += 1;
            if meets_target(&hash, &job.target) {
                node.miner.hashes.fetch_add(local_hashes, Ordering::Relaxed);
                let block = Block { header: header.clone(), txs: job.block.txs.clone() };
                info!(height = header.height, hash = %block.hash(), "block found!");
                // Stop other workers from mining the same job.
                if node.miner.job_id.load(Ordering::SeqCst) == job.id {
                    node.miner.set_job(None);
                }
                let _ = node.block_queue.blocking_send(BlockJob { block, source: None, pow_checked: true });
                if node.params.network == thecoin_core::Network::Regtest {
                    // Regtest PoW is trivial; throttle so timestamps stay sane.
                    std::thread::sleep(Duration::from_millis(500));
                }
                break;
            }
            if local_hashes % 8 == 0 {
                node.miner.hashes.fetch_add(local_hashes, Ordering::Relaxed);
                local_hashes = 0;
                if node.miner.job_id.load(Ordering::Relaxed) != job.id || node.is_stopping() {
                    break;
                }
                if last_ts.elapsed() > Duration::from_secs(1) {
                    header.timestamp = now_secs().max(job.block.header.timestamp);
                    last_ts = Instant::now();
                }
            }
        }
    }
}
