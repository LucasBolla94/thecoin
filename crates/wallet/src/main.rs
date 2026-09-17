//! `thecoin-wallet` — reference command-line wallet for The Coin.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use rand::seq::SliceRandom;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use thecoin_core::amount::{format_amount, parse_amount, TICKER};
use thecoin_core::api::{ActionView, TxView};
use thecoin_core::contracts::{ContractCall, ContractSpec};
use thecoin_core::crypto::SecretKey;
use thecoin_core::governance::{GovParamId, ProposalAction, ProposalSpec, VoteChoice};
use thecoin_core::hash::Hash32;
use thecoin_core::params::MAX_TX_FUEL;
use thecoin_core::programs::program_address;
use thecoin_core::tccl::abi::parse_arg;
use thecoin_core::tccl::{Type, Value};
use thecoin_core::tx::FLAG_REPLACEABLE;
use thecoin_core::{Address, Network, Transaction, TxAction};
use thecoin_wallet::builder::{self, build_tx, FeePolicy, Priority};
use thecoin_wallet::client::NodeClient;
use thecoin_wallet::keys::{generate_mnemonic, parse_mnemonic, HdKeys};
use thecoin_wallet::keystore::{AccountEntry, WalletFile};
use thecoin_wallet::uri::PaymentRequest;

#[derive(Parser)]
#[command(name = "thecoin-wallet", version, about = "The Coin reference wallet")]
struct Cli {
    /// Wallet file (default: ~/.thecoin/wallet-<network>.json).
    #[arg(short, long, env = "THECOIN_WALLET")]
    wallet: Option<PathBuf>,

    /// Network: mainnet, testnet, regtest.
    #[arg(long, env = "THECOIN_NETWORK", default_value = "mainnet")]
    network: Network,

    /// Node API URL (default: http://127.0.0.1:<api port>).
    #[arg(long, env = "THECOIN_NODE")]
    node: Option<String>,

    /// Address index inside the wallet to use.
    #[arg(long, default_value_t = 0, global = true)]
    from: u32,

    /// Fee priority: pay more to be confirmed sooner.
    #[arg(long, value_enum, default_value_t = Priority::Normal, global = true)]
    priority: Priority,

    /// Allow replacing this transaction later with a higher fee (`bump-fee`).
    /// Receivers see it as replaceable, so they may wait for a confirmation.
    #[arg(long, global = true)]
    replaceable: bool,

    /// Skip confirmation prompts.
    #[arg(short, long, global = true)]
    yes: bool,

    /// Build and print the signed transaction hex without broadcasting.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new wallet (prints the recovery phrase once).
    Create {
        #[arg(long, default_value_t = 24)]
        words: usize,
    },
    /// Restore a wallet from a recovery phrase.
    Restore,
    /// Show the address at --from (or derive a new one with --new).
    Address {
        #[arg(long)]
        new: bool,
        #[arg(long)]
        label: Option<String>,
    },
    /// List wallet addresses with balances.
    List,
    /// Show balance of --from.
    Balance,
    /// Send TCN: `send <address> <amount>`.
    Send {
        to: String,
        amount: String,
        #[arg(long, default_value = "")]
        memo: String,
    },
    /// Pay many recipients from a CSV file with lines `address,amount`.
    Batch {
        file: PathBuf,
        #[arg(long, default_value = "")]
        memo: String,
    },
    /// Create a payment request URI to receive a payment.
    Request {
        amount: Option<String>,
        #[arg(long)]
        memo: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// Pay a `thecoin:` payment request URI.
    Pay { uri: String },
    /// Raise the fee of a pending transaction sent with --replaceable.
    BumpFee { txid: String },
    /// Fees right now (minimum, congestion and priority levels).
    Fees,
    /// Recommended confirmations before trusting a payment of this amount.
    Confirmations { amount: String },
    /// Double-spend attempts seen by the node.
    Alerts,
    /// Transaction history of --from.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show a transaction.
    Tx { txid: String },
    /// Node status.
    Status,
    /// Payment contracts and TCCL smart contracts.
    #[command(subcommand)]
    Contract(ContractCmd),
    /// Private payments through TCCL privacy pools (TCCL-PRIV-1 interface).
    #[command(subcommand)]
    Privacy(PrivacyCmd),
    /// Governance: proposals and voting.
    #[command(subcommand)]
    Gov(GovCmd),
    /// Print the recovery phrase (keep it secret!).
    ShowMnemonic,
}

#[derive(Args, Clone)]
struct ProgramTxArgs {
    /// TCN sent to the contract with the call.
    #[arg(long)]
    value: Option<String>,
    /// Fuel limit; by default it is measured by simulating the call (+30%).
    #[arg(long)]
    max_fuel: Option<u64>,
    /// Most TCN you accept to lock as refundable storage deposit.
    #[arg(long, default_value = "1")]
    max_deposit: String,
}

#[derive(Subcommand)]
enum ContractCmd {
    /// Show a native payment contract.
    Show {
        id: String,
    },
    /// Escrow: lock funds for a payee; release or refund later.
    EscrowCreate {
        #[arg(long)]
        payee: String,
        #[arg(long)]
        amount: String,
        /// Blocks until the payer can refund alone (1 block ≈ 15 s; 40 320 ≈ 7 days).
        #[arg(long, default_value_t = 40_320)]
        deadline_blocks: u64,
        #[arg(long)]
        arbiter: Option<String>,
    },
    EscrowRelease {
        id: String,
    },
    EscrowRefund {
        id: String,
    },
    /// Vesting: linear release to a beneficiary.
    VestingCreate {
        #[arg(long)]
        beneficiary: String,
        #[arg(long)]
        amount: String,
        /// Blocks from now until vesting starts.
        #[arg(long, default_value_t = 0)]
        start_in: u64,
        #[arg(long, default_value_t = 0)]
        cliff_blocks: u64,
        #[arg(long)]
        duration_blocks: u64,
        #[arg(long)]
        revocable: bool,
    },
    VestingClaim {
        id: String,
    },
    VestingRevoke {
        id: String,
    },
    /// Subscription: prepaid recurring payments.
    SubscriptionCreate {
        #[arg(long)]
        payee: String,
        /// Amount per period.
        #[arg(long)]
        amount: String,
        /// Period length in blocks (172 800 ≈ 30 days).
        #[arg(long)]
        period_blocks: u64,
        #[arg(long)]
        periods: u32,
    },
    SubscriptionClaim {
        id: String,
    },
    SubscriptionCancel {
        id: String,
    },
    /// HTLC: hash time-locked payment (atomic swaps).
    HtlcCreate {
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        amount: String,
        /// SHA-256 hash lock (hex). If omitted, a random secret is generated and printed.
        #[arg(long)]
        hash_lock: Option<String>,
        /// Blocks until the sender can take the funds back (5 760 ≈ 1 day).
        #[arg(long, default_value_t = 5_760)]
        timeout_blocks: u64,
    },
    HtlcRedeem {
        id: String,
        #[arg(long)]
        preimage: String,
    },
    HtlcRefund {
        id: String,
    },
    /// Multisig vault (M-of-N).
    MultisigCreate {
        /// Comma-separated signer addresses.
        #[arg(long)]
        signers: String,
        #[arg(long)]
        threshold: u8,
        #[arg(long, default_value = "0")]
        deposit: String,
    },
    MultisigDeposit {
        id: String,
        amount: String,
    },
    MultisigPropose {
        id: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: String,
        #[arg(long, default_value = "")]
        memo: String,
    },
    MultisigApprove {
        id: String,
        spend_id: u32,
    },
    MultisigCancel {
        id: String,
        spend_id: u32,
    },
    /// Close an empty multisig and refund its storage deposit to the creator.
    MultisigClose {
        id: String,
    },
    /// Deploy a TCCL smart contract: `deploy file.tccl [init args...]`.
    Deploy {
        file: PathBuf,
        args: Vec<String>,
        #[command(flatten)]
        opts: ProgramTxArgs,
    },
    /// Call an action of a TCCL contract: `invoke <address> <function> [args...]`.
    Invoke {
        address: String,
        function: String,
        args: Vec<String>,
        #[command(flatten)]
        opts: ProgramTxArgs,
    },
    /// Replace the code of a contract you control: `upgrade <address> file.tccl [upgrade args...]`.
    Upgrade {
        address: String,
        file: PathBuf,
        args: Vec<String>,
        /// Required when the wallet cannot ask (the change is permanent for users of the contract).
        #[arg(long)]
        yes_upgrade: bool,
        #[command(flatten)]
        opts: ProgramTxArgs,
    },
    /// Hand the upgrade authority to another address, or `none` to make the
    /// contract final for ever.
    Authority {
        address: String,
        new_authority: String,
    },
    /// Query a view of a TCCL contract (free, no transaction).
    View {
        address: String,
        function: String,
        args: Vec<String>,
    },
    /// Show a TCCL contract: balance, storage, deposit and functions.
    Program {
        address: String,
    },
}

#[derive(Subcommand)]
enum PrivacyCmd {
    /// Show the ring public key for a key index (deposit it into a pool).
    Keygen {
        #[arg(long, default_value_t = 0)]
        key: u32,
    },
    /// Deposit the pool denomination with the ring key `--key`.
    Deposit {
        pool: String,
        #[arg(long, default_value_t = 0)]
        key: u32,
    },
    /// Withdraw a deposit to any address, hidden among `--ring-size` deposits.
    Withdraw {
        pool: String,
        /// Destination address (use a fresh address).
        #[arg(long)]
        to: String,
        #[arg(long, default_value_t = 0)]
        key: u32,
        #[arg(long, default_value_t = 16)]
        ring_size: usize,
        /// Address that submits the transaction and receives `--fee` (default: --from).
        #[arg(long)]
        relayer: Option<String>,
        /// Relayer fee in TCN taken from the withdrawal.
        #[arg(long, default_value = "0")]
        fee: String,
    },
    /// Show whether the deposit of `--key` is in the pool and still unspent.
    Status {
        pool: String,
        #[arg(long, default_value_t = 0)]
        key: u32,
    },
}

#[derive(Subcommand)]
enum GovCmd {
    /// List proposals.
    List,
    /// Show a proposal.
    Show { id: String },
    /// Create a proposal (locks the proposal deposit).
    Propose(ProposeArgs),
    /// Vote: `vote <proposal> yes|no|abstain <weight TCN>` (weight is locked until the vote ends).
    Vote { id: String, choice: String, weight: String },
}

#[derive(Args)]
struct ProposeArgs {
    #[arg(long)]
    title: String,
    /// Link to the full proposal text.
    #[arg(long, default_value = "")]
    url: String,
    /// File with the full text; its SHA-256 is recorded on-chain.
    #[arg(long)]
    text_file: Option<PathBuf>,
    /// Parameter change, e.g. `fee_per_kb=20000`.
    #[arg(long)]
    set_param: Option<String>,
    /// Software upgrade version, e.g. `0.3.0`.
    #[arg(long)]
    upgrade_version: Option<String>,
    /// SHA-256 of the release artifact (hex), used with --upgrade-version.
    #[arg(long)]
    release_hash: Option<String>,
}

struct Ctx {
    network: Network,
    wallet_path: PathBuf,
    client: NodeClient,
    from: u32,
    priority: Priority,
    replaceable: bool,
    yes: bool,
    dry_run: bool,
}

fn password(prompt: &str) -> Result<String> {
    if let Ok(p) = std::env::var("THECOIN_WALLET_PASSWORD") {
        return Ok(p);
    }
    Ok(rpassword::prompt_password(prompt)?)
}

fn confirm(ctx: &Ctx, question: &str) -> Result<bool> {
    if ctx.yes || ctx.dry_run {
        return Ok(true);
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes" | "s" | "sim"))
}

fn tcn(motes: u64) -> String {
    format!("{} {TICKER}", format_amount(motes))
}

fn amount(s: &str) -> Result<u64> {
    parse_amount(s).map_err(|e| anyhow!("invalid amount '{s}': {e}"))
}

fn hash(s: &str) -> Result<Hash32> {
    Hash32::from_hex(s).map_err(|_| anyhow!("invalid id/hash '{s}' (expected 64 hex chars)"))
}

/// Signing key of one wallet address.
struct Signer {
    key: SecretKey,
    address: Address,
    index: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SentRecord {
    txid: Hash32,
    from_index: u32,
    tx: String,
}

impl Ctx {
    fn addr(&self, s: &str) -> Result<Address> {
        Address::decode(s, self.network).map_err(|e| anyhow!("invalid address '{s}': {e}"))
    }

    fn load(&self) -> Result<WalletFile> {
        let w = WalletFile::load(&self.wallet_path).with_context(|| "no wallet found — run `thecoin-wallet create` first")?;
        if w.network != self.network {
            bail!("wallet is for {}, but --network is {}", w.network, self.network);
        }
        Ok(w)
    }

    fn unlock(&self) -> Result<(WalletFile, HdKeys)> {
        let w = self.load()?;
        let keys = w.unlock(&password("Wallet password: ")?)?;
        Ok((w, keys))
    }

    fn signer(&self, keys: &HdKeys, index: u32) -> Signer {
        let key = keys.secret_key(0, index);
        let address = Address::from_public_key(&key.public_key());
        Signer { key, address, index }
    }

    fn flags(&self) -> u8 {
        if self.replaceable {
            FLAG_REPLACEABLE
        } else {
            0
        }
    }

    fn sent_log(&self) -> PathBuf {
        self.wallet_path.with_extension("sent.jsonl")
    }

    fn record_sent(&self, signer: &Signer, tx: &Transaction) -> Result<()> {
        let rec = SentRecord { txid: tx.txid(), from_index: signer.index, tx: hex::encode(tx.to_bytes()) };
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(self.sent_log())?;
        writeln!(f, "{}", serde_json::to_string(&rec)?)?;
        Ok(())
    }

    fn build(&self, signer: &Signer, nonce: u64, action: TxAction) -> Result<Transaction> {
        let fees = self.client.fees()?;
        let policy = FeePolicy::from_fees(&fees, self.priority);
        Ok(build_tx(&signer.key, self.network, nonce, &policy, self.flags(), 0, action))
    }

    /// Signs, shows and broadcasts an action.
    fn send_with(&self, signer: &Signer, action: TxAction, summary: &str) -> Result<Option<Hash32>> {
        let from = signer.address.encode(self.network);
        let account = self.client.account(&from)?;
        let tx = self.build(signer, account.next_nonce, action)?;
        let mut debit = tx.max_debit();
        match &tx.body.action {
            TxAction::Propose { .. } => debit = debit.saturating_add(self.client.status()?.params.proposal_deposit),
            TxAction::CreateContract { .. } => debit = debit.saturating_add(self.client.fees()?.storage_deposit_per_kb),
            _ => {}
        }
        println!("From:     {from}");
        println!("Action:   {summary}");
        println!(
            "Fee:      {} ({} bytes, priority {:?}{})",
            tcn(tx.body.fee),
            tx.size(),
            self.priority,
            if self.replaceable { ", replaceable" } else { "" }
        );
        if debit > account.spendable {
            bail!("insufficient spendable balance: need up to {}, have {}", tcn(debit), tcn(account.spendable));
        }
        if self.dry_run {
            println!("Signed transaction (not broadcast):\n{}", hex::encode(tx.to_bytes()));
            println!("txid: {}", tx.txid());
            return Ok(Some(tx.txid()));
        }
        if !confirm(self, "Broadcast this transaction?")? {
            println!("Cancelled.");
            return Ok(None);
        }
        let txid = self.client.submit(&tx)?;
        self.record_sent(signer, &tx)?;
        println!("Broadcast OK. txid: {txid}");
        Ok(Some(txid))
    }

    fn send_action(&self, action: TxAction, summary: &str) -> Result<Option<Hash32>> {
        let (_, keys) = self.unlock()?;
        let signer = self.signer(&keys, self.from);
        self.send_with(&signer, action, summary)
    }

    fn tip_height(&self) -> Result<u64> {
        Ok(self.client.status()?.height)
    }

    /// Measures fuel by simulation and returns the action with a safe `max_fuel`.
    fn with_measured_fuel(&self, signer: &Signer, make: impl Fn(u64) -> TxAction, fixed: Option<u64>) -> Result<TxAction> {
        if let Some(f) = fixed {
            return Ok(make(f));
        }
        let from = signer.address.encode(self.network);
        let nonce = self.client.account(&from)?.next_nonce;
        let probe = self.build(signer, nonce, make(2_000_000))?;
        let sim = self.client.simulate(&probe)?;
        if !sim.valid {
            bail!("transaction would be rejected: {}", sim.invalid_reason.unwrap_or_default());
        }
        if !sim.success {
            bail!("contract call would fail: {} (nothing was sent)", sim.error.unwrap_or_default());
        }
        let fuel = (sim.fuel_used.saturating_mul(13) / 10 + 5_000).min(MAX_TX_FUEL);
        println!("Fuel:     {} measured, limit {fuel}", sim.fuel_used);
        if let Some(r) = &sim.return_value {
            println!("Returns:  {r}");
        }
        for l in &sim.logs {
            println!(
                "Preview:  {}({}) (simulated on the current state)",
                l.event,
                l.fields.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ")
            );
        }
        Ok(make(fuel))
    }

    /// Parses text arguments with the types of a contract function from the node.
    fn program_args(&self, address: &str, function: &str, raw: &[String]) -> Result<(Vec<Value>, bool)> {
        let info = self.client.program(address)?;
        let f = info.functions.iter().find(|f| f.name == function).ok_or_else(|| {
            anyhow!(
                "contract {} has no function '{function}'. Functions: {}",
                info.name,
                info.functions.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
            )
        })?;
        if raw.len() != f.params.len() {
            bail!(
                "{function} expects {} argument(s): {}",
                f.params.len(),
                f.params.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ")
            );
        }
        let mut out = Vec::new();
        for (a, (name, t)) in raw.iter().zip(&f.params) {
            let ty: Type = t.parse().map_err(|e: String| anyhow!(e))?;
            out.push(parse_arg(a, &ty).map_err(|e| anyhow!("argument '{name}': {e}"))?);
        }
        Ok((out, f.payable))
    }

    /// Calls a view and parses the result with its declared return type.
    fn view_value(&self, address: &str, function: &str, args: &[String]) -> Result<Value> {
        let info = self.client.program(address)?;
        let f = info
            .functions
            .iter()
            .find(|f| f.name == function)
            .ok_or_else(|| anyhow!("pool contract has no view '{function}' (not a TCCL-PRIV-1 pool)"))?;
        let ret: Type = f.returns.parse().map_err(|e: String| anyhow!(e))?;
        let resp = self.client.view(address, function, args)?;
        if let Some(e) = resp.error {
            bail!("{function}: {e}");
        }
        let text = resp.result.unwrap_or_default();
        parse_arg(&text, &ret).map_err(|e| anyhow!("unexpected {function} result '{text}': {e}"))
    }
}

fn describe(v: &TxView) -> String {
    let base = match &v.action {
        ActionView::Transfer { to, amount, memo_text, .. } => {
            format!("transfer {} → {}{}", tcn(*amount), to, memo_text.as_ref().map(|m| format!(" \"{m}\"")).unwrap_or_default())
        }
        ActionView::BatchTransfer { outputs, total, .. } => format!("batch of {} payments, {}", outputs.len(), tcn(*total)),
        ActionView::CreateContract { spec } => format!("create contract {}", serde_json::to_string(spec).unwrap_or_default()),
        ActionView::CallContract { contract, call } => format!("call {} on {}", serde_json::to_string(call).unwrap_or_default(), contract),
        ActionView::Propose { title, .. } => format!("proposal \"{title}\""),
        ActionView::Vote { proposal, choice, weight } => format!("vote {:?} with {} on {}", choice, tcn(*weight), proposal),
        ActionView::Deploy { source_hash, value, .. } => format!(
            "deploy TCCL contract (source {}…){}",
            &source_hash.to_hex()[..12],
            if *value > 0 { format!(" with {}", tcn(*value)) } else { String::new() }
        ),
        ActionView::Invoke { contract, function, args, value, .. } => {
            format!(
                "{function}({}) on {contract}{}",
                args.join(", "),
                if *value > 0 { format!(" with {}", tcn(*value)) } else { String::new() }
            )
        }
        ActionView::Upgrade { contract, source_hash, .. } => {
            format!("upgrade contract {contract} to source {}…", &source_hash.to_hex()[..12])
        }
        ActionView::SetUpgradeAuthority { contract, new_authority, .. } => match new_authority {
            Some(a) => format!("hand the upgrade authority of {contract} to {a}"),
            None => format!("make contract {contract} final (no more upgrades, for ever)"),
        },
    };
    if v.success {
        base
    } else {
        format!("{base} — FAILED: {}", v.error.clone().unwrap_or_default())
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let network = cli.network;
    let wallet_path =
        cli.wallet.clone().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".thecoin").join(format!("wallet-{network}.json")));
    let node_url = cli
        .node
        .clone()
        .or_else(|| WalletFile::load(&wallet_path).ok().and_then(|w| w.node_url))
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", network.params().default_rpc_port));
    let ctx = Ctx {
        network,
        wallet_path,
        client: NodeClient::new(&node_url),
        from: cli.from,
        priority: cli.priority,
        replaceable: cli.replaceable,
        yes: cli.yes,
        dry_run: cli.dry_run,
    };

    match cli.cmd {
        Cmd::Create { words } => {
            if ctx.wallet_path.exists() {
                bail!("wallet {} already exists", ctx.wallet_path.display());
            }
            let m = generate_mnemonic(words)?;
            let pw = password("New wallet password: ")?;
            if std::env::var("THECOIN_WALLET_PASSWORD").is_err() && pw != password("Repeat password: ")? {
                bail!("passwords do not match");
            }
            let w = WalletFile::create(network, &m.to_string(), "", &pw)?;
            w.save(&ctx.wallet_path)?;
            let keys = w.unlock(&pw)?;
            println!("Wallet created: {}", ctx.wallet_path.display());
            println!("\nRECOVERY PHRASE — write it down on paper and keep it offline.\nAnyone with these words controls your coins:\n");
            for (i, word) in m.to_string().split(' ').enumerate() {
                print!("{:>2}. {:<12}", i + 1, word);
                if (i + 1) % 4 == 0 {
                    println!();
                }
            }
            println!("\nAddress: {}", keys.address_string(0, 0, network));
        }
        Cmd::Restore => {
            if ctx.wallet_path.exists() {
                bail!("wallet {} already exists", ctx.wallet_path.display());
            }
            let phrase = match std::env::var("THECOIN_WALLET_MNEMONIC") {
                Ok(p) => p,
                Err(_) => {
                    print!("Recovery phrase: ");
                    std::io::stdout().flush()?;
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line
                }
            };
            let m = parse_mnemonic(&phrase)?;
            let pw = password("New wallet password: ")?;
            let w = WalletFile::create(network, &m.to_string(), "", &pw)?;
            w.save(&ctx.wallet_path)?;
            println!("Wallet restored: {}\nAddress: {}", ctx.wallet_path.display(), w.unlock(&pw)?.address_string(0, 0, network));
        }
        Cmd::Address { new, label } => {
            let (mut w, keys) = ctx.unlock()?;
            let index = if new {
                let i = w.next_index();
                w.accounts.push(AccountEntry { index: i, label: label.unwrap_or_else(|| format!("address {i}")) });
                w.save(&ctx.wallet_path)?;
                i
            } else {
                ctx.from
            };
            println!("{}", keys.address_string(0, index, network));
        }
        Cmd::List => {
            let (w, keys) = ctx.unlock()?;
            let mut total = 0;
            for a in &w.accounts {
                let addr = keys.address_string(0, a.index, network);
                let bal = ctx.client.account(&addr).map(|v| v.balance).unwrap_or(0);
                total += bal;
                println!("#{:<3} {:<16} {}  {}", a.index, a.label, addr, tcn(bal));
            }
            println!("Total: {}", tcn(total));
        }
        Cmd::Balance => {
            let (_, keys) = ctx.unlock()?;
            let addr = keys.address_string(0, ctx.from, network);
            let v = ctx.client.account(&addr)?;
            println!("Address:   {addr}");
            println!("Balance:   {}", tcn(v.balance));
            println!("Spendable: {}", tcn(v.spendable));
            if v.locked > 0 {
                println!("Locked:    {} (governance vote, until block {})", tcn(v.locked), v.locked_until);
            }
            if v.immature > 0 {
                println!("Immature:  {} (mining rewards in cooldown)", tcn(v.immature));
            }
            if v.mempool_txs > 0 {
                println!("Pending:   {} transaction(s) in mempool", v.mempool_txs);
            }
        }
        Cmd::Send { to, amount: a, memo } => {
            let to_addr = ctx.addr(&to)?;
            let value = amount(&a)?;
            ctx.send_action(builder::transfer(to_addr, value, &memo), &format!("send {} to {to}", tcn(value)))?;
        }
        Cmd::Batch { file, memo } => {
            let text = std::fs::read_to_string(&file)?;
            let mut outputs = Vec::new();
            for (n, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (a, v) = line.split_once(',').ok_or_else(|| anyhow!("line {}: expected address,amount", n + 1))?;
                outputs.push((ctx.addr(a.trim())?, amount(v.trim())?));
            }
            let total: u64 = outputs.iter().map(|o| o.1).sum();
            let count = outputs.len();
            ctx.send_action(builder::batch(outputs, &memo), &format!("batch payment of {} to {count} recipients", tcn(total)))?;
        }
        Cmd::Request { amount: a, memo, label } => {
            let (_, keys) = ctx.unlock()?;
            let req = PaymentRequest {
                address: keys.address_string(0, ctx.from, network),
                amount: a.map(|x| amount(&x)).transpose()?,
                memo,
                label,
            };
            println!("{}", req.to_uri());
        }
        Cmd::Pay { uri } => {
            let req = PaymentRequest::parse(&uri, network)?;
            let value = req.amount.ok_or_else(|| anyhow!("payment request has no amount"))?;
            let memo = req.memo.clone().unwrap_or_default();
            ctx.send_action(
                builder::transfer(ctx.addr(&req.address)?, value, &memo),
                &format!(
                    "pay {} to {}{}",
                    tcn(value),
                    req.label.as_deref().unwrap_or(&req.address),
                    if memo.is_empty() { String::new() } else { format!(" ({memo})") }
                ),
            )?;
        }
        Cmd::BumpFee { txid } => bump_fee(&ctx, &hash(&txid)?)?,
        Cmd::Fees => {
            let f = ctx.client.fees()?;
            let policy = |p| FeePolicy::from_fees(&f, p);
            println!("Congestion: {:.2}× (the surcharge above 1× is burned)", f.congestion_bp as f64 / 10_000.0);
            println!("Minimum:    base {} + {} per kB + {} per 1000 fuel", tcn(f.base_fee), tcn(f.fee_per_kb), tcn(f.fee_per_kfuel));
            println!("Storage:    {} per kB of contract state (refundable)", tcn(f.storage_deposit_per_kb));
            println!("Mempool:    {} transaction(s), {} bytes", f.mempool_txs, f.mempool_bytes);
            println!("Typical 160-byte transfer:");
            for (name, p) in [("low", Priority::Low), ("normal", Priority::Normal), ("high", Priority::High), ("urgent", Priority::Urgent)]
            {
                println!("  --priority {name:<7} {}", tcn(policy(p).fee(160, 0)));
            }
        }
        Cmd::Confirmations { amount: a } => {
            let value = amount(&a)?;
            let s = ctx.client.security(value)?;
            match s.blocks_to_finality {
                Some(b) => println!("For {}: wait {} block(s) — it is then final and can never be reversed.", tcn(value), b),
                None => println!("For {}: wait {} confirmation(s) (~{} min).", tcn(value), s.confirmations, s.minutes.max(1)),
            }
            println!("{}", s.explanation);
        }
        Cmd::Alerts => {
            let list = ctx.client.alerts()?;
            if list.is_empty() {
                println!("No double-spend attempts seen by this node.");
            }
            for a in list {
                println!("sender {} nonce {}: {} vs {} (seen at {})", a.sender, a.nonce, a.first, a.second, a.seen_at);
            }
        }
        Cmd::History { limit } => {
            let (_, keys) = ctx.unlock()?;
            let addr = keys.address_string(0, ctx.from, network);
            for v in ctx.client.history(&addr, limit)? {
                let when = match v.block_height {
                    Some(h) => format!("block {h}"),
                    None => "mempool".into(),
                };
                let warn = if v.conflict.is_some() { "  ⚠ double spend attempt" } else { "" };
                println!("{:<12} {}  {}{warn}", when, &v.txid.to_hex()[..16], describe(&v));
            }
        }
        Cmd::Tx { txid } => {
            let v = ctx.client.tx(&hash(&txid)?)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Status => {
            let s = ctx.client.status()?;
            println!("{}", serde_json::to_string_pretty(&s)?);
        }
        Cmd::ShowMnemonic => {
            let w = ctx.load()?;
            let (m, _) = w.reveal(&password("Wallet password: ")?)?;
            println!("{}", m.as_str());
        }
        Cmd::Contract(c) => contract_cmd(&ctx, c)?,
        Cmd::Privacy(p) => privacy_cmd(&ctx, p)?,
        Cmd::Gov(g) => gov_cmd(&ctx, g)?,
    }
    Ok(())
}

fn bump_fee(ctx: &Ctx, txid: &Hash32) -> Result<()> {
    let log = std::fs::read_to_string(ctx.sent_log()).map_err(|_| anyhow!("no transactions sent from this wallet yet"))?;
    let rec = log
        .lines()
        .filter_map(|l| serde_json::from_str::<SentRecord>(l).ok())
        .find(|r| r.txid == *txid)
        .ok_or_else(|| anyhow!("transaction {txid} was not sent from this wallet"))?;
    let tx = Transaction::from_bytes(&hex::decode(&rec.tx)?)?;
    if !tx.is_replaceable() {
        bail!("this transaction was not sent with --replaceable, so the network will not accept a replacement");
    }
    let view = ctx.client.tx(txid)?;
    if !view.in_mempool {
        bail!("transaction is not pending anymore (already confirmed or dropped)");
    }
    let (_, keys) = ctx.unlock()?;
    let signer = ctx.signer(&keys, rec.from_index);
    let fees = ctx.client.fees()?;
    let wanted = FeePolicy::from_fees(&fees, if ctx.priority == Priority::Normal { Priority::High } else { ctx.priority })
        .fee(tx.size(), tx.max_fuel());
    let new_fee = wanted.max(tx.body.fee.saturating_add(tx.body.fee / 4).saturating_add(1));
    let bumped = builder::with_fee(&signer.key, &tx, new_fee);
    println!("Old fee:  {}", tcn(tx.body.fee));
    println!("New fee:  {}", tcn(new_fee));
    if ctx.dry_run {
        println!("{}", hex::encode(bumped.to_bytes()));
        return Ok(());
    }
    if !confirm(ctx, "Replace the pending transaction?")? {
        return Ok(());
    }
    let new_id = ctx.client.submit(&bumped)?;
    ctx.record_sent(&signer, &bumped)?;
    println!("Replacement broadcast. New txid: {new_id}");
    Ok(())
}

fn contract_cmd(ctx: &Ctx, c: ContractCmd) -> Result<()> {
    let call = |id: &str, call: ContractCall, what: &str| -> Result<()> {
        ctx.send_action(builder::call_contract(hash(id)?, call), &format!("{what} on contract {id}")).map(|_| ())
    };
    let create = |spec: ContractSpec, what: String| -> Result<()> {
        if let Some(txid) = ctx.send_action(builder::create_contract(spec), &what)? {
            if !ctx.dry_run {
                println!("The contract id is shown by `thecoin-wallet tx {txid}` (field \"created\") once confirmed.");
                println!("A small refundable storage deposit is locked until the contract finishes.");
            }
        }
        Ok(())
    };
    match c {
        ContractCmd::Show { id } => {
            let v = ctx.client.contract(&hash(&id)?)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        ContractCmd::EscrowCreate { payee, amount: a, deadline_blocks, arbiter } => {
            let value = amount(&a)?;
            let deadline_height = ctx.tip_height()? + deadline_blocks;
            let spec = ContractSpec::Escrow {
                payee: ctx.addr(&payee)?,
                arbiter: arbiter.as_deref().map(|x| ctx.addr(x)).transpose()?,
                amount: value,
                deadline_height,
            };
            create(spec, format!("escrow {} for {payee} (deadline block {deadline_height})", tcn(value)))?;
        }
        ContractCmd::EscrowRelease { id } => call(&id, ContractCall::EscrowRelease, "release escrow")?,
        ContractCmd::EscrowRefund { id } => call(&id, ContractCall::EscrowRefund, "refund escrow")?,
        ContractCmd::VestingCreate { beneficiary, amount: a, start_in, cliff_blocks, duration_blocks, revocable } => {
            let value = amount(&a)?;
            let start = ctx.tip_height()? + 1 + start_in;
            let spec = ContractSpec::Vesting {
                beneficiary: ctx.addr(&beneficiary)?,
                amount: value,
                start_height: start,
                cliff_height: start + cliff_blocks,
                end_height: start + duration_blocks,
                revocable,
            };
            create(spec, format!("vesting {} to {beneficiary} over {duration_blocks} blocks", tcn(value)))?;
        }
        ContractCmd::VestingClaim { id } => call(&id, ContractCall::VestingClaim, "claim vesting")?,
        ContractCmd::VestingRevoke { id } => call(&id, ContractCall::VestingRevoke, "revoke vesting")?,
        ContractCmd::SubscriptionCreate { payee, amount: a, period_blocks, periods } => {
            let value = amount(&a)?;
            let spec =
                ContractSpec::Subscription { payee: ctx.addr(&payee)?, amount_per_period: value, period_blocks, max_periods: periods };
            create(spec, format!("subscription {} × {periods} periods of {period_blocks} blocks to {payee}", tcn(value)))?;
        }
        ContractCmd::SubscriptionClaim { id } => call(&id, ContractCall::SubscriptionClaim, "claim subscription")?,
        ContractCmd::SubscriptionCancel { id } => call(&id, ContractCall::SubscriptionCancel, "cancel subscription")?,
        ContractCmd::HtlcCreate { recipient, amount: a, hash_lock, timeout_blocks } => {
            let value = amount(&a)?;
            let lock = match hash_lock {
                Some(h) => hash(&h)?,
                None => {
                    let mut secret = [0u8; 32];
                    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut secret);
                    println!("Generated secret (preimage) — keep it safe: {}", hex::encode(secret));
                    Hash32(Sha256::digest(secret).into())
                }
            };
            let timeout_height = ctx.tip_height()? + timeout_blocks;
            println!("Hash lock: {lock}");
            create(
                ContractSpec::Htlc { recipient: ctx.addr(&recipient)?, amount: value, hash_lock: lock, timeout_height },
                format!("HTLC {} to {recipient} (timeout block {timeout_height})", tcn(value)),
            )?;
        }
        ContractCmd::HtlcRedeem { id, preimage } => {
            let pre = hex::decode(preimage.trim()).context("preimage must be hex")?;
            call(&id, ContractCall::HtlcRedeem { preimage: pre }, "redeem HTLC")?
        }
        ContractCmd::HtlcRefund { id } => call(&id, ContractCall::HtlcRefund, "refund HTLC")?,
        ContractCmd::MultisigCreate { signers, threshold, deposit } => {
            let list = signers.split(',').map(|s| ctx.addr(s.trim())).collect::<Result<Vec<_>>>()?;
            let n = list.len();
            let dep = amount(&deposit)?;
            create(
                ContractSpec::Multisig { signers: list, threshold, initial_deposit: dep },
                format!("{threshold}-of-{n} multisig vault with {}", tcn(dep)),
            )?;
        }
        ContractCmd::MultisigDeposit { id, amount: a } => {
            let v = amount(&a)?;
            call(&id, ContractCall::MultisigDeposit { amount: v }, &format!("deposit {}", tcn(v)))?
        }
        ContractCmd::MultisigPropose { id, to, amount: a, memo } => {
            let v = amount(&a)?;
            call(
                &id,
                ContractCall::MultisigPropose { to: ctx.addr(&to)?, amount: v, memo: memo.into_bytes() },
                &format!("propose spend of {} to {to}", tcn(v)),
            )?
        }
        ContractCmd::MultisigApprove { id, spend_id } => {
            call(&id, ContractCall::MultisigApprove { spend_id }, &format!("approve spend #{spend_id}"))?
        }
        ContractCmd::MultisigCancel { id, spend_id } => {
            call(&id, ContractCall::MultisigCancel { spend_id }, &format!("cancel spend #{spend_id}"))?
        }
        ContractCmd::MultisigClose { id } => call(&id, ContractCall::MultisigClose, "close multisig")?,
        ContractCmd::Deploy { file, args, opts } => {
            let source = std::fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
            let compile_opts =
                thecoin_core::tccl::CompileOptions::network(ctx.network.hrp(), thecoin_core::tccl::program::LANGUAGE_VERSION);
            let program = thecoin_core::tccl::compile(&source, &compile_opts).map_err(|e| anyhow!("{}:{e}", file.display()))?;
            let init_params = program.find("init").map(|(_, f)| f.params.clone()).unwrap_or_default();
            if args.len() != init_params.len() {
                bail!(
                    "init expects {} argument(s): {}",
                    init_params.len(),
                    init_params.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ")
                );
            }
            let init_args = args
                .iter()
                .zip(&init_params)
                .map(|(a, (n, t))| parse_arg(a, t).map_err(|e| anyhow!("argument '{n}': {e}")))
                .collect::<Result<Vec<_>>>()?;
            let value = opts.value.as_deref().map(amount).transpose()?.unwrap_or(0);
            let max_deposit = amount(&opts.max_deposit)?;
            let (_, keys) = ctx.unlock()?;
            let signer = ctx.signer(&keys, ctx.from);
            let action = ctx.with_measured_fuel(
                &signer,
                |fuel| builder::deploy(&source, init_args.clone(), value, fuel, max_deposit),
                opts.max_fuel,
            )?;
            let nonce = ctx.client.account(&signer.address.encode(ctx.network))?.next_nonce;
            let address = program_address(&signer.address, nonce).encode(ctx.network);
            if ctx.send_with(&signer, action, &format!("deploy contract {} ({} bytes of TCCL)", program.name, source.len()))?.is_some() {
                println!("Contract address: {address}");
            }
        }
        ContractCmd::Upgrade { address, file, args, yes_upgrade, opts } => {
            let contract = ctx.addr(&address)?;
            let source = std::fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
            let current = ctx.client.program(&address)?;
            let Some(authority) = current.upgrade_authority.clone() else {
                bail!("contract {address} is final: its code can never change");
            };
            let compile_opts =
                thecoin_core::tccl::CompileOptions::network(ctx.network.hrp(), thecoin_core::tccl::program::LANGUAGE_VERSION);
            let program = thecoin_core::tccl::compile(&source, &compile_opts).map_err(|e| anyhow!("{}:{e}", file.display()))?;
            let hook = program.find("upgrade").map(|(_, f)| f.params.clone()).unwrap_or_default();
            if args.len() != hook.len() {
                bail!(
                    "upgrade() expects {} argument(s): {}",
                    hook.len(),
                    hook.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ")
                );
            }
            let values = args
                .iter()
                .zip(&hook)
                .map(|(a, (n, t))| parse_arg(a, t).map_err(|e| anyhow!("argument '{n}': {e}")))
                .collect::<Result<Vec<_>>>()?;
            let expected = ctx.client.program_code_hash(&address)?;
            println!("Contract:  {} ({address})", current.name);
            println!("Authority: {authority}");
            println!("Version:   {} → {}", current.code_version, current.code_version + 1);
            if !yes_upgrade && !ctx.yes {
                print!("Replace the code of this contract? Users of it will run the new code. [y/N] ");
                use std::io::Write;
                std::io::stdout().flush().ok();
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if !line.trim().eq_ignore_ascii_case("y") {
                    bail!("cancelled");
                }
            }
            let max_deposit = amount(&opts.max_deposit)?;
            let (_, keys) = ctx.unlock()?;
            let signer = ctx.signer(&keys, ctx.from);
            let action = ctx.with_measured_fuel(
                &signer,
                |fuel| builder::upgrade(contract, &source, expected, values.clone(), fuel, max_deposit),
                opts.max_fuel,
            )?;
            ctx.send_with(&signer, action, &format!("upgrade contract {} at {address}", program.name))?;
        }
        ContractCmd::Authority { address, new_authority } => {
            let contract = ctx.addr(&address)?;
            let expected = ctx.client.program_code_hash(&address)?;
            let next = if new_authority.trim().eq_ignore_ascii_case("none") {
                if !ctx.yes {
                    print!("Make this contract final? Nobody will ever be able to change its code. [y/N] ");
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    if !line.trim().eq_ignore_ascii_case("y") {
                        bail!("cancelled");
                    }
                }
                None
            } else {
                Some(ctx.addr(&new_authority)?)
            };
            let (_, keys) = ctx.unlock()?;
            let signer = ctx.signer(&keys, ctx.from);
            let action = builder::set_upgrade_authority(contract, next, expected);
            let what = match next {
                Some(a) => format!("hand the upgrade authority of {address} to {}", a.encode(ctx.network)),
                None => format!("make contract {address} final (no more upgrades, ever)"),
            };
            ctx.send_with(&signer, action, &what)?;
        }
        ContractCmd::Invoke { address, function, args, opts } => {
            let contract = ctx.addr(&address)?;
            let (values, payable) = ctx.program_args(&address, &function, &args)?;
            let value = opts.value.as_deref().map(amount).transpose()?.unwrap_or(0);
            if value > 0 && !payable {
                bail!("{function} is not payable; remove --value");
            }
            let max_deposit = amount(&opts.max_deposit)?;
            let (_, keys) = ctx.unlock()?;
            let signer = ctx.signer(&keys, ctx.from);
            let action = ctx.with_measured_fuel(
                &signer,
                |fuel| builder::invoke(contract, &function, values.clone(), value, fuel, max_deposit),
                opts.max_fuel,
            )?;
            ctx.send_with(&signer, action, &format!("{function}({}) on {address}", args.join(", ")))?;
        }
        ContractCmd::View { address, function, args } => {
            let resp = ctx.client.view(&address, &function, &args)?;
            match (resp.result, resp.error) {
                (Some(r), _) => println!("{r}"),
                (None, Some(e)) => bail!("{e}"),
                _ => {}
            }
        }
        ContractCmd::Program { address } => {
            let p = ctx.client.program(&address)?;
            println!("Contract:  {} at {}", p.name, p.address);
            println!("Creator:   {} (block {}, tx {})", p.creator, p.created_height, p.deploy_txid);
            println!("Balance:   {}", tcn(p.balance));
            println!("Storage:   {} bytes in {} entries · deposit {}", p.state_bytes, p.storage_items, tcn(p.deposit));
            println!("Language:  TCCL version {} · code version {}", p.language, p.code_version);
            match &p.upgrade_authority {
                Some(a) => println!("Upgrades:  allowed by {a} (it can change this code)"),
                None => println!("Upgrades:  final — this code can never change"),
            }
            for f in p.functions {
                let params = f.params.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ");
                let ret = if f.returns == "nothing" { String::new() } else { format!(" -> {}", f.returns) };
                println!("  {} {}({params}){ret}{}", f.kind, f.name, if f.payable { " payable" } else { "" });
            }
        }
    }
    Ok(())
}

fn privacy_cmd(ctx: &Ctx, p: PrivacyCmd) -> Result<()> {
    let int_of = |v: Value| -> Result<i128> {
        match v {
            Value::Int(i) => Ok(i),
            other => bail!("expected int, got {other:?}"),
        }
    };
    let bytes_of = |v: Value| -> Result<Vec<u8>> {
        match v {
            Value::Bytes(b) => Ok(b),
            other => bail!("expected bytes, got {other:?}"),
        }
    };
    // Downloads every deposited key. Always all of them, never stopping at ours:
    // the node answering these views must not learn which deposit is ours.
    let pool_keys = |pool: &str| -> Result<Vec<Vec<u8>>> {
        let count = int_of(ctx.view_value(pool, "deposits", &[])?)?;
        (0..count).map(|i| bytes_of(ctx.view_value(pool, "key_at", &[i.to_string()])?)).collect()
    };
    let index_in =
        |keys: &[Vec<u8>], public: &[u8; 32]| -> Option<i128> { keys.iter().position(|k| k.as_slice() == public).map(|i| i as i128) };
    match p {
        PrivacyCmd::Keygen { key } => {
            let (_, keys) = ctx.unlock()?;
            let (_, public) = keys.ring_keypair(key);
            println!("0x{}", hex::encode(public));
        }
        PrivacyCmd::Deposit { pool, key } => {
            let contract = ctx.addr(&pool)?;
            let denomination = int_of(ctx.view_value(&pool, "denomination", &[])?)?;
            let (_, keys) = ctx.unlock()?;
            let (_, public) = keys.ring_keypair(key);
            if index_in(&pool_keys(&pool)?, &public).is_some() {
                bail!("ring key #{key} is already deposited in this pool; use another --key");
            }
            let signer = ctx.signer(&keys, ctx.from);
            let value = u64::try_from(denomination).map_err(|_| anyhow!("invalid pool denomination"))?;
            let args = vec![Value::Bytes(public.to_vec())];
            let action = ctx.with_measured_fuel(
                &signer,
                |fuel| builder::invoke(contract, "deposit", args.clone(), value, fuel, parse_amount("1").unwrap_or(0)),
                None,
            )?;
            ctx.send_with(&signer, action, &format!("private deposit of {} into pool {pool} (ring key #{key})", tcn(value)))?;
            println!("Keep your recovery phrase: it is the only way to withdraw (key index #{key}).");
        }
        PrivacyCmd::Status { pool, key } => {
            let (_, keys) = ctx.unlock()?;
            let (secret, public) = keys.ring_keypair(key);
            let image = thecoin_core::tccl::ring::key_image(&secret).ok_or_else(|| anyhow!("invalid ring key"))?;
            match index_in(&pool_keys(&pool)?, &public) {
                None => println!("Ring key #{key} has no deposit in this pool."),
                Some(i) => {
                    let spent = matches!(ctx.view_value(&pool, "is_withdrawn", &[format!("0x{}", hex::encode(image))])?, Value::Bool(true));
                    let total = int_of(ctx.view_value(&pool, "deposits", &[])?)?;
                    println!(
                        "Deposit at index {i} ({total} deposits in the pool) — {}",
                        if spent { "already withdrawn" } else { "available to withdraw" }
                    );
                }
            }
        }
        PrivacyCmd::Withdraw { pool, to, key, ring_size, relayer, fee } => {
            let contract = ctx.addr(&pool)?;
            let to_addr = ctx.addr(&to)?;
            let fee_motes = amount(&fee)?;
            let (_, keys) = ctx.unlock()?;
            let signer = ctx.signer(&keys, ctx.from);
            let relayer_addr = match relayer {
                Some(r) => ctx.addr(&r)?,
                None => signer.address,
            };
            let (secret, public) = keys.ring_keypair(key);
            let all_keys = pool_keys(&pool)?;
            let my_index = index_in(&all_keys, &public).ok_or_else(|| anyhow!("ring key #{key} has no deposit in this pool"))?;
            let image = thecoin_core::tccl::ring::key_image(&secret).ok_or_else(|| anyhow!("invalid ring key"))?;
            if matches!(ctx.view_value(&pool, "is_withdrawn", &[format!("0x{}", hex::encode(image))])?, Value::Bool(true)) {
                bail!("this deposit was already withdrawn");
            }
            let total = all_keys.len() as i128;
            let size = ring_size.clamp(2, 32).min(total as usize);
            if size < 2 {
                bail!("the pool needs at least 2 deposits before anyone can withdraw privately");
            }
            // Random ring that includes our deposit, in random order.
            let mut others: Vec<i128> = (0..total).filter(|i| *i != my_index).collect();
            others.shuffle(&mut rand::thread_rng());
            let mut members: Vec<i128> = others.into_iter().take(size - 1).collect();
            members.push(my_index);
            members.shuffle(&mut rand::thread_rng());
            let mut ring = Vec::with_capacity(size);
            for m in &members {
                let k = &all_keys[*m as usize];
                ring.push(<[u8; 32]>::try_from(k.as_slice()).map_err(|_| anyhow!("pool key #{m} is not 32 bytes"))?);
            }
            let position = members.iter().position(|m| *m == my_index).expect("included");
            let message = bytes_of(ctx.view_value(
                &pool,
                "message_for",
                &[to_addr.encode(ctx.network), relayer_addr.encode(ctx.network), fee_motes.to_string()],
            )?)?;
            let (signature, key_image) = thecoin_core::tccl::ring::sign(&message, &ring, position, &secret, &mut rand::rngs::OsRng)
                .map_err(|e| anyhow!("ring signature: {e}"))?;
            let args = vec![
                Value::Address(to_addr.0),
                Value::Address(relayer_addr.0),
                Value::Int(fee_motes as i128),
                Value::List(members.iter().map(|m| Value::Int(*m)).collect()),
                Value::Bytes(signature),
                Value::Bytes(key_image.to_vec()),
            ];
            let action = ctx.with_measured_fuel(
                &signer,
                |fuel| builder::invoke(contract, "withdraw", args.clone(), 0, fuel, parse_amount("1").unwrap_or(0)),
                None,
            )?;
            ctx.send_with(&signer, action, &format!("private withdrawal to {to} hidden among {size} deposits"))?;
            if relayer_addr == signer.address {
                println!("Note: this transaction was sent from your own address {}. For stronger privacy, send it from an unrelated address or a relayer.", signer.address.encode(ctx.network));
            }
        }
    }
    Ok(())
}

fn gov_cmd(ctx: &Ctx, g: GovCmd) -> Result<()> {
    match g {
        GovCmd::List => {
            for p in ctx.client.proposals()? {
                let t = &p.tally;
                println!(
                    "{}  [{}] {}\n    yes {} / no {} / abstain {} · miners {}/{} blocks · ends at block {}",
                    p.id,
                    p.status,
                    p.title,
                    tcn(t.yes),
                    tcn(t.no),
                    tcn(t.abstain),
                    t.miner_yes_blocks,
                    t.miner_total_blocks,
                    p.end_height
                );
            }
        }
        GovCmd::Show { id } => println!("{}", serde_json::to_string_pretty(&ctx.client.proposal(&hash(&id)?)?)?),
        GovCmd::Propose(a) => {
            let content_hash = match &a.text_file {
                Some(f) => Hash32(Sha256::digest(std::fs::read(f)?).into()),
                None => Hash32::ZERO,
            };
            let action = if let Some(sp) = &a.set_param {
                let (name, value) = sp.split_once('=').ok_or_else(|| anyhow!("--set-param must be name=value"))?;
                let param = GovParamId::from_name(name.trim()).ok_or_else(|| {
                    anyhow!(
                        "unknown parameter '{name}'. Parameters: {}",
                        GovParamId::ALL.iter().map(|p| p.name()).collect::<Vec<_>>().join(", ")
                    )
                })?;
                ProposalAction::SetParam { param, value: value.trim().parse().context("parameter value must be an integer")? }
            } else if let Some(v) = &a.upgrade_version {
                ProposalAction::SoftwareUpgrade {
                    version: v.clone(),
                    release_hash: a.release_hash.as_deref().map(hash).transpose()?.unwrap_or(Hash32::ZERO),
                }
            } else {
                ProposalAction::Text
            };
            let deposit = ctx.client.status()?.params.proposal_deposit;
            let spec = ProposalSpec { title: a.title.clone(), url: a.url.clone(), content_hash, action };
            if let Some(txid) =
                ctx.send_action(builder::propose(spec), &format!("open proposal \"{}\" (deposit {})", a.title, tcn(deposit)))?
            {
                if !ctx.dry_run {
                    println!("The proposal id is shown by `thecoin-wallet tx {txid}` (field \"created\").");
                }
            }
        }
        GovCmd::Vote { id, choice, weight } => {
            let choice = match choice.to_lowercase().as_str() {
                "yes" | "sim" => VoteChoice::Yes,
                "no" | "nao" | "não" => VoteChoice::No,
                "abstain" | "abster" => VoteChoice::Abstain,
                other => bail!("choice must be yes, no or abstain (got {other})"),
            };
            let w = amount(&weight)?;
            ctx.send_action(
                builder::vote(hash(&id)?, choice, w),
                &format!("vote {choice:?} with {} (locked until the vote ends)", tcn(w)),
            )?;
        }
    }
    Ok(())
}
