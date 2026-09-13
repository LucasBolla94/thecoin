//! The state-transition function.
//!
//! Block processing order (consensus-critical):
//!
//! 1. **begin** — the reward of block `height - coinbase_maturity` becomes
//!    spendable by its miner.
//! 2. **transactions** — applied in block order; any invalid transaction
//!    invalidates the whole block.
//! 3. **end** — governance (miner signals, vote closing, activations), then
//!    `subsidy + fees` of this block is stored as a pending reward.
//!
//! Header-level rules (PoW, timestamps, difficulty, state root) are checked by
//! the node's chain manager using [`crate::difficulty`] and [`crate::lthash`].

use crate::address::Address;
use crate::block::Block;
use crate::contracts::{self, credit};
use crate::emission::block_subsidy;
use crate::error::{BlockError, TxError};
use crate::governance::{self, assigned_signal_mask};
use crate::hash::Hash32;
use crate::params::{ChainParams, MAX_BATCH_OUTPUTS, MAX_MEMO_BYTES, MAX_TX_BYTES};
use crate::state::{pending_reward_key, Overlay, PendingReward, StateReader};
use crate::tx::{Transaction, TxAction, TX_VERSION};
use std::collections::HashSet;

/// Effects of a transaction, used for indexing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxReceipt {
    pub txid: Hash32,
    pub fee: u64,
    /// Every address whose balance/role is affected (sender first).
    pub touched: Vec<Address>,
    /// Contract or proposal created by this transaction, if any.
    pub created: Option<Hash32>,
}

#[derive(Clone, Debug, Default)]
pub struct BlockReceipt {
    pub txs: Vec<TxReceipt>,
    pub fees: u64,
    pub subsidy: u64,
    pub matured: Option<PendingReward>,
}

/// Checks that do not depend on the state. Returns the serialized size.
pub fn check_tx_stateless(p: &ChainParams, tx: &Transaction) -> Result<usize, TxError> {
    check_tx_context_free(p, tx, true)
}

/// Like [`check_tx_stateless`]; `verify_sig = false` skips only the signature
/// check (for transactions whose signature was verified before).
pub fn check_tx_context_free(p: &ChainParams, tx: &Transaction, verify_sig: bool) -> Result<usize, TxError> {
    let size = tx.size();
    if size > MAX_TX_BYTES {
        return Err(TxError::TooLarge { size, max: MAX_TX_BYTES });
    }
    if tx.body.version != TX_VERSION {
        return Err(TxError::BadVersion(tx.body.version));
    }
    if tx.body.chain_id != p.chain_id {
        return Err(TxError::WrongChain { expected: p.chain_id, got: tx.body.chain_id });
    }
    match &tx.body.action {
        TxAction::Transfer { amount, memo, .. } => {
            if *amount == 0 {
                return Err(TxError::ZeroAmount);
            }
            if memo.len() > MAX_MEMO_BYTES {
                return Err(TxError::MemoTooLong(memo.len()));
            }
        }
        TxAction::BatchTransfer { outputs, memo } => {
            if outputs.is_empty() {
                return Err(TxError::BadBatch("no outputs"));
            }
            if outputs.len() > MAX_BATCH_OUTPUTS {
                return Err(TxError::BadBatch("too many outputs"));
            }
            if outputs.iter().any(|o| o.amount == 0) {
                return Err(TxError::ZeroAmount);
            }
            if memo.len() > MAX_MEMO_BYTES {
                return Err(TxError::MemoTooLong(memo.len()));
            }
            outputs.iter().try_fold(0u64, |a, o| a.checked_add(o.amount)).ok_or(TxError::Overflow)?;
        }
        TxAction::CreateContract { spec } => spec.validate_static()?,
        TxAction::CallContract { call, .. } => call.validate_static()?,
        TxAction::Propose { proposal } => proposal.validate_static()?,
        TxAction::Vote { weight, .. } => {
            if *weight == 0 {
                return Err(TxError::ZeroAmount);
            }
        }
    }
    if verify_sig && !tx.verify_signature() {
        return Err(TxError::BadSignature);
    }
    Ok(size)
}

/// Applies a transaction whose stateless checks already passed.
pub fn apply_tx<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    height: u64,
    tx: &Transaction,
    size: usize,
) -> Result<TxReceipt, TxError> {
    let body = &tx.body;
    if body.expiry_height != 0 && height > body.expiry_height {
        return Err(TxError::Expired { expiry: body.expiry_height });
    }
    let g = state.global()?;
    let min_fee = g.params.min_fee_per_byte.checked_mul(size as u64).ok_or(TxError::Overflow)?;
    if body.fee < min_fee {
        return Err(TxError::FeeTooLow { min: min_fee, got: body.fee });
    }

    let sender = tx.sender();
    let mut acc = state.account(&sender)?;
    if body.nonce != acc.nonce {
        return Err(TxError::BadNonce { expected: acc.nonce, got: body.nonce });
    }

    let debit_amount: u64 = match &body.action {
        TxAction::Transfer { amount, .. } => *amount,
        TxAction::BatchTransfer { outputs, .. } => outputs.iter().try_fold(0u64, |a, o| a.checked_add(o.amount)).ok_or(TxError::Overflow)?,
        TxAction::CreateContract { spec } => spec.funding().ok_or(TxError::Overflow)?,
        TxAction::CallContract { call, .. } => call.deposit_amount(),
        TxAction::Propose { .. } => g.params.proposal_deposit,
        TxAction::Vote { .. } => 0,
    };
    let needed = debit_amount.checked_add(body.fee).ok_or(TxError::Overflow)?;
    let available = acc.spendable(height);
    if needed > available {
        return Err(TxError::InsufficientFunds { needed, available });
    }
    acc.balance -= needed;
    acc.nonce = acc.nonce.checked_add(1).ok_or(TxError::Overflow)?;
    state.put_account(&sender, &acc);

    let mut touched = vec![sender];
    let mut created = None;
    match &body.action {
        TxAction::Transfer { to, amount, .. } => {
            credit(state, to, *amount)?;
            touched.push(*to);
        }
        TxAction::BatchTransfer { outputs, .. } => {
            for o in outputs {
                credit(state, &o.to, o.amount)?;
                touched.push(o.to);
            }
        }
        TxAction::CreateContract { spec } => {
            let (id, parties) = contracts::create(state, height, sender, body.nonce, spec)?;
            g_increment_contracts(state)?;
            touched.extend(parties);
            created = Some(id);
        }
        TxAction::CallContract { contract, call } => {
            touched.extend(contracts::call(state, height, sender, contract, call)?);
        }
        TxAction::Propose { proposal } => {
            let id = governance::propose(state, &p.gov_bounds, height, sender, body.nonce, proposal, g.params.proposal_deposit)?;
            created = Some(id);
        }
        TxAction::Vote { proposal, choice, weight } => {
            governance::vote(state, height, sender, proposal, *choice, *weight)?;
        }
    }
    let first = touched[0];
    touched.sort();
    touched.dedup();
    touched.retain(|a| *a != first);
    touched.insert(0, first);
    Ok(TxReceipt { txid: tx.txid(), fee: body.fee, touched, created })
}

fn g_increment_contracts<R: StateReader + ?Sized>(state: &mut Overlay<'_, R>) -> Result<(), TxError> {
    let mut g = state.global()?;
    g.contract_count += 1;
    state.put_global(&g);
    Ok(())
}

/// Step 1 of block processing. Returns the matured reward, if any.
pub fn begin_block<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    height: u64,
) -> Result<Option<PendingReward>, BlockError> {
    if height <= p.coinbase_maturity {
        return Ok(None);
    }
    let key = pending_reward_key(height - p.coinbase_maturity);
    let Some(reward) = state.get::<PendingReward>(&key)? else {
        return Ok(None);
    };
    state.delete_raw(key);
    credit(state, &reward.miner, reward.amount).map_err(|e| BlockError::Governance(e.to_string()))?;
    Ok(Some(reward))
}

/// Step 3 of block processing. Returns the subsidy created.
pub fn end_block<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    height: u64,
    miner: &Address,
    signal: u32,
    fees: u64,
) -> Result<u64, BlockError> {
    governance::end_block(state, &p.gov_bounds, height, signal)?;
    let subsidy = block_subsidy(p, height);
    let mut g = state.global()?;
    g.emitted = g.emitted.checked_add(subsidy).ok_or(BlockError::Governance("emission overflow".into()))?;
    if g.emitted > crate::params::MAX_SUPPLY {
        return Err(BlockError::Governance("supply cap exceeded".into()));
    }
    state.put_global(&g);
    let total = subsidy.checked_add(fees).ok_or(BlockError::Governance("reward overflow".into()))?;
    if total > 0 {
        state.put(pending_reward_key(height), &PendingReward { miner: *miner, amount: total });
    }
    Ok(subsidy)
}

/// Signal bits a block at this state may use.
pub fn allowed_signal_mask<R: StateReader + ?Sized>(state: &Overlay<'_, R>) -> Result<u32, BlockError> {
    Ok(assigned_signal_mask(&state.global()?.voting))
}

/// Applies a full block body (header-level checks must be done by the caller).
///
/// `verified_sigs`: set to `true` only if every transaction signature was
/// already verified (e.g. by the mempool or a parallel pre-check).
pub fn apply_block<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    block: &Block,
    verified_sigs: bool,
) -> Result<BlockReceipt, BlockError> {
    let h = &block.header;
    let height = h.height;
    let g = state.global()?;
    let size = block.serialized_size() as u64;
    if size > g.params.max_block_bytes {
        return Err(BlockError::TooLarge { size, max: g.params.max_block_bytes });
    }
    let mask = assigned_signal_mask(&g.voting);
    if h.signal & !mask != 0 {
        return Err(BlockError::BadSignal(h.signal));
    }

    let mut receipt = BlockReceipt { matured: begin_block(p, state, height)?, ..Default::default() };

    let mut seen = HashSet::with_capacity(block.txs.len());
    for (index, tx) in block.txs.iter().enumerate() {
        let txid = tx.txid();
        if !seen.insert(txid) {
            return Err(BlockError::DuplicateTx(txid));
        }
        let size = check_tx_context_free(p, tx, !verified_sigs).map_err(|error| BlockError::Tx { index, txid, error })?;
        let r = apply_tx(p, state, height, tx, size).map_err(|error| BlockError::Tx { index, txid, error })?;
        receipt.fees = receipt.fees.checked_add(r.fee).ok_or(BlockError::Governance("fee overflow".into()))?;
        receipt.txs.push(r);
    }

    receipt.subsidy = end_block(p, state, height, &h.miner, h.signal, receipt.fees)?;
    Ok(receipt)
}

/// Incremental block assembly for miners: begin → try_add(tx)* → finish.
pub struct BlockBuilder<'a, R: StateReader + ?Sized> {
    p: &'static ChainParams,
    pub state: Overlay<'a, R>,
    pub height: u64,
    pub txs: Vec<Transaction>,
    pub receipts: Vec<TxReceipt>,
    pub fees: u64,
    size: u64,
    max_bytes: u64,
    signal_mask: u32,
}

/// Borsh size of a block with no transactions (180-byte header + 4-byte vec length).
pub const EMPTY_BLOCK_BYTES: u64 = 184;

impl<'a, R: StateReader + ?Sized> BlockBuilder<'a, R> {
    pub fn new(p: &'static ChainParams, base: &'a R, height: u64) -> Result<Self, BlockError> {
        let mut state = Overlay::new(base);
        let g = state.global()?;
        let signal_mask = assigned_signal_mask(&g.voting);
        begin_block(p, &mut state, height)?;
        Ok(BlockBuilder {
            p,
            state,
            height,
            txs: Vec::new(),
            receipts: Vec::new(),
            fees: 0,
            size: EMPTY_BLOCK_BYTES,
            max_bytes: g.params.max_block_bytes,
            signal_mask,
        })
    }

    /// Proposals currently voting, `(bit, id)`, to choose signal bits from.
    pub fn signal_mask(&self) -> u32 {
        self.signal_mask
    }

    pub fn remaining_bytes(&self) -> u64 {
        self.max_bytes.saturating_sub(self.size)
    }

    /// Tries to add a transaction (signature assumed verified by the mempool).
    pub fn try_add(&mut self, tx: &Transaction) -> Result<(), TxError> {
        let size = tx.size();
        if self.size + size as u64 > self.max_bytes {
            return Err(TxError::TooLarge { size, max: self.remaining_bytes() as usize });
        }
        check_tx_context_free(self.p, tx, false)?;
        if self.txs.iter().any(|t| t == tx) {
            return Err(TxError::Contract("duplicate".into()));
        }
        let (changes, receipt) = {
            let mut child = Overlay::new(&self.state);
            let r = apply_tx(self.p, &mut child, self.height, tx, size)?;
            (child.into_changes(), r)
        };
        self.state.absorb(changes);
        self.fees = self.fees.checked_add(receipt.fee).ok_or(TxError::Overflow)?;
        self.size += size as u64;
        self.txs.push(tx.clone());
        self.receipts.push(receipt);
        Ok(())
    }

    /// Runs end-of-block processing. `signal` is masked to assigned bits.
    pub fn finish(mut self, miner: &Address, signal: u32) -> Result<(Overlay<'a, R>, Vec<Transaction>, u32), BlockError> {
        let signal = signal & self.signal_mask;
        end_block(self.p, &mut self.state, self.height, miner, signal, self.fees)?;
        Ok((self.state, self.txs, signal))
    }
}
