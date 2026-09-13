//! Node configuration (`thecoind.toml`).
//!
//! Defaults are tuned for small VPS machines (1–2 vCPU, 1–4 GB RAM).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use thecoin_core::Network;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodeConfig {
    pub network: Network,
    /// Data directory (database, peers, logs).
    pub data_dir: PathBuf,
    pub p2p: P2pConfig,
    pub rpc: RpcConfig,
    pub mining: MiningConfig,
    pub storage: StorageConfig,
    pub mempool: MempoolConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct P2pConfig {
    pub enabled: bool,
    /// Address to accept peers on. Empty string disables listening.
    pub listen: String,
    pub max_inbound: usize,
    pub max_outbound: usize,
    /// Extra seed nodes (`host:port`), added to the built-in seeds.
    pub seeds: Vec<String>,
    /// If non-empty, connect only to these peers.
    pub connect: Vec<String>,
    /// Accept private/loopback addresses from gossip (for LAN/test networks).
    pub allow_private: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RpcConfig {
    pub enabled: bool,
    /// Keep on 127.0.0.1 unless the API is meant to be public (explorer/site).
    pub listen: String,
    /// Allowed CORS origins (`*` for any).
    pub cors_origins: Vec<String>,
    pub max_concurrency: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MiningConfig {
    pub enabled: bool,
    /// Payout address (bech32m). Mining is disabled if empty.
    pub address: String,
    /// Worker threads; 0 = automatic (CPU cores − 1, at least 1).
    pub threads: usize,
    /// Proposal ids (hex) this miner supports in governance signalling.
    pub signal: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    pub cache_mb: usize,
    /// Delete old block bodies, keeping `prune_keep` recent blocks.
    pub prune: bool,
    pub prune_keep: u64,
    /// Maintain the address history index.
    pub address_index: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MempoolConfig {
    pub max_mb: usize,
}

impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig::for_network(Network::Mainnet)
    }
}

impl Default for P2pConfig {
    fn default() -> Self {
        P2pConfig { enabled: true, listen: "0.0.0.0:7333".into(), max_inbound: 32, max_outbound: 8, seeds: vec![], connect: vec![], allow_private: false }
    }
}

impl Default for RpcConfig {
    fn default() -> Self {
        RpcConfig { enabled: true, listen: "127.0.0.1:7334".into(), cors_origins: vec!["*".into()], max_concurrency: 64 }
    }
}

impl Default for MiningConfig {
    fn default() -> Self {
        MiningConfig { enabled: true, address: String::new(), threads: 0, signal: vec![] }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig { cache_mb: 64, prune: false, prune_keep: 10_000, address_index: true }
    }
}

impl Default for MempoolConfig {
    fn default() -> Self {
        MempoolConfig { max_mb: 32 }
    }
}

fn merge_toml(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(existing) => merge_toml(existing, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

pub fn default_data_dir(network: Network) -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".thecoin");
    match network {
        Network::Mainnet => base,
        other => base.join(other.as_str()),
    }
}

impl NodeConfig {
    pub fn for_network(network: Network) -> Self {
        let p = network.params();
        let mut c = NodeConfig {
            network,
            data_dir: default_data_dir(network),
            p2p: P2pConfig::default(),
            rpc: RpcConfig::default(),
            mining: MiningConfig::default(),
            storage: StorageConfig::default(),
            mempool: MempoolConfig::default(),
        };
        c.p2p.listen = format!("0.0.0.0:{}", p.default_p2p_port);
        c.rpc.listen = format!("127.0.0.1:{}", p.default_rpc_port);
        if network == Network::Regtest {
            c.p2p.allow_private = true;
        }
        c
    }

    pub fn load(path: &Path) -> Result<NodeConfig> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_toml_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Parses a config; missing fields take the defaults of the configured network.
    pub fn from_toml_str(text: &str) -> Result<NodeConfig> {
        let user: toml::Value = toml::from_str(text)?;
        let network: Network = match user.get("network").and_then(|v| v.as_str()) {
            Some(s) => s.parse().map_err(anyhow::Error::msg)?,
            None => Network::Mainnet,
        };
        let mut merged = toml::Value::try_from(NodeConfig::for_network(network))?;
        merge_toml(&mut merged, user);
        Ok(merged.try_into()?)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("config serializes")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("chain.redb")
    }

    pub fn peers_path(&self) -> PathBuf {
        self.data_dir.join("peers.json")
    }

    pub fn mining_threads(&self) -> usize {
        if self.mining.threads > 0 {
            return self.mining.threads;
        }
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        cores.saturating_sub(1).max(1)
    }

    pub fn p2p_listen(&self) -> Result<Option<SocketAddr>> {
        if !self.p2p.enabled || self.p2p.listen.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(self.p2p.listen.parse().with_context(|| format!("invalid p2p.listen '{}'", self.p2p.listen))?))
    }
}
