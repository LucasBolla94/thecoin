//! Transactions.
//!
//! The Coin uses an **account model**: each address has a balance and a nonce.
//! A transaction is signed by one account and executes one [`TxAction`].
//!
//! ```text
//! signing message = tagged_hash("tx-sign", borsh(TxBody))
//! txid            = tagged_hash("txid",    borsh(Transaction))
//! sender          = Address::from_public_key(public_key)
//! ```
//!
//! Replay protection: `chain_id` (between networks) + `nonce` (within a network).
//! A transaction is either fully valid and applied, or it cannot be included in
//! a block at all — there is no "failed but fee charged" state.

use crate::address::Address;
use crate::contracts::{ContractCall, ContractSpec};
use crate::crypto;
use crate::governance::{ProposalSpec, VoteChoice};
use crate::hash::{tagged_hash, tags, Hash32};
use borsh::{BorshDeserialize, BorshSerialize};

pub const TX_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TransferOutput {
    pub to: Address,
    pub amount: u64,
}

/// What a transaction does. **Variant order is consensus-critical** — new
/// variants may only be appended.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TxAction {
    /// Simple payment. `memo` is free data (invoice id, message; max 256 bytes).
    Transfer { to: Address, amount: u64, memo: Vec<u8> },
    /// Pays up to 128 recipients at once (payroll, pools, exchanges).
    BatchTransfer { outputs: Vec<TransferOutput>, memo: Vec<u8> },
    /// Creates a payment contract and funds it from the sender's balance.
    CreateContract { spec: ContractSpec },
    /// Calls an existing payment contract.
    CallContract { contract: Hash32, call: ContractCall },
    /// Opens a governance proposal (locks the proposal deposit).
    Propose { proposal: ProposalSpec },
    /// Votes on a proposal with `weight` coins (locked until the vote ends).
    Vote { proposal: Hash32, choice: VoteChoice, weight: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TxBody {
    pub version: u8,
    /// Network id, see [`crate::ChainParams::chain_id`].
    pub chain_id: u32,
    /// Must equal the sender account nonce; incremented on inclusion.
    pub nonce: u64,
    /// Fee paid to the miner, in motes. Must be `>= min_fee_per_byte * size`.
    pub fee: u64,
    /// Last block height at which this tx may be included (0 = no expiry).
    pub expiry_height: u64,
    pub action: TxAction,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Transaction {
    pub body: TxBody,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

impl TxBody {
    pub fn signing_hash(&self) -> Hash32 {
        let bytes = borsh::to_vec(self).expect("serialization cannot fail");
        tagged_hash(tags::TX_SIGN, &[&bytes])
    }

    /// Signs the body. The fee does not change the size, so wallets can compute
    /// `size = sign(body with fee 0).size()` first and then set the final fee.
    pub fn sign(self, key: &crypto::SecretKey) -> Transaction {
        let sig = key.sign(&self.signing_hash().0);
        Transaction { body: self, public_key: key.public_key(), signature: sig }
    }
}

impl Transaction {
    pub fn txid(&self) -> Hash32 {
        tagged_hash(tags::TXID, &[&self.to_bytes()])
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("serialization cannot fail")
    }

    pub fn from_bytes(mut b: &[u8]) -> Result<Transaction, std::io::Error> {
        let tx = Transaction::deserialize(&mut b)?;
        if !b.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "trailing bytes"));
        }
        Ok(tx)
    }

    pub fn size(&self) -> usize {
        self.to_bytes().len()
    }

    pub fn sender(&self) -> Address {
        Address::from_public_key(&self.public_key)
    }

    pub fn verify_signature(&self) -> bool {
        crypto::verify(&self.public_key, &self.body.signing_hash().0, &self.signature)
    }

    /// Fee per serialized byte (for mempool ordering).
    pub fn fee_rate(&self) -> u64 {
        self.body.fee / self.size().max(1) as u64
    }

    /// Upper bound of coins that leave the sender's balance (amounts + fee).
    pub fn max_debit(&self) -> u64 {
        let amount: u64 = match &self.body.action {
            TxAction::Transfer { amount, .. } => *amount,
            TxAction::BatchTransfer { outputs, .. } => outputs.iter().fold(0u64, |a, o| a.saturating_add(o.amount)),
            TxAction::CreateContract { spec } => spec.funding().unwrap_or(u64::MAX),
            TxAction::CallContract { call, .. } => call.deposit_amount(),
            TxAction::Propose { .. } => 0, // deposit is a chain parameter; checked on apply
            TxAction::Vote { .. } => 0,
        };
        amount.saturating_add(self.body.fee)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_fee_independent_size() {
        let key = crypto::SecretKey::from_bytes(&[9u8; 32]);
        let body = |fee| TxBody {
            version: TX_VERSION,
            chain_id: 1,
            nonce: 0,
            fee,
            expiry_height: 0,
            action: TxAction::Transfer { to: Address::ZERO, amount: 5, memo: b"hi".to_vec() },
        };
        let a = body(0).sign(&key);
        let b = body(123_456_789).sign(&key);
        assert_eq!(a.size(), b.size());
        assert!(b.verify_signature());
        let mut c = b.clone();
        c.body.fee += 1;
        assert!(!c.verify_signature());
        assert_eq!(Transaction::from_bytes(&b.to_bytes()).unwrap(), b);
    }
}
