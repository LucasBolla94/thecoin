//! Peer-to-peer networking: connections, handshake, block/tx relay, sync.
//!
//! * Each connection has a reader task (decodes frames), a writer task (bounded
//!   queue of encoded frames — slow peers are dropped instead of stalling us)
//!   and a handler loop.
//! * Blocks received from a peer go through a small pipeline that verifies
//!   CoinHash on several CPU cores in parallel while keeping the original
//!   order, then into the single block-processor thread.
//! * Misbehaving peers accumulate a score; at 100 they are disconnected and
//!   their IP banned for 24 h.

use crate::addrman::is_routable;
use crate::chain::{now_secs, RelayedHeader};
use crate::node::{BlockJob, Node};
use crate::protocol::*;
use anyhow::{bail, Result};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thecoin_core::hash::Hash32;
use thecoin_core::pow::PowHasher;
use thecoin_core::Block;
use thecoin_storage::DbRead;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};
use tracing::{debug, info, warn};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const PING_INTERVAL: Duration = Duration::from_secs(60);
const PONG_TIMEOUT: Duration = Duration::from_secs(180);
const SYNC_STALL: Duration = Duration::from_secs(90);
const BAN_DURATION: Duration = Duration::from_secs(24 * 3600);
const MAX_PER_IP_INBOUND: usize = 4;
const KNOWN_CAPACITY: usize = 8192;
const WRITE_QUEUE: usize = 4096;
/// A peer whose unsent data exceeds this is disconnected.
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;
/// Bulk senders (block serving) wait while more than this is queued.
const SOFT_QUEUED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct SyncState {
    pub peer: Option<u64>,
    pub last_progress: Option<Instant>,
    /// Last block hash requested from the sync peer.
    pub last_request: Option<Hash32>,
    /// When the sync peer last delivered a block that extended our chain.
    pub last_block: Option<Instant>,
}

struct KnownSet {
    set: HashSet<Hash32>,
    order: VecDeque<Hash32>,
}

impl KnownSet {
    fn insert(&mut self, h: Hash32) {
        if self.set.insert(h) {
            self.order.push_back(h);
            if self.order.len() > KNOWN_CAPACITY {
                if let Some(old) = self.order.pop_front() {
                    self.set.remove(&old);
                }
            }
        }
    }
}

pub struct PeerInfo {
    pub version: Option<VersionMsg>,
    pub got_verack: bool,
    pub misbehavior: u32,
    pub last_pong: Instant,
    pub ping_nonce: u64,
    pub last_getblocks: Option<Instant>,
    /// Compact blocks waiting for missing transactions.
    pub pending_compact: Vec<PendingCompact>,
}

pub struct PendingCompact {
    pub hash: Hash32,
    pub header: thecoin_core::BlockHeader,
    pub slots: Vec<Option<thecoin_core::Transaction>>,
}

const MAX_PENDING_COMPACT: usize = 8;

pub struct Peer {
    pub id: u64,
    pub addr: SocketAddr,
    pub inbound: bool,
    pub connected_at: Instant,
    frames: mpsc::Sender<Arc<Vec<u8>>>,
    magic: [u8; 4],
    pub info: crate::chain::NodeMutex<PeerInfo>,
    known: crate::chain::NodeMutex<KnownSet>,
    best_height: AtomicU64,
    closing: AtomicBool,
    queued_bytes: std::sync::atomic::AtomicUsize,
    close_notify: Notify,
}

impl Peer {
    pub fn send(&self, msg: &Message) {
        match encode_frame(self.magic, msg) {
            Ok(frame) => {
                let len = frame.len();
                if self.queued_bytes.fetch_add(len, Ordering::SeqCst) + len > MAX_QUEUED_BYTES
                    || self.frames.try_send(Arc::new(frame)).is_err()
                {
                    debug!(peer = self.id, "write queue full; disconnecting slow peer");
                    self.close();
                }
            }
            Err(e) => warn!(error = %e, "failed to encode message"),
        }
    }

    /// Like [`Peer::send`] but waits (blocking, up to 60 s) while the peer has a
    /// large backlog. Use only from blocking threads. Returns false if the peer
    /// was disconnected.
    pub fn send_bulk(&self, msg: &Message) -> bool {
        let start = Instant::now();
        while self.queued_bytes.load(Ordering::SeqCst) > SOFT_QUEUED_BYTES {
            if self.closing.load(Ordering::Relaxed) {
                return false;
            }
            if start.elapsed() > Duration::from_secs(60) {
                debug!(peer = self.id, "peer not reading; disconnecting");
                self.close();
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.send(msg);
        !self.closing.load(Ordering::Relaxed)
    }

    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.close_notify.notify_waiters();
    }

    pub fn is_ready(&self) -> bool {
        let i = self.info.lock();
        i.version.is_some() && i.got_verack
    }

    pub fn knows(&self, h: &Hash32) -> bool {
        self.known.lock().set.contains(h)
    }

    pub fn mark_known(&self, h: Hash32) {
        self.known.lock().insert(h);
    }

    pub fn best_height(&self) -> u64 {
        self.best_height.load(Ordering::Relaxed)
    }

    pub fn update_best_height(&self, h: u64) {
        self.best_height.fetch_max(h, Ordering::Relaxed);
    }

    pub fn user_agent(&self) -> String {
        self.info.lock().version.as_ref().map(|v| v.user_agent.clone()).unwrap_or_default()
    }
}

/// Starts the listener, the outbound connection manager and maintenance timers.
pub async fn start(node: Arc<Node>) -> Result<()> {
    if let Some(listen) = node.config.p2p_listen()? {
        let listener = TcpListener::bind(listen).await.map_err(|e| anyhow::anyhow!("cannot bind P2P port {listen}: {e}"))?;
        let local = listener.local_addr()?;
        let _ = node.p2p_addr.set(local);
        let n = node.clone();
        tokio::spawn(async move { accept_loop(n, listener).await });
    }
    let n = node.clone();
    tokio::spawn(async move { outbound_loop(n).await });
    let n = node.clone();
    tokio::spawn(async move { maintenance_loop(n).await });
    Ok(())
}

async fn accept_loop(node: Arc<Node>, listener: TcpListener) {
    let mut shutdown = node.shutdown.subscribe();
    loop {
        let accepted = tokio::select! {
            r = listener.accept() => r,
            _ = shutdown.changed() => return,
        };
        let (stream, addr) = match accepted {
            Ok(x) => x,
            Err(e) => {
                warn!(error = %e, "accept failed");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        if is_banned(&node, &addr) {
            continue;
        }
        let (inbound, same_ip) = {
            let peers = node.peers.read();
            (peers.values().filter(|p| p.inbound).count(), peers.values().filter(|p| p.inbound && p.addr.ip() == addr.ip()).count())
        };
        let local_ip = !is_routable(&addr.ip());
        if inbound >= node.config.p2p.max_inbound || (same_ip >= MAX_PER_IP_INBOUND && !local_ip) {
            debug!(%addr, "inbound limit reached; dropping connection");
            continue;
        }
        let n = node.clone();
        tokio::spawn(async move {
            if let Err(e) = run_peer(n, stream, addr, true).await {
                debug!(%addr, error = %e, "inbound peer disconnected");
            }
        });
    }
}

fn is_banned(node: &Node, addr: &SocketAddr) -> bool {
    let mut bans = node.bans.lock();
    if let Some(until) = bans.get(&addr.ip()) {
        if Instant::now() < *until {
            return true;
        }
        bans.remove(&addr.ip());
    }
    false
}

async fn resolve(host: &str, default_port: u16) -> Vec<SocketAddr> {
    // "host:port", "1.2.3.4:port" and "[::1]:port" are used as-is; bare hosts get the default port.
    let has_port = host.parse::<SocketAddr>().is_ok() || (host.matches(':').count() == 1 && !host.starts_with('['));
    let target = if has_port { host.to_string() } else { format!("{host}:{default_port}") };
    match tokio::time::timeout(Duration::from_secs(10), tokio::net::lookup_host(target)).await {
        Ok(Ok(it)) => it.collect(),
        _ => vec![],
    }
}

async fn outbound_loop(node: Arc<Node>) {
    let mut shutdown = node.shutdown.subscribe();
    let port = node.params.default_p2p_port;
    let mut last_seed_lookup: Option<Instant> = None;
    loop {
        let connected: HashSet<SocketAddr> = node.peers.read().values().map(|p| p.addr).collect();
        let outbound = node.peers.read().values().filter(|p| !p.inbound).count();
        let fixed = &node.config.p2p.connect;

        let mut targets: Vec<SocketAddr> = Vec::new();
        if !fixed.is_empty() {
            for host in fixed {
                for a in resolve(host, port).await.into_iter().take(1) {
                    if !connected.contains(&a) {
                        targets.push(a);
                    }
                }
            }
        } else if outbound < node.config.p2p.max_outbound {
            let need_seeds =
                node.addrman.lock().is_empty() || (outbound == 0 && last_seed_lookup.is_none_or(|t| t.elapsed() > Duration::from_secs(60)));
            if need_seeds {
                last_seed_lookup = Some(Instant::now());
                let seeds: Vec<String> =
                    node.params.seeds.iter().map(|s| s.to_string()).chain(node.config.p2p.seeds.iter().cloned()).collect();
                for s in seeds {
                    for a in resolve(&s, port).await {
                        node.addrman.lock().add_trusted(a, now_secs());
                    }
                }
            }
            let mut exclude = connected.clone();
            if let Some(own) = node.p2p_addr.get() {
                exclude.insert(*own);
            }
            targets = node.addrman.lock().pick(node.config.p2p.max_outbound - outbound, &exclude, now_secs());
        }

        for addr in targets {
            if is_banned(&node, &addr) {
                continue;
            }
            node.addrman.lock().mark_try(&addr, now_secs());
            let n = node.clone();
            tokio::spawn(async move {
                match tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(addr)).await {
                    Ok(Ok(stream)) => {
                        if let Err(e) = run_peer(n.clone(), stream, addr, false).await {
                            debug!(%addr, error = %e, "outbound peer disconnected");
                        }
                    }
                    _ => {
                        n.addrman.lock().mark_failure(&addr);
                    }
                }
            });
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            _ = shutdown.changed() => return,
        }
    }
}

async fn maintenance_loop(node: Arc<Node>) {
    let mut shutdown = node.shutdown.subscribe();
    let mut ticks = 0u64;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            _ = shutdown.changed() => return,
        }
        ticks += 1;
        // Sync stall detection.
        let stalled = {
            let mut s = node.sync.lock();
            match (s.peer, s.last_progress) {
                (Some(p), Some(t)) if t.elapsed() > SYNC_STALL => {
                    *s = SyncState::default();
                    Some(p)
                }
                _ => None,
            }
        };
        if let Some(p) = stalled {
            info!(peer = p, "sync peer stalled; switching");
            // Claiming a higher chain and not delivering it wastes our time.
            misbehave(&node, p, 50, "stalled block sync");
            if let Some(peer) = node.peers.read().get(&p) {
                peer.close();
            }
        }
        maybe_start_sync(&node);
        if ticks.is_multiple_of(30) {
            node.addrman.lock().save();
        }
    }
}

/// Starts block download from the best peer if one is ahead of us.
pub fn maybe_start_sync(node: &Arc<Node>) {
    let our_height = node.chain.tip().height;
    let mut sync = node.sync.lock();
    if sync.peer.is_some() {
        return;
    }
    let best = node.peers.read().values().filter(|p| p.is_ready() && p.best_height() > our_height).max_by_key(|p| p.best_height()).cloned();
    if let Some(peer) = best {
        let Ok(locator) = node.chain.locator() else { return };
        info!(peer = peer.id, addr = %peer.addr, from = our_height, to = peer.best_height(), "starting block sync");
        sync.peer = Some(peer.id);
        sync.last_progress = Some(Instant::now());
        sync.last_request = None;
        peer.send(&Message::GetBlocks { locator, stop: Hash32::ZERO });
    }
}

/// Called by the block processor after each block.
pub fn on_block_processed(node: &Arc<Node>, _source: Option<u64>, _hash: &Hash32) {
    let (peer_id, req) = {
        let s = node.sync.lock();
        (s.peer, s.last_request)
    };
    let (Some(peer_id), Some(req)) = (peer_id, req) else { return };
    let known = node.chain.read().ok().and_then(|r| r.header(&req).ok().flatten()).is_some();
    if !known {
        node.sync.lock().last_progress = Some(Instant::now());
        return;
    }
    let peer = node.peers.read().get(&peer_id).cloned();
    let Some(peer) = peer else {
        *node.sync.lock() = SyncState::default();
        return;
    };
    let Ok(mut locator) = node.chain.locator() else { return };
    locator.insert(0, req);
    locator.truncate(MAX_LOCATOR);
    {
        let mut s = node.sync.lock();
        s.last_request = None;
        s.last_progress = Some(Instant::now());
    }
    peer.send(&Message::GetBlocks { locator, stop: Hash32::ZERO });
}

/// Asks a peer for the blocks we are missing (after receiving an orphan).
pub fn request_missing_blocks(node: &Arc<Node>, peer_id: u64, height: u64) {
    let Some(peer) = node.peers.read().get(&peer_id).cloned() else { return };
    peer.update_best_height(height);
    {
        let mut info = peer.info.lock();
        if info.last_getblocks.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        info.last_getblocks = Some(Instant::now());
    }
    if let Ok(locator) = node.chain.locator() {
        peer.send(&Message::GetBlocks { locator, stop: Hash32::ZERO });
    }
    maybe_start_sync(node);
}

pub fn misbehave(node: &Arc<Node>, peer_id: u64, score: u32, reason: &str) {
    let Some(peer) = node.peers.read().get(&peer_id).cloned() else { return };
    let total = {
        let mut i = peer.info.lock();
        i.misbehavior += score;
        i.misbehavior
    };
    warn!(peer = peer_id, addr = %peer.addr, score = total, reason, "peer misbehaving");
    if total >= 100 {
        if is_routable(&peer.addr.ip()) {
            node.bans.lock().insert(peer.addr.ip(), Instant::now() + BAN_DURATION);
        }
        peer.close();
    }
}

fn version_msg(node: &Node) -> Message {
    let tip = node.chain.tip();
    let mut services = SERVICE_INDEX;
    if node.chain.options().prune_keep.is_none() {
        services |= SERVICE_ARCHIVE;
    }
    Message::Version(VersionMsg {
        protocol: PROTOCOL_VERSION,
        genesis: node.chain.genesis_hash(),
        height: tip.height,
        tip: tip.hash,
        services,
        user_agent: format!("/thecoind:{}/", thecoin_core::VERSION),
        listen_port: node.p2p_addr.get().map(|a| a.port()).unwrap_or(0),
        nonce: node.local_nonce,
        timestamp: now_secs(),
    })
}

async fn run_peer(node: Arc<Node>, stream: TcpStream, addr: SocketAddr, inbound: bool) -> Result<()> {
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let (frame_tx, mut frame_rx) = mpsc::channel::<Arc<Vec<u8>>>(WRITE_QUEUE);
    let id = node.next_peer_id.fetch_add(1, Ordering::Relaxed);
    let peer = Arc::new(Peer {
        id,
        addr,
        inbound,
        connected_at: Instant::now(),
        frames: frame_tx,
        magic: node.params.magic,
        info: crate::chain::NodeMutex::new(PeerInfo {
            version: None,
            got_verack: false,
            misbehavior: 0,
            last_pong: Instant::now(),
            ping_nonce: 0,
            last_getblocks: None,
            pending_compact: Vec::new(),
        }),
        known: crate::chain::NodeMutex::new(KnownSet { set: HashSet::new(), order: VecDeque::new() }),
        best_height: AtomicU64::new(0),
        closing: AtomicBool::new(false),
        queued_bytes: std::sync::atomic::AtomicUsize::new(0),
        close_notify: Notify::new(),
    });

    // Writer task.
    let writer_peer = peer.clone();
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = frame_rx.recv().await {
            let ok = tokio::time::timeout(Duration::from_secs(60), writer.write_all(&frame)).await.is_ok_and(|r| r.is_ok());
            writer_peer.queued_bytes.fetch_sub(frame.len(), Ordering::SeqCst);
            if !ok {
                writer_peer.close();
                break;
            }
        }
        let _ = writer.shutdown().await;
    });

    // Reader task.
    let (msg_tx, mut msg_rx) = mpsc::channel::<Message>(64);
    let magic = node.params.magic;
    let reader_peer = peer.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            match read_message(&mut reader, magic).await {
                Ok(m) => {
                    if msg_tx.send(m).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    debug!(peer = reader_peer.id, error = %e, "read error");
                    reader_peer.close();
                    break;
                }
            }
        }
    });

    // Block verification pipeline.
    let (block_tx, block_rx) = mpsc::channel::<Block>(600);
    let pipeline_task = tokio::spawn(block_pipeline(node.clone(), id, block_rx));

    peer.send(&version_msg(&node));
    let result = peer_loop(&node, &peer, &mut msg_rx, &block_tx).await;

    // Cleanup.
    peer.close();
    node.peers.write().remove(&id);
    reader_task.abort();
    writer_task.abort();
    drop(block_tx);
    let _ = pipeline_task.await;
    {
        let mut s = node.sync.lock();
        if s.peer == Some(id) {
            *s = SyncState::default();
        }
    }
    if peer.is_ready() {
        info!(peer = id, %addr, inbound, "peer disconnected");
    }
    maybe_start_sync(&node);
    result
}

async fn peer_loop(node: &Arc<Node>, peer: &Arc<Peer>, msg_rx: &mut mpsc::Receiver<Message>, block_tx: &mpsc::Sender<Block>) -> Result<()> {
    // Handshake: the first message must be Version.
    let first = tokio::time::timeout(HANDSHAKE_TIMEOUT, msg_rx.recv()).await.map_err(|_| anyhow::anyhow!("handshake timeout"))?;
    let Some(Message::Version(v)) = first else { bail!("expected version message") };
    if v.nonce == node.local_nonce {
        if !peer.inbound {
            node.addrman.lock().mark_failure(&peer.addr);
        }
        bail!("connected to self");
    }
    if v.genesis != node.chain.genesis_hash() {
        bail!("peer is on a different network (genesis {})", v.genesis);
    }
    if v.protocol < PROTOCOL_VERSION {
        bail!("peer protocol {} too old", v.protocol);
    }
    // The same remote node may be reachable twice (we dialed it and it dialed us).
    let duplicate = node.peers.read().values().any(|p| p.info.lock().version.as_ref().is_some_and(|pv| pv.nonce == v.nonce));
    if duplicate {
        bail!("already connected to this node");
    }
    peer.update_best_height(v.height);
    let listen_port = v.listen_port;
    peer.info.lock().version = Some(v);
    peer.send(&Message::Verack);

    let verack = tokio::time::timeout(HANDSHAKE_TIMEOUT, msg_rx.recv()).await.map_err(|_| anyhow::anyhow!("handshake timeout"))?;
    if !matches!(verack, Some(Message::Verack)) {
        bail!("expected verack");
    }
    peer.info.lock().got_verack = true;
    // Register only after a successful handshake.
    node.peers.write().insert(peer.id, peer.clone());
    if peer.inbound {
        if listen_port != 0 {
            node.addrman.lock().add(SocketAddr::new(peer.addr.ip(), listen_port), now_secs());
        }
    } else {
        node.addrman.lock().mark_success(&peer.addr, now_secs());
        peer.send(&Message::GetAddr);
    }
    peer.send(&Message::GetMempool);
    info!(peer = peer.id, addr = %peer.addr, inbound = peer.inbound, height = peer.best_height(), agent = %peer.user_agent(), "peer connected");
    maybe_start_sync(node);

    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    let mut shutdown = node.shutdown.subscribe();
    loop {
        if peer.closing.load(Ordering::Relaxed) {
            return Ok(());
        }
        tokio::select! {
            m = msg_rx.recv() => {
                let Some(m) = m else { return Ok(()) };
                if let Err(e) = handle_message(node, peer, m, block_tx).await {
                    misbehave(node, peer.id, 20, &e.to_string());
                }
            }
            _ = ping.tick() => {
                let (nonce, last_pong) = {
                    let mut i = peer.info.lock();
                    i.ping_nonce = rand::random();
                    (i.ping_nonce, i.last_pong)
                };
                if last_pong.elapsed() > PONG_TIMEOUT {
                    bail!("ping timeout");
                }
                peer.send(&Message::Ping(nonce));
            }
            _ = peer.close_notify.notified() => return Ok(()),
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

async fn handle_message(node: &Arc<Node>, peer: &Arc<Peer>, msg: Message, block_tx: &mpsc::Sender<Block>) -> Result<()> {
    match msg {
        Message::Version(_) | Message::Verack => bail!("duplicate handshake message"),
        Message::Ping(n) => peer.send(&Message::Pong(n)),
        Message::Pong(n) => {
            let mut i = peer.info.lock();
            if i.ping_nonce == n {
                i.last_pong = Instant::now();
            }
        }
        Message::GetAddr => {
            let addrs: Vec<NetAddr> = node.addrman.lock().sample(MAX_ADDRS, now_secs()).iter().map(NetAddr::from_socket).collect();
            peer.send(&Message::Addr(addrs));
        }
        Message::Addr(list) => {
            let now = now_secs();
            let mut am = node.addrman.lock();
            for a in list {
                am.add(a.to_socket(), now);
            }
        }
        Message::Inv(items) => handle_inv(node, peer, items).await?,
        Message::GetData(items) => {
            let node2 = node.clone();
            let peer2 = peer.clone();
            tokio::task::spawn_blocking(move || serve_getdata(&node2, &peer2, items)).await??;
        }
        Message::NotFound(_) => {}
        Message::GetBlocks { locator, stop } => {
            let node2 = node.clone();
            let hashes =
                tokio::task::spawn_blocking(move || node2.chain.hashes_after_locator(&locator, &stop, MAX_BLOCKS_PER_INV)).await??;
            for h in &hashes {
                peer.mark_known(*h);
            }
            peer.send(&Message::Inv(hashes.into_iter().map(|hash| InvItem { kind: InvKind::Block, hash }).collect()));
        }
        Message::Block(block) => {
            peer.mark_known(block.hash());
            if block_tx.send(*block).await.is_err() {
                bail!("block pipeline closed");
            }
        }
        Message::Tx(tx) => {
            let txid = tx.txid();
            peer.mark_known(txid);
            let node2 = node.clone();
            let id = peer.id;
            let res = tokio::task::spawn_blocking(move || node2.submit_tx(*tx, Some(id))).await?;
            if let Err(crate::mempool::MempoolError::Invalid(thecoin_core::TxError::BadSignature)) = res {
                misbehave(node, peer.id, 10, "transaction with bad signature");
            }
        }
        Message::CompactBlock(c) => {
            let hash = c.header.hash();
            peer.mark_known(hash);
            // Validate the header (context + proof of work) before allocating
            // anything for its transactions: fake compact blocks cost real work.
            let node2 = node.clone();
            let header = c.header.clone();
            let check = tokio::task::spawn_blocking(move || -> Result<Option<RelayedHeader>> {
                let status = node2.chain.check_relayed_header(&header)?;
                if !matches!(status, RelayedHeader::Valid) {
                    return Ok(Some(status));
                }
                let pow = node2.params.pow;
                let ok = POW_HASHER.with(|cell| {
                    let mut slot = cell.borrow_mut();
                    header.check_pow(slot.get_or_insert_with(|| PowHasher::new(pow)))
                });
                Ok(if ok { None } else { Some(RelayedHeader::Invalid(thecoin_core::BlockError::BadPow)) })
            })
            .await??;
            match check {
                None => {}
                Some(RelayedHeader::Known) => return Ok(()),
                Some(RelayedHeader::UnknownParent) => {
                    // Out of order: fetch the full block and let orphan handling work.
                    peer.send(&Message::GetData(vec![InvItem { kind: InvKind::Block, hash }]));
                    return Ok(());
                }
                Some(RelayedHeader::Invalid(e)) => {
                    // A timestamp slightly in the future may be clock skew: ignore without penalty.
                    if !matches!(e, thecoin_core::BlockError::TimestampInFuture { .. }) {
                        misbehave(node, peer.id, 100, &format!("invalid compact block header: {e}"));
                    }
                    return Ok(());
                }
                Some(RelayedHeader::Valid) => unreachable!("valid headers are returned as None"),
            }
            let n = c.short_ids.len();
            let mut slots: Vec<Option<thecoin_core::Transaction>> = vec![None; n];
            for (i, tx) in c.prefilled {
                if (i as usize) < n {
                    slots[i as usize] = Some(tx);
                }
            }
            node.mempool.lock().fill_compact(&hash, &c.short_ids, &mut slots);
            let missing: Vec<u32> = slots.iter().enumerate().filter(|(_, s)| s.is_none()).map(|(i, _)| i as u32).collect();
            if missing.is_empty() {
                finish_compact(node, peer, c.header, slots, block_tx).await?;
            } else {
                {
                    let mut info = peer.info.lock();
                    if info.pending_compact.len() >= MAX_PENDING_COMPACT {
                        info.pending_compact.remove(0);
                    }
                    info.pending_compact.push(PendingCompact { hash, header: c.header, slots });
                }
                peer.send(&Message::GetBlockTxs { block: hash, indexes: missing });
            }
        }
        Message::GetBlockTxs { block, indexes } => {
            let node2 = node.clone();
            let found = tokio::task::spawn_blocking(move || -> Result<Option<Vec<thecoin_core::Transaction>>> {
                node2.chain.read()?.block_txs(&block)
            })
            .await??;
            match found {
                Some(txs) => {
                    let mut out = Vec::with_capacity(indexes.len());
                    for i in indexes {
                        match txs.get(i as usize) {
                            Some(t) => out.push(t.clone()),
                            None => bail!("getblocktxs index out of range"),
                        }
                    }
                    peer.send(&Message::BlockTxs { block, txs: out });
                }
                None => peer.send(&Message::NotFound(vec![InvItem { kind: InvKind::Block, hash: block }])),
            }
        }
        Message::BlockTxs { block, txs } => {
            let pending = {
                let mut info = peer.info.lock();
                info.pending_compact.iter().position(|p| p.hash == block).map(|i| info.pending_compact.remove(i))
            };
            let Some(mut pending) = pending else { return Ok(()) };
            let mut it = txs.into_iter();
            for slot in pending.slots.iter_mut() {
                if slot.is_none() {
                    *slot = it.next();
                }
            }
            if it.next().is_some() || pending.slots.iter().any(|s| s.is_none()) {
                // Mismatch: fall back to downloading the full block.
                peer.send(&Message::GetData(vec![InvItem { kind: InvKind::Block, hash: block }]));
                return Ok(());
            }
            finish_compact(node, peer, pending.header, pending.slots, block_tx).await?;
        }
        Message::DoubleSpend { first, second } => {
            let valid = first.sender() == second.sender()
                && first.body.nonce == second.body.nonce
                && first.txid() != second.txid()
                && thecoin_core::execution::check_tx_stateless(node.params, &first).is_ok()
                && thecoin_core::execution::check_tx_stateless(node.params, &second).is_ok();
            if !valid {
                bail!("invalid double-spend report");
            }
            // Only a funded account's nonce that can still be spent is worth an
            // alert (fresh keys cost nothing, so they could flood the network).
            let node2 = node.clone();
            let sender = first.sender();
            let account = tokio::task::spawn_blocking(move || -> Result<Option<thecoin_core::state::Account>> {
                let r = node2.chain.read()?;
                Ok(thecoin_core::state::read_typed::<thecoin_core::state::Account, _>(&r, &thecoin_core::state::account_key(&sender))?)
            })
            .await??;
            let Some(account) = account else { return Ok(()) };
            let nonce = first.body.nonce;
            let min_fee = first.body.fee.min(second.body.fee);
            if account.balance < min_fee || nonce < account.nonce || nonce >= account.nonce + crate::mempool::MAX_PER_SENDER as u64 {
                return Ok(());
            }
            let new = node.mempool.lock().record_conflict(&first, &second);
            if new {
                warn!(sender = %first.sender().encode(node.params.network), nonce = first.body.nonce, "double spend attempt reported by peer");
                node.broadcast_double_spend(&first, &second, Some(peer.id));
            }
        }
        Message::GetMempool => {
            let ids = node.mempool.lock().txids(MAX_INV_ITEMS);
            if !ids.is_empty() {
                for id in &ids {
                    peer.mark_known(*id);
                }
                peer.send(&Message::Inv(ids.into_iter().map(|hash| InvItem { kind: InvKind::Tx, hash }).collect()));
            }
        }
    }
    Ok(())
}

/// Completes a reconstructed compact block and sends it to the verification pipeline.
async fn finish_compact(
    _node: &Arc<Node>,
    peer: &Arc<Peer>,
    header: thecoin_core::BlockHeader,
    slots: Vec<Option<thecoin_core::Transaction>>,
    block_tx: &mpsc::Sender<Block>,
) -> Result<()> {
    let txs: Vec<thecoin_core::Transaction> = slots.into_iter().map(|s| s.expect("all slots filled")).collect();
    let block = Block { header, txs };
    if block.compute_tx_root() != block.header.tx_root {
        // Short-id collision or wrong transactions: download the full block.
        peer.send(&Message::GetData(vec![InvItem { kind: InvKind::Block, hash: block.hash() }]));
        return Ok(());
    }
    if block_tx.send(block).await.is_err() {
        bail!("block pipeline closed");
    }
    Ok(())
}

async fn handle_inv(node: &Arc<Node>, peer: &Arc<Peer>, items: Vec<InvItem>) -> Result<()> {
    let is_sync_peer = node.sync.lock().peer == Some(peer.id);
    let block_hashes: Vec<Hash32> = items.iter().filter(|i| i.kind == InvKind::Block).map(|i| i.hash).collect();
    let tx_hashes: Vec<Hash32> = items.iter().filter(|i| i.kind == InvKind::Tx).map(|i| i.hash).collect();

    if is_sync_peer && block_hashes.is_empty() && tx_hashes.is_empty() {
        // The sync peer has nothing more for us.
        info!(peer = peer.id, height = node.chain.tip().height, "block sync finished");
        *node.sync.lock() = SyncState::default();
        return Ok(());
    }

    let mut want = Vec::new();
    if !block_hashes.is_empty() {
        let node2 = node.clone();
        let hashes = block_hashes.clone();
        let unknown: Vec<Hash32> = tokio::task::spawn_blocking(move || -> Result<Vec<Hash32>> {
            let r = node2.chain.read()?;
            let mut out = Vec::new();
            for h in hashes {
                if r.header(&h)?.is_none() {
                    out.push(h);
                }
            }
            Ok(out)
        })
        .await??;
        for h in &block_hashes {
            peer.mark_known(*h);
        }
        if is_sync_peer {
            let mut s = node.sync.lock();
            s.last_request = block_hashes.last().copied();
            s.last_progress = Some(Instant::now());
        }
        if unknown.is_empty() && is_sync_peer {
            on_block_processed(node, Some(peer.id), &Hash32::ZERO);
        }
        want.extend(unknown.into_iter().map(|hash| InvItem { kind: InvKind::Block, hash }));
    }
    if !tx_hashes.is_empty() && !node.is_syncing() {
        let pool = node.mempool.lock();
        for h in tx_hashes {
            peer.mark_known(h);
            if !pool.contains(&h) {
                want.push(InvItem { kind: InvKind::Tx, hash: h });
            }
        }
    }
    if !want.is_empty() {
        peer.send(&Message::GetData(want));
    }
    Ok(())
}

fn serve_getdata(node: &Arc<Node>, peer: &Arc<Peer>, items: Vec<InvItem>) -> Result<()> {
    let r = node.chain.read()?;
    let mut not_found = Vec::new();
    for item in items {
        if peer.closing.load(Ordering::Relaxed) {
            break;
        }
        match item.kind {
            InvKind::Block => match r.block(&item.hash)? {
                Some(b) => {
                    if !peer.send_bulk(&Message::Block(Box::new(b))) {
                        return Ok(());
                    }
                }
                None => not_found.push(item),
            },
            InvKind::Tx => {
                let tx = node.mempool.lock().get(&item.hash).cloned();
                match tx {
                    Some(tx) => peer.send(&Message::Tx(Box::new(tx))),
                    None => not_found.push(item),
                }
            }
        }
    }
    if !not_found.is_empty() {
        peer.send(&Message::NotFound(not_found));
    }
    Ok(())
}

thread_local! {
    static POW_HASHER: std::cell::RefCell<Option<PowHasher>> = const { std::cell::RefCell::new(None) };
}

/// Verifies CoinHash in parallel (bounded) while preserving block order.
async fn block_pipeline(node: Arc<Node>, peer_id: u64, mut rx: mpsc::Receiver<Block>) {
    let parallel = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let mut pending: VecDeque<tokio::task::JoinHandle<(Block, bool)>> = VecDeque::new();
    loop {
        let next = if pending.is_empty() {
            rx.recv().await
        } else {
            match tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
                Ok(m) => m,
                Err(_) => {
                    // Idle: flush everything that is queued.
                    while let Some(h) = pending.pop_front() {
                        if !forward(&node, peer_id, h).await {
                            return;
                        }
                    }
                    continue;
                }
            }
        };
        let Some(block) = next else { break };
        let pow = node.params.pow;
        pending.push_back(tokio::task::spawn_blocking(move || {
            let ok = POW_HASHER.with(|cell| {
                let mut slot = cell.borrow_mut();
                let hasher = slot.get_or_insert_with(|| PowHasher::new(pow));
                block.header.check_pow(hasher)
            });
            (block, ok)
        }));
        while pending.len() > parallel || pending.front().is_some_and(|h| h.is_finished()) {
            let h = pending.pop_front().expect("non-empty");
            if !forward(&node, peer_id, h).await {
                return;
            }
        }
    }
    while let Some(h) = pending.pop_front() {
        if !forward(&node, peer_id, h).await {
            return;
        }
    }
}

async fn forward(node: &Arc<Node>, peer_id: u64, h: tokio::task::JoinHandle<(Block, bool)>) -> bool {
    let Ok((block, pow_ok)) = h.await else { return false };
    if !pow_ok {
        misbehave(node, peer_id, 100, "block with invalid proof of work");
        return false;
    }
    node.block_queue.send(BlockJob { block, source: Some(peer_id), pow_checked: true }).await.is_ok()
}
