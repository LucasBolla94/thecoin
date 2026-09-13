//! Native **payment contracts**.
//!
//! Version 0.1 ships a set of audited, non-Turing-complete contract templates
//! instead of a general virtual machine. Their cost is fixed and predictable
//! (no gas, no infinite loops, no re-entrancy), which keeps validation cheap on
//! small machines. New templates — and later a WASM VM — can be added by
//! appending enum variants through a governance-approved upgrade.
//!
//! | Template       | Use case                                        |
//! |----------------|-------------------------------------------------|
//! | `Escrow`       | Buyer/seller with optional arbiter and deadline  |
//! | `Vesting`      | Salaries, token vesting with cliff (linear)      |
//! | `Subscription` | Recurring payments claimable once per period     |
//! | `Htlc`         | Hash time-locked payments, atomic swaps (SHA-256)|
//! | `Multisig`     | Shared vault, M-of-N approvals                   |
//!
//! Funds of a contract are held in the contract record (`balance`) and can
//! only leave it through the rules below. Finished contracts are deleted from
//! the state to keep it compact.

use crate::address::Address;
use crate::error::TxError;
use crate::hash::{tagged_hash, tags, Hash32};
use crate::params::MAX_MEMO_BYTES;
use crate::state::{contract_key, Overlay, StateReader};
use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest, Sha256};

pub const MAX_MULTISIG_SIGNERS: usize = 16;
pub const MAX_PENDING_SPENDS: usize = 16;
pub const MAX_HTLC_PREIMAGE: usize = 64;
pub const MAX_SUBSCRIPTION_PERIODS: u32 = 100_000;

/// Parameters to create a contract. **Variant order is consensus-critical.**
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ContractSpec {
    /// Sender (payer) locks `amount`. Payer or arbiter releases to payee;
    /// payee or arbiter refunds; payer may refund alone after `deadline_height`.
    Escrow { payee: Address, arbiter: Option<Address>, amount: u64, deadline_height: u64 },
    /// `amount` vests linearly from `start_height` to `end_height`; nothing is
    /// claimable before `cliff_height`. If `revocable`, the creator can stop it
    /// and recover the unvested part.
    Vesting { beneficiary: Address, amount: u64, start_height: u64, cliff_height: u64, end_height: u64, revocable: bool },
    /// Prepaid subscription: `amount_per_period * max_periods` is locked; the
    /// payee may claim one period immediately and one more every
    /// `period_blocks`. The payer can cancel and recover unclaimed future periods.
    Subscription { payee: Address, amount_per_period: u64, period_blocks: u64, max_periods: u32 },
    /// Pays `recipient` when someone reveals `preimage` with
    /// `SHA256(preimage) == hash_lock` up to `timeout_height`; afterwards the
    /// sender can take the funds back.
    Htlc { recipient: Address, amount: u64, hash_lock: Hash32, timeout_height: u64 },
    /// Vault controlled by `threshold` of `signers`.
    Multisig { signers: Vec<Address>, threshold: u8, initial_deposit: u64 },
}

/// Contract calls. **Variant order is consensus-critical.**
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ContractCall {
    EscrowRelease,
    EscrowRefund,
    VestingClaim,
    VestingRevoke,
    SubscriptionClaim,
    SubscriptionCancel,
    HtlcRedeem { preimage: Vec<u8> },
    HtlcRefund,
    MultisigDeposit { amount: u64 },
    MultisigPropose { to: Address, amount: u64, memo: Vec<u8> },
    MultisigApprove { spend_id: u32 },
    MultisigCancel { spend_id: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PendingSpend {
    pub id: u32,
    pub to: Address,
    pub amount: u64,
    pub memo: Vec<u8>,
    pub approvals: Vec<Address>,
    pub created_height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ContractState {
    Escrow { payer: Address, payee: Address, arbiter: Option<Address>, deadline_height: u64 },
    Vesting { beneficiary: Address, total: u64, claimed: u64, start_height: u64, cliff_height: u64, end_height: u64, revocable: bool },
    Subscription { payer: Address, payee: Address, amount_per_period: u64, period_blocks: u64, max_periods: u32, start_height: u64, claimed_periods: u32 },
    Htlc { sender: Address, recipient: Address, hash_lock: Hash32, timeout_height: u64 },
    Multisig { signers: Vec<Address>, threshold: u8, next_spend_id: u32, pending: Vec<PendingSpend> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Contract {
    pub id: Hash32,
    pub creator: Address,
    pub created_height: u64,
    /// Motes held by the contract.
    pub balance: u64,
    pub state: ContractState,
}

impl ContractSpec {
    /// Coins moved from the creator into the contract.
    pub fn funding(&self) -> Option<u64> {
        match self {
            ContractSpec::Escrow { amount, .. } | ContractSpec::Vesting { amount, .. } | ContractSpec::Htlc { amount, .. } => Some(*amount),
            ContractSpec::Subscription { amount_per_period, max_periods, .. } => amount_per_period.checked_mul(*max_periods as u64),
            ContractSpec::Multisig { initial_deposit, .. } => Some(*initial_deposit),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ContractSpec::Escrow { .. } => "escrow",
            ContractSpec::Vesting { .. } => "vesting",
            ContractSpec::Subscription { .. } => "subscription",
            ContractSpec::Htlc { .. } => "htlc",
            ContractSpec::Multisig { .. } => "multisig",
        }
    }

    /// Context-free checks.
    pub fn validate_static(&self) -> Result<(), TxError> {
        let err = |m: &str| Err(TxError::Contract(m.to_string()));
        match self {
            ContractSpec::Escrow { amount, .. } | ContractSpec::Htlc { amount, .. } if *amount == 0 => return Err(TxError::ZeroAmount),
            ContractSpec::Vesting { amount, start_height, cliff_height, end_height, .. } => {
                if *amount == 0 {
                    return Err(TxError::ZeroAmount);
                }
                if !(start_height <= cliff_height && cliff_height <= end_height && start_height < end_height) {
                    return err("vesting requires start <= cliff <= end and start < end");
                }
            }
            ContractSpec::Subscription { amount_per_period, period_blocks, max_periods, .. } => {
                if *amount_per_period == 0 {
                    return Err(TxError::ZeroAmount);
                }
                if *period_blocks == 0 || *max_periods == 0 || *max_periods > MAX_SUBSCRIPTION_PERIODS {
                    return err("subscription requires period_blocks >= 1 and 1..=100000 periods");
                }
                if self.funding().is_none() {
                    return Err(TxError::Overflow);
                }
            }
            ContractSpec::Multisig { signers, threshold, .. } => {
                if signers.is_empty() || signers.len() > MAX_MULTISIG_SIGNERS {
                    return err("multisig requires 1..=16 signers");
                }
                if *threshold == 0 || *threshold as usize > signers.len() {
                    return err("multisig threshold must be 1..=signers");
                }
                let mut s = signers.clone();
                s.sort();
                s.dedup();
                if s.len() != signers.len() {
                    return err("duplicate multisig signer");
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl ContractCall {
    /// Coins the caller moves into the contract with this call.
    pub fn deposit_amount(&self) -> u64 {
        match self {
            ContractCall::MultisigDeposit { amount } => *amount,
            _ => 0,
        }
    }

    pub fn validate_static(&self) -> Result<(), TxError> {
        match self {
            ContractCall::HtlcRedeem { preimage } if preimage.len() > MAX_HTLC_PREIMAGE => {
                Err(TxError::Contract("preimage too long".into()))
            }
            ContractCall::MultisigDeposit { amount } if *amount == 0 => Err(TxError::ZeroAmount),
            ContractCall::MultisigPropose { amount, memo, .. } => {
                if *amount == 0 {
                    Err(TxError::ZeroAmount)
                } else if memo.len() > MAX_MEMO_BYTES {
                    Err(TxError::MemoTooLong(memo.len()))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

impl Contract {
    pub fn kind(&self) -> &'static str {
        match self.state {
            ContractState::Escrow { .. } => "escrow",
            ContractState::Vesting { .. } => "vesting",
            ContractState::Subscription { .. } => "subscription",
            ContractState::Htlc { .. } => "htlc",
            ContractState::Multisig { .. } => "multisig",
        }
    }

    /// Addresses with a role in this contract (for indexing and wallets).
    pub fn parties(&self) -> Vec<Address> {
        let mut v = vec![self.creator];
        match &self.state {
            ContractState::Escrow { payer, payee, arbiter, .. } => {
                v.push(*payer);
                v.push(*payee);
                if let Some(a) = arbiter {
                    v.push(*a);
                }
            }
            ContractState::Vesting { beneficiary, .. } => v.push(*beneficiary),
            ContractState::Subscription { payer, payee, .. } => {
                v.push(*payer);
                v.push(*payee);
            }
            ContractState::Htlc { sender, recipient, .. } => {
                v.push(*sender);
                v.push(*recipient);
            }
            ContractState::Multisig { signers, pending, .. } => {
                v.extend(signers.iter().copied());
                v.extend(pending.iter().map(|p| p.to));
            }
        }
        v.sort();
        v.dedup();
        v
    }
}

/// Amount vested at `height` (linear between start and end, zero before cliff).
pub fn vested_amount(total: u64, start: u64, cliff: u64, end: u64, height: u64) -> u64 {
    if height < cliff || height <= start {
        return 0;
    }
    if height >= end {
        return total;
    }
    ((total as u128) * ((height - start) as u128) / ((end - start) as u128)) as u64
}

/// Periods of a subscription available at `height` (the first is available at creation).
pub fn subscription_available_periods(start: u64, period_blocks: u64, max_periods: u32, height: u64) -> u32 {
    if height < start {
        return 0;
    }
    let p = (height - start) / period_blocks + 1;
    p.min(max_periods as u64) as u32
}

pub fn contract_id(creator: &Address, nonce: u64) -> Hash32 {
    tagged_hash(tags::CONTRACT_ID, &[&creator.0, &nonce.to_le_bytes()])
}

/// Adds `amount` to an account balance.
pub(crate) fn credit<R: StateReader + ?Sized>(state: &mut Overlay<'_, R>, to: &Address, amount: u64) -> Result<(), TxError> {
    if amount == 0 {
        return Ok(());
    }
    let mut acc = state.account(to)?;
    acc.balance = acc.balance.checked_add(amount).ok_or(TxError::Overflow)?;
    state.put_account(to, &acc);
    Ok(())
}

/// Creates a contract. The funding has already been debited from `creator`.
/// Returns the new contract id and the addresses involved.
pub(crate) fn create<R: StateReader + ?Sized>(
    state: &mut Overlay<'_, R>,
    height: u64,
    creator: Address,
    nonce: u64,
    spec: &ContractSpec,
) -> Result<(Hash32, Vec<Address>), TxError> {
    let id = contract_id(&creator, nonce);
    if state.contract(&id)?.is_some() {
        return Err(TxError::Contract("contract id collision".into()));
    }
    let err = |m: &str| Err(TxError::Contract(m.to_string()));
    let funding = spec.funding().ok_or(TxError::Overflow)?;
    let cstate = match spec.clone() {
        ContractSpec::Escrow { payee, arbiter, deadline_height, .. } => {
            if payee == creator {
                return err("escrow payee must differ from payer");
            }
            if arbiter == Some(payee) || arbiter == Some(creator) {
                return err("escrow arbiter must be a third party");
            }
            if deadline_height <= height {
                return err("escrow deadline must be in the future");
            }
            ContractState::Escrow { payer: creator, payee, arbiter, deadline_height }
        }
        ContractSpec::Vesting { beneficiary, amount, start_height, cliff_height, end_height, revocable } => {
            if end_height <= height {
                return err("vesting end must be in the future");
            }
            ContractState::Vesting { beneficiary, total: amount, claimed: 0, start_height, cliff_height, end_height, revocable }
        }
        ContractSpec::Subscription { payee, amount_per_period, period_blocks, max_periods } => {
            if payee == creator {
                return err("subscription payee must differ from payer");
            }
            ContractState::Subscription { payer: creator, payee, amount_per_period, period_blocks, max_periods, start_height: height, claimed_periods: 0 }
        }
        ContractSpec::Htlc { recipient, hash_lock, timeout_height, .. } => {
            if timeout_height <= height {
                return err("htlc timeout must be in the future");
            }
            ContractState::Htlc { sender: creator, recipient, hash_lock, timeout_height }
        }
        ContractSpec::Multisig { signers, threshold, .. } => ContractState::Multisig { signers, threshold, next_spend_id: 0, pending: Vec::new() },
    };
    let contract = Contract { id, creator, created_height: height, balance: funding, state: cstate };
    let parties = contract.parties();
    state.put(contract_key(&id), &contract);
    Ok((id, parties))
}

/// Executes a contract call. Any deposit has already been debited from `caller`.
/// Returns the addresses whose balances changed.
pub(crate) fn call<R: StateReader + ?Sized>(
    state: &mut Overlay<'_, R>,
    height: u64,
    caller: Address,
    id: &Hash32,
    call: &ContractCall,
) -> Result<Vec<Address>, TxError> {
    let mut c = state.contract(id)?.ok_or(TxError::ContractNotFound(*id))?;
    let err = |m: &str| -> Result<Vec<Address>, TxError> { Err(TxError::Contract(m.to_string())) };
    let mut touched = vec![caller];
    let mut payouts: Vec<(Address, u64)> = Vec::new();

    match (&mut c.state, call) {
        (ContractState::Escrow { payer, payee, arbiter, .. }, ContractCall::EscrowRelease) => {
            if caller != *payer && Some(caller) != *arbiter {
                return err("only payer or arbiter can release escrow");
            }
            payouts.push((*payee, c.balance));
        }
        (ContractState::Escrow { payer, payee, arbiter, deadline_height }, ContractCall::EscrowRefund) => {
            let allowed = caller == *payee || Some(caller) == *arbiter || (caller == *payer && height > *deadline_height);
            if !allowed {
                return err("refund requires payee, arbiter, or payer after the deadline");
            }
            payouts.push((*payer, c.balance));
        }
        (ContractState::Vesting { beneficiary, total, claimed, start_height, cliff_height, end_height, .. }, ContractCall::VestingClaim) => {
            if caller != *beneficiary {
                return err("only the beneficiary can claim");
            }
            let vested = vested_amount(*total, *start_height, *cliff_height, *end_height, height);
            let amount = vested.saturating_sub(*claimed);
            if amount == 0 {
                return err("nothing to claim yet");
            }
            *claimed += amount;
            payouts.push((*beneficiary, amount));
        }
        (ContractState::Vesting { beneficiary, total, claimed, start_height, cliff_height, end_height, revocable }, ContractCall::VestingRevoke) => {
            if caller != c.creator || !*revocable {
                return err("vesting is not revocable by caller");
            }
            let vested = vested_amount(*total, *start_height, *cliff_height, *end_height, height);
            let to_beneficiary = vested.saturating_sub(*claimed);
            let to_creator = c.balance - to_beneficiary;
            payouts.push((*beneficiary, to_beneficiary));
            payouts.push((c.creator, to_creator));
        }
        (ContractState::Subscription { payee, amount_per_period, period_blocks, max_periods, start_height, claimed_periods, .. }, ContractCall::SubscriptionClaim) => {
            if caller != *payee {
                return err("only the payee can claim");
            }
            let avail = subscription_available_periods(*start_height, *period_blocks, *max_periods, height);
            let periods = avail.saturating_sub(*claimed_periods);
            if periods == 0 {
                return err("no period available to claim");
            }
            *claimed_periods += periods;
            payouts.push((*payee, *amount_per_period * periods as u64));
        }
        (ContractState::Subscription { payer, payee, amount_per_period, period_blocks, max_periods, start_height, claimed_periods }, ContractCall::SubscriptionCancel) => {
            if caller != *payer {
                return err("only the payer can cancel");
            }
            let avail = subscription_available_periods(*start_height, *period_blocks, *max_periods, height);
            let owed = *amount_per_period * avail.saturating_sub(*claimed_periods) as u64;
            payouts.push((*payee, owed));
            payouts.push((*payer, c.balance - owed));
        }
        (ContractState::Htlc { recipient, hash_lock, timeout_height, .. }, ContractCall::HtlcRedeem { preimage }) => {
            if height > *timeout_height {
                return err("htlc expired");
            }
            let digest: [u8; 32] = Sha256::digest(preimage).into();
            if digest != hash_lock.0 {
                return err("wrong preimage");
            }
            payouts.push((*recipient, c.balance));
        }
        (ContractState::Htlc { sender, timeout_height, .. }, ContractCall::HtlcRefund) => {
            if height <= *timeout_height {
                return err("htlc not yet expired");
            }
            payouts.push((*sender, c.balance));
        }
        (ContractState::Multisig { .. }, ContractCall::MultisigDeposit { amount }) => {
            c.balance = c.balance.checked_add(*amount).ok_or(TxError::Overflow)?;
        }
        (ContractState::Multisig { signers, threshold, next_spend_id, pending }, ContractCall::MultisigPropose { to, amount, memo }) => {
            if !signers.contains(&caller) {
                return err("only signers can propose spends");
            }
            if pending.len() >= MAX_PENDING_SPENDS {
                return err("too many pending spends");
            }
            let spend = PendingSpend { id: *next_spend_id, to: *to, amount: *amount, memo: memo.clone(), approvals: vec![caller], created_height: height };
            *next_spend_id = next_spend_id.checked_add(1).ok_or(TxError::Overflow)?;
            if *threshold <= 1 {
                if *amount > c.balance {
                    return err("multisig balance too low");
                }
                payouts.push((*to, *amount));
            } else {
                pending.push(spend);
            }
        }
        (ContractState::Multisig { signers, threshold, pending, .. }, ContractCall::MultisigApprove { spend_id }) => {
            if !signers.contains(&caller) {
                return err("only signers can approve");
            }
            let Some(pos) = pending.iter().position(|s| s.id == *spend_id) else {
                return err("unknown spend id");
            };
            if pending[pos].approvals.contains(&caller) {
                return err("already approved");
            }
            pending[pos].approvals.push(caller);
            if pending[pos].approvals.len() >= *threshold as usize {
                if pending[pos].amount > c.balance {
                    return err("multisig balance too low");
                }
                let s = pending.remove(pos);
                payouts.push((s.to, s.amount));
            }
        }
        (ContractState::Multisig { pending, .. }, ContractCall::MultisigCancel { spend_id }) => {
            let Some(pos) = pending.iter().position(|s| s.id == *spend_id) else {
                return err("unknown spend id");
            };
            if pending[pos].approvals.first() != Some(&caller) {
                return err("only the proposer can cancel a spend");
            }
            pending.remove(pos);
        }
        _ => return err("call does not match contract type"),
    }

    let mut paid: u64 = 0;
    for (to, amount) in &payouts {
        if *amount == 0 {
            continue;
        }
        paid = paid.checked_add(*amount).ok_or(TxError::Overflow)?;
        credit(state, to, *amount)?;
        touched.push(*to);
    }
    c.balance = c.balance.checked_sub(paid).ok_or_else(|| TxError::Contract("payout exceeds contract balance".into()))?;

    let finished = match &c.state {
        ContractState::Multisig { .. } => false,
        ContractState::Subscription { claimed_periods, max_periods, .. } => c.balance == 0 || claimed_periods >= max_periods,
        _ => c.balance == 0,
    };
    if finished && c.balance == 0 {
        state.delete_raw(contract_key(id));
    } else {
        state.put(contract_key(id), &c);
    }
    touched.extend(c.parties());
    touched.sort();
    touched.dedup();
    Ok(touched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vesting_curve() {
        assert_eq!(vested_amount(1000, 100, 150, 200, 99), 0);
        assert_eq!(vested_amount(1000, 100, 150, 200, 149), 0);
        assert_eq!(vested_amount(1000, 100, 150, 200, 150), 500);
        assert_eq!(vested_amount(1000, 100, 150, 200, 175), 750);
        assert_eq!(vested_amount(1000, 100, 150, 200, 500), 1000);
    }

    #[test]
    fn subscription_periods() {
        assert_eq!(subscription_available_periods(10, 5, 3, 10), 1);
        assert_eq!(subscription_available_periods(10, 5, 3, 14), 1);
        assert_eq!(subscription_available_periods(10, 5, 3, 15), 2);
        assert_eq!(subscription_available_periods(10, 5, 3, 1000), 3);
    }
}
