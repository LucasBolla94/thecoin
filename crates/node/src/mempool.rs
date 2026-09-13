//! Transaction pool.
//!
//! * Every transaction is fully validated before admission by simulating the
//!   sender's pending transactions (in nonce order) on top of the current state.
//! * Bounded memory: total size limit, per-sender limit, 72 h expiry.
//! * Ordering for blocks by fee rate while respecting nonce order per sender.
//! * Replace-by-fee: same sender + nonce with at least 25% higher fee.

use crate::chain::Chain;
use anyhow::Result;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::time::{Duration, Instant};
use thecoin_core::error::TxError;
use thecoin_core::execution::{apply_tx, check_tx_stateless};
use thecoin_core::hash::Hash32;
use thecoin_core::state::{Overlay, StateReader};
use thecoin_core::{Address, Transaction};

pub const MAX_PER_SENDER: usize = 32;
pub const EXPIRY: Duration = Duration::from_secs(72 * 3600);

#[derive(Debug)]
pub enum MempoolError {
    AlreadyKnown,
    Invalid(TxError),
    SenderLimit,
    Full,
    ReplacementFeeTooLow,
    Storage(String),
}

impl std::error::Error for MempoolError {}

impl std::fmt::Display for MempoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MempoolError::AlreadyKnown => write!(f, "transaction already in mempool"),
            MempoolError::Invalid(e) => write!(f, "invalid transaction: {e}"),
            MempoolError::SenderLimit => write!(f, "too many pending transactions from this sender"),
            MempoolError::Full => write!(f, "mempool full and fee rate too low"),
            MempoolError::ReplacementFeeTooLow => write!(f, "replacement needs a fee at least 25% higher"),
            MempoolError::Storage(e) => write!(f, "storage error: {e}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub tx: Transaction,
    pub txid: Hash32,
    pub size: usize,
    pub fee_rate: u64,
    pub sender: Address,
    pub added: Instant,
}

pub struct Mempool {
    entries: HashMap<Hash32, Entry>,
    by_sender: HashMap<Address, BTreeMap<u64, Hash32>>,
    bytes: usize,
    max_bytes: usize,
}

impl Mempool {
    pub fn new(max_bytes: usize) -> Self {
        Mempool { entries: HashMap::new(), by_sender: HashMap::new(), bytes: 0, max_bytes }
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

    /// Transactions (in the pool) that involve `addr` as sender.
    pub fn next_nonce(&self, sender: &Address, account_nonce: u64) -> u64 {
        let mut n = account_nonce;
        if let Some(m) = self.by_sender.get(sender) {
            while m.contains_key(&n) {
                n += 1;
            }
        }
        n
    }

    /// Suggested fee rate: the minimum, raised when the pool is filling up.
    pub fn suggested_fee_rate(&self, min: u64) -> u64 {
        let fill = self.bytes as f64 / self.max_bytes.max(1) as f64;
        if fill < 0.1 {
            return min;
        }
        let mut rates: Vec<u64> = self.entries.values().map(|e| e.fee_rate).collect();
        rates.sort_unstable();
        let idx = ((rates.len() as f64) * 0.5) as usize;
        rates.get(idx).copied().unwrap_or(min).max(min)
    }

    /// Simulates `txs` (one sender, nonce order) on the tip state.
    fn simulate<R: StateReader + ?Sized>(chain: &Chain, reader: &R, height: u64, txs: &[&Transaction]) -> Vec<Result<(), TxError>> {
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
                apply_tx(chain.params, &mut child, height, tx, size).map(|_| child.into_changes())
            };
            match res {
                Ok(changes) => {
                    ov.absorb(changes);
                    out.push(Ok(()));
                }
                Err(e) => {
                    failed = true;
                    out.push(Err(e));
                }
            }
        }
        out
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
            let old = &pending[pos];
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
        if let Err(e) = &results[my_pos] {
            // Give a precise error: re-simulate the new tx alone if a predecessor failed.
            return Err(MempoolError::Invalid(e.clone()));
        }

        let fee_rate = tx.body.fee / size.max(1) as u64;
        if self.bytes + size > self.max_bytes && !self.evict_for(fee_rate, size) {
            return Err(MempoolError::Full);
        }
        if let Some(old) = replaced {
            self.remove(&old);
        }
        self.bytes += size;
        self.by_sender.entry(sender).or_default().insert(tx.body.nonce, txid);
        self.entries.insert(txid, Entry { tx, txid, size, fee_rate, sender, added: Instant::now() });
        Ok(txid)
    }

    /// Evicts lowest fee-rate tail transactions until `size` bytes fit.
    fn evict_for(&mut self, fee_rate: u64, size: usize) -> bool {
        while self.bytes + size > self.max_bytes {
            // candidates: the highest-nonce tx of each sender (evicting it leaves no gap)
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

    /// Re-validates the whole pool against the new tip: removes confirmed,
    /// conflicting and expired transactions, and re-adds transactions from
    /// disconnected blocks.
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
            for (tx, r) in txs.iter().zip(results) {
                if r.is_err() {
                    self.remove(&tx.txid());
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
