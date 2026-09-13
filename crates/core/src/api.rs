//! JSON views of chain objects, shared by the node REST API, the reference
//! wallet, the explorer and third-party wallets.
//!
//! Conventions:
//! * amounts are integers in **motes** (1 TCN = 100 000 000 motes; the maximum
//!   supply 5·10^15 fits in a JSON/JavaScript safe integer);
//! * hashes are lowercase hex; addresses are bech32m strings;
//! * binary memos are hex (`memo_hex`) plus `memo_text` when valid UTF-8.

use crate::address::Address;
use crate::block::Block;
use crate::contracts::{Contract, ContractCall, ContractSpec, ContractState, PendingSpend};
use crate::governance::{GovParams, Outcome, Proposal, ProposalAction, ProposalStatus, Tally, VoteChoice};
use crate::hash::Hash32;
use crate::params::Network;
use crate::pow::difficulty_from_target;
use crate::state::Account;
use crate::tx::{Transaction, TxAction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusView {
    pub network: Network,
    pub version: String,
    pub genesis: Hash32,
    pub height: u64,
    pub tip: Hash32,
    pub tip_timestamp: u64,
    pub chainwork: String,
    pub difficulty: f64,
    pub next_target: String,
    pub hashrate_estimate: f64,
    pub peers: usize,
    pub mempool_txs: usize,
    pub mempool_bytes: usize,
    pub syncing: bool,
    pub supply: SupplyView,
    pub params: GovParams,
    /// Current congestion fee multiplier (10 000 = 1.0×).
    pub congestion_bp: u64,
    pub software_upgrade_required: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyView {
    pub max_supply: u64,
    pub emitted: u64,
    pub burned: u64,
    pub circulating: u64,
    pub current_block_reward: u64,
    pub era: u64,
    pub next_halving_height: u64,
    pub halving_interval: u64,
    pub target_block_time: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockSummaryView {
    pub height: u64,
    pub hash: Hash32,
    pub prev_hash: Hash32,
    pub timestamp: u64,
    pub tx_count: usize,
    pub size: usize,
    pub miner: String,
    pub difficulty: f64,
    pub signal: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockView {
    #[serde(flatten)]
    pub summary: BlockSummaryView,
    pub version: u32,
    pub tx_root: Hash32,
    pub state_root: Hash32,
    pub target: String,
    /// Decimal string: u64 nonces exceed JavaScript's safe integer range.
    pub nonce: String,
    pub confirmations: u64,
    pub subsidy: u64,
    pub fees: u64,
    pub txs: Vec<TxView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxView {
    pub txid: Hash32,
    pub sender: String,
    pub nonce: u64,
    pub fee: u64,
    pub expiry_height: u64,
    pub size: usize,
    pub action: ActionView,
    #[serde(default)]
    pub block_height: Option<u64>,
    #[serde(default)]
    pub block_hash: Option<Hash32>,
    /// Position of the transaction inside its block (for `cursor=height:position` paging).
    #[serde(default)]
    pub position: Option<u32>,
    #[serde(default)]
    pub confirmations: u64,
    #[serde(default)]
    pub in_mempool: bool,
    /// Contract or proposal id created by this transaction.
    #[serde(default)]
    pub created: Option<Hash32>,
    /// The sender allows replacing this transaction with a higher fee.
    #[serde(default)]
    pub replaceable: bool,
    /// Execution result (always true for confirmed native transactions).
    #[serde(default = "default_true")]
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub fuel_used: u64,
    /// Part of the fee burned by the congestion surcharge.
    #[serde(default)]
    pub burned: u64,
    #[serde(default)]
    pub logs: Vec<LogView>,
    /// Address of the TCCL contract deployed by this transaction.
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub return_value: Option<String>,
    /// Another transaction with the same sender and nonce was seen (possible double spend).
    #[serde(default)]
    pub conflict: Option<Hash32>,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogView {
    pub contract: String,
    pub event: String,
    /// `(field name, rendered value)`.
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputView {
    pub to: String,
    pub amount: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionView {
    Transfer { to: String, amount: u64, memo_hex: String, memo_text: Option<String> },
    BatchTransfer { outputs: Vec<OutputView>, total: u64, memo_hex: String, memo_text: Option<String> },
    CreateContract { spec: ContractSpecView },
    CallContract { contract: Hash32, call: ContractCallView },
    Propose { title: String, url: String, content_hash: Hash32, action: ProposalActionView },
    Vote { proposal: Hash32, choice: VoteChoice, weight: u64 },
    Deploy { source: String, source_hash: Hash32, init_args: Vec<String>, value: u64, max_fuel: u64, max_deposit: u64 },
    Invoke { contract: String, function: String, args: Vec<String>, value: u64, max_fuel: u64, max_deposit: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContractSpecView {
    Escrow { payee: String, arbiter: Option<String>, amount: u64, deadline_height: u64 },
    Vesting { beneficiary: String, amount: u64, start_height: u64, cliff_height: u64, end_height: u64, revocable: bool },
    Subscription { payee: String, amount_per_period: u64, period_blocks: u64, max_periods: u32 },
    Htlc { recipient: String, amount: u64, hash_lock: Hash32, timeout_height: u64 },
    Multisig { signers: Vec<String>, threshold: u8, initial_deposit: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum ContractCallView {
    EscrowRelease,
    EscrowRefund,
    VestingClaim,
    VestingRevoke,
    SubscriptionClaim,
    SubscriptionCancel,
    HtlcRedeem { preimage_hex: String },
    HtlcRefund,
    MultisigDeposit { amount: u64 },
    MultisigPropose { to: String, amount: u64, memo_hex: String, memo_text: Option<String> },
    MultisigApprove { spend_id: u32 },
    MultisigCancel { spend_id: u32 },
    MultisigClose,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProposalActionView {
    Text,
    SetParam { param: String, value: u64 },
    SoftwareUpgrade { version: String, release_hash: Hash32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountView {
    pub address: String,
    pub balance: u64,
    pub spendable: u64,
    pub locked: u64,
    pub locked_until: u64,
    pub nonce: u64,
    /// Next nonce to use, counting transactions waiting in the mempool.
    pub next_nonce: u64,
    /// Mining rewards still in cooldown (not yet spendable).
    pub immature: u64,
    pub mempool_txs: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingSpendView {
    pub id: u32,
    pub to: String,
    pub amount: u64,
    pub memo_hex: String,
    pub approvals: Vec<String>,
    pub created_height: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContractStateView {
    Escrow {
        payer: String,
        payee: String,
        arbiter: Option<String>,
        deadline_height: u64,
    },
    Vesting {
        beneficiary: String,
        total: u64,
        claimed: u64,
        vested_now: u64,
        start_height: u64,
        cliff_height: u64,
        end_height: u64,
        revocable: bool,
    },
    Subscription {
        payer: String,
        payee: String,
        amount_per_period: u64,
        period_blocks: u64,
        max_periods: u32,
        start_height: u64,
        claimed_periods: u32,
        claimable_periods_now: u32,
    },
    Htlc {
        sender: String,
        recipient: String,
        hash_lock: Hash32,
        timeout_height: u64,
    },
    Multisig {
        signers: Vec<String>,
        threshold: u8,
        next_spend_id: u32,
        pending: Vec<PendingSpendView>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractView {
    pub id: Hash32,
    pub creator: String,
    pub created_height: u64,
    pub balance: u64,
    /// Refundable storage deposit returned to the creator when the contract ends.
    pub deposit: u64,
    pub state: ContractStateView,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalView {
    pub id: Hash32,
    pub proposer: String,
    pub title: String,
    pub url: String,
    pub content_hash: Hash32,
    pub action: ProposalActionView,
    pub deposit: u64,
    pub created_height: u64,
    pub end_height: u64,
    pub signal_bit: u8,
    pub status: String,
    pub activation_height: Option<u64>,
    pub tally: Tally,
    pub outcome: Option<Outcome>,
    /// Live projections against the current parameters.
    pub projection: Option<ProposalProjection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalProjection {
    pub quorum_needed: u64,
    pub quorum_progress_bp: u64,
    pub approval_bp: u64,
    pub approval_needed_bp: u64,
    pub miner_approval_bp: u64,
    pub miner_approval_needed_bp: u64,
    pub blocks_left: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmitTxRequest {
    /// Borsh-encoded signed transaction, hex.
    pub tx: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmitTxResponse {
    pub txid: Hash32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerView {
    pub addr: String,
    pub inbound: bool,
    pub height: u64,
    pub user_agent: String,
    pub connected_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MiningView {
    pub enabled: bool,
    pub threads: usize,
    pub address: Option<String>,
    pub hashrate: f64,
    pub blocks_found: u64,
    pub signal_proposals: Vec<Hash32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeeView {
    pub base_fee: u64,
    pub fee_per_kb: u64,
    pub fee_per_kfuel: u64,
    pub storage_deposit_per_kb: u64,
    /// Current congestion multiplier (10 000 = 1.0×). The surcharge is burned.
    pub congestion_bp: u64,
    /// Minimum fee of a typical 160-byte transfer right now.
    pub typical_transfer_fee: u64,
    /// Multipliers (basis points over the minimum fee) for each priority level.
    pub priority: PriorityView,
    pub mempool_txs: usize,
    pub mempool_bytes: usize,
}

/// Fee multipliers over the minimum fee. Higher priority confirms sooner:
/// miners order transactions by fee per weight unit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriorityView {
    /// Minimum fee: fine when blocks are not full.
    pub low_bp: u64,
    /// Survives one full block of congestion increase.
    pub normal_bp: u64,
    /// Beats most waiting transactions.
    pub high_bp: u64,
    /// Next block with very high probability.
    pub urgent_bp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgramFunctionView {
    pub name: String,
    pub kind: String,
    pub payable: bool,
    pub params: Vec<(String, String)>,
    pub returns: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgramView {
    pub address: String,
    pub name: String,
    pub creator: String,
    pub created_height: u64,
    pub deploy_txid: Hash32,
    pub source_hash: Hash32,
    pub balance: u64,
    pub state_bytes: u64,
    pub storage_items: u64,
    pub deposit: u64,
    pub functions: Vec<ProgramFunctionView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewCallRequest {
    pub function: String,
    /// Arguments as text, parsed with the declared parameter types.
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewCallResponse {
    pub result: Option<String>,
    pub error: Option<String>,
    pub fuel_used: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulateResponse {
    pub valid: bool,
    /// Why the transaction cannot be included (invalid), if so.
    pub invalid_reason: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub fuel_used: u64,
    pub required_fee: u64,
    pub logs: Vec<LogView>,
    pub return_value: Option<String>,
    pub program: Option<String>,
}

/// How many confirmations make reversing a payment uneconomic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecurityView {
    pub amount: u64,
    /// Recommended confirmations for this amount.
    pub confirmations: u64,
    /// Approximate minutes until that many confirmations.
    pub minutes: u64,
    /// Block subsidy an attacker gives up per block rewritten (motes; fees are not counted).
    pub value_per_block: u64,
    pub network_hashrate: f64,
    pub explanation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoubleSpendView {
    pub sender: String,
    pub nonce: u64,
    pub first: Hash32,
    pub second: Hash32,
    pub seen_at: u64,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn memo_parts(memo: &[u8]) -> (String, Option<String>) {
    (hex::encode(memo), std::str::from_utf8(memo).ok().filter(|s| !s.is_empty()).map(str::to_string))
}

pub fn contract_spec_view(spec: &ContractSpec, n: Network) -> ContractSpecView {
    match spec {
        ContractSpec::Escrow { payee, arbiter, amount, deadline_height } => ContractSpecView::Escrow {
            payee: payee.encode(n),
            arbiter: arbiter.map(|a| a.encode(n)),
            amount: *amount,
            deadline_height: *deadline_height,
        },
        ContractSpec::Vesting { beneficiary, amount, start_height, cliff_height, end_height, revocable } => ContractSpecView::Vesting {
            beneficiary: beneficiary.encode(n),
            amount: *amount,
            start_height: *start_height,
            cliff_height: *cliff_height,
            end_height: *end_height,
            revocable: *revocable,
        },
        ContractSpec::Subscription { payee, amount_per_period, period_blocks, max_periods } => ContractSpecView::Subscription {
            payee: payee.encode(n),
            amount_per_period: *amount_per_period,
            period_blocks: *period_blocks,
            max_periods: *max_periods,
        },
        ContractSpec::Htlc { recipient, amount, hash_lock, timeout_height } => ContractSpecView::Htlc {
            recipient: recipient.encode(n),
            amount: *amount,
            hash_lock: *hash_lock,
            timeout_height: *timeout_height,
        },
        ContractSpec::Multisig { signers, threshold, initial_deposit } => ContractSpecView::Multisig {
            signers: signers.iter().map(|s| s.encode(n)).collect(),
            threshold: *threshold,
            initial_deposit: *initial_deposit,
        },
    }
}

pub fn contract_call_view(call: &ContractCall, n: Network) -> ContractCallView {
    match call {
        ContractCall::EscrowRelease => ContractCallView::EscrowRelease,
        ContractCall::EscrowRefund => ContractCallView::EscrowRefund,
        ContractCall::VestingClaim => ContractCallView::VestingClaim,
        ContractCall::VestingRevoke => ContractCallView::VestingRevoke,
        ContractCall::SubscriptionClaim => ContractCallView::SubscriptionClaim,
        ContractCall::SubscriptionCancel => ContractCallView::SubscriptionCancel,
        ContractCall::HtlcRedeem { preimage } => ContractCallView::HtlcRedeem { preimage_hex: hex::encode(preimage) },
        ContractCall::HtlcRefund => ContractCallView::HtlcRefund,
        ContractCall::MultisigDeposit { amount } => ContractCallView::MultisigDeposit { amount: *amount },
        ContractCall::MultisigPropose { to, amount, memo } => {
            let (memo_hex, memo_text) = memo_parts(memo);
            ContractCallView::MultisigPropose { to: to.encode(n), amount: *amount, memo_hex, memo_text }
        }
        ContractCall::MultisigApprove { spend_id } => ContractCallView::MultisigApprove { spend_id: *spend_id },
        ContractCall::MultisigCancel { spend_id } => ContractCallView::MultisigCancel { spend_id: *spend_id },
        ContractCall::MultisigClose => ContractCallView::MultisigClose,
    }
}

pub fn proposal_action_view(a: &ProposalAction) -> ProposalActionView {
    match a {
        ProposalAction::Text => ProposalActionView::Text,
        ProposalAction::SetParam { param, value } => ProposalActionView::SetParam { param: param.name().to_string(), value: *value },
        ProposalAction::SoftwareUpgrade { version, release_hash } => {
            ProposalActionView::SoftwareUpgrade { version: version.clone(), release_hash: *release_hash }
        }
    }
}

pub fn action_view(a: &TxAction, n: Network) -> ActionView {
    match a {
        TxAction::Transfer { to, amount, memo } => {
            let (memo_hex, memo_text) = memo_parts(memo);
            ActionView::Transfer { to: to.encode(n), amount: *amount, memo_hex, memo_text }
        }
        TxAction::BatchTransfer { outputs, memo } => {
            let (memo_hex, memo_text) = memo_parts(memo);
            ActionView::BatchTransfer {
                outputs: outputs.iter().map(|o| OutputView { to: o.to.encode(n), amount: o.amount }).collect(),
                total: outputs.iter().fold(0u64, |a, o| a.saturating_add(o.amount)),
                memo_hex,
                memo_text,
            }
        }
        TxAction::CreateContract { spec } => ActionView::CreateContract { spec: contract_spec_view(spec, n) },
        TxAction::CallContract { contract, call } => ActionView::CallContract { contract: *contract, call: contract_call_view(call, n) },
        TxAction::Propose { proposal } => ActionView::Propose {
            title: proposal.title.clone(),
            url: proposal.url.clone(),
            content_hash: proposal.content_hash,
            action: proposal_action_view(&proposal.action),
        },
        TxAction::Vote { proposal, choice, weight } => ActionView::Vote { proposal: *proposal, choice: *choice, weight: *weight },
        TxAction::Deploy { source, init_args, value, max_fuel, max_deposit } => ActionView::Deploy {
            source: source.clone(),
            source_hash: crate::programs::source_hash(source),
            init_args: init_args.iter().map(|v| tccl::abi::display(v, n.hrp())).collect(),
            value: *value,
            max_fuel: *max_fuel,
            max_deposit: *max_deposit,
        },
        TxAction::Invoke { contract, function, args, value, max_fuel, max_deposit } => ActionView::Invoke {
            contract: contract.encode(n),
            function: function.clone(),
            args: args.iter().map(|v| tccl::abi::display(v, n.hrp())).collect(),
            value: *value,
            max_fuel: *max_fuel,
            max_deposit: *max_deposit,
        },
    }
}

pub fn tx_view(tx: &Transaction, n: Network) -> TxView {
    TxView {
        txid: tx.txid(),
        sender: tx.sender().encode(n),
        nonce: tx.body.nonce,
        fee: tx.body.fee,
        expiry_height: tx.body.expiry_height,
        size: tx.size(),
        action: action_view(&tx.body.action, n),
        block_height: None,
        block_hash: None,
        position: None,
        confirmations: 0,
        in_mempool: false,
        created: None,
        replaceable: tx.is_replaceable(),
        success: true,
        error: None,
        fuel_used: 0,
        burned: 0,
        logs: Vec::new(),
        program: None,
        return_value: None,
        conflict: None,
    }
}

/// Adds execution results from a receipt to a transaction view.
pub fn apply_receipt(v: &mut TxView, r: &crate::execution::TxReceipt, n: Network) {
    v.success = r.success;
    v.error = r.error.clone();
    v.fuel_used = r.fuel_used;
    v.burned = r.burned;
    v.created = r.created.or(v.created);
    v.program = r.program.map(|a| a.encode(n));
    v.return_value = r.return_value.as_ref().map(|x| tccl::abi::display(x, n.hrp()));
    v.logs = r.logs.iter().map(|l| log_view(l, n)).collect();
}

pub fn log_view(l: &crate::execution::LogEntry, n: Network) -> LogView {
    LogView {
        contract: l.contract.encode(n),
        event: l.event.clone(),
        fields: l.fields.iter().map(|(k, x)| (k.clone(), tccl::abi::display(x, n.hrp()))).collect(),
    }
}

pub fn block_summary_view(b: &Block, n: Network) -> BlockSummaryView {
    BlockSummaryView {
        height: b.header.height,
        hash: b.hash(),
        prev_hash: b.header.prev_hash,
        timestamp: b.header.timestamp,
        tx_count: b.txs.len(),
        size: b.serialized_size(),
        miner: b.header.miner.encode(n),
        difficulty: difficulty_from_target(&b.header.target_u256()),
        signal: b.header.signal,
    }
}

pub fn account_view(addr: &Address, acc: &Account, height: u64, n: Network) -> AccountView {
    AccountView {
        address: addr.encode(n),
        balance: acc.balance,
        spendable: acc.spendable(height + 1),
        locked: if height < acc.locked_until { acc.locked } else { 0 },
        locked_until: acc.locked_until,
        nonce: acc.nonce,
        next_nonce: acc.nonce,
        immature: 0,
        mempool_txs: 0,
    }
}

fn pending_spend_view(s: &PendingSpend, n: Network) -> PendingSpendView {
    PendingSpendView {
        id: s.id,
        to: s.to.encode(n),
        amount: s.amount,
        memo_hex: hex::encode(&s.memo),
        approvals: s.approvals.iter().map(|a| a.encode(n)).collect(),
        created_height: s.created_height,
    }
}

/// `height` = current tip height (used for "claimable now" values).
pub fn contract_view(c: &Contract, height: u64, n: Network) -> ContractView {
    let next = height + 1;
    let state = match &c.state {
        ContractState::Escrow { payer, payee, arbiter, deadline_height } => ContractStateView::Escrow {
            payer: payer.encode(n),
            payee: payee.encode(n),
            arbiter: arbiter.map(|a| a.encode(n)),
            deadline_height: *deadline_height,
        },
        ContractState::Vesting { beneficiary, total, claimed, start_height, cliff_height, end_height, revocable } => {
            ContractStateView::Vesting {
                beneficiary: beneficiary.encode(n),
                total: *total,
                claimed: *claimed,
                vested_now: crate::contracts::vested_amount(*total, *start_height, *cliff_height, *end_height, next),
                start_height: *start_height,
                cliff_height: *cliff_height,
                end_height: *end_height,
                revocable: *revocable,
            }
        }
        ContractState::Subscription { payer, payee, amount_per_period, period_blocks, max_periods, start_height, claimed_periods } => {
            let avail = crate::contracts::subscription_available_periods(*start_height, *period_blocks, *max_periods, next);
            ContractStateView::Subscription {
                payer: payer.encode(n),
                payee: payee.encode(n),
                amount_per_period: *amount_per_period,
                period_blocks: *period_blocks,
                max_periods: *max_periods,
                start_height: *start_height,
                claimed_periods: *claimed_periods,
                claimable_periods_now: avail.saturating_sub(*claimed_periods),
            }
        }
        ContractState::Htlc { sender, recipient, hash_lock, timeout_height } => ContractStateView::Htlc {
            sender: sender.encode(n),
            recipient: recipient.encode(n),
            hash_lock: *hash_lock,
            timeout_height: *timeout_height,
        },
        ContractState::Multisig { signers, threshold, next_spend_id, pending } => ContractStateView::Multisig {
            signers: signers.iter().map(|s| s.encode(n)).collect(),
            threshold: *threshold,
            next_spend_id: *next_spend_id,
            pending: pending.iter().map(|s| pending_spend_view(s, n)).collect(),
        },
    };
    ContractView { id: c.id, creator: c.creator.encode(n), created_height: c.created_height, balance: c.balance, deposit: c.deposit, state }
}

pub fn proposal_view(p: &Proposal, height: u64, params: &GovParams, circulating: u64, n: Network) -> ProposalView {
    let (status, activation_height) = match &p.status {
        ProposalStatus::Voting => ("voting".to_string(), None),
        ProposalStatus::Approved { activation_height } => ("approved".to_string(), Some(*activation_height)),
        ProposalStatus::Rejected => ("rejected".to_string(), None),
        ProposalStatus::Activated { height } => ("activated".to_string(), Some(*height)),
    };
    let projection = if p.status == ProposalStatus::Voting {
        let t = &p.tally;
        let votes = t.yes as u128 + t.no as u128 + t.abstain as u128;
        let quorum_needed = (params.quorum_bp as u128 * circulating as u128 / 10_000) as u64;
        Some(ProposalProjection {
            quorum_needed,
            quorum_progress_bp: if quorum_needed == 0 { 10_000 } else { (votes * 10_000 / quorum_needed as u128).min(10_000) as u64 },
            approval_bp: if t.yes + t.no == 0 { 0 } else { (t.yes as u128 * 10_000 / (t.yes as u128 + t.no as u128)) as u64 },
            approval_needed_bp: params.approval_bp,
            miner_approval_bp: (t.miner_yes_blocks * 10_000).checked_div(t.miner_total_blocks).unwrap_or(0),
            miner_approval_needed_bp: params.miner_approval_bp,
            blocks_left: p.end_height.saturating_sub(height),
        })
    } else {
        None
    };
    ProposalView {
        id: p.id,
        proposer: p.proposer.encode(n),
        title: p.spec.title.clone(),
        url: p.spec.url.clone(),
        content_hash: p.spec.content_hash,
        action: proposal_action_view(&p.spec.action),
        deposit: p.deposit,
        created_height: p.created_height,
        end_height: p.end_height,
        signal_bit: p.signal_bit,
        status,
        activation_height,
        tally: p.tally.clone(),
        outcome: p.outcome.clone(),
        projection,
    }
}
