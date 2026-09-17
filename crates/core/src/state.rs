//! Global state: key layout, records and the copy-on-write [`Overlay`].
//!
//! The state is a key/value map. Keys start with a one-byte prefix:
//!
//! | prefix | key                         | value                      |
//! |--------|-----------------------------|----------------------------|
//! | `0x01` | address (20)                | [`Account`]                |
//! | `0x02` | contract id (32)            | [`Contract`](crate::contracts::Contract) |
//! | `0x03` | proposal id (32)            | [`Proposal`](crate::governance::Proposal) |
//! | `0x04` | proposal id (32) + addr(20) | [`VoteRecord`](crate::governance::VoteRecord) |
//! | `0x05` | height (8, big-endian)      | [`PendingReward`]          |
//! | `0x06` | (empty)                     | [`ChainGlobal`]            |
//! | `0x07` | program address (20)        | [`ProgramMeta`]            |
//! | `0x08` | program address (20)        | compiled TCCL program      |
//! | `0x09` | program address (20) + key  | contract storage value     |
//!
//! Values are Borsh-encoded. All records together are committed by the LtHash
//! state root of every block.

use crate::address::Address;
use crate::contracts::Contract;
use crate::governance::{GovParams, Proposal, VoteRecord};
use crate::hash::Hash32;
use crate::lthash::LtHash;
use borsh::{BorshDeserialize, BorshSerialize};
use std::collections::BTreeMap;

pub const PREFIX_ACCOUNT: u8 = 0x01;
pub const PREFIX_CONTRACT: u8 = 0x02;
pub const PREFIX_PROPOSAL: u8 = 0x03;
pub const PREFIX_VOTE: u8 = 0x04;
pub const PREFIX_PENDING_REWARD: u8 = 0x05;
pub const PREFIX_GLOBAL: u8 = 0x06;
pub const PREFIX_PROGRAM_META: u8 = 0x07;
pub const PREFIX_PROGRAM_CODE: u8 = 0x08;
pub const PREFIX_PROGRAM_STORAGE: u8 = 0x09;

pub fn account_key(a: &Address) -> Vec<u8> {
    let mut k = vec![PREFIX_ACCOUNT];
    k.extend_from_slice(&a.0);
    k
}
pub fn contract_key(id: &Hash32) -> Vec<u8> {
    let mut k = vec![PREFIX_CONTRACT];
    k.extend_from_slice(&id.0);
    k
}
pub fn proposal_key(id: &Hash32) -> Vec<u8> {
    let mut k = vec![PREFIX_PROPOSAL];
    k.extend_from_slice(&id.0);
    k
}
pub fn vote_key(proposal: &Hash32, voter: &Address) -> Vec<u8> {
    let mut k = vec![PREFIX_VOTE];
    k.extend_from_slice(&proposal.0);
    k.extend_from_slice(&voter.0);
    k
}
pub fn pending_reward_key(height: u64) -> Vec<u8> {
    let mut k = vec![PREFIX_PENDING_REWARD];
    k.extend_from_slice(&height.to_be_bytes());
    k
}
pub fn global_key() -> Vec<u8> {
    vec![PREFIX_GLOBAL]
}
pub fn program_meta_key(a: &Address) -> Vec<u8> {
    let mut k = vec![PREFIX_PROGRAM_META];
    k.extend_from_slice(&a.0);
    k
}
pub fn program_code_key(a: &Address) -> Vec<u8> {
    let mut k = vec![PREFIX_PROGRAM_CODE];
    k.extend_from_slice(&a.0);
    k
}
pub fn program_storage_key(a: &Address, local: &[u8]) -> Vec<u8> {
    let mut k = Vec::with_capacity(21 + local.len());
    k.push(PREFIX_PROGRAM_STORAGE);
    k.extend_from_slice(&a.0);
    k.extend_from_slice(local);
    k
}

/// Metadata of a deployed TCCL contract. Its TCN balance is the [`Account`]
/// at the same address.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProgramMeta {
    pub creator: Address,
    pub created_height: u64,
    /// Transaction that deployed the contract (its source code is in it).
    pub deploy_txid: Hash32,
    /// BLAKE3 of the source code.
    pub source_hash: Hash32,
    pub name: String,
    /// Bytes of compiled code plus storage entries (keys + values).
    pub state_bytes: u64,
    pub storage_items: u64,
    /// Refundable storage deposit held for `state_bytes`.
    pub deposit: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize)]
pub struct Account {
    /// Total balance in motes (includes `locked`).
    pub balance: u64,
    /// Number of transactions sent by this account.
    pub nonce: u64,
    /// Coins locked by governance votes...
    pub locked: u64,
    /// ...until this height (inclusive).
    pub locked_until: u64,
}

impl Account {
    /// Balance that can be spent at `height`.
    pub fn spendable(&self, height: u64) -> u64 {
        if height <= self.locked_until {
            self.balance.saturating_sub(self.locked)
        } else {
            self.balance
        }
    }

    /// Accounts that never sent a transaction and hold nothing are not stored.
    pub fn is_empty(&self) -> bool {
        self.balance == 0 && self.nonce == 0
    }
}

/// Block reward waiting for maturity.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PendingReward {
    /// Who is paid by this block: the miner first, then the uncle miners.
    pub payouts: Vec<Payout>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Payout {
    pub who: Address,
    /// Total reward (subsidy share + fees).
    pub amount: u64,
    /// Part already released (the early quarter).
    pub released: u64,
}

impl PendingReward {
    pub fn total(&self) -> u64 {
        self.payouts.iter().map(|p| p.amount).sum()
    }

    /// Still locked by the cooldown.
    pub fn locked(&self) -> u64 {
        self.payouts.iter().map(|p| p.amount - p.released).sum()
    }
}

impl Payout {
    /// Quarter released after `coinbase_maturity` blocks.
    pub fn early_part(&self) -> u64 {
        self.amount / 4
    }
}

/// Chain-wide values.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ChainGlobal {
    /// Motes created by block subsidies so far.
    pub emitted: u64,
    /// Motes destroyed (e.g. deposits of proposals that missed quorum).
    pub burned: u64,
    /// Current governance-controlled parameters.
    pub params: GovParams,
    /// Proposals in voting: `(signal bit, proposal id)`.
    pub voting: Vec<(u8, Hash32)>,
    /// Approved proposals waiting for activation: `(activation height, id)`.
    pub pending_activations: Vec<(u64, Hash32)>,
    /// Number of proposals ever created.
    pub proposal_count: u64,
    /// Number of contracts ever created.
    pub contract_count: u64,
    /// Congestion multiplier applied to minimum fees, in basis points
    /// (10 000 = 1.0×). Adjusted after every block from how full it was.
    pub congestion_bp: u64,
}

impl ChainGlobal {
    pub fn circulating(&self) -> u64 {
        self.emitted.saturating_sub(self.burned)
    }
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("state backend error: {0}")]
pub struct StateError(pub String);

/// Read access to a state snapshot.
pub trait StateReader {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError>;
}

impl StateReader for BTreeMap<Vec<u8>, Vec<u8>> {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError> {
        Ok(self.get(key).cloned())
    }
}

/// One changed record: `(key, old value, new value)`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StateChange {
    pub key: Vec<u8>,
    pub old: Option<Vec<u8>>,
    pub new: Option<Vec<u8>>,
}

/// Copy-on-write view over a [`StateReader`]. Changes are buffered in memory
/// and can be merged into a parent overlay (sub-transactions) or turned into a
/// list of [`StateChange`]s for the storage layer.
pub struct Overlay<'a, R: StateReader + ?Sized> {
    base: &'a R,
    changes: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl<R: StateReader + ?Sized> StateReader for Overlay<'_, R> {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError> {
        match self.changes.get(key) {
            Some(v) => Ok(v.clone()),
            None => self.base.get_raw(key),
        }
    }
}

fn decode<T: BorshDeserialize>(bytes: &[u8]) -> Result<T, StateError> {
    T::try_from_slice(bytes).map_err(|e| StateError(format!("corrupt state record: {e}")))
}

impl<'a, R: StateReader + ?Sized> Overlay<'a, R> {
    pub fn new(base: &'a R) -> Self {
        Overlay { base, changes: BTreeMap::new() }
    }

    pub fn put_raw(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.changes.insert(key, Some(value));
    }

    pub fn delete_raw(&mut self, key: Vec<u8>) {
        self.changes.insert(key, None);
    }

    /// Moves all buffered changes of `child` into this overlay.
    pub fn absorb(&mut self, changes: BTreeMap<Vec<u8>, Option<Vec<u8>>>) {
        self.changes.extend(changes);
    }

    pub fn into_changes(self) -> BTreeMap<Vec<u8>, Option<Vec<u8>>> {
        self.changes
    }

    pub fn is_dirty(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Resolves buffered changes against the base, dropping no-op writes.
    pub fn diff(&self) -> Result<Vec<StateChange>, StateError> {
        let mut out = Vec::with_capacity(self.changes.len());
        for (k, new) in &self.changes {
            let old = self.base.get_raw(k)?;
            if &old != new {
                out.push(StateChange { key: k.clone(), old, new: new.clone() });
            }
        }
        Ok(out)
    }

    // ---- typed helpers ----------------------------------------------------

    pub fn get<T: BorshDeserialize>(&self, key: &[u8]) -> Result<Option<T>, StateError> {
        match self.get_raw(key)? {
            Some(b) => Ok(Some(decode(&b)?)),
            None => Ok(None),
        }
    }

    pub fn put<T: BorshSerialize>(&mut self, key: Vec<u8>, value: &T) {
        self.put_raw(key, borsh::to_vec(value).expect("serialization cannot fail"));
    }

    pub fn account(&self, a: &Address) -> Result<Account, StateError> {
        Ok(self.get(&account_key(a))?.unwrap_or_default())
    }

    pub fn put_account(&mut self, a: &Address, acc: &Account) {
        if acc.is_empty() {
            self.delete_raw(account_key(a));
        } else {
            self.put(account_key(a), acc);
        }
    }

    pub fn global(&self) -> Result<ChainGlobal, StateError> {
        self.get(&global_key())?.ok_or_else(|| StateError("missing chain global record".into()))
    }

    pub fn put_global(&mut self, g: &ChainGlobal) {
        self.put(global_key(), g);
    }

    pub fn contract(&self, id: &Hash32) -> Result<Option<Contract>, StateError> {
        self.get(&contract_key(id))
    }

    pub fn program_meta(&self, a: &Address) -> Result<Option<ProgramMeta>, StateError> {
        self.get(&program_meta_key(a))
    }

    pub fn proposal(&self, id: &Hash32) -> Result<Option<Proposal>, StateError> {
        self.get(&proposal_key(id))
    }

    pub fn vote(&self, id: &Hash32, voter: &Address) -> Result<Option<VoteRecord>, StateError> {
        self.get(&vote_key(id, voter))
    }
}

/// Convenience typed reads on any [`StateReader`] (used by RPC/wallets).
pub fn read_typed<T: BorshDeserialize, R: StateReader + ?Sized>(r: &R, key: &[u8]) -> Result<Option<T>, StateError> {
    match r.get_raw(key)? {
        Some(b) => Ok(Some(decode(&b)?)),
        None => Ok(None),
    }
}

/// Applies a list of changes to an LtHash commitment.
pub fn apply_changes_to_lthash(h: &mut LtHash, changes: &[StateChange]) {
    for c in changes {
        h.apply_change(&c.key, c.old.as_deref(), c.new.as_deref());
    }
}

/// Reverts a list of changes from an LtHash commitment.
pub fn revert_changes_from_lthash(h: &mut LtHash, changes: &[StateChange]) {
    for c in changes.iter().rev() {
        h.apply_change(&c.key, c.new.as_deref(), c.old.as_deref());
    }
}
