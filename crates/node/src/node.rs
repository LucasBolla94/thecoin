//! Node wiring: shared state, the block processor thread and startup.

use crate::addrman::AddrMan;
use crate::chain::{now_secs, Chain, ChainOptions, NodeMutex, NodeRwLock, ProcessResult};
use crate::config::NodeConfig;
use crate::mempool::{Mempool, MempoolError};
use crate::miner::MinerState;
use crate::net::{Peer, SyncState};
use crate::protocol::{InvItem, InvKind, Message};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use thecoin_core::hash::Hash32;
use thecoin_core::params::ChainParams;
use thecoin_core::{Address, Block, Transaction};
use thecoin_storage::DbOptions;
use tokio::sync::{mpsc, watch, Notify};
use tracing::{debug, info, warn};

pub struct BlockJob {
    pub block: Block,
    /// Peer that sent the block (None = local miner / API).
    pub source: Option<u64>,
    pub pow_checked: bool,
}

pub struct Node {
    pub config: NodeConfig,
    pub params: &'static ChainParams,
    pub chain: Arc<Chain>,
    pub mempool: NodeMutex<Mempool>,
    pub miner: MinerState,
    pub started: Instant,

    // networking
    pub peers: NodeRwLock<HashMap<u64, Arc<Peer>>>,
    pub next_peer_id: AtomicU64,
    pub addrman: NodeMutex<AddrMan>,
    pub bans: NodeMutex<HashMap<IpAddr, Instant>>,
    pub sync: NodeMutex<SyncState>,
    pub local_nonce: u64,
    pub p2p_addr: OnceLock<SocketAddr>,
    pub rpc_addr: OnceLock<SocketAddr>,

    // events
    pub block_queue: mpsc::Sender<BlockJob>,
    pub tip_tx: watch::Sender<Hash32>,
    pub mempool_changed: Notify,
    pub shutdown: watch::Sender<bool>,
    pub stopping: AtomicBool,
}

impl Node {
    /// Opens the database and starts all services (P2P, RPC, miner).
    /// Must be called inside a Tokio runtime.
    pub async fn start(config: NodeConfig) -> Result<Arc<Node>> {
        let params = config.network.params();
        std::fs::create_dir_all(&config.data_dir).with_context(|| format!("creating data dir {}", config.data_dir.display()))?;
        let opts = ChainOptions {
            prune_keep: if config.storage.prune { Some(config.storage.prune_keep) } else { None },
            address_index: config.storage.address_index,
            db: DbOptions { cache_bytes: config.storage.cache_mb.max(4) * 1024 * 1024 },
        };
        let db_path = config.db_path();
        let chain = tokio::task::spawn_blocking(move || Chain::open(params, &db_path, opts)).await??;
        let chain = Arc::new(chain);

        let miner_address = if config.mining.enabled && !config.mining.address.trim().is_empty() {
            Some(Address::decode(&config.mining.address, config.network).map_err(|e| anyhow!("invalid mining.address: {e}"))?)
        } else {
            None
        };
        let mut signal = Vec::new();
        for s in &config.mining.signal {
            signal.push(Hash32::from_hex(s).map_err(|_| anyhow!("invalid proposal id in mining.signal: {s}"))?);
        }
        if config.mining.enabled && miner_address.is_none() {
            warn!("mining enabled but no mining.address configured; mining is disabled");
        }

        let (block_tx, block_rx) = mpsc::channel::<BlockJob>(2048);
        let (tip_tx, _) = watch::channel(chain.tip().hash);
        let (shutdown, _) = watch::channel(false);
        let addrman = AddrMan::load(&config.peers_path(), config.p2p.allow_private);

        let node = Arc::new(Node {
            params,
            mempool: NodeMutex::new(Mempool::new(config.mempool.max_mb.max(1) * 1024 * 1024)),
            miner: MinerState::new(miner_address, config.mining_threads(), signal),
            started: Instant::now(),
            peers: NodeRwLock::new(HashMap::new()),
            next_peer_id: AtomicU64::new(1),
            addrman: NodeMutex::new(addrman),
            bans: NodeMutex::new(HashMap::new()),
            sync: NodeMutex::new(SyncState::default()),
            local_nonce: rand::random(),
            p2p_addr: OnceLock::new(),
            rpc_addr: OnceLock::new(),
            block_queue: block_tx,
            tip_tx,
            mempool_changed: Notify::new(),
            shutdown,
            stopping: AtomicBool::new(false),
            chain,
            config,
        });

        spawn_block_processor(node.clone(), block_rx);
        if node.config.p2p.enabled {
            crate::net::start(node.clone()).await?;
        }
        if node.config.rpc.enabled {
            crate::rpc::start(node.clone()).await?;
        }
        if node.miner.address.is_some() {
            crate::miner::start(node.clone());
        }
        info!(
            network = %node.params.network,
            height = node.chain.tip().height,
            p2p = ?node.p2p_addr.get(),
            rpc = ?node.rpc_addr.get(),
            mining = node.miner.address.is_some(),
            "The Coin node started"
        );
        Ok(node)
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.shutdown.send(true);
        self.addrman.lock().save();
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }

    /// True while downloading blocks from a peer that is ahead of us.
    pub fn is_syncing(&self) -> bool {
        let our = self.chain.tip().height;
        let best_peer = self.peers.read().values().map(|p| p.best_height()).max().unwrap_or(0);
        best_peer > our + 1
    }

    /// Validates a transaction, adds it to the mempool and relays it.
    pub fn submit_tx(&self, tx: Transaction, source: Option<u64>) -> std::result::Result<Hash32, MempoolError> {
        let txid = self.mempool.lock().add(&self.chain, tx)?;
        self.mempool_changed.notify_waiters();
        self.broadcast_inv(InvItem { kind: InvKind::Tx, hash: txid }, source);
        Ok(txid)
    }

    /// Sends an inventory announcement to every connected peer except `except`
    /// and those known to have the item.
    pub fn broadcast_inv(&self, item: InvItem, except: Option<u64>) {
        let peers: Vec<Arc<Peer>> = self.peers.read().values().cloned().collect();
        for p in peers {
            if Some(p.id) == except || !p.is_ready() || p.knows(&item.hash) {
                continue;
            }
            p.mark_known(item.hash);
            p.send(&Message::Inv(vec![item]));
        }
    }
}

fn spawn_block_processor(node: Arc<Node>, mut rx: mpsc::Receiver<BlockJob>) {
    std::thread::Builder::new()
        .name("block-processor".into())
        .spawn(move || {
            let mut last_log = Instant::now();
            while let Some(job) = rx.blocking_recv() {
                if node.is_stopping() {
                    break;
                }
                let hash = job.block.hash();
                let height = job.block.header.height;
                let tx_count = job.block.txs.len();
                let local = job.source.is_none();
                let result = node.chain.submit_block(job.block, job.pow_checked);
                match result {
                    Ok(ProcessResult::NewTip { disconnected, connected }) => {
                        let tip = node.chain.tip();
                        if let Err(e) = node.mempool.lock().on_new_tip(&node.chain, &disconnected, &connected) {
                            warn!(error = %e, "mempool update failed");
                        }
                        let _ = node.tip_tx.send(tip.hash);
                        let recent = now_secs().saturating_sub(tip.header.timestamp) < 3600;
                        if local || recent || last_log.elapsed().as_secs() >= 10 {
                            info!(height = tip.height, hash = %tip.hash, txs = tx_count, local, "new tip");
                            last_log = Instant::now();
                        }
                        if local {
                            node.miner.blocks_found.fetch_add(1, Ordering::Relaxed);
                        }
                        if recent || local {
                            node.broadcast_inv(InvItem { kind: InvKind::Block, hash: tip.hash }, job.source);
                        }
                        if let Some(src) = job.source {
                            if let Some(p) = node.peers.read().get(&src) {
                                p.update_best_height(height);
                            }
                        }
                        crate::net::on_block_processed(&node, job.source, &hash);
                    }
                    Ok(ProcessResult::Orphan) => {
                        debug!(%hash, height, "orphan block");
                        if let Some(src) = job.source {
                            crate::net::request_missing_blocks(&node, src, height);
                        }
                    }
                    Ok(ProcessResult::Invalid(e)) => {
                        warn!(%hash, height, error = %e, "rejected invalid block");
                        if let Some(src) = job.source {
                            crate::net::misbehave(&node, src, 100, "invalid block");
                        }
                    }
                    Ok(ProcessResult::SideChain) | Ok(ProcessResult::AlreadyKnown) => {
                        crate::net::on_block_processed(&node, job.source, &hash);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "storage error while processing block");
                    }
                }
            }
        })
        .expect("spawn block processor");
}
