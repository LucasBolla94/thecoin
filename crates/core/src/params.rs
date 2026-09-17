//! Network (chain) parameters.
//!
//! Values that are marked *consensus* can only be changed by a hard fork.
//! Values in [`GovParams`](crate::governance::GovParams) can be changed on-chain
//! by the governance system, inside the bounds defined here.

use crate::amount::COIN;
use crate::governance::{GovBounds, GovParams};
use crate::pow::PowParams;
use crate::U256;
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
    Regtest,
}

impl Network {
    pub fn params(self) -> &'static ChainParams {
        match self {
            Network::Mainnet => &MAINNET,
            Network::Testnet => &TESTNET,
            Network::Regtest => &REGTEST,
        }
    }

    pub fn hrp(self) -> &'static str {
        match self {
            Network::Mainnet => "tc",
            Network::Testnet => "tct",
            Network::Regtest => "tcr",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Regtest => "regtest",
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Network {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "mainnet" | "main" => Ok(Network::Mainnet),
            "testnet" | "test" => Ok(Network::Testnet),
            "regtest" | "reg" => Ok(Network::Regtest),
            other => Err(format!("unknown network '{other}'")),
        }
    }
}

/// Hard cap on the total supply: 50 000 000 TCN.
pub const MAX_SUPPLY: u64 = 50_000_000 * COIN;

/// Absolute upper bound for a serialized block (governance can never exceed it).
pub const MAX_BLOCK_BYTES_HARD: u64 = 8_000_000;
/// Maximum serialized size of a single transaction.
pub const MAX_TX_BYTES: usize = 16_384;
/// Maximum serialized size of a contract deployment transaction.
pub const MAX_DEPLOY_TX_BYTES: usize = 64_000;
/// Maximum fuel a single transaction may reserve.
pub const MAX_TX_FUEL: u64 = 10_000_000;
/// Maximum size of a compiled contract program.
pub const MAX_PROGRAM_BYTES: usize = 262_144;
/// Maximum events a contract call may emit.
pub const MAX_EVENTS_PER_CALL: usize = 64;
/// Congestion multiplier bounds in basis points (1.0× … 1000×).
pub const CONGESTION_MIN_BP: u64 = 10_000;
pub const CONGESTION_MAX_BP: u64 = 10_000_000;
/// LWMA starts adjusting once this many solve times are available.
pub const LWMA_MIN_WINDOW: u64 = 6;
/// Maximum memo size in bytes.
pub const MAX_MEMO_BYTES: usize = 256;
/// Maximum number of outputs in a `BatchTransfer`.
pub const MAX_BATCH_OUTPUTS: usize = 128;
/// Number of past blocks used for the median-time-past rule.
pub const MTP_WINDOW: usize = 11;
/// Current block header version.
pub const BLOCK_VERSION: u32 = 1;

#[derive(Debug)]
pub struct ChainParams {
    pub network: Network,
    /// 4 bytes at the start of every P2P frame.
    pub magic: [u8; 4],
    /// Included in every signed transaction (replay protection between networks).
    pub chain_id: u32,
    pub default_p2p_port: u16,
    pub default_rpc_port: u16,

    // ---- Proof of work -------------------------------------------------
    pub pow: PowParams,
    /// Easiest allowed target is `U256::MAX >> pow_limit_shift`.
    pub pow_limit_shift: u32,
    /// Target of the first blocks is `U256::MAX >> genesis_target_shift`.
    pub genesis_target_shift: u32,
    /// Whether LWMA retargeting is enabled (disabled on regtest).
    pub retarget: bool,
    /// Target seconds between blocks.
    pub target_block_time: u64,
    /// LWMA averaging window in blocks.
    pub lwma_window: u64,
    /// Maximum seconds a block timestamp may be ahead of local time.
    pub max_future_drift: u64,
    /// Blocks whose miners may vote on finality (see [`crate::finality`]).
    pub finality_window: u64,

    // ---- Emission ------------------------------------------------------
    pub initial_reward: u64,
    pub halving_interval: u64,
    /// Blocks before the first quarter of a mining reward becomes spendable.
    pub coinbase_maturity: u64,
    /// Blocks before the rest of the reward (cooldown) becomes spendable.
    /// Chosen deeper than `max_reorg_depth`, so rewards of blocks that could
    /// still be reorganized away are never fully spendable.
    pub reward_unlock_blocks: u64,

    // ---- Chain safety --------------------------------------------------
    /// Reorganizations deeper than this many blocks are refused.
    pub max_reorg_depth: u64,
    /// Hardcoded `(height, block hash hex)` checkpoints. A block at a
    /// checkpoint height must have exactly that hash, and no fork below the
    /// last checkpoint is accepted.
    pub checkpoints: &'static [(u64, &'static str)],

    // ---- Genesis -------------------------------------------------------
    pub genesis_timestamp: u64,
    pub genesis_message: &'static str,

    // ---- Networking ----------------------------------------------------
    /// DNS names or `ip:port` of seed nodes.
    pub seeds: &'static [&'static str],

    // ---- Governance ----------------------------------------------------
    pub gov_defaults: GovParams,
    pub gov_bounds: GovBounds,
}

impl ChainParams {
    pub fn pow_limit(&self) -> U256 {
        U256::MAX >> self.pow_limit_shift
    }

    pub fn genesis_target(&self) -> U256 {
        U256::MAX >> self.genesis_target_shift
    }

    pub fn hrp(&self) -> &'static str {
        self.network.hrp()
    }
}

const GENESIS_MESSAGE: &str = "The Coin | the-coin.cloud | 2026-09-13 | A democratic, lightweight proof-of-work money for everyone";

const MAINNET_GOV_BOUNDS: GovBounds = GovBounds {
    max_block_bytes: (250_000, MAX_BLOCK_BYTES_HARD),
    max_block_fuel: (5_000_000, 500_000_000),
    base_fee: (0, 10_000_000),
    fee_per_kb: (1, 100_000_000),
    fee_per_kfuel: (1, 100_000_000),
    storage_deposit_per_kb: (0, 1_000_000_000),
    proposal_deposit: (COIN, 1_000_000 * COIN),
    vote_period: (5_760, 806_400),
    quorum_bp: (100, 5_000),
    approval_bp: (5_001, 9_500),
    miner_approval_bp: (5_000, 9_500),
    activation_delay: (720, 172_800),
};

const MAINNET_GOV_DEFAULTS: GovParams = GovParams {
    max_block_bytes: 1_000_000,
    max_block_fuel: 50_000_000,
    // 0.00001 TCN per transaction
    base_fee: 1_000,
    // 10 motes per byte
    fee_per_kb: 10_000,
    // 0.00001 TCN per 1 000 fuel
    fee_per_kfuel: 1_000,
    // 0.001 TCN locked per kB of contract state, refunded when freed
    storage_deposit_per_kb: 100_000,
    proposal_deposit: 100 * COIN,
    // 14 days of 15 s blocks
    vote_period: 80_640,
    quorum_bp: 1_000,
    approval_bp: 6_667,
    miner_approval_bp: 6_000,
    // 12 hours
    activation_delay: 2_880,
};

pub static MAINNET: ChainParams = ChainParams {
    network: Network::Mainnet,
    magic: *b"TCN1",
    chain_id: 0x5443_0001,
    default_p2p_port: 7333,
    default_rpc_port: 7334,
    pow: PowParams { epoch_blocks: 2_048 },
    pow_limit_shift: 8,
    genesis_target_shift: 12,
    retarget: true,
    target_block_time: 15,
    lwma_window: 120,
    max_future_drift: 180,
    finality_window: 200,
    // 10 TCN every 15 s = the same 40 TCN per minute as a 60 s block, and the
    // halving interval is four times longer, so the schedule in *time* and the
    // 50 000 000 TCN cap are unchanged.
    initial_reward: 10 * COIN,
    halving_interval: 2_500_000,
    coinbase_maturity: 400,
    reward_unlock_blocks: 4_000,
    max_reorg_depth: 2_880,
    checkpoints: &[],
    genesis_timestamp: 1_789_257_600, // 2026-09-13 00:00:00 UTC
    genesis_message: GENESIS_MESSAGE,
    seeds: &["seed1.the-coin.cloud:7333", "seed2.the-coin.cloud:7333"],
    gov_defaults: MAINNET_GOV_DEFAULTS,
    gov_bounds: MAINNET_GOV_BOUNDS,
};

pub static TESTNET: ChainParams = ChainParams {
    network: Network::Testnet,
    magic: *b"TCNT",
    chain_id: 0x5443_0002,
    default_p2p_port: 17333,
    default_rpc_port: 17334,
    pow: PowParams { epoch_blocks: 2_048 },
    pow_limit_shift: 6,
    genesis_target_shift: 9,
    retarget: true,
    target_block_time: 15,
    lwma_window: 120,
    max_future_drift: 180,
    finality_window: 200,
    // 10 TCN every 15 s = the same 40 TCN per minute as a 60 s block, and the
    // halving interval is four times longer, so the schedule in *time* and the
    // 50 000 000 TCN cap are unchanged.
    initial_reward: 10 * COIN,
    halving_interval: 2_500_000,
    coinbase_maturity: 400,
    reward_unlock_blocks: 4_000,
    max_reorg_depth: 2_880,
    checkpoints: &[],
    genesis_timestamp: 1_789_257_600,
    genesis_message: GENESIS_MESSAGE,
    seeds: &["testnet-seed1.the-coin.cloud:17333", "testnet-seed2.the-coin.cloud:17333"],
    gov_defaults: GovParams {
        proposal_deposit: 10 * COIN,
        vote_period: 5_760,
        quorum_bp: 500,
        miner_approval_bp: 5_000,
        activation_delay: 720,
        ..MAINNET_GOV_DEFAULTS
    },
    gov_bounds: MAINNET_GOV_BOUNDS,
};

/// Local testing network: trivial PoW, fast halvings and short governance periods.
pub static REGTEST: ChainParams = ChainParams {
    network: Network::Regtest,
    magic: *b"TCNR",
    chain_id: 0x5443_0003,
    default_p2p_port: 27333,
    default_rpc_port: 27334,
    pow: PowParams { epoch_blocks: 64 },
    pow_limit_shift: 0,
    genesis_target_shift: 0,
    retarget: false,
    target_block_time: 60,
    lwma_window: 60,
    max_future_drift: 180,
    finality_window: 20,
    initial_reward: 40 * COIN,
    halving_interval: 150,
    coinbase_maturity: 5,
    reward_unlock_blocks: 12,
    max_reorg_depth: 720,
    checkpoints: &[],
    genesis_timestamp: 1_789_257_600,
    genesis_message: GENESIS_MESSAGE,
    seeds: &[],
    gov_defaults: GovParams {
        base_fee: 100,
        fee_per_kb: 1_000,
        fee_per_kfuel: 100,
        storage_deposit_per_kb: 10_000,
        proposal_deposit: COIN,
        vote_period: 20,
        quorum_bp: 1_000,
        approval_bp: 6_667,
        miner_approval_bp: 5_000,
        activation_delay: 5,
        ..MAINNET_GOV_DEFAULTS
    },
    gov_bounds: GovBounds {
        max_block_bytes: (10_000, MAX_BLOCK_BYTES_HARD),
        max_block_fuel: (1_000_000, 500_000_000),
        base_fee: (0, 10_000_000),
        fee_per_kb: (0, 100_000_000),
        fee_per_kfuel: (0, 100_000_000),
        storage_deposit_per_kb: (0, 1_000_000_000),
        proposal_deposit: (0, 1_000_000 * COIN),
        vote_period: (5, 201_600),
        quorum_bp: (0, 10_000),
        approval_bp: (5_001, 10_000),
        miner_approval_bp: (0, 10_000),
        activation_delay: (1, 43_200),
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_within_bounds() {
        for n in [Network::Mainnet, Network::Testnet, Network::Regtest] {
            let p = n.params();
            p.gov_defaults.validate(&p.gov_bounds).unwrap();
            assert!(p.genesis_target() <= p.pow_limit());
            assert!(p.coinbase_maturity < p.reward_unlock_blocks);
            assert!(p.network == Network::Regtest || p.reward_unlock_blocks > p.max_reorg_depth);
        }
    }
}
