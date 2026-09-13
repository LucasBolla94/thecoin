//! # thecoin-core
//!
//! Consensus-critical code of **The Coin** (<https://the-coin.cloud>).
//!
//! Everything in this crate is deterministic and free of I/O so that it can be
//! reused by full nodes, wallets, explorers and alternative implementations:
//!
//! * [`hash`], [`crypto`], [`address`] — BLAKE3 hashing with domain separation,
//!   Ed25519 keys and bech32m addresses.
//! * [`amount`] — fixed-point amounts (1 TCN = 10^8 motes).
//! * [`params`] / [`emission`] — network parameters and the 50M TCN emission curve.
//! * [`pow`] / [`difficulty`] — CoinHash (Argon2id) proof of work and LWMA retargeting.
//! * [`block`], [`tx`], [`merkle`] — canonical (Borsh) wire and hashing formats.
//! * [`state`], [`lthash`] — state keys/records and the homomorphic state commitment.
//! * [`contracts`], [`governance`], [`execution`] — the state-transition function.
//! * [`genesis`] — deterministic genesis blocks.
//! * [`api`] — JSON views shared by the node REST API and wallets.
//!
//! **Changing anything consensus-related here is a hard fork.** See
//! `docs/PROTOCOL.md` in the repository before editing.

pub mod address;
pub mod amount;
pub mod api;
pub mod block;
pub mod contracts;
pub mod crypto;
pub mod difficulty;
pub mod emission;
pub mod error;
pub mod execution;
pub mod genesis;
pub mod governance;
pub mod hash;
pub mod lthash;
pub mod merkle;
pub mod params;
pub mod pow;
pub mod state;
pub mod tx;

pub use address::Address;
pub use block::{Block, BlockHeader};
pub use error::{BlockError, TxError};
pub use hash::Hash32;
pub use params::{ChainParams, Network};
pub use primitive_types::{U256, U512};
pub use tx::{Transaction, TxAction, TxBody};

/// Software version string reported by nodes and wallets.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
