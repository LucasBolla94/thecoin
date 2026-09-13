//! `thecoin-wallet` — reference command-line wallet for The Coin.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use thecoin_core::amount::{format_amount, parse_amount, TICKER};
use thecoin_core::api::{ActionView, TxView};
use thecoin_core::contracts::{ContractCall, ContractSpec};
use thecoin_core::governance::{GovParamId, ProposalAction, ProposalSpec, VoteChoice};
use thecoin_core::hash::Hash32;
use thecoin_core::{Address, Network, Transaction, TxAction};
use thecoin_wallet::builder::{self, build_tx};
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
    /// Transaction history of --from.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show a transaction.
    Tx { txid: String },
    /// Node status.
    Status,
    /// Payment contracts.
    #[command(subcommand)]
    Contract(ContractCmd),
    /// Governance: proposals and voting.
    #[command(subcommand)]
    Gov(GovCmd),
    /// Print the recovery phrase (keep it secret!).
    ShowMnemonic,
}

#[derive(Subcommand)]
enum ContractCmd {
    /// Show a contract.
    Show { id: String },
    /// Escrow: lock funds for a payee; release or refund later.
    EscrowCreate {
        #[arg(long)]
        payee: String,
        #[arg(long)]
        amount: String,
        /// Blocks until the payer can refund alone (1 block ≈ 1 minute).
        #[arg(long, default_value_t = 10_080)]
        deadline_blocks: u64,
        #[arg(long)]
        arbiter: Option<String>,
    },
    EscrowRelease { id: String },
    EscrowRefund { id: String },
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
    VestingClaim { id: String },
    VestingRevoke { id: String },
    /// Subscription: prepaid recurring payments.
    SubscriptionCreate {
        #[arg(long)]
        payee: String,
        /// Amount per period.
        #[arg(long)]
        amount: String,
        /// Period length in blocks (43 200 ≈ 30 days).
        #[arg(long)]
        period_blocks: u64,
        #[arg(long)]
        periods: u32,
    },
    SubscriptionClaim { id: String },
    SubscriptionCancel { id: String },
    /// HTLC: hash time-locked payment (atomic swaps).
    HtlcCreate {
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        amount: String,
        /// SHA-256 hash lock (hex). If omitted, a random secret is generated and printed.
        #[arg(long)]
        hash_lock: Option<String>,
        #[arg(long, default_value_t = 1_440)]
        timeout_blocks: u64,
    },
    HtlcRedeem {
        id: String,
        #[arg(long)]
        preimage: String,
    },
    HtlcRefund { id: String },
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
    MultisigDeposit { id: String, amount: String },
    MultisigPropose {
        id: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: String,
        #[arg(long, default_value = "")]
        memo: String,
    },
    MultisigApprove { id: String, spend_id: u32 },
    MultisigCancel { id: String, spend_id: u32 },
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
    /// Parameter change, e.g. `min_fee_per_byte=20`.
    #[arg(long)]
    set_param: Option<String>,
    /// Software upgrade version, e.g. `0.2.0`.
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

    /// Signs and broadcasts an action from address index `self.from`.
    fn send_action(&self, action: TxAction, summary: &str) -> Result<Option<Hash32>> {
        let (_, keys) = self.unlock()?;
        let sk = keys.secret_key(0, self.from);
        let from = Address::from_public_key(&sk.public_key()).encode(self.network);
        let account = self.client.account(&from)?;
        let fees = self.client.fees()?;
        let tx: Transaction = build_tx(&sk, self.network, account.next_nonce, fees.suggested_fee_per_byte, 0, action);
        let debit = tx.max_debit();
        println!("From:   {from}");
        println!("Action: {summary}");
        println!("Fee:    {} ({} bytes)", tcn(tx.body.fee), tx.size());
        if debit > account.spendable {
            bail!("insufficient spendable balance: need {}, have {}", tcn(debit), tcn(account.spendable));
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
        println!("Broadcast OK. txid: {txid}");
        Ok(Some(txid))
    }

    fn tip_height(&self) -> Result<u64> {
        Ok(self.client.status()?.height)
    }
}

fn describe(v: &TxView) -> String {
    match &v.action {
        ActionView::Transfer { to, amount, memo_text, .. } => format!("transfer {} → {}{}", tcn(*amount), to, memo_text.as_ref().map(|m| format!(" \"{m}\"")).unwrap_or_default()),
        ActionView::BatchTransfer { outputs, total, .. } => format!("batch of {} payments, {}", outputs.len(), tcn(*total)),
        ActionView::CreateContract { spec } => format!("create contract {}", serde_json::to_string(spec).unwrap_or_default()),
        ActionView::CallContract { contract, call } => format!("call {} on {}", serde_json::to_string(call).unwrap_or_default(), contract),
        ActionView::Propose { title, .. } => format!("proposal \"{title}\""),
        ActionView::Vote { proposal, choice, weight } => format!("vote {:?} with {} on {}", choice, tcn(*weight), proposal),
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
    let wallet_path = cli.wallet.clone().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".thecoin").join(format!("wallet-{network}.json")));
    let node_url = cli
        .node
        .clone()
        .or_else(|| WalletFile::load(&wallet_path).ok().and_then(|w| w.node_url))
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", network.params().default_rpc_port));
    let ctx = Ctx { network, wallet_path, client: NodeClient::new(&node_url), from: cli.from, yes: cli.yes, dry_run: cli.dry_run };

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
                println!("Immature:  {} (mining rewards maturing)", tcn(v.immature));
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
            let req = PaymentRequest { address: keys.address_string(0, ctx.from, network), amount: a.map(|x| amount(&x)).transpose()?, memo, label };
            println!("{}", req.to_uri());
        }
        Cmd::Pay { uri } => {
            let req = PaymentRequest::parse(&uri, network)?;
            let value = req.amount.ok_or_else(|| anyhow!("payment request has no amount"))?;
            let memo = req.memo.clone().unwrap_or_default();
            ctx.send_action(builder::transfer(ctx.addr(&req.address)?, value, &memo), &format!("pay {} to {}{}", tcn(value), req.label.as_deref().unwrap_or(&req.address), if memo.is_empty() { String::new() } else { format!(" ({memo})") }))?;
        }
        Cmd::History { limit } => {
            let (_, keys) = ctx.unlock()?;
            let addr = keys.address_string(0, ctx.from, network);
            for v in ctx.client.history(&addr, limit)? {
                let when = match v.block_height {
                    Some(h) => format!("block {h}"),
                    None => "mempool".into(),
                };
                println!("{:<12} {}  {}", when, &v.txid.to_hex()[..16], describe(&v));
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
        Cmd::Gov(g) => gov_cmd(&ctx, g)?,
    }
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
            let spec = ContractSpec::Escrow { payee: ctx.addr(&payee)?, arbiter: arbiter.as_deref().map(|x| ctx.addr(x)).transpose()?, amount: value, deadline_height };
            create(spec, format!("escrow {} for {payee} (deadline block {deadline_height})", tcn(value)))?;
        }
        ContractCmd::EscrowRelease { id } => call(&id, ContractCall::EscrowRelease, "release escrow")?,
        ContractCmd::EscrowRefund { id } => call(&id, ContractCall::EscrowRefund, "refund escrow")?,
        ContractCmd::VestingCreate { beneficiary, amount: a, start_in, cliff_blocks, duration_blocks, revocable } => {
            let value = amount(&a)?;
            let start = ctx.tip_height()? + 1 + start_in;
            let spec = ContractSpec::Vesting { beneficiary: ctx.addr(&beneficiary)?, amount: value, start_height: start, cliff_height: start + cliff_blocks, end_height: start + duration_blocks, revocable };
            create(spec, format!("vesting {} to {beneficiary} over {duration_blocks} blocks", tcn(value)))?;
        }
        ContractCmd::VestingClaim { id } => call(&id, ContractCall::VestingClaim, "claim vesting")?,
        ContractCmd::VestingRevoke { id } => call(&id, ContractCall::VestingRevoke, "revoke vesting")?,
        ContractCmd::SubscriptionCreate { payee, amount: a, period_blocks, periods } => {
            let value = amount(&a)?;
            let spec = ContractSpec::Subscription { payee: ctx.addr(&payee)?, amount_per_period: value, period_blocks, max_periods: periods };
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
            create(ContractSpec::Htlc { recipient: ctx.addr(&recipient)?, amount: value, hash_lock: lock, timeout_height }, format!("HTLC {} to {recipient} (timeout block {timeout_height})", tcn(value)))?;
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
            create(ContractSpec::Multisig { signers: list, threshold, initial_deposit: dep }, format!("{threshold}-of-{n} multisig vault with {}", tcn(dep)))?;
        }
        ContractCmd::MultisigDeposit { id, amount: a } => {
            let v = amount(&a)?;
            call(&id, ContractCall::MultisigDeposit { amount: v }, &format!("deposit {}", tcn(v)))?
        }
        ContractCmd::MultisigPropose { id, to, amount: a, memo } => {
            let v = amount(&a)?;
            call(&id, ContractCall::MultisigPropose { to: ctx.addr(&to)?, amount: v, memo: memo.into_bytes() }, &format!("propose spend of {} to {to}", tcn(v)))?
        }
        ContractCmd::MultisigApprove { id, spend_id } => call(&id, ContractCall::MultisigApprove { spend_id }, &format!("approve spend #{spend_id}"))?,
        ContractCmd::MultisigCancel { id, spend_id } => call(&id, ContractCall::MultisigCancel { spend_id }, &format!("cancel spend #{spend_id}"))?,
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
                let param = GovParamId::from_name(name.trim()).ok_or_else(|| anyhow!("unknown parameter '{name}'"))?;
                ProposalAction::SetParam { param, value: value.trim().parse().context("parameter value must be an integer")? }
            } else if let Some(v) = &a.upgrade_version {
                ProposalAction::SoftwareUpgrade { version: v.clone(), release_hash: a.release_hash.as_deref().map(hash).transpose()?.unwrap_or(Hash32::ZERO) }
            } else {
                ProposalAction::Text
            };
            let deposit = ctx.client.status()?.params.proposal_deposit;
            let spec = ProposalSpec { title: a.title.clone(), url: a.url.clone(), content_hash, action };
            if let Some(txid) = ctx.send_action(builder::propose(spec), &format!("open proposal \"{}\" (deposit {})", a.title, tcn(deposit)))? {
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
            ctx.send_action(builder::vote(hash(&id)?, choice, w), &format!("vote {choice:?} with {} (locked until the vote ends)", tcn(w)))?;
        }
    }
    Ok(())
}
