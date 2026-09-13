//! Transaction pool.
//!
//! * Every transaction is fully validated before admission by simulating the
//!   sender's pending transactions (in nonce order) on top of the current state.
//!   Contract calls that would fail are refused (they would cost the user a fee).
//! * **Priority:** transactions are ordered by fee per weight unit
//!   (`bytes + max_fuel / 100`); paying above the minimum confirms sooner.
//! * **Replacement ("fee bump")** is only allowed when the original transaction
//!   set `FLAG_REPLACEABLE` and the new fee is at least 25% higher. Any other
//!   second transaction with the same sender and nonce is recorded as a
//!   **double-spend attempt** and reported to peers and the API, so merchants
//!   accepting unconfirmed payments are warned immediately.
//! * Bounded memory: total size limit, per-sender limit, 72 h expiry.
//! * When congestion raises the minimum fee, underpriced transactions stay in
//!   the pool (they become valid again when congestion falls) instead of being
//!   dropped.

use crate::chain::{now_secs, Chain};
use anyhow::Result;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use thecoin_core::api::{DoubleSpendView, PriorityView};
use thecoin_core::error::TxError;
use thecoin_core::execution::{apply_tx, check_tx_stateless, TxReceipt};
use thecoin_core::hash::Hash32;
use thecoin_core::state::{Overlay, StateReader};
use thecoin_core::{Address, Network, Transaction};

pub const MAX_PER_SENDER: usize = 32;
pub const EXPIRY: Duration = Duration::from_secs(72 * 3600);
const MAX_CONFLICTS: usize = 512;

#[derive(Debug)]
pub enum MempoolError {
    AlreadyKnown,
    Invalid(TxError),
    ContractWouldFail(String),
    SenderLimit,
    Full,
    ReplacementFeeTooLow,
    NotReplaceable { existing: Hash32 },
    Storage(String),
}

impl std::error::Error for MempoolError {}

impl std::fmt::Display for MempoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MempoolError::AlreadyKnown => write!(f, "transaction already in mempool"),
            MempoolError::Invalid(e) => write!(f, "invalid transaction: {e}"),
            MempoolError::ContractWouldFail(e) => write!(f, "contract call would fail: {e}"),
            MempoolError::SenderLimit => write!(f, "too many pending transactions from this sender"),
            MempoolError::Full => write!(f, "mempool full and fee rate too low"),
            MempoolError::ReplacementFeeTooLow => write!(f, "replacement needs a fee at least 25% higher"),
            MempoolError::NotReplaceable { existing } => {
                write!(f, "a transaction with this nonce is already pending ({existing}) and it is not replaceable — double spend attempt recorded")
            }
            MempoolError::Storage(e) => write!(f, "storage error: {e}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub tx: Transaction,
    pub txid: Hash32,
    pub size: usize,
    pub weight: u64,
    pub fee_rate: u64,
    pub sender: Address,
    pub added: Instant,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub sender: Address,
    pub nonce: u64,
    pub first: Hash32,
    pub second: Hash32,
    pub seen_at: u64,
}

pub struct Mempool {
    entries: HashMap<Hash32, Entry>,
    by_sender: HashMap<Address, BTreeMap<u64, Hash32>>,
    bytes: usize,
    max_bytes: usize,
    conflicts: VecDeque<Conflict>,
    conflict_of: HashMap<Hash32, Hash32>,
}

/// Result of simulating one transaction.
pub type SimResult = Result<TxReceipt, TxError>;

impl Mempool {
    pub fn new(max_bytes: usize) -> Self {
        Mempool {
            entries: HashMap::new(),
            by_sender: HashMap::new(),
            bytes: 0,
            max_bytes,
            conflicts: VecDeque::new(),
            conflict_of: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn contains(&self, txid: &Hash32) -> bool {
        self.entries.contains_key(txid)
    }

    pub fn get(&self, txid: &Hash32) -> Option<&Transaction> {
        self.entries.get(txid).map(|e| &e.tx)
    }

    pub fn txids(&self, limit: usize) -> Vec<Hash32> {
        let mut v: Vec<&Entry> = self.entries.values().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.fee_rate));
        v.into_iter().take(limit).map(|e| e.txid).collect()
    }

    pub fn sender_txs(&self, sender: &Address) -> Vec<Transaction> {
        self.by_sender
            .get(sender)
            .map(|m| m.values().filter_map(|id| self.entries.get(id).map(|e| e.tx.clone())).collect())
            .unwrap_or_default()
    }

    /// Next nonce to use, counting pending transactions.
    pub fn next_nonce(&self, sender: &Address, account_nonce: u64) -> u64 {
        let mut n = account_nonce;
        if let Some(m) = self.by_sender.get(sender) {
            while m.contains_key(&n) {
                n += 1;
            }
        }
        n
    }

    /// Fills the empty slots of a compact block with matching pool transactions.
    pub fn fill_compact(&self, block: &Hash32, short_ids: &[[u8; 6]], slots: &mut [Option<Transaction>]) {
        let mut wanted: HashMap<[u8; 6], Vec<usize>> = HashMap::new();
        for (i, sid) in short_ids.iter().enumerate() {
            if slots[i].is_none() {
                wanted.entry(*sid).or_default().push(i);
            }
        }
        if wanted.is_empty() {
            return;
        }
        for e in self.entries.values() {
            if let Some(idxs) = wanted.get(&crate::protocol::short_id(block, &e.txid)) {
                for i in idxs {
                    slots[*i] = Some(e.tx.clone());
                }
            }
        }
    }

    /// Double-spend attempt involving this transaction, if any.
    pub fn conflict_for(&self, txid: &Hash32) -> Option<Hash32> {
        self.conflict_of.get(txid).copied()
    }

    pub fn conflicts(&self, n: Network) -> Vec<DoubleSpendView> {
        self.conflicts
            .iter()
            .rev()
            .map(|c| DoubleSpendView { sender: c.sender.encode(n), nonce: c.nonce, first: c.first, second: c.second, seen_at: c.seen_at })
            .collect()
    }

    /// Records two different transactions with the same sender and nonce.
    /// Returns true if it is new (worth relaying).
    pub fn record_conflict(&mut self, first: &Transaction, second: &Transaction) -> bool {
        let (a, b) = (first.txid(), second.txid());
        if a == b || first.sender() != second.sender() || first.body.nonce != second.body.nonce {
            return false;
        }
        // One alert per (sender, nonce): creating conflicting variants is free, so
        // more alerts for the same slot would let one key flood the network.
        if self.conflicts.iter().any(|c| c.sender == first.sender() && c.nonce == first.body.nonce) {
            return false;
        }
        if self.conflicts.len() >= MAX_CONFLICTS {
            if let Some(old) = self.conflicts.pop_front() {
                self.conflict_of.remove(&old.first);
                self.conflict_of.remove(&old.second);
            }
        }
        self.conflict_of.insert(a, b);
        self.conflict_of.insert(b, a);
        self.conflicts.push_back(Conflict { sender: first.sender(), nonce: first.body.nonce, first: a, second: b, seen_at: now_secs() });
        true
    }

    /// Fee multipliers for the priority levels, from the current pool.
    pub fn priority_levels(&self, block_weight: u64, min_rate_for_typical: u64) -> PriorityView {
        let mut rates: Vec<(u64, u64)> = self.entries.values().map(|e| (e.fee_rate, e.weight)).collect();
        rates.sort_by_key(|r| std::cmp::Reverse(r.0));
        // Fee rate needed to be within the first `share`% of a block.
        let cutoff = |share: u64| -> u64 {
            let cap = block_weight * share / 100;
            let mut acc = 0u64;
            for (rate, w) in &rates {
                acc += w;
                if acc >= cap {
                    return *rate;
                }
            }
            0
        };
        let min = min_rate_for_typical.max(1);
        let to_bp = |rate: u64| -> u64 { (rate as u128 * 10_000 / min as u128).min(u64::MAX as u128) as u64 };
        let high = to_bp(cutoff(100)).saturating_add(1_000).max(20_000);
        let urgent = to_bp(cutoff(25)).saturating_add(2_500).max(high.saturating_mul(2));
        PriorityView { low_bp: 10_000, normal_bp: 12_500, high_bp: high, urgent_bp: urgent }
    }

    /// Simulates `txs` (one sender, nonce order) on the tip state.
    fn simulate<R: StateReader + ?Sized>(chain: &Chain, reader: &R, height: u64, txs: &[&Transaction]) -> Vec<SimResult> {
        let mut ov = Overlay::new(reader);
        let mut out = Vec::with_capacity(txs.len());
        let mut failed = false;
        for tx in txs {
            if failed {
                out.push(Err(TxError::BadNonce { expected: 0, got: tx.body.nonce }));
                continue;
            }
            let size = tx.size();
            let res = {
                let mut child = Overlay::new(&ov);
                apply_tx(chain.params, &mut child, height, tx, size).map(|r| (r, child.into_changes()))
            };
            match res {
                Ok((receipt, changes)) => {
                    ov.absorb(changes);
                    out.push(Ok(receipt));
                }
                Err(e) => {
                    failed = true;
                    out.push(Err(e));
                }
            }
        }
        out
    }

    /// Simulates a transaction as if it were included in the next block after
    /// this sender's pending transactions (for the API).
    pub fn simulate_one(&self, chain: &Chain, tx: &Transaction) -> Result<SimResult> {
        let (reader, tip_height) = chain.read_at_tip()?;
        let sender = tx.sender();
        let mut pending: Vec<Transaction> = self.sender_txs(&sender).into_iter().filter(|t| t.body.nonce < tx.body.nonce).collect();
        pending.push(tx.clone());
        let refs: Vec<&Transaction> = pending.iter().collect();
        let mut results = Self::simulate(chain, &reader, tip_height + 1, &refs);
        Ok(results.pop().expect("one result per tx"))
    }

    /// Validates and inserts a transaction.
    pub fn add(&mut self, chain: &Chain, tx: Transaction) -> std::result::Result<Hash32, MempoolError> {
        let txid = tx.txid();
        if self.entries.contains_key(&txid) {
            return Err(MempoolError::AlreadyKnown);
        }
        let size = check_tx_stateless(chain.params, &tx).map_err(MempoolError::Invalid)?;
        let sender = tx.sender();
        let (reader, tip_height) = chain.read_at_tip().map_err(|e| MempoolError::Storage(e.to_string()))?;
        let height = tip_height + 1;

        let mut pending: Vec<Transaction> = self.sender_txs(&sender);
        let mut replaced: Option<Hash32> = None;
        if let Some(pos) = pending.iter().position(|p| p.body.nonce == tx.body.nonce) {
            let old = pending[pos].clone();
            if !old.is_replaceable() {
                self.record_conflict(&old, &tx);
                return Err(MempoolError::NotReplaceable { existing: old.txid() });
            }
            if tx.body.fee < old.body.fee.saturating_add(old.body.fee / 4).max(old.body.fee + 1) {
                return Err(MempoolError::ReplacementFeeTooLow);
            }
            replaced = Some(old.txid());
            pending[pos] = tx.clone();
        } else {
            if pending.len() >= MAX_PER_SENDER {
                return Err(MempoolError::SenderLimit);
            }
            pending.push(tx.clone());
            pending.sort_by_key(|t| t.body.nonce);
        }
        let refs: Vec<&Transaction> = pending.iter().collect();
        let results = Self::simulate(chain, &reader, height, &refs);
        let my_pos = pending.iter().position(|t| t.txid() == txid).expect("inserted");
        match &results[my_pos] {
            Err(e) => return Err(MempoolError::Invalid(e.clone())),
            Ok(r) if !r.success => return Err(MempoolError::ContractWouldFail(r.error.clone().unwrap_or_default())),
            Ok(_) => {}
        }
        drop(reader);

        let weight = tx.weight();
        let fee_rate = tx.body.fee / weight.max(1);
        if self.bytes + size > self.max_bytes && !self.evict_for(fee_rate, size) {
            return Err(MempoolError::Full);
        }
        if let Some(old) = replaced {
            self.remove(&old);
        }
        self.bytes += size;
        self.by_sender.entry(sender).or_default().insert(tx.body.nonce, txid);
        self.entries.insert(txid, Entry { tx, txid, size, weight, fee_rate, sender, added: Instant::now() });
        Ok(txid)
    }

    /// Evicts lowest fee-rate tail transactions until `size` bytes fit.
    fn evict_for(&mut self, fee_rate: u64, size: usize) -> bool {
        while self.bytes + size > self.max_bytes {
            let victim = self
                .by_sender
                .values()
                .filter_map(|m| m.values().next_back())
                .filter_map(|id| self.entries.get(id))
                .min_by_key(|e| e.fee_rate)
                .map(|e| (e.txid, e.fee_rate));
            match victim {
                Some((id, rate)) if rate < fee_rate => {
                    self.remove(&id);
                }
                _ => return false,
            }
        }
        true
    }

    pub fn remove(&mut self, txid: &Hash32) -> Option<Entry> {
        let e = self.entries.remove(txid)?;
        self.bytes -= e.size;
        if let Some(m) = self.by_sender.get_mut(&e.sender) {
            m.remove(&e.tx.body.nonce);
            if m.is_empty() {
                self.by_sender.remove(&e.sender);
            }
        }
        Some(e)
    }

    /// Re-validates the pool against the new tip: removes confirmed,
    /// conflicting, failing and expired transactions, keeps transactions that
    /// are only underpriced because of congestion, and re-adds transactions
    /// from disconnected blocks.
    pub fn on_new_tip(&mut self, chain: &Chain, disconnected: &[thecoin_core::Block], connected: &[thecoin_core::Block]) -> Result<()> {
        let confirmed: HashSet<Hash32> = connected.iter().flat_map(|b| b.txs.iter().map(|t| t.txid())).collect();
        for id in &confirmed {
            self.remove(id);
        }
        let now = Instant::now();
        let expired: Vec<Hash32> = self.entries.values().filter(|e| now.duration_since(e.added) > EXPIRY).map(|e| e.txid).collect();
        for id in expired {
            self.remove(&id);
        }

        let (reader, tip_height) = chain.read_at_tip()?;
        let height = tip_height + 1;
        let senders: Vec<Address> = self.by_sender.keys().copied().collect();
        for sender in senders {
            let txs = self.sender_txs(&sender);
            let refs: Vec<&Transaction> = txs.iter().collect();
            let results = Self::simulate(chain, &reader, height, &refs);
            let mut keep_rest = false;
            for (tx, r) in txs.iter().zip(results) {
                if keep_rest {
                    continue;
                }
                match r {
                    Ok(receipt) if receipt.success => {}
                    Err(TxError::FeeTooLow { .. }) => keep_rest = true,
                    _ => {
                        self.remove(&tx.txid());
                    }
                }
            }
        }
        drop(reader);
        for b in disconnected.iter().rev() {
            for tx in &b.txs {
                if !confirmed.contains(&tx.txid()) {
                    let _ = self.add(chain, tx.clone());
                }
            }
        }
        Ok(())
    }

    /// Candidates for the next block: highest fee rate first, nonces in order.
    pub fn ordered_for_block(&self, max_bytes: usize) -> Vec<Transaction> {
        #[derive(PartialEq, Eq)]
        struct Head {
            fee_rate: u64,
            txid: Hash32,
            sender: Address,
        }
        impl Ord for Head {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                self.fee_rate.cmp(&o.fee_rate).then_with(|| o.txid.cmp(&self.txid))
            }
        }
        impl PartialOrd for Head {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        let mut cursors: HashMap<Address, std::collections::btree_map::Iter<'_, u64, Hash32>> = HashMap::new();
        let mut heap = BinaryHeap::new();
        for (sender, m) in &self.by_sender {
            let mut it = m.iter();
            if let Some((_, id)) = it.next() {
                let e = &self.entries[id];
                heap.push(Head { fee_rate: e.fee_rate, txid: *id, sender: *sender });
            }
            cursors.insert(*sender, it);
        }
        let mut out = Vec::new();
        let mut bytes = 0usize;
        while let Some(head) = heap.pop() {
            let e = &self.entries[&head.txid];
            if bytes + e.size > max_bytes {
                continue;
            }
            bytes += e.size;
            out.push(e.tx.clone());
            if let Some((_, next)) = cursors.get_mut(&head.sender).and_then(|it| it.next()) {
                let ne = &self.entries[next];
                heap.push(Head { fee_rate: ne.fee_rate, txid: *next, sender: head.sender });
            }
        }
        out
    }
}
