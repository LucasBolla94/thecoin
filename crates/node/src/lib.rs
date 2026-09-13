//! # thecoin-node
//!
//! Full node of **The Coin**: validates and stores the chain, relays blocks and
//! transactions over P2P, mines with the CPU and serves the REST API.
//!
//! Module map:
//! * [`chain`] — block validation, fork choice, reorgs, pruning, templates
//! * [`mempool`] — transaction pool
//! * [`protocol`] / [`net`] / [`addrman`] — peer-to-peer networking
//! * [`miner`] — CPU miner
//! * [`rpc`] — REST API
//! * [`config`] — `thecoind.toml`
//! * [`node`] — wiring and startup

pub mod addrman;
pub mod chain;
pub mod config;
pub mod mempool;
pub mod miner;
pub mod net;
pub mod node;
pub mod protocol;
pub mod rpc;

pub use config::NodeConfig;
pub use node::Node;
