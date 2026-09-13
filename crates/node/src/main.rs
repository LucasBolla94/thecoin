//! `thecoind` — The Coin full node.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use thecoin_core::Network;
use thecoin_node::{Node, NodeConfig};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "thecoind", version, about = "The Coin full node — validates, relays and mines blocks", long_about = None)]
struct Cli {
    /// Configuration file (TOML). Defaults to <data-dir>/thecoind.toml if it exists.
    #[arg(short, long, env = "THECOIN_CONFIG")]
    config: Option<PathBuf>,

    /// Network: mainnet, testnet or regtest.
    #[arg(long, env = "THECOIN_NETWORK")]
    network: Option<Network>,

    /// Data directory.
    #[arg(long, env = "THECOIN_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Mining payout address (enables mining).
    #[arg(long, env = "THECOIN_MINER_ADDRESS")]
    miner_address: Option<String>,

    /// Mining threads (0 = cores − 1).
    #[arg(long)]
    threads: Option<usize>,

    /// Disable mining.
    #[arg(long)]
    no_mine: bool,

    /// P2P listen address, e.g. 0.0.0.0:7333.
    #[arg(long)]
    listen: Option<String>,

    /// API listen address, e.g. 127.0.0.1:7334.
    #[arg(long)]
    rpc: Option<String>,

    /// Extra peer to connect to (repeatable).
    #[arg(long = "peer")]
    peers: Vec<String>,

    /// Connect only to the given --peer addresses.
    #[arg(long)]
    connect_only: bool,

    /// Keep only recent block bodies (saves disk).
    #[arg(long)]
    prune: bool,

    /// Log level: error, warn, info, debug, trace.
    #[arg(long, default_value = "info", env = "THECOIN_LOG")]
    log_level: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the node (default).
    Run,
    /// Write a default configuration file and exit.
    Init {
        /// Where to write the file.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Print network parameters (supply, emission, ports, genesis).
    Params,
    /// Compact the database file to reclaim disk space (stop the node first).
    Compact,
}

fn build_config(cli: &Cli) -> Result<NodeConfig> {
    let network = cli.network.unwrap_or(Network::Mainnet);
    let default_dir = cli.data_dir.clone().unwrap_or_else(|| thecoin_node::config::default_data_dir(network));
    let config_path = cli.config.clone().or_else(|| {
        let p = default_dir.join("thecoind.toml");
        p.exists().then_some(p)
    });
    let mut cfg = match &config_path {
        Some(p) => NodeConfig::load(p)?,
        None => NodeConfig::for_network(network),
    };
    if let Some(n) = cli.network {
        if n != cfg.network {
            let old = cfg.clone();
            cfg = NodeConfig::for_network(n);
            cfg.mining = old.mining;
        }
    }
    if let Some(d) = &cli.data_dir {
        cfg.data_dir = d.clone();
    }
    if let Some(a) = &cli.miner_address {
        cfg.mining.address = a.clone();
        cfg.mining.enabled = true;
    }
    if let Some(t) = cli.threads {
        cfg.mining.threads = t;
    }
    if cli.no_mine {
        cfg.mining.enabled = false;
    }
    if let Some(l) = &cli.listen {
        cfg.p2p.listen = l.clone();
    }
    if let Some(r) = &cli.rpc {
        cfg.rpc.listen = r.clone();
    }
    if cli.connect_only {
        cfg.p2p.connect = cli.peers.clone();
    } else {
        cfg.p2p.seeds.extend(cli.peers.iter().cloned());
    }
    if cli.prune {
        cfg.storage.prune = true;
    }
    Ok(cfg)
}

fn print_params(network: Network) {
    use thecoin_core::amount::format_amount;
    use thecoin_core::emission::{cumulative_emission, total_emission};
    let p = network.params();
    let g = thecoin_core::genesis::genesis_block(p);
    println!("The Coin — {network}");
    println!("  genesis hash        {}", g.hash());
    println!("  genesis message     {}", p.genesis_message);
    if network == Network::Regtest {
        println!("  total emission      {} TCN (regtest uses fast halvings)", format_amount(total_emission(p)));
    } else {
        println!("  max supply          50,000,000 TCN (total emission {} TCN)", format_amount(total_emission(p)));
    }
    println!("  block time          {} s", p.target_block_time);
    println!("  initial reward      {} TCN", format_amount(p.initial_reward));
    println!("  halving interval    {} blocks", p.halving_interval);
    for era in 1..=4u64 {
        let h = era * p.halving_interval;
        println!(
            "  after era {era}         {} TCN emitted (~{:.2} years)",
            format_amount(cumulative_emission(p, h)),
            h as f64 * p.target_block_time as f64 / 31_557_600.0
        );
    }
    println!("  coinbase maturity   {} blocks", p.coinbase_maturity);
    println!("  PoW                 CoinHash (Argon2id, {} KiB, t={})", p.pow.mem_kib, p.pow.iterations);
    println!("  ports               p2p {} / api {}", p.default_p2p_port, p.default_rpc_port);
    println!("  address prefix      {}1...", p.hrp());
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let level: tracing::Level = cli.log_level.parse().unwrap_or(tracing::Level::INFO);
    tracing_subscriber::fmt().with_max_level(level).with_target(false).init();

    match &cli.command {
        Some(Command::Params) => {
            print_params(cli.network.unwrap_or(Network::Mainnet));
            return Ok(());
        }
        Some(Command::Init { output, force }) => {
            let cfg = build_config(&cli)?;
            let path = output.clone().unwrap_or_else(|| cfg.data_dir.join("thecoind.toml"));
            if path.exists() && !force {
                anyhow::bail!("{} already exists (use --force to overwrite)", path.display());
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, cfg.to_toml()).with_context(|| format!("writing {}", path.display()))?;
            println!("wrote {}", path.display());
            return Ok(());
        }
        Some(Command::Compact) => {
            let cfg = build_config(&cli)?;
            let path = cfg.db_path();
            let before = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let mut db = thecoin_storage::ChainDb::open(&path, &Default::default())?;
            db.compact()?;
            drop(db);
            let after = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!("compacted {}: {} MB -> {} MB", path.display(), before / 1_048_576, after / 1_048_576);
            return Ok(());
        }
        Some(Command::Run) | None => {}
    }

    let cfg = build_config(&cli)?;
    // Small, fixed runtime: networking and API are I/O bound; CPU work runs on
    // dedicated threads (block processor, miners) and the blocking pool.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(16)
        // Contract execution (mempool checks, API views) runs on these threads.
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .thread_name("thecoind")
        .build()?;
    runtime.block_on(async move {
        let node = Node::start(cfg).await?;
        wait_for_signal().await;
        info!("shutting down...");
        node.stop();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        Ok::<_, anyhow::Error>(())
    })?;
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    Ok(())
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("signal handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
