//! Transaction construction helpers.

use thecoin_core::api::FeeView;
use thecoin_core::contracts::{ContractCall, ContractSpec};
use thecoin_core::crypto::SecretKey;
use thecoin_core::execution::required_fee;
use thecoin_core::governance::{GovParams, ProposalSpec, VoteChoice};
use thecoin_core::hash::Hash32;
use thecoin_core::tccl::Value;
use thecoin_core::tx::{TransferOutput, TxAction, TxBody, TX_VERSION};
use thecoin_core::{Address, Network, Transaction};

/// How much above the minimum fee to pay. Higher priority confirms sooner
/// because miners order transactions by fee per weight unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Priority {
    /// The minimum fee (fine when blocks are not full).
    Low,
    /// Minimum + 25%: still valid if congestion rises for a block.
    Normal,
    /// Above most waiting transactions.
    High,
    /// Next block with very high probability.
    Urgent,
}

/// Everything needed to compute a fee.
#[derive(Clone, Debug)]
pub struct FeePolicy {
    pub base_fee: u64,
    pub fee_per_kb: u64,
    pub fee_per_kfuel: u64,
    pub congestion_bp: u64,
    /// Multiplier over the minimum fee in basis points (10 000 = 1.0×).
    pub priority_bp: u64,
}

impl FeePolicy {
    pub fn from_fees(fees: &FeeView, priority: Priority) -> FeePolicy {
        let priority_bp = match priority {
            Priority::Low => fees.priority.low_bp,
            Priority::Normal => fees.priority.normal_bp,
            Priority::High => fees.priority.high_bp,
            Priority::Urgent => fees.priority.urgent_bp,
        };
        FeePolicy {
            base_fee: fees.base_fee,
            fee_per_kb: fees.fee_per_kb,
            fee_per_kfuel: fees.fee_per_kfuel,
            congestion_bp: fees.congestion_bp,
            priority_bp,
        }
    }

    /// Minimum fee right now (no priority).
    pub fn minimum(&self, size: usize, max_fuel: u64) -> u64 {
        let params = GovParams {
            base_fee: self.base_fee,
            fee_per_kb: self.fee_per_kb,
            fee_per_kfuel: self.fee_per_kfuel,
            ..thecoin_core::params::MAINNET.gov_defaults
        };
        required_fee(&params, self.congestion_bp, size, max_fuel).0
    }

    /// Fee to pay for a transaction of `size` bytes reserving `max_fuel` fuel.
    pub fn fee(&self, size: usize, max_fuel: u64) -> u64 {
        let min = self.minimum(size, max_fuel) as u128;
        (min * self.priority_bp.max(10_000) as u128).div_ceil(10_000).min(u64::MAX as u128) as u64
    }
}

/// Signs `action` with `key`, with the fee chosen by `policy`.
///
/// Because `fee` is a fixed-width integer, the size of the signed transaction
/// does not depend on the fee value, so one trial signature gives the exact size.
pub fn build_tx(
    key: &SecretKey,
    network: Network,
    nonce: u64,
    policy: &FeePolicy,
    flags: u8,
    expiry_height: u64,
    action: TxAction,
) -> Transaction {
    let body = TxBody { version: TX_VERSION, chain_id: network.params().chain_id, flags, nonce, fee: 0, expiry_height, action };
    let trial = body.clone().sign(key);
    let fee = policy.fee(trial.size(), trial.max_fuel());
    TxBody { fee, ..body }.sign(key)
}

/// Re-signs `tx` with a new fee (same nonce, flags and action).
pub fn with_fee(key: &SecretKey, tx: &Transaction, fee: u64) -> Transaction {
    TxBody { fee, ..tx.body.clone() }.sign(key)
}

pub fn transfer(to: Address, amount: u64, memo: &str) -> TxAction {
    TxAction::Transfer { to, amount, memo: memo.as_bytes().to_vec() }
}

pub fn batch(outputs: Vec<(Address, u64)>, memo: &str) -> TxAction {
    TxAction::BatchTransfer {
        outputs: outputs.into_iter().map(|(to, amount)| TransferOutput { to, amount }).collect(),
        memo: memo.as_bytes().to_vec(),
    }
}

pub fn create_contract(spec: ContractSpec) -> TxAction {
    TxAction::CreateContract { spec }
}

pub fn call_contract(contract: Hash32, call: ContractCall) -> TxAction {
    TxAction::CallContract { contract, call }
}

pub fn propose(proposal: ProposalSpec) -> TxAction {
    TxAction::Propose { proposal }
}

pub fn vote(proposal: Hash32, choice: VoteChoice, weight: u64) -> TxAction {
    TxAction::Vote { proposal, choice, weight }
}

pub fn deploy(source: &str, init_args: Vec<Value>, value: u64, max_fuel: u64, max_deposit: u64) -> TxAction {
    TxAction::Deploy { source: source.to_string(), init_args, value, max_fuel, max_deposit }
}

pub fn invoke(contract: Address, function: &str, args: Vec<Value>, value: u64, max_fuel: u64, max_deposit: u64) -> TxAction {
    TxAction::Invoke { contract, function: function.to_string(), args, value, max_fuel, max_deposit }
}

pub fn upgrade(contract: Address, source: &str, expected_code_hash: Hash32, args: Vec<Value>, max_fuel: u64, max_deposit: u64) -> TxAction {
    TxAction::Upgrade { contract, source: source.to_string(), expected_code_hash, args, max_fuel, max_deposit }
}

pub fn set_upgrade_authority(contract: Address, new_authority: Option<Address>, expected_code_hash: Hash32) -> TxAction {
    TxAction::SetUpgradeAuthority { contract, new_authority, expected_code_hash }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thecoin_core::api::PriorityView;
    use thecoin_core::execution::check_tx_stateless;

    fn fees() -> FeeView {
        FeeView {
            base_fee: 100,
            fee_per_kb: 1_000,
            fee_per_kfuel: 100,
            storage_deposit_per_kb: 10_000,
            congestion_bp: 10_000,
            typical_transfer_fee: 260,
            priority: PriorityView { low_bp: 10_000, normal_bp: 12_500, high_bp: 20_000, urgent_bp: 40_000 },
            mempool_txs: 0,
            mempool_bytes: 0,
        }
    }

    #[test]
    fn fee_follows_policy() {
        let key = SecretKey::from_bytes(&[5; 32]);
        let low = FeePolicy::from_fees(&fees(), Priority::Low);
        let tx = build_tx(&key, Network::Regtest, 3, &low, 0, 0, transfer(Address::ZERO, 1000, "hello"));
        assert_eq!(tx.body.fee, 100 + tx.size() as u64);
        check_tx_stateless(Network::Regtest.params(), &tx).unwrap();
        let urgent = FeePolicy::from_fees(&fees(), Priority::Urgent);
        let tx2 = build_tx(&key, Network::Regtest, 3, &urgent, 0, 0, transfer(Address::ZERO, 1000, "hello"));
        assert_eq!(tx2.body.fee, 4 * (100 + tx2.size() as u64));
        let bumped = with_fee(&key, &tx, tx.body.fee * 2);
        assert!(bumped.verify_signature());
        assert_eq!(bumped.body.nonce, tx.body.nonce);
    }
}
