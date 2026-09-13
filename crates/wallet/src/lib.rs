//! # thecoin-wallet
//!
//! Reference wallet library for **The Coin**. Other wallets (mobile, web,
//! hardware) can reuse it directly or re-implement the same open standards:
//!
//! * [`keys`] — BIP-39 mnemonics + SLIP-0010 Ed25519 derivation (`m/44'/7333'/a'/0'/i'`)
//! * [`keystore`] — encrypted wallet file (Argon2id + ChaCha20-Poly1305)
//! * [`builder`] — building and signing transactions
//! * [`client`] — node REST API client
//! * [`uri`] — `thecoin:` payment request URIs
//!
//! See `docs/WALLET_DEVELOPERS.md` for the full specification.

pub mod builder;
pub mod client;
pub mod keys;
pub mod keystore;
pub mod uri;
