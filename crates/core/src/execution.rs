//! The state-transition function.
//!
//! Block processing order (consensus-critical):
//!
//! 1. **begin** — mining-reward cooldown: the first quarter of the reward of
//!    block `height - coinbase_maturity` and the rest of the reward of block
//!    `height - reward_unlock_blocks` become spendable by their miners.
//! 2. **transactions** — applied in block order. Native transactions are
//!    atomic (an invalid one invalidates the block). TCCL contract
//!    transactions always pay their fee; if the contract code fails, every
//!    other effect is reverted and the receipt records the failure.
//! 3. **end** — governance (miner signals, vote closing, activations), the
//!    congestion multiplier update, then `subsidy + miner fees` of this block
//!    is stored as a pending reward.
//!
//! Fees: `required = (base_fee + fee_per_kb·kB + fee_per_kfuel·kfuel) ×
//! congestion`. The congestion surcharge (`required − required at 1×`) is
//! **burned**, so miners gain nothing by filling blocks with their own
//! transactions to push fees up; anything paid above `required` is a priority
//! tip that goes to the miner.
//!
//! Header-level rules (PoW, timestamps, difficulty, state root) are checked by
//! the node's chain manager using [`crate::difficulty`] and [`crate::lthash`].

use crate::address::Address;
use crate::block::Block;
use crate::contracts::{self, credit};
use crate::emission::block_subsidy;
use crate::error::{BlockError, TxError};
use crate::governance::{self, assigned_signal_mask, GovParams};
use crate::hash::Hash32;
use crate::params::{
    ChainParams, CONGESTION_MAX_BP, CONGESTION_MIN_BP, MAX_BATCH_OUTPUTS, MAX_DEPLOY_TX_BYTES, MAX_MEMO_BYTES, MAX_TX_BYTES, MAX_TX_FUEL,
};
use crate::programs;
use crate::state::{pending_reward_key, Overlay, Payout, PendingReward, StateReader};
use crate::tx::{Transaction, TxAction, KNOWN_FLAGS, TX_VERSION};
use std::collections::HashSet;

/// An event emitted by a TCCL contract.
#[derive(Clone, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct LogEntry {
    pub contract: Address,
    pub event: String,
    pub fields: Vec<(String, tccl::Value)>,
}

/// Effects of a transaction, used for indexing, explorers and wallets.
#[derive(Clone, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct TxReceipt {
    pub txid: Hash32,
    /// Total fee paid by the sender.
    pub fee: u64,
    /// Part of the fee burned (congestion surcharge).
    pub burned: u64,
    /// Every address whose balance/role is affected (sender first).
    pub touched: Vec<Address>,
    /// Native contract or proposal created by this transaction, if any.
    pub created: Option<Hash32>,
    /// TCCL contract deployed by this transaction, if any.
    pub program: Option<Address>,
    /// False only for TCCL transactions whose code failed (fee still charged).
    pub success: bool,
    pub error: Option<String>,
    pub fuel_used: u64,
    pub logs: Vec<LogEntry>,
    pub return_value: Option<tccl::Value>,
}

impl TxReceipt {
    /// Fee going to the miner.
    pub fn miner_fee(&self) -> u64 {
        self.fee - self.burned
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlockReceipt {
    pub txs: Vec<TxReceipt>,
    /// Fees going to the miner (after burning).
    pub fees: u64,
    pub burned: u64,
    pub subsidy: u64,
    /// Rewards released to miners at the beginning of this block.
    pub released: u64,
}

/// Resource usage of a block (drives the congestion multiplier).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockUsage {
    pub bytes: u64,
    pub fuel: u64,
}

fn ceil_div(a: u128, b: u128) -> u128 {
    a.div_ceil(b)
}

/// Minimum fee of a transaction of `size` bytes reserving `max_fuel` fuel.
/// Returns `(required fee, required fee at 1× congestion)`.
pub fn required_fee(params: &GovParams, congestion_bp: u64, size: usize, max_fuel: u64) -> (u64, u64) {
    let base = params.base_fee as u128
        + ceil_div(size as u128 * params.fee_per_kb as u128, 1_000)
        + ceil_div(max_fuel as u128 * params.fee_per_kfuel as u128, 1_000);
    let required = ceil_div(base * congestion_bp.max(CONGESTION_MIN_BP) as u128, 10_000);
    (required.min(u64::MAX as u128) as u64, base.min(u64::MAX as u128) as u64)
}

/// Congestion multiplier for the next block given this block's usage.
/// Target usage is 50% of the limits; the multiplier moves at most 12.5% per block.
pub fn next_congestion(current_bp: u64, usage: BlockUsage, params: &GovParams) -> u64 {
    let fill = |used: u64, max: u64| -> u128 { (used as u128 * 10_000 / max.max(1) as u128).min(10_000) };
    let fill_bp = fill(usage.bytes, params.max_block_bytes).max(fill(usage.fuel, params.max_block_fuel));
    let target: u128 = 5_000;
    let cur = current_bp.max(CONGESTION_MIN_BP) as u128;
    let next = if fill_bp > target {
        let delta = (cur * (fill_bp - target) / target / 8).max(1);
        cur + delta
    } else if fill_bp < target {
        let delta = cur * (target - fill_bp) / target / 8;
        cur.saturating_sub(delta)
    } else {
        cur
    };
    next.clamp(CONGESTION_MIN_BP as u128, CONGESTION_MAX_BP as u128) as u64
}

/// Checks that do not depend on the state. Returns the serialized size.
pub fn check_tx_stateless(p: &ChainParams, tx: &Transaction) -> Result<usize, TxError> {
    check_tx_context_free(p, tx, true)
}

/// Like [`check_tx_stateless`]; `verify_sig = false` skips only the signature
/// check (for transactions whose signature was verified before).
pub fn check_tx_context_free(p: &ChainParams, tx: &Transaction, verify_sig: bool) -> Result<usize, TxError> {
    let size = tx.size();
    let max = if matches!(tx.body.action, TxAction::Deploy { .. }) { MAX_DEPLOY_TX_BYTES } else { MAX_TX_BYTES };
    if size > max {
        return Err(TxError::TooLarge { size, max });
    }
    if tx.body.version != TX_VERSION {
        return Err(TxError::BadVersion(tx.body.version));
    }
    if tx.body.chain_id != p.chain_id {
        return Err(TxError::WrongChain { expected: p.chain_id, got: tx.body.chain_id });
    }
    if tx.body.flags & !KNOWN_FLAGS != 0 {
        return Err(TxError::BadFlags(tx.body.flags));
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
        TxAction::Deploy { source, init_args, max_fuel, .. } => {
            if source.trim().is_empty() {
                return Err(TxError::BadContractTx("empty source code".into()));
            }
            check_fuel(*max_fuel)?;
            check_args(init_args)?;
        }
        TxAction::Invoke { function, args, max_fuel, .. } => {
            if function.is_empty() || function.len() > 64 {
                return Err(TxError::BadContractTx("function name must be 1..=64 bytes".into()));
            }
            check_fuel(*max_fuel)?;
            check_args(args)?;
        }
    }
    if verify_sig && !tx.verify_signature() {
        return Err(TxError::BadSignature);
    }
    Ok(size)
}

fn check_fuel(max_fuel: u64) -> Result<(), TxError> {
    if max_fuel == 0 || max_fuel > MAX_TX_FUEL {
        return Err(TxError::BadContractTx(format!("max_fuel must be 1..={MAX_TX_FUEL}")));
    }
    Ok(())
}

fn check_args(args: &[tccl::Value]) -> Result<(), TxError> {
    if args.len() > 32 {
        return Err(TxError::BadContractTx("at most 32 arguments".into()));
    }
    Ok(())
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
    let (required, base) = required_fee(&g.params, g.congestion_bp, size, tx.max_fuel());
    if body.fee < required {
        return Err(TxError::FeeTooLow { min: required, got: body.fee });
    }
    let burned = required - base;

    let sender = tx.sender();
    let mut acc = state.account(&sender)?;
    if body.nonce != acc.nonce {
        return Err(TxError::BadNonce { expected: acc.nonce, got: body.nonce });
    }

    let is_program_tx = matches!(body.action, TxAction::Deploy { .. } | TxAction::Invoke { .. });
    let debit_amount: u64 = match &body.action {
        TxAction::Transfer { amount, .. } => *amount,
        TxAction::BatchTransfer { outputs, .. } => {
            outputs.iter().try_fold(0u64, |a, o| a.checked_add(o.amount)).ok_or(TxError::Overflow)?
        }
        TxAction::CreateContract { spec } => spec.funding().ok_or(TxError::Overflow)?,
        TxAction::CallContract { call, .. } => call.deposit_amount(),
        TxAction::Propose { .. } => g.params.proposal_deposit,
        TxAction::Vote { .. } => 0,
        TxAction::Deploy { value, .. } | TxAction::Invoke { value, .. } => *value,
    };
    let needed = debit_amount.checked_add(body.fee).ok_or(TxError::Overflow)?;
    let available = acc.spendable(height);
    if needed > available {
        return Err(TxError::InsufficientFunds { needed, available });
    }
    // Contract transactions move `value` inside the revertible execution.
    acc.balance -= if is_program_tx { body.fee } else { needed };
    acc.nonce = acc.nonce.checked_add(1).ok_or(TxError::Overflow)?;
    state.put_account(&sender, &acc);

    let mut receipt = TxReceipt {
        txid: tx.txid(),
        fee: body.fee,
        burned,
        touched: vec![sender],
        created: None,
        program: None,
        success: true,
        error: None,
        fuel_used: 0,
        logs: Vec::new(),
        return_value: None,
    };
    match &body.action {
        TxAction::Transfer { to, amount, .. } => {
            credit(state, to, *amount)?;
            receipt.touched.push(*to);
        }
        TxAction::BatchTransfer { outputs, .. } => {
            for o in outputs {
                credit(state, &o.to, o.amount)?;
                receipt.touched.push(o.to);
            }
        }
        TxAction::CreateContract { spec } => {
            let (id, parties) = contracts::create(state, height, sender, body.nonce, spec, g.params.storage_deposit_per_kb)?;
            g_increment_contracts(state)?;
            receipt.touched.extend(parties);
            receipt.created = Some(id);
        }
        TxAction::CallContract { contract, call } => {
            receipt.touched.extend(contracts::call(state, height, sender, contract, call)?);
        }
        TxAction::Propose { proposal } => {
            let id = governance::propose(state, &p.gov_bounds, height, sender, body.nonce, proposal, g.params.proposal_deposit)?;
            receipt.created = Some(id);
        }
        TxAction::Vote { proposal, choice, weight } => {
            governance::vote(state, height, sender, proposal, *choice, *weight)?;
        }
        TxAction::Deploy { source, init_args, value, max_fuel, max_deposit } => {
            let call = programs::ProgramCall {
                sender,
                height,
                value: *value,
                max_fuel: *max_fuel,
                max_deposit: *max_deposit,
                deposit_per_kb: g.params.storage_deposit_per_kb,
            };
            let out = programs::deploy(p, state, &call, body.nonce, receipt.txid, source, init_args.clone())?;
            if out.success {
                g_increment_contracts(state)?;
            }
            out.fill(&mut receipt);
        }
        TxAction::Invoke { contract, function, args, value, max_fuel, max_deposit } => {
            let call = programs::ProgramCall {
                sender,
                height,
                value: *value,
                max_fuel: *max_fuel,
                max_deposit: *max_deposit,
                deposit_per_kb: g.params.storage_deposit_per_kb,
            };
            let out = programs::invoke(state, &call, contract, function, args.clone())?;
            out.fill(&mut receipt);
        }
    }
    let first = receipt.touched[0];
    receipt.touched.sort();
    receipt.touched.dedup();
    receipt.touched.retain(|a| *a != first);
    receipt.touched.insert(0, first);
    Ok(receipt)
}

/// Like [`apply_tx`] but **without running contract code** — for mempool
/// admission and re-validation, where executing every pending call would let
/// anyone make nodes burn CPU for free.
///
/// Native transactions are applied exactly as in a block (they are cheap). For
/// `Deploy`/`Invoke` it makes every check `apply_tx` makes before execution
/// (expiry, minimum fee, nonce, spendable funds for fee + value) and applies
/// the same account effects (fee and value debited, nonce incremented). The
/// storage deposit and the contract's own effects are only known when the
/// transaction is executed in a block, where a failing call still pays its fee.
pub fn apply_tx_unexecuted<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    height: u64,
    tx: &Transaction,
    size: usize,
) -> Result<(), TxError> {
    let value = match &tx.body.action {
        TxAction::Deploy { value, .. } | TxAction::Invoke { value, .. } => *value,
        _ => return apply_tx(p, state, height, tx, size).map(|_| ()),
    };
    let body = &tx.body;
    if body.expiry_height != 0 && height > body.expiry_height {
        return Err(TxError::Expired { expiry: body.expiry_height });
    }
    let g = state.global()?;
    let (required, _) = required_fee(&g.params, g.congestion_bp, size, tx.max_fuel());
    if body.fee < required {
        return Err(TxError::FeeTooLow { min: required, got: body.fee });
    }
    let sender = tx.sender();
    let mut acc = state.account(&sender)?;
    if body.nonce != acc.nonce {
        return Err(TxError::BadNonce { expected: acc.nonce, got: body.nonce });
    }
    let needed = value.checked_add(body.fee).ok_or(TxError::Overflow)?;
    let available = acc.spendable(height);
    if needed > available {
        return Err(TxError::InsufficientFunds { needed, available });
    }
    acc.balance -= needed;
    acc.nonce = acc.nonce.checked_add(1).ok_or(TxError::Overflow)?;
    state.put_account(&sender, &acc);
    Ok(())
}

fn g_increment_contracts<R: StateReader + ?Sized>(state: &mut Overlay<'_, R>) -> Result<(), TxError> {
    let mut g = state.global()?;
    g.contract_count += 1;
    state.put_global(&g);
    Ok(())
}

/// Step 1 of block processing: reward cooldown. Returns the motes released.
pub fn begin_block<R: StateReader + ?Sized>(p: &ChainParams, state: &mut Overlay<'_, R>, height: u64) -> Result<u64, BlockError> {
    let mut released = 0u64;
    let werr = |e: TxError| BlockError::Governance(e.to_string());
    // Early quarter.
    if height > p.coinbase_maturity {
        let key = pending_reward_key(height - p.coinbase_maturity);
        if let Some(mut reward) = state.get::<PendingReward>(&key)? {
            let mut changed = false;
            for payout in reward.payouts.iter_mut() {
                let early = payout.early_part();
                if payout.released == 0 && early > 0 {
                    credit(state, &payout.who, early).map_err(werr)?;
                    payout.released = early;
                    released += early;
                    changed = true;
                }
            }
            if changed {
                state.put(key, &reward);
            }
        }
    }
    // Rest, after the full cooldown.
    if height > p.reward_unlock_blocks {
        let key = pending_reward_key(height - p.reward_unlock_blocks);
        if let Some(reward) = state.get::<PendingReward>(&key)? {
            state.delete_raw(key);
            for payout in &reward.payouts {
                let rest = payout.amount - payout.released;
                if rest > 0 {
                    credit(state, &payout.who, rest).map_err(werr)?;
                    released += rest;
                }
            }
        }
    }
    Ok(released)
}

/// Step 3 of block processing. Returns the subsidy created.
#[allow(clippy::too_many_arguments)]
pub fn end_block<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    height: u64,
    miner: &Address,
    signal: u32,
    miner_fees: u64,
    burned: u64,
    usage: BlockUsage,
    // Uncles included by this block: who mined each and how many blocks old it is.
    uncles: &[(Address, u64)],
) -> Result<u64, BlockError> {
    let gerr = |m: &str| BlockError::Governance(m.to_string());
    // The congestion multiplier is driven by the limits in force for this block.
    let params_before = state.global()?.params;
    governance::end_block(state, &p.gov_bounds, height, signal)?;
    let subsidy = block_subsidy(p, height);
    let mut g = state.global()?;
    g.emitted = g.emitted.checked_add(subsidy).ok_or_else(|| gerr("emission overflow"))?;
    if g.emitted > crate::params::MAX_SUPPLY {
        return Err(gerr("supply cap exceeded"));
    }
    g.burned = g.burned.checked_add(burned).ok_or_else(|| gerr("burn overflow"))?;
    g.congestion_bp = next_congestion(g.congestion_bp, usage, &params_before);
    state.put_global(&g);
    // Uncles are paid out of this block's subsidy, so the emission schedule and
    // the 50 000 000 TCN cap do not change (docs/ESCALA.md §2.3).
    let mut payouts: Vec<Payout> = Vec::with_capacity(1 + uncles.len());
    let mut to_uncles = 0u64;
    for (who, depth) in uncles {
        let amount = crate::block::uncle_reward(subsidy, *depth);
        if amount > 0 {
            to_uncles = to_uncles.checked_add(amount).ok_or_else(|| gerr("uncle reward overflow"))?;
            payouts.push(Payout { who: *who, amount, released: 0 });
        }
    }
    if to_uncles > subsidy {
        return Err(gerr("uncle rewards exceed the subsidy"));
    }
    let mine = (subsidy - to_uncles).checked_add(miner_fees).ok_or_else(|| gerr("reward overflow"))?;
    if mine > 0 {
        payouts.insert(0, Payout { who: *miner, amount: mine, released: 0 });
    }
    if !payouts.is_empty() {
        state.put(pending_reward_key(height), &PendingReward { payouts });
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
    let fuel: u64 = block.txs.iter().try_fold(0u64, |a, t| a.checked_add(t.max_fuel())).ok_or(BlockError::FuelLimit)?;
    if fuel > g.params.max_block_fuel {
        return Err(BlockError::FuelLimit);
    }
    let mask = assigned_signal_mask(&g.voting);
    if h.signal & !mask != 0 {
        return Err(BlockError::BadSignal(h.signal));
    }
    // Structure of the uncles (the chain manager also checks their proof of
    // work and that they really fork off this chain).
    if block.uncles.len() > crate::block::MAX_UNCLES {
        return Err(BlockError::BadUncle(format!("at most {} uncles per block", crate::block::MAX_UNCLES)));
    }
    if block.compute_uncles_root() != h.uncles_root {
        return Err(BlockError::BadUncle("uncles root does not match the header".into()));
    }
    let mut uncles: Vec<(Address, u64)> = Vec::with_capacity(block.uncles.len());
    for u in &block.uncles {
        if u.height >= height || height - u.height > crate::block::MAX_UNCLE_DEPTH {
            return Err(BlockError::BadUncle(format!("uncle at height {} cannot be included at {height}", u.height)));
        }
        if block.uncles.iter().filter(|o| o.hash() == u.hash()).count() > 1 {
            return Err(BlockError::BadUncle("the same uncle twice".into()));
        }
        uncles.push((u.miner, height - u.height));
    }

    let mut receipt = BlockReceipt { released: begin_block(p, state, height)?, ..Default::default() };

    let mut seen = HashSet::with_capacity(block.txs.len());
    for (index, tx) in block.txs.iter().enumerate() {
        let txid = tx.txid();
        if !seen.insert(txid) {
            return Err(BlockError::DuplicateTx(txid));
        }
        let size = check_tx_context_free(p, tx, !verified_sigs).map_err(|error| BlockError::Tx { index, txid, error })?;
        let r = apply_tx(p, state, height, tx, size).map_err(|error| BlockError::Tx { index, txid, error })?;
        receipt.fees = receipt.fees.checked_add(r.miner_fee()).ok_or(BlockError::Governance("fee overflow".into()))?;
        receipt.burned = receipt.burned.checked_add(r.burned).ok_or(BlockError::Governance("burn overflow".into()))?;
        receipt.txs.push(r);
    }

    receipt.subsidy =
        end_block(p, state, height, &h.miner, h.signal, receipt.fees, receipt.burned, BlockUsage { bytes: size, fuel }, &uncles)?;
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
    pub burned: u64,
    size: u64,
    fuel: u64,
    max_bytes: u64,
    max_fuel: u64,
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
            burned: 0,
            size: EMPTY_BLOCK_BYTES,
            fuel: 0,
            max_bytes: g.params.max_block_bytes,
            max_fuel: g.params.max_block_fuel,
            signal_mask,
        })
    }

    /// Signal bits assigned to voting proposals.
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
        if self.fuel + tx.max_fuel() > self.max_fuel {
            return Err(TxError::BadContractTx("block fuel limit reached".into()));
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
        self.fees = self.fees.checked_add(receipt.miner_fee()).ok_or(TxError::Overflow)?;
        self.burned = self.burned.checked_add(receipt.burned).ok_or(TxError::Overflow)?;
        self.size += size as u64;
        self.fuel += tx.max_fuel();
        self.txs.push(tx.clone());
        self.receipts.push(receipt);
        Ok(())
    }

    /// Runs end-of-block processing. `signal` is masked to assigned bits.
    pub fn finish(
        mut self,
        miner: &Address,
        signal: u32,
        uncles: &[(Address, u64)],
    ) -> Result<(Overlay<'a, R>, Vec<Transaction>, u32), BlockError> {
        let signal = signal & self.signal_mask;
        let usage = BlockUsage { bytes: self.size, fuel: self.fuel };
        end_block(self.p, &mut self.state, self.height, miner, signal, self.fees, self.burned, usage, uncles)?;
        Ok((self.state, self.txs, signal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MAINNET;

    #[test]
    fn fee_formula() {
        let params = MAINNET.gov_defaults;
        // base 1000 + 158 bytes × 10 = 2580
        assert_eq!(required_fee(&params, 10_000, 158, 0), (2_580, 2_580));
        // 2× congestion doubles, the surcharge is burned
        assert_eq!(required_fee(&params, 20_000, 158, 0), (5_160, 2_580));
        // fuel: 1 001 units × 1 000 motes / 1 000 units = 1 001 motes
        assert_eq!(required_fee(&params, 10_000, 158, 1_001), (3_581, 3_581));
    }

    #[test]
    fn congestion_moves_gradually() {
        let params = MAINNET.gov_defaults;
        let full = BlockUsage { bytes: params.max_block_bytes, fuel: 0 };
        let empty = BlockUsage::default();
        let mut c = CONGESTION_MIN_BP;
        for _ in 0..10 {
            c = next_congestion(c, full, &params);
        }
        assert!(c > 30_000 && c < 33_000, "{c}"); // 1.125^10 ≈ 3.25
        for _ in 0..200 {
            c = next_congestion(c, empty, &params);
        }
        assert_eq!(c, CONGESTION_MIN_BP);
        let half = BlockUsage { bytes: params.max_block_bytes / 2, fuel: 0 };
        assert_eq!(next_congestion(40_000, half, &params), 40_000);
        let mut c = CONGESTION_MIN_BP;
        for _ in 0..10_000 {
            c = next_congestion(c, full, &params);
        }
        assert_eq!(c, CONGESTION_MAX_BP);
    }
}
