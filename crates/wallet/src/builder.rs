//! Transaction construction helpers.

use thecoin_core::contracts::{ContractCall, ContractSpec};
use thecoin_core::crypto::SecretKey;
use thecoin_core::governance::{ProposalSpec, VoteChoice};
use thecoin_core::hash::Hash32;
use thecoin_core::tx::{TransferOutput, TxAction, TxBody, TX_VERSION};
use thecoin_core::{Address, Network, Transaction};

/// Signs `action` with `key`, setting `fee = fee_per_byte × size`.
///
/// Because `fee` is a fixed-width integer, the size of the signed transaction
/// does not depend on the fee value, so one trial signature gives the exact size.
pub fn build_tx(key: &SecretKey, network: Network, nonce: u64, fee_per_byte: u64, expiry_height: u64, action: TxAction) -> Transaction {
    let body = TxBody { version: TX_VERSION, chain_id: network.params().chain_id, nonce, fee: 0, expiry_height, action };
    let size = body.clone().sign(key).size() as u64;
    TxBody { fee: size.saturating_mul(fee_per_byte), ..body }.sign(key)
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

#[cfg(test)]
mod tests {
    use super::*;
    use thecoin_core::execution::check_tx_stateless;

    #[test]
    fn fee_matches_size() {
        let key = SecretKey::from_bytes(&[5; 32]);
        let tx = build_tx(&key, Network::Regtest, 3, 10, 0, transfer(Address::ZERO, 1000, "hello"));
        assert_eq!(tx.body.fee, tx.size() as u64 * 10);
        check_tx_stateless(Network::Regtest.params(), &tx).unwrap();
    }
}
