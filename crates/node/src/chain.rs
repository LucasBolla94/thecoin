//! Chain manager: header/body validation, fork choice (most cumulative work),
//! reorganizations, pruning and block-template construction.
//!
//! All mutations go through a single mutex, and every block is written in one
//! atomic database transaction together with its state changes, undo data,
//! indexes and the new state commitment.

use anyhow::{anyhow, Context, Result};
use locks::{Mutex, RwLock};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use thecoin_core::block::{Block, BlockHeader};
use thecoin_core::difficulty::{self, BlockTimeInfo};
use thecoin_core::error::BlockError;
use thecoin_core::execution::{apply_block, BlockBuilder};
use thecoin_core::genesis::{genesis_block, genesis_lthash, genesis_state};
use thecoin_core::hash::Hash32;
use thecoin_core::lthash::LtHash;
use thecoin_core::merkle::merkle_root;
use thecoin_core::params::{ChainParams, BLOCK_VERSION, MAX_BLOCK_BYTES_HARD, MTP_WINDOW};
use thecoin_core::pow::{u256_from_bytes, u256_to_bytes, work_for_target, PowHasher};
use thecoin_core::state::{apply_changes_to_lthash, read_typed, revert_changes_from_lthash, ChainGlobal, Overlay, StateError};
use thecoin_core::{Address, Transaction, U256};
use thecoin_storage::{BlockStatus, BlockUndo, ChainDb, DbOptions, DbRead, HeaderRecord, ReadTx, WriteTx};
use tracing::{info, warn};

/// Thin wrappers over std locks that ignore poisoning (a panicked thread must
/// not make the whole node unusable).
mod locks {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub fn new(v: T) -> Self {
            Mutex(std::sync::Mutex::new(v))
        }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap_or_else(|e| e.into_inner())
        }
    }
    pub struct RwLock<T>(std::sync::RwLock<T>);
    impl<T> RwLock<T> {
        pub fn new(v: T) -> Self {
            RwLock(std::sync::RwLock::new(v))
        }
        pub fn read(&self) -> std::sync::RwLockReadGuard<'_, T> {
            self.0.read().unwrap_or_else(|e| e.into_inner())
        }
        pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, T> {
            self.0.write().unwrap_or_else(|e| e.into_inner())
        }
    }
}
pub use locks::{Mutex as NodeMutex, RwLock as NodeRwLock};

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct TipInfo {
    pub hash: Hash32,
    pub height: u64,
    pub chainwork: U256,
    pub header: BlockHeader,
}

#[derive(Clone, Debug)]
pub struct ChainOptions {
    /// Keep only the last `n` block bodies (None = archive node).
    pub prune_keep: Option<u64>,
    /// Maintain the address → transactions index (explorers, wallets).
    pub address_index: bool,
    pub db: DbOptions,
}

impl Default for ChainOptions {
    fn default() -> Self {
        ChainOptions { prune_keep: None, address_index: true, db: DbOptions::default() }
    }
}

/// Result of submitting a block.
#[derive(Debug)]
pub enum ProcessResult {
    AlreadyKnown,
    /// Parent unknown; the block was kept in the orphan pool.
    Orphan,
    /// Valid-looking block stored on a side chain with less work.
    SideChain,
    /// The active chain changed.
    NewTip {
        disconnected: Vec<Block>,
        connected: Vec<Block>,
    },
    Invalid(BlockError),
}

/// Minimum bodies kept by pruned nodes (must cover the maximum reorg depth).
pub const MIN_PRUNE_KEEP: u64 = 1_000;
const UNDO_MARGIN: u64 = 16;
const MAX_ORPHANS: usize = 256;
/// Memory budget of the orphan pool (small machines must not be exhausted).
const MAX_ORPHAN_BYTES: usize = 16 * 1024 * 1024;
const MAX_ORPHAN_BLOCK_BYTES: usize = 2 * 1024 * 1024;

struct Inner {
    lthash: LtHash,
    hasher: PowHasher,
    /// Blocks whose parent is unknown, with their serialized size.
    orphans: Vec<(Block, usize)>,
}

pub struct Chain {
    pub params: &'static ChainParams,
    db: ChainDb,
    opts: ChainOptions,
    inner: Mutex<Inner>,
    tip: RwLock<TipInfo>,
}

/// Result of [`Chain::check_relayed_header`].
#[derive(Debug)]
pub enum RelayedHeader {
    Known,
    UnknownParent,
    Invalid(BlockError),
    /// Context-valid (proof of work still to be checked).
    Valid,
}

enum Fail {
    Invalid(BlockError),
    Storage(anyhow::Error),
}

impl From<anyhow::Error> for Fail {
    fn from(e: anyhow::Error) -> Self {
        Fail::Storage(e)
    }
}

impl From<BlockError> for Fail {
    fn from(e: BlockError) -> Self {
        match e {
            BlockError::State(s) => Fail::Storage(anyhow!(s.0)),
            other => Fail::Invalid(other),
        }
    }
}

impl From<StateError> for Fail {
    fn from(e: StateError) -> Self {
        Fail::Storage(anyhow!(e.0))
    }
}

impl Chain {
    pub fn open(params: &'static ChainParams, path: &Path, mut opts: ChainOptions) -> Result<Chain> {
        if let Some(k) = opts.prune_keep {
            opts.prune_keep = Some(k.max(MIN_PRUNE_KEEP));
            // The address history index would point to pruned bodies; pruned
            // nodes do not maintain it (explorers use archive nodes).
            opts.address_index = false;
        }
        let db = ChainDb::open(path, &opts.db)?;
        let genesis = genesis_block(params);
        let ghash = genesis.hash();
        {
            let r = db.read()?;
            if r.tip()?.is_none() {
                drop(r);
                let w = db.write()?;
                let work = work_for_target(&genesis.header.target_u256());
                w.put_header(
                    &ghash,
                    &HeaderRecord {
                        header: genesis.header.clone(),
                        chainwork: u256_to_bytes(&work),
                        status: BlockStatus::Valid,
                        has_body: true,
                        tx_count: 0,
                        size: genesis.serialized_size() as u32,
                    },
                )?;
                w.put_block_txs(&ghash, &genesis.txs)?;
                w.set_main(0, &ghash)?;
                for (k, v) in genesis_state(params) {
                    w.put_state_raw(&k, &v)?;
                }
                w.set_lthash(&genesis_lthash(params))?;
                w.set_tip(&ghash)?;
                w.set_meta(thecoin_storage::META_GENESIS, &ghash.0)?;
                w.commit()?;
                info!(network = %params.network, genesis = %ghash, "initialized new chain database");
            } else {
                let stored = r.get_meta(thecoin_storage::META_GENESIS)?;
                if stored.as_deref() != Some(ghash.0.as_slice()) {
                    return Err(anyhow!("database at {} belongs to a different network/genesis", path.display()));
                }
            }
        }
        let r = db.read()?;
        let tip_hash = r.tip()?.context("missing tip")?;
        let rec = r.header(&tip_hash)?.context("missing tip header")?;
        let lthash = r.lthash()?.context("missing state commitment")?;
        if lthash.root() != rec.header.state_root {
            return Err(anyhow!("state commitment does not match tip header; database is corrupt (delete the data directory to resync)"));
        }
        let tip = TipInfo { hash: tip_hash, height: rec.header.height, chainwork: u256_from_bytes(&rec.chainwork), header: rec.header };
        drop(r);
        info!(height = tip.height, tip = %tip.hash, "chain loaded");
        Ok(Chain {
            params,
            db,
            opts,
            inner: Mutex::new(Inner { lthash, hasher: PowHasher::new(params.chain_id, params.pow), orphans: Vec::new() }),
            tip: RwLock::new(tip),
        })
    }

    pub fn tip(&self) -> TipInfo {
        self.tip.read().clone()
    }

    pub fn read(&self) -> Result<ReadTx> {
        self.db.read()
    }

    /// A read snapshot together with the height of the tip *in that snapshot*
    /// (the cached tip may be updated a moment after a commit).
    pub fn read_at_tip(&self) -> Result<(ReadTx, u64)> {
        let r = self.db.read()?;
        let hash = r.tip()?.context("missing tip")?;
        let height = r.header(&hash)?.context("missing tip header")?.header.height;
        Ok((r, height))
    }

    /// `(stored, metadata, fragmented)` bytes in the database file.
    pub fn space_stats(&self) -> Result<(u64, u64, u64)> {
        let _guard = self.inner.lock();
        self.db.space_stats()
    }

    pub fn options(&self) -> &ChainOptions {
        &self.opts
    }

    pub fn genesis_hash(&self) -> Hash32 {
        genesis_block(self.params).hash()
    }

    pub fn global(&self) -> Result<ChainGlobal> {
        let r = self.db.read()?;
        read_typed::<ChainGlobal, _>(&r, &thecoin_core::state::global_key())?.context("missing global record")
    }

    /// Last `count` headers of the branch ending at `last` (oldest first).
    fn branch_headers<R: DbRead>(r: &R, last: &HeaderRecord, count: usize) -> Result<Vec<HeaderRecord>> {
        let mut out = Vec::with_capacity(count);
        out.push(last.clone());
        while out.len() < count {
            let cur = out.last().expect("non-empty");
            if cur.header.height == 0 {
                break;
            }
            let parent = r.header(&cur.header.prev_hash)?.context("missing ancestor header")?;
            out.push(parent);
        }
        out.reverse();
        Ok(out)
    }

    /// Cheap context checks for a header relayed ahead of its transactions
    /// (compact blocks), done before allocating anything for it. PoW is not checked.
    pub fn check_relayed_header(&self, h: &BlockHeader) -> Result<RelayedHeader> {
        if h.target_u256() > self.params.pow_limit() {
            return Ok(RelayedHeader::Invalid(BlockError::BadTarget));
        }
        let r = self.db.read()?;
        if r.header(&h.hash())?.is_some() {
            return Ok(RelayedHeader::Known);
        }
        let Some(parent) = r.header(&h.prev_hash)? else { return Ok(RelayedHeader::UnknownParent) };
        if h.height.saturating_add(self.params.max_reorg_depth) < self.tip().height {
            // Too deep to ever matter: ignore like an already known block.
            return Ok(RelayedHeader::Known);
        }
        match self.check_header(&r, &parent, h, now_secs()) {
            Ok(()) => Ok(RelayedHeader::Valid),
            Err(Fail::Invalid(e)) => Ok(RelayedHeader::Invalid(e)),
            Err(Fail::Storage(e)) => Err(e),
        }
    }

    fn expected_target<R: DbRead>(&self, r: &R, parent: &HeaderRecord) -> Result<(U256, u64)> {
        let need = difficulty::required_ancestors(self.params).max(MTP_WINDOW);
        let hdrs = Self::branch_headers(r, parent, need)?;
        let infos: Vec<BlockTimeInfo> =
            hdrs.iter().map(|h| BlockTimeInfo { timestamp: h.header.timestamp, target: h.header.target_u256() }).collect();
        let target = difficulty::next_target(self.params, parent.header.height + 1, &infos);
        let ts: Vec<u64> = hdrs.iter().map(|h| h.header.timestamp).collect();
        let mtp = difficulty::median_time_past(&ts);
        Ok((target, mtp))
    }

    /// Context checks of a header against its parent (no PoW).
    fn check_header<R: DbRead>(&self, r: &R, parent: &HeaderRecord, h: &BlockHeader, now: u64) -> std::result::Result<(), Fail> {
        if h.version != BLOCK_VERSION {
            return Err(Fail::Invalid(BlockError::BadVersion(h.version)));
        }
        if h.height != parent.header.height + 1 {
            return Err(Fail::Invalid(BlockError::BadHeight { expected: parent.header.height + 1, got: h.height }));
        }
        let (target, mtp) = self.expected_target(r, parent)?;
        if h.timestamp <= mtp {
            return Err(Fail::Invalid(BlockError::TimestampTooOld { ts: h.timestamp, mtp }));
        }
        if h.timestamp > now + self.params.max_future_drift {
            return Err(Fail::Invalid(BlockError::TimestampInFuture { ts: h.timestamp, now }));
        }
        if h.target_u256() != target {
            return Err(Fail::Invalid(BlockError::BadTarget));
        }
        for (cp_height, cp_hash) in self.params.checkpoints {
            if *cp_height == h.height && Hash32::from_hex(cp_hash).ok() != Some(h.hash()) {
                return Err(Fail::Invalid(BlockError::Checkpoint(h.height)));
            }
        }
        if let Some((last_cp, _)) = self.params.checkpoints.last() {
            if h.height <= *last_cp && r.main_hash(parent.header.height)? != Some(h.prev_hash) {
                return Err(Fail::Invalid(BlockError::Checkpoint(*last_cp)));
            }
        }
        Ok(())
    }

    /// Submits a block (from the network or the local miner) and then tries to
    /// connect any orphans that were waiting for it.
    ///
    /// `pow_checked` must only be `true` if CoinHash was verified by the caller.
    pub fn submit_block(&self, block: Block, pow_checked: bool) -> Result<ProcessResult> {
        let mut inner = self.inner.lock();
        let first = self.process_locked(&mut inner, block, pow_checked)?;
        if matches!(first, ProcessResult::NewTip { .. } | ProcessResult::SideChain) {
            // Connect orphans whose parent is now known.
            let mut merged_disc = Vec::new();
            let mut merged_conn = Vec::new();
            let mut changed = false;
            if let ProcessResult::NewTip { disconnected, connected } = first {
                merged_disc = disconnected;
                merged_conn = connected;
                changed = true;
            }
            loop {
                let r = self.db.read()?;
                let pos = inner.orphans.iter().position(|(o, _)| r.header(&o.header.prev_hash).ok().flatten().is_some());
                drop(r);
                let Some(pos) = pos else { break };
                let (orphan, _) = inner.orphans.swap_remove(pos);
                if let ProcessResult::NewTip { disconnected, connected } = self.process_locked(&mut inner, orphan, false)? {
                    changed = true;
                    // A later reorg may disconnect blocks connected earlier in this loop.
                    for d in disconnected {
                        if let Some(i) = merged_conn.iter().position(|c: &Block| c.hash() == d.hash()) {
                            merged_conn.remove(i);
                        } else {
                            merged_disc.push(d);
                        }
                    }
                    merged_conn.extend(connected);
                }
            }
            return Ok(if changed {
                ProcessResult::NewTip { disconnected: merged_disc, connected: merged_conn }
            } else {
                ProcessResult::SideChain
            });
        }
        Ok(first)
    }

    fn process_locked(&self, inner: &mut Inner, block: Block, pow_checked: bool) -> Result<ProcessResult> {
        match self.process_inner(inner, block, pow_checked) {
            Ok(r) => Ok(r),
            Err(Fail::Invalid(e)) => Ok(ProcessResult::Invalid(e)),
            Err(Fail::Storage(e)) => Err(e),
        }
    }

    fn process_inner(&self, inner: &mut Inner, block: Block, pow_checked: bool) -> std::result::Result<ProcessResult, Fail> {
        let hash = block.hash();
        // Cheap sanity check first: a target easier than the network limit can
        // never be valid (and would make fake orphans free to produce).
        if block.header.target_u256() > self.params.pow_limit() {
            return Err(Fail::Invalid(BlockError::BadTarget));
        }
        let r = self.db.read()?;
        if let Some(rec) = r.header(&hash)? {
            return match rec.status {
                BlockStatus::Invalid => Err(Fail::Invalid(BlockError::InvalidAncestor)),
                _ => Ok(ProcessResult::AlreadyKnown),
            };
        }
        let tip = self.tip();
        // Forks that start deeper than the maximum reorganization depth can never
        // become active; do not spend disk or CPU on them.
        if block.header.height.saturating_add(self.params.max_reorg_depth) < tip.height {
            return Ok(ProcessResult::SideChain);
        }
        let Some(parent) = r.header(&block.header.prev_hash)? else {
            let size = block.serialized_size();
            if size <= MAX_ORPHAN_BLOCK_BYTES && !inner.orphans.iter().any(|(o, _)| o.hash() == hash) {
                inner.orphans.push((block, size));
                while inner.orphans.len() > MAX_ORPHANS || inner.orphans.iter().map(|(_, s)| s).sum::<usize>() > MAX_ORPHAN_BYTES {
                    inner.orphans.remove(0);
                }
            }
            return Ok(ProcessResult::Orphan);
        };
        if parent.status == BlockStatus::Invalid {
            return Err(Fail::Invalid(BlockError::InvalidAncestor));
        }
        self.check_header(&r, &parent, &block.header, now_secs())?;
        let size = block.serialized_size() as u64;
        if size > MAX_BLOCK_BYTES_HARD {
            return Err(Fail::Invalid(BlockError::TooLarge { size, max: MAX_BLOCK_BYTES_HARD }));
        }
        if block.compute_tx_root() != block.header.tx_root {
            return Err(Fail::Invalid(BlockError::BadTxRoot));
        }
        if !pow_checked && !block.header.check_pow(&mut inner.hasher) {
            return Err(Fail::Invalid(BlockError::BadPow));
        }
        drop(r);

        let chainwork = u256_from_bytes(&parent.chainwork).saturating_add(work_for_target(&block.header.target_u256()));
        let record = HeaderRecord {
            header: block.header.clone(),
            chainwork: u256_to_bytes(&chainwork),
            status: BlockStatus::Stored,
            has_body: true,
            tx_count: block.txs.len() as u32,
            size: size as u32,
        };

        if chainwork <= tip.chainwork {
            let w = self.db.write()?;
            w.put_header(&hash, &record)?;
            w.put_block_txs(&hash, &block.txs)?;
            w.commit()?;
            return Ok(ProcessResult::SideChain);
        }

        // Find the fork point between the new branch and the active chain.
        let mut branch: Vec<HeaderRecord> = vec![record.clone()];
        let fork_height;
        {
            let r = self.db.read()?;
            let mut cur = parent.clone();
            loop {
                if r.main_hash(cur.header.height)? == Some(cur.header.hash()) {
                    fork_height = cur.header.height;
                    break;
                }
                branch.push(cur.clone());
                cur = r.header(&cur.header.prev_hash)?.ok_or_else(|| anyhow!("missing branch ancestor"))?;
            }
        }
        branch.reverse();
        let depth = tip.height - fork_height;
        if depth > self.params.max_reorg_depth {
            warn!(depth, fork_height, "refusing reorganization deeper than the maximum; ignoring block");
            return Ok(ProcessResult::SideChain);
        }

        let w = self.db.write()?;
        w.put_header(&hash, &record)?;
        w.put_block_txs(&hash, &block.txs)?;
        let mut lthash = inner.lthash.clone();

        let mut disconnected = Vec::new();
        for h in ((fork_height + 1)..=tip.height).rev() {
            disconnected.push(self.disconnect_block(&w, h, &mut lthash)?);
        }

        let mut connected = Vec::with_capacity(branch.len());
        for (i, rec) in branch.iter().enumerate() {
            let bhash = rec.header.hash();
            let blk = if bhash == hash {
                block.clone()
            } else {
                let txs = w.block_txs(&bhash)?.ok_or_else(|| anyhow!("missing body of branch block {bhash}"))?;
                Block { header: rec.header.clone(), txs }
            };
            let result = if rec.status == BlockStatus::Invalid {
                Err(Fail::Invalid(BlockError::InvalidAncestor))
            } else {
                self.connect_block(&w, &blk, rec, &mut lthash)
            };
            if let Err(fail) = result {
                w.abort()?;
                if let Fail::Invalid(e) = &fail {
                    warn!(height = rec.header.height, hash = %bhash, error = %e, "invalid block");
                    let w2 = self.db.write()?;
                    for bad in &branch[i..] {
                        let mut bad_rec = bad.clone();
                        bad_rec.status = BlockStatus::Invalid;
                        bad_rec.has_body = false;
                        w2.put_header(&bad.header.hash(), &bad_rec)?;
                        w2.delete_block_txs(&bad.header.hash())?;
                    }
                    w2.commit()?;
                }
                return Err(fail);
            }
            connected.push(blk);
        }
        w.commit()?;
        inner.lthash = lthash;
        *self.tip.write() = TipInfo { hash, height: block.header.height, chainwork, header: block.header.clone() };
        if !disconnected.is_empty() {
            info!(depth, new_height = block.header.height, "chain reorganization");
        }
        Ok(ProcessResult::NewTip { disconnected, connected })
    }

    fn connect_block(&self, w: &WriteTx, block: &Block, rec: &HeaderRecord, lthash: &mut LtHash) -> std::result::Result<(), Fail> {
        let height = block.header.height;
        let hash = block.hash();
        let (receipt, changes) = {
            let mut ov = Overlay::new(w);
            let receipt = apply_block(self.params, &mut ov, block, false)?;
            (receipt, ov.diff()?)
        };
        apply_changes_to_lthash(lthash, &changes);
        let computed = lthash.root();
        if computed != block.header.state_root {
            return Err(Fail::Invalid(BlockError::BadStateRoot { expected: block.header.state_root, computed }));
        }
        w.apply_state_changes(&changes)?;
        let mut addr_index = Vec::new();
        for (pos, r) in receipt.txs.iter().enumerate() {
            w.index_tx(&r.txid, height, pos as u32)?;
            w.put_receipt(r)?;
            if self.opts.address_index {
                for a in &r.touched {
                    w.index_address(a, height, pos as u32)?;
                    addr_index.push((*a, pos as u32));
                }
            }
        }
        w.put_undo(height, &BlockUndo { changes, addr_index })?;
        w.set_main(height, &hash)?;
        let mut rec = rec.clone();
        rec.status = BlockStatus::Valid;
        rec.has_body = true;
        w.put_header(&hash, &rec)?;
        w.set_tip(&hash)?;
        w.set_lthash(lthash)?;

        // Undo data is only needed for reorganizations.
        let undo_keep = self.params.max_reorg_depth + UNDO_MARGIN;
        if height > undo_keep {
            w.delete_undo(height - undo_keep)?;
        }
        if let Some(keep) = self.opts.prune_keep {
            if height > keep {
                let old = height - keep;
                if let Some(old_hash) = w.main_hash(old)? {
                    if let Some(mut old_rec) = w.header(&old_hash)? {
                        if old_rec.has_body && old > 0 {
                            // Pruning removes everything derived from the body too:
                            // its transaction index entries and receipts. Headers and
                            // the state are always kept.
                            if let Some(txs) = w.block_txs(&old_hash)? {
                                for tx in &txs {
                                    let txid = tx.txid();
                                    w.unindex_tx(&txid)?;
                                    w.delete_receipt(&txid)?;
                                }
                            }
                            old_rec.has_body = false;
                            w.put_header(&old_hash, &old_rec)?;
                            w.delete_block_txs(&old_hash)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn disconnect_block(&self, w: &WriteTx, height: u64, lthash: &mut LtHash) -> Result<Block> {
        let hash = w.main_hash(height)?.context("missing main hash")?;
        let block = w.block(&hash)?.context("cannot reorganize: block body pruned")?;
        let undo = w.undo(height)?.context("cannot reorganize: undo data missing")?;
        revert_changes_from_lthash(lthash, &undo.changes);
        w.revert_state_changes(&undo.changes)?;
        for tx in &block.txs {
            let txid = tx.txid();
            w.unindex_tx(&txid)?;
            w.delete_receipt(&txid)?;
        }
        for (addr, pos) in &undo.addr_index {
            w.unindex_address(addr, height, *pos)?;
        }
        w.remove_main(height)?;
        w.delete_undo(height)?;
        let parent = block.header.prev_hash;
        if let Some(mut rec) = w.header(&hash)? {
            rec.status = BlockStatus::Stored;
            w.put_header(&hash, &rec)?;
        }
        w.set_tip(&parent)?;
        w.set_lthash(lthash)?;
        Ok(block)
    }

    /// Builds a block template on top of the current tip. `candidates` must be
    /// ordered so that each sender's nonces are increasing (see the mempool).
    pub fn build_template(&self, miner: &Address, signal_proposals: &[Hash32], candidates: &[Transaction]) -> Result<Block> {
        let inner = self.inner.lock();
        let tip = self.tip();
        let r = self.db.read()?;
        let parent = r.header(&tip.hash)?.context("missing tip header")?;
        let (target, mtp) = self.expected_target(&r, &parent)?;
        let height = tip.height + 1;
        let timestamp = now_secs().max(mtp + 1);

        let mut builder = BlockBuilder::new(self.params, &r, height).map_err(|e| anyhow!("{e}"))?;
        for tx in candidates {
            if builder.remaining_bytes() < 128 {
                break;
            }
            let _ = builder.try_add(tx);
        }
        let g: ChainGlobal = read_typed(&r, &thecoin_core::state::global_key())?.context("global")?;
        let mut signal = 0u32;
        for (bit, id) in &g.voting {
            if signal_proposals.contains(id) {
                signal |= 1 << bit;
            }
        }
        let (overlay, txs, signal) = builder.finish(miner, signal).map_err(|e| anyhow!("{e}"))?;
        let changes = overlay.diff()?;
        let mut lthash = inner.lthash.clone();
        apply_changes_to_lthash(&mut lthash, &changes);
        let tx_root = merkle_root(&txs.iter().map(|t| t.txid()).collect::<Vec<_>>());
        let header = BlockHeader {
            version: BLOCK_VERSION,
            height,
            prev_hash: tip.hash,
            tx_root,
            state_root: lthash.root(),
            timestamp,
            target: u256_to_bytes(&target),
            nonce: 0,
            miner: *miner,
            signal,
        };
        Ok(Block { header, txs })
    }

    /// Block locator: recent hashes densely, then exponentially sparser, ending at genesis.
    pub fn locator(&self) -> Result<Vec<Hash32>> {
        let tip = self.tip();
        let r = self.db.read()?;
        let mut out = Vec::new();
        let mut step = 1u64;
        let mut h = tip.height as i64;
        while h > 0 {
            if let Some(hash) = r.main_hash(h as u64)? {
                out.push(hash);
            }
            if out.len() >= 10 {
                step *= 2;
            }
            h -= step as i64;
        }
        out.push(self.genesis_hash());
        Ok(out)
    }

    /// Hashes of main-chain blocks following the first locator entry we know.
    pub fn hashes_after_locator(&self, locator: &[Hash32], stop: &Hash32, max: usize) -> Result<Vec<Hash32>> {
        let r = self.db.read()?;
        let tip = self.tip();
        let mut start = 0u64;
        for hash in locator {
            if let Some(rec) = r.header(hash)? {
                if r.main_hash(rec.header.height)? == Some(*hash) {
                    start = rec.header.height;
                    break;
                }
            }
        }
        let mut out = Vec::new();
        let mut h = start + 1;
        while h <= tip.height && out.len() < max {
            let Some(hash) = r.main_hash(h)? else { break };
            out.push(hash);
            if &hash == stop {
                break;
            }
            h += 1;
        }
        Ok(out)
    }

    /// Estimated network hashrate (hashes/s) over the last `window` blocks.
    pub fn network_hashrate(&self, window: u64) -> Result<f64> {
        let tip = self.tip();
        if tip.height < 2 {
            return Ok(0.0);
        }
        let r = self.db.read()?;
        let start_h = tip.height.saturating_sub(window).max(1);
        let Some(start_hash) = r.main_hash(start_h)? else { return Ok(0.0) };
        let Some(start) = r.header(&start_hash)? else { return Ok(0.0) };
        let work = tip.chainwork.saturating_sub(u256_from_bytes(&start.chainwork));
        let secs = tip.header.timestamp.saturating_sub(start.header.timestamp).max(1);
        Ok(thecoin_core::pow::u256_to_f64(&work) / secs as f64)
    }

    pub fn next_target(&self) -> Result<U256> {
        let tip = self.tip();
        let r = self.db.read()?;
        let parent = r.header(&tip.hash)?.context("missing tip header")?;
        Ok(self.expected_target(&r, &parent)?.0)
    }

    /// Block at a main-chain height.
    pub fn block_at(&self, height: u64) -> Result<Option<Block>> {
        let r = self.db.read()?;
        match r.main_hash(height)? {
            Some(h) => r.block(&h),
            None => Ok(None),
        }
    }
}
