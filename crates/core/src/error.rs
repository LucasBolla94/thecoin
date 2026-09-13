//! Validation errors.

use crate::hash::Hash32;
use crate::state::StateError;

#[derive(Clone, Debug, thiserror::Error)]
pub enum TxError {
    #[error("transaction too large ({size} bytes, max {max})")]
    TooLarge { size: usize, max: usize },
    #[error("unsupported transaction version {0}")]
    BadVersion(u8),
    #[error("transaction is for another network (chain id {got:#x}, expected {expected:#x})")]
    WrongChain { expected: u32, got: u32 },
    #[error("invalid signature")]
    BadSignature,
    #[error("unknown transaction flags {0:#x}")]
    BadFlags(u8),
    #[error("invalid contract transaction: {0}")]
    BadContractTx(String),
    #[error("transaction expired at height {expiry}")]
    Expired { expiry: u64 },
    #[error("bad nonce: expected {expected}, got {got}")]
    BadNonce { expected: u64, got: u64 },
    #[error("insufficient funds: need {needed}, spendable {available}")]
    InsufficientFunds { needed: u64, available: u64 },
    #[error("fee too low: minimum {min}, got {got}")]
    FeeTooLow { min: u64, got: u64 },
    #[error("amount must be greater than zero")]
    ZeroAmount,
    #[error("memo too long ({0} bytes)")]
    MemoTooLong(usize),
    #[error("invalid batch: {0}")]
    BadBatch(&'static str),
    #[error("arithmetic overflow")]
    Overflow,
    #[error("contract error: {0}")]
    Contract(String),
    #[error("contract {0} not found")]
    ContractNotFound(Hash32),
    #[error("governance error: {0}")]
    Governance(String),
    #[error(transparent)]
    State(#[from] StateError),
}

#[derive(Debug, thiserror::Error)]
pub enum BlockError {
    #[error("unsupported block version {0}")]
    BadVersion(u32),
    #[error("unknown parent block {0}")]
    UnknownParent(Hash32),
    #[error("bad height: expected {expected}, got {got}")]
    BadHeight { expected: u64, got: u64 },
    #[error("timestamp {ts} is not after median time past {mtp}")]
    TimestampTooOld { ts: u64, mtp: u64 },
    #[error("timestamp {ts} is too far in the future (now {now})")]
    TimestampInFuture { ts: u64, now: u64 },
    #[error("wrong difficulty target")]
    BadTarget,
    #[error("proof of work does not meet target")]
    BadPow,
    #[error("transaction merkle root mismatch")]
    BadTxRoot,
    #[error("state root mismatch (block {expected}, computed {computed})")]
    BadStateRoot { expected: Hash32, computed: Hash32 },
    #[error("block too large ({size} bytes, max {max})")]
    TooLarge { size: u64, max: u64 },
    #[error("block exceeds the fuel limit")]
    FuelLimit,
    #[error("signal bits {0:#x} use unassigned bits")]
    BadSignal(u32),
    #[error("duplicate transaction {0}")]
    DuplicateTx(Hash32),
    #[error("transaction #{index} ({txid}) invalid: {error}")]
    Tx { index: usize, txid: Hash32, error: TxError },
    #[error("block conflicts with checkpoint at height {0}")]
    Checkpoint(u64),
    #[error("block descends from an invalid block")]
    InvalidAncestor,
    #[error("governance processing failed: {0}")]
    Governance(String),
    #[error(transparent)]
    State(#[from] StateError),
}
