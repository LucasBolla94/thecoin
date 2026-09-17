//! REST/JSON API (`/api/v1/...`) used by wallets, the explorer and the website.
//!
//! Read endpoints are safe to expose publicly (behind a reverse proxy with TLS).
//! The only write endpoint, `POST /api/v1/tx`, accepts already-signed
//! transactions — the node never holds user private keys.
//!
//! Protection: request timeout, body size limit, global concurrency limit, CORS.

use crate::chain::now_secs;
use crate::node::Node;
use crate::protocol::PROTOCOL_VERSION;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use thecoin_core::api::*;
use thecoin_core::emission::{block_subsidy, era};
use thecoin_core::execution::required_fee;
use thecoin_core::governance::{Proposal, ProposalAction, ProposalStatus};
use thecoin_core::hash::Hash32;
use thecoin_core::params::MAX_SUPPLY;
use thecoin_core::pow::difficulty_from_target;
use thecoin_core::programs::program_address;
use thecoin_core::state::{self, read_typed, Account, ChainGlobal, PendingReward};
use thecoin_core::{Address, Transaction};
use thecoin_storage::DbRead;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tracing::info;

type AppState = Arc<Node>;

pub struct ApiErr(StatusCode, String);

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (self.0, Json(ApiError { error: self.1 })).into_response()
    }
}

impl From<anyhow::Error> for ApiErr {
    fn from(e: anyhow::Error) -> Self {
        ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

fn bad(msg: impl Into<String>) -> ApiErr {
    ApiErr(StatusCode::BAD_REQUEST, msg.into())
}

fn not_found(msg: impl Into<String>) -> ApiErr {
    ApiErr(StatusCode::NOT_FOUND, msg.into())
}

type ApiResult<T> = Result<Json<T>, ApiErr>;

/// Runs a blocking closure (database access) off the async executor.
async fn blocking<T: Send + 'static>(node: &AppState, f: impl FnOnce(&Node) -> Result<T, ApiErr> + Send + 'static) -> Result<T, ApiErr> {
    let n = node.clone();
    tokio::task::spawn_blocking(move || f(&n)).await.map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

pub fn router(node: AppState) -> Router {
    let cors = {
        let origins = &node.config.rpc.cors_origins;
        let base = CorsLayer::new().allow_methods([Method::GET, Method::POST, Method::OPTIONS]).allow_headers(Any);
        if origins.iter().any(|o| o == "*") {
            base.allow_origin(Any)
        } else {
            base.allow_origin(origins.iter().filter_map(|o| HeaderValue::from_str(o).ok()).collect::<Vec<_>>())
        }
    };
    Router::new()
        .route("/", get(root))
        .route("/api/v1/status", get(status))
        .route("/api/v1/supply", get(supply))
        .route("/api/v1/fees", get(fees))
        .route("/api/v1/blocks", get(blocks))
        .route("/api/v1/block/{id}", get(block))
        .route("/api/v1/tx/{txid}", get(tx))
        .route("/api/v1/tx", post(submit_tx))
        .route("/api/v1/address/{addr}", get(address))
        .route("/api/v1/address/{addr}/txs", get(address_txs))
        .route("/api/v1/contract/{id}", get(contract))
        .route("/api/v1/governance/proposals", get(proposals))
        .route("/api/v1/governance/proposal/{id}", get(proposal))
        .route("/api/v1/governance/params", get(gov_params))
        .route("/api/v1/mempool", get(mempool))
        .route("/api/v1/peers", get(peers))
        .route("/api/v1/mining", get(mining))
        .route("/api/v1/tx/simulate", post(simulate_tx))
        .route("/api/v1/program/{addr}", get(program))
        .route("/api/v1/program/{addr}/view", post(program_view))
        .route("/api/v1/security", get(security))
        .route("/api/v1/alerts", get(alerts))
        .layer(cors)
        .layer(RequestBodyLimitLayer::new(256 * 1024))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(20)))
        .layer(ConcurrencyLimitLayer::new(node.config.rpc.max_concurrency.max(1)))
        .with_state(node)
}

pub async fn start(node: AppState) -> anyhow::Result<()> {
    let listen: std::net::SocketAddr =
        node.config.rpc.listen.parse().map_err(|_| anyhow::anyhow!("invalid rpc.listen '{}'", node.config.rpc.listen))?;
    let listener = tokio::net::TcpListener::bind(listen).await.map_err(|e| anyhow::anyhow!("cannot bind API port {listen}: {e}"))?;
    let _ = node.rpc_addr.set(listener.local_addr()?);
    let app = router(node.clone());
    let mut shutdown = node.shutdown.subscribe();
    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        });
        if let Err(e) = server.await {
            tracing::error!(error = %e, "API server stopped");
        }
    });
    info!(%listen, "REST API listening");
    Ok(())
}

async fn root(State(node): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "The Coin node",
        "version": thecoin_core::VERSION,
        "protocol": PROTOCOL_VERSION,
        "network": node.params.network,
        "docs": "https://the-coin.cloud/docs/api",
    }))
}

fn global(r: &impl DbRead) -> Result<ChainGlobal, ApiErr> {
    read_typed::<ChainGlobal, _>(&ReaderAdapter(r), &state::global_key())
        .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.0))?
        .ok_or_else(|| not_found("global"))
}

/// Adapts any [`DbRead`] to a core `StateReader`.
struct ReaderAdapter<'a, R: DbRead>(&'a R);
impl<R: DbRead> thecoin_core::state::StateReader for ReaderAdapter<'_, R> {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, thecoin_core::state::StateError> {
        self.0.state_get(key).map_err(|e| thecoin_core::state::StateError(e.to_string()))
    }
}

fn supply_view(node: &Node, g: &ChainGlobal, height: u64) -> SupplyView {
    let p = node.params;
    let next = height + 1;
    let e = era(p, next);
    SupplyView {
        max_supply: MAX_SUPPLY,
        emitted: g.emitted,
        burned: g.burned,
        circulating: g.circulating(),
        current_block_reward: block_subsidy(p, next),
        era: e,
        next_halving_height: (e + 1) * p.halving_interval + 1,
        halving_interval: p.halving_interval,
        target_block_time: p.target_block_time,
    }
}

fn required_upgrade(r: &impl DbRead) -> Option<String> {
    let rows = r.state_prefix(&[state::PREFIX_PROPOSAL], 10_000).ok()?;
    let mut best: Option<String> = None;
    for (_, v) in rows {
        let Ok(p) = borsh::from_slice::<Proposal>(&v) else { continue };
        if let (ProposalStatus::Activated { .. }, ProposalAction::SoftwareUpgrade { version, .. }) = (&p.status, &p.spec.action) {
            if version_newer(version, thecoin_core::VERSION) {
                best = Some(version.clone());
            }
        }
    }
    best
}

fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parse(a) > parse(b)
}

async fn status(State(node): State<AppState>) -> ApiResult<StatusView> {
    let n = node.clone();
    let view = blocking(&node, move |node| {
        let tip = node.chain.tip();
        let r = node.chain.read()?;
        let g = global(&r)?;
        let next_target = node.chain.next_target()?;
        let (mempool_txs, mempool_bytes) = {
            let m = node.mempool.lock();
            (m.len(), m.bytes())
        };
        Ok(StatusView {
            network: node.params.network,
            version: thecoin_core::VERSION.to_string(),
            genesis: node.chain.genesis_hash(),
            height: tip.height,
            tip: tip.hash,
            tip_timestamp: tip.header.timestamp,
            chainwork: format!("{:x}", tip.chainwork),
            difficulty: difficulty_from_target(&tip.header.target_u256()),
            next_target: format!("{:064x}", next_target),
            hashrate_estimate: node.chain.network_hashrate(120).unwrap_or(0.0),
            peers: node.peers.read().len(),
            mempool_txs,
            mempool_bytes,
            syncing: node.is_syncing(),
            supply: supply_view(node, &g, tip.height),
            params: g.params,
            congestion_bp: g.congestion_bp,
            software_upgrade_required: required_upgrade(&r),
        })
    })
    .await?;
    let _ = n;
    Ok(Json(view))
}

async fn supply(State(node): State<AppState>) -> ApiResult<SupplyView> {
    Ok(Json(
        blocking(&node, |node| {
            let r = node.chain.read()?;
            let g = global(&r)?;
            Ok(supply_view(node, &g, node.chain.tip().height))
        })
        .await?,
    ))
}

/// Size of a typical transfer used to express fee levels.
const TYPICAL_TRANSFER_BYTES: usize = 160;

async fn fees(State(node): State<AppState>) -> ApiResult<FeeView> {
    let g = node.chain.global()?;
    let (typical, _) = required_fee(&g.params, g.congestion_bp, TYPICAL_TRANSFER_BYTES, 0);
    let m = node.mempool.lock();
    let min_rate = typical / TYPICAL_TRANSFER_BYTES as u64;
    let priority = m.priority_levels(g.params.max_block_bytes, min_rate);
    Ok(Json(FeeView {
        base_fee: g.params.base_fee,
        fee_per_kb: g.params.fee_per_kb,
        fee_per_kfuel: g.params.fee_per_kfuel,
        storage_deposit_per_kb: g.params.storage_deposit_per_kb,
        congestion_bp: g.congestion_bp,
        typical_transfer_fee: typical,
        priority,
        mempool_txs: m.len(),
        mempool_bytes: m.bytes(),
    }))
}

/// Adds receipt data (execution result, logs) and double-spend info to a view.
fn decorate(node: &Node, r: &impl DbRead, v: &mut TxView) -> Result<(), ApiErr> {
    if v.block_height.is_some() {
        if let Some(receipt) = r.receipt(&v.txid)? {
            apply_receipt(v, &receipt, node.params.network);
        }
    }
    v.conflict = node.mempool.lock().conflict_for(&v.txid);
    Ok(())
}

#[derive(Deserialize)]
struct PageQuery {
    limit: Option<usize>,
    /// For blocks: list blocks with height < before.
    before: Option<u64>,
    /// For address history: cursor "height:position".
    cursor: Option<String>,
}

async fn blocks(State(node): State<AppState>, Query(q): Query<PageQuery>) -> ApiResult<Vec<BlockSummaryView>> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    Ok(Json(
        blocking(&node, move |node| {
            let tip = node.chain.tip().height;
            let start = q.before.map(|b| b.saturating_sub(1)).unwrap_or(tip).min(tip);
            let r = node.chain.read()?;
            let mut out = Vec::new();
            let mut h = start as i64;
            while h >= 0 && out.len() < limit {
                let Some(hash) = r.main_hash(h as u64)? else { break };
                let rec = r.header(&hash)?.ok_or_else(|| not_found("header"))?;
                out.push(BlockSummaryView {
                    height: rec.header.height,
                    hash,
                    prev_hash: rec.header.prev_hash,
                    timestamp: rec.header.timestamp,
                    tx_count: rec.tx_count as usize,
                    size: rec.size as usize,
                    miner: rec.header.miner.encode(node.params.network),
                    difficulty: difficulty_from_target(&rec.header.target_u256()),
                    signal: rec.header.signal,
                });
                h -= 1;
            }
            Ok(out)
        })
        .await?,
    ))
}

async fn block(State(node): State<AppState>, Path(id): Path<String>) -> ApiResult<BlockView> {
    Ok(Json(
        blocking(&node, move |node| {
            let r = node.chain.read()?;
            let hash = if let Ok(height) = id.parse::<u64>() {
                r.main_hash(height)?.ok_or_else(|| not_found("no block at that height"))?
            } else {
                Hash32::from_hex(&id).map_err(|_| bad("id must be a height or a block hash"))?
            };
            let rec = r.header(&hash)?.ok_or_else(|| not_found("block not found"))?;
            let body = r.block_body(&hash)?.ok_or_else(|| not_found("block body pruned on this node"))?;
            let b = thecoin_core::Block { header: rec.header.clone(), txs: body.txs, uncles: body.uncles };
            let n = node.params.network;
            let tip = node.chain.tip().height;
            let on_main = r.main_hash(rec.header.height)? == Some(hash);
            let confirmations = if on_main { tip - rec.header.height + 1 } else { 0 };
            let fees: u64 = b.txs.iter().map(|t| t.body.fee).sum();
            let mut tx_views = Vec::with_capacity(b.txs.len());
            for (pos, t) in b.txs.iter().enumerate() {
                let mut v = tx_view(t, n);
                v.block_height = Some(rec.header.height);
                v.block_hash = Some(hash);
                v.position = Some(pos as u32);
                v.confirmations = confirmations;
                v.created = created_id(t);
                decorate(node, &r, &mut v)?;
                tx_views.push(v);
            }
            Ok(BlockView {
                summary: block_summary_view(&b, n),
                version: rec.header.version,
                tx_root: rec.header.tx_root,
                state_root: rec.header.state_root,
                target: hex::encode(rec.header.target),
                nonce: rec.header.nonce.to_string(),
                confirmations,
                subsidy: block_subsidy(node.params, rec.header.height),
                fees,
                txs: tx_views,
            })
        })
        .await?,
    ))
}

fn created_id(t: &Transaction) -> Option<Hash32> {
    match &t.body.action {
        thecoin_core::TxAction::CreateContract { .. } => Some(thecoin_core::contracts::contract_id(&t.sender(), t.body.nonce)),
        thecoin_core::TxAction::Propose { .. } => Some(thecoin_core::governance::proposal_id(&t.sender(), t.body.nonce)),
        _ => None,
    }
}

fn created_program(t: &Transaction) -> Option<Address> {
    match &t.body.action {
        thecoin_core::TxAction::Deploy { .. } => Some(program_address(&t.sender(), t.body.nonce)),
        _ => None,
    }
}

/// View of a transaction waiting in the mempool.
fn pending_view(pool: &crate::mempool::Mempool, tx: &Transaction, n: thecoin_core::Network) -> TxView {
    let mut v = tx_view(tx, n);
    v.in_mempool = true;
    v.created = created_id(tx);
    v.program = created_program(tx).map(|a| a.encode(n));
    v.conflict = pool.conflict_for(&v.txid);
    v
}

fn find_tx(node: &Node, txid: &Hash32) -> Result<Option<TxView>, ApiErr> {
    let n = node.params.network;
    {
        let pool = node.mempool.lock();
        if let Some(tx) = pool.get(txid) {
            return Ok(Some(pending_view(&pool, tx, n)));
        }
    }
    let r = node.chain.read()?;
    let Some((height, pos)) = r.tx_location(txid)? else { return Ok(None) };
    let Some(hash) = r.main_hash(height)? else { return Ok(None) };
    let Some(txs) = r.block_txs(&hash)? else { return Err(not_found("block body pruned on this node")) };
    let Some(tx) = txs.get(pos as usize) else { return Ok(None) };
    let mut v = tx_view(tx, n);
    v.block_height = Some(height);
    v.block_hash = Some(hash);
    v.position = Some(pos);
    v.confirmations = node.chain.tip().height - height + 1;
    v.created = created_id(tx);
    decorate(node, &r, &mut v)?;
    Ok(Some(v))
}

async fn tx(State(node): State<AppState>, Path(txid): Path<String>) -> ApiResult<TxView> {
    let txid = Hash32::from_hex(&txid).map_err(|_| bad("invalid txid"))?;
    Ok(Json(blocking(&node, move |node| find_tx(node, &txid)?.ok_or_else(|| not_found("transaction not found"))).await?))
}

async fn submit_tx(State(node): State<AppState>, Json(req): Json<SubmitTxRequest>) -> ApiResult<SubmitTxResponse> {
    let bytes = hex::decode(req.tx.trim()).map_err(|_| bad("tx must be hex"))?;
    let tx = Transaction::from_bytes(&bytes).map_err(|e| bad(format!("malformed transaction: {e}")))?;
    let txid = blocking(&node, move |node| node.submit_tx(tx, None).map_err(|e| bad(e.to_string()))).await?;
    Ok(Json(SubmitTxResponse { txid }))
}

async fn simulate_tx(State(node): State<AppState>, Json(req): Json<SubmitTxRequest>) -> ApiResult<SimulateResponse> {
    let bytes = hex::decode(req.tx.trim()).map_err(|_| bad("tx must be hex"))?;
    let tx = Transaction::from_bytes(&bytes).map_err(|e| bad(format!("malformed transaction: {e}")))?;
    Ok(Json(
        blocking(&node, move |node| {
            let n = node.params.network;
            let g = node.chain.global()?;
            let (required, _) = required_fee(&g.params, g.congestion_bp, tx.size(), tx.max_fuel());
            // Signature and structure first.
            if let Err(e) = thecoin_core::execution::check_tx_stateless(node.params, &tx) {
                return Ok(SimulateResponse {
                    valid: false,
                    invalid_reason: Some(e.to_string()),
                    success: false,
                    error: None,
                    fuel_used: 0,
                    required_fee: required,
                    logs: vec![],
                    return_value: None,
                    program: None,
                });
            }
            let result = node.mempool.lock().simulate_one(&node.chain, &tx)?;
            Ok(match result {
                Ok(r) => SimulateResponse {
                    valid: true,
                    invalid_reason: None,
                    success: r.success,
                    error: r.error.clone(),
                    fuel_used: r.fuel_used,
                    required_fee: required,
                    logs: r.logs.iter().map(|l| log_view(l, n)).collect(),
                    return_value: r.return_value.as_ref().map(|v| thecoin_core::tccl::abi::display(v, n.hrp())),
                    program: r.program.map(|a| a.encode(n)),
                },
                Err(e) => SimulateResponse {
                    valid: false,
                    invalid_reason: Some(e.to_string()),
                    success: false,
                    error: None,
                    fuel_used: 0,
                    required_fee: required,
                    logs: vec![],
                    return_value: None,
                    program: None,
                },
            })
        })
        .await?,
    ))
}

async fn program(State(node): State<AppState>, Path(addr): Path<String>) -> ApiResult<ProgramView> {
    let a = parse_addr(&node, &addr)?;
    Ok(Json(
        blocking(&node, move |node| {
            let r = node.chain.read()?;
            let reader = ReaderAdapter(&r);
            let (meta, prog) = thecoin_core::programs::describe(&reader, &a)
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| not_found("no contract at this address (it may have been destroyed)"))?;
            let acc: Account = read_typed(&reader, &state::account_key(&a))
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.0))?
                .unwrap_or_default();
            let n = node.params.network;
            Ok(ProgramView {
                address: a.encode(n),
                name: meta.name,
                creator: meta.creator.encode(n),
                created_height: meta.created_height,
                deploy_txid: meta.deploy_txid,
                source_hash: meta.source_hash,
                balance: acc.balance,
                state_bytes: meta.state_bytes,
                storage_items: meta.storage_items,
                deposit: meta.deposit,
                functions: prog
                    .abi()
                    .into_iter()
                    .map(|f| ProgramFunctionView {
                        name: f.name,
                        kind: format!("{:?}", f.kind).to_lowercase(),
                        payable: f.payable,
                        params: f.params,
                        returns: f.returns,
                    })
                    .collect(),
            })
        })
        .await?,
    ))
}

/// Fuel limit for read-only contract queries through the API.
const VIEW_FUEL: u64 = 2_000_000;

async fn program_view(
    State(node): State<AppState>,
    Path(addr): Path<String>,
    Json(req): Json<ViewCallRequest>,
) -> ApiResult<ViewCallResponse> {
    let a = parse_addr(&node, &addr)?;
    Ok(Json(
        blocking(&node, move |node| {
            let r = node.chain.read()?;
            let reader = ReaderAdapter(&r);
            let (_, prog) = thecoin_core::programs::describe(&reader, &a)
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| not_found("no contract at this address"))?;
            let (_, f) = prog.find(&req.function).ok_or_else(|| bad(format!("unknown function '{}'", req.function)))?;
            if req.args.len() != f.params.len() {
                return Err(bad(format!("{} expects {} argument(s)", f.name, f.params.len())));
            }
            let mut args = Vec::new();
            for (raw, (name, t)) in req.args.iter().zip(&f.params) {
                args.push(thecoin_core::tccl::abi::parse_arg(raw, t).map_err(|e| bad(format!("argument '{name}': {e}")))?);
            }
            let height = node.chain.tip().height + 1;
            let (result, fuel_used) = thecoin_core::programs::view(&reader, height, &a, &req.function, args, VIEW_FUEL)
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let hrp = node.params.network.hrp();
            Ok(match result {
                Ok(v) => ViewCallResponse { result: Some(thecoin_core::tccl::abi::display(&v, hrp)), error: None, fuel_used },
                Err(e) => ViewCallResponse { result: None, error: Some(e), fuel_used },
            })
        })
        .await?,
    ))
}

#[derive(Deserialize)]
struct SecurityQuery {
    /// Amount in motes (validated in the handler so errors are JSON).
    amount: Option<String>,
}

/// Recommended confirmations: an attacker rewriting N blocks gives up about N
/// block rewards (and must out-mine the network for that long). We recommend N
/// such that the rewards at stake are at least twice the payment, never less
/// than 1 and never more than the maximum reorganization depth.
async fn security(State(node): State<AppState>, Query(q): Query<SecurityQuery>) -> ApiResult<SecurityView> {
    let amount: u64 = q
        .amount
        .as_deref()
        .ok_or_else(|| bad("query parameter 'amount' (motes) is required"))?
        .trim()
        .parse()
        .map_err(|_| bad("amount must be a whole number of motes"))?;
    let tip = node.chain.tip();
    let p = node.params;
    let per_block = block_subsidy(p, tip.height + 1).max(1);
    let wanted = (amount as u128 * 2).div_ceil(per_block as u128) as u64;
    let confirmations = wanted.clamp(1, p.max_reorg_depth);
    let hashrate = node.chain.network_hashrate(120).unwrap_or(0.0);
    let explanation = format!(
        "Reversing {confirmations} block(s) means redoing their proof of work and giving up about {} TCN of rewards. \
         For larger amounts wait for more confirmations; beyond {} blocks the chain never reorganizes.",
        thecoin_core::amount::format_amount(per_block.saturating_mul(confirmations)),
        p.max_reorg_depth
    );
    Ok(Json(SecurityView {
        amount,
        confirmations,
        minutes: confirmations * p.target_block_time / 60,
        value_per_block: per_block,
        network_hashrate: hashrate,
        explanation,
    }))
}

async fn alerts(State(node): State<AppState>) -> ApiResult<Vec<DoubleSpendView>> {
    Ok(Json(node.mempool.lock().conflicts(node.params.network)))
}

fn parse_addr(node: &Node, s: &str) -> Result<Address, ApiErr> {
    Address::decode(s, node.params.network).map_err(|e| bad(e.to_string()))
}

async fn address(State(node): State<AppState>, Path(addr): Path<String>) -> ApiResult<AccountView> {
    let a = parse_addr(&node, &addr)?;
    Ok(Json(
        blocking(&node, move |node| {
            let r = node.chain.read()?;
            let acc: Account = read_typed(&ReaderAdapter(&r), &state::account_key(&a))
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.0))?
                .unwrap_or_default();
            let tip = node.chain.tip().height;
            let mut v = account_view(&a, &acc, tip, node.params.network);
            // Rewards still in cooldown.
            let rows = r.state_prefix(&[state::PREFIX_PENDING_REWARD], 100_000)?;
            v.immature = rows
                .iter()
                .filter_map(|(_, val)| borsh::from_slice::<PendingReward>(val).ok())
                .flat_map(|p| p.payouts)
                .filter(|p| p.who == a)
                .map(|p| p.amount - p.released)
                .sum();
            let m = node.mempool.lock();
            v.next_nonce = m.next_nonce(&a, acc.nonce);
            v.mempool_txs = m.sender_txs(&a).len();
            Ok(v)
        })
        .await?,
    ))
}

async fn address_txs(State(node): State<AppState>, Path(addr): Path<String>, Query(q): Query<PageQuery>) -> ApiResult<Vec<TxView>> {
    let a = parse_addr(&node, &addr)?;
    if !node.chain.options().address_index {
        return Err(bad("address index disabled on this node (pruned nodes do not keep it)"));
    }
    let limit = q.limit.unwrap_or(25).clamp(1, 100);
    let cursor = match q.cursor {
        Some(c) => {
            let (h, p) = c.split_once(':').ok_or_else(|| bad("cursor must be height:position"))?;
            Some((h.parse::<u64>().map_err(|_| bad("bad cursor"))?, p.parse::<u32>().map_err(|_| bad("bad cursor"))?))
        }
        None => None,
    };
    Ok(Json(
        blocking(&node, move |node| {
            let n = node.params.network;
            let mut out: Vec<TxView> = Vec::new();
            if cursor.is_none() {
                let pool = node.mempool.lock();
                for tx in pool.sender_txs(&a) {
                    out.push(pending_view(&pool, &tx, n));
                }
            }
            let r = node.chain.read()?;
            let tip = node.chain.tip().height;
            let rows = r.address_txs(&a, cursor, limit)?;
            let mut cache: Option<(u64, Vec<Transaction>)> = None;
            for (height, pos) in rows {
                if cache.as_ref().map(|c| c.0) != Some(height) {
                    let Some(hash) = r.main_hash(height)? else { continue };
                    let Some(txs) = r.block_txs(&hash)? else { continue };
                    cache = Some((height, txs));
                }
                let (_, txs) = cache.as_ref().expect("cached");
                if let Some(tx) = txs.get(pos as usize) {
                    let mut v = tx_view(tx, n);
                    v.block_height = Some(height);
                    v.block_hash = r.main_hash(height)?;
                    v.position = Some(pos);
                    v.confirmations = tip - height + 1;
                    v.created = created_id(tx);
                    decorate(node, &r, &mut v)?;
                    out.push(v);
                }
            }
            Ok(out)
        })
        .await?,
    ))
}

async fn contract(State(node): State<AppState>, Path(id): Path<String>) -> ApiResult<ContractView> {
    let id = Hash32::from_hex(&id).map_err(|_| bad("invalid contract id"))?;
    Ok(Json(
        blocking(&node, move |node| {
            let r = node.chain.read()?;
            let c: thecoin_core::contracts::Contract = read_typed(&ReaderAdapter(&r), &state::contract_key(&id))
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.0))?
                .ok_or_else(|| not_found("contract not found (it may have been completed)"))?;
            Ok(contract_view(&c, node.chain.tip().height, node.params.network))
        })
        .await?,
    ))
}

async fn proposals(State(node): State<AppState>) -> ApiResult<Vec<ProposalView>> {
    Ok(Json(
        blocking(&node, |node| {
            let r = node.chain.read()?;
            let g = global(&r)?;
            let tip = node.chain.tip().height;
            let rows = r.state_prefix(&[state::PREFIX_PROPOSAL], 100_000)?;
            let mut out: Vec<ProposalView> = rows
                .iter()
                .filter_map(|(_, v)| borsh::from_slice::<Proposal>(v).ok())
                .map(|p| proposal_view(&p, tip, &g.params, g.circulating(), node.params.network))
                .collect();
            out.sort_by_key(|p| std::cmp::Reverse(p.created_height));
            Ok(out)
        })
        .await?,
    ))
}

async fn proposal(State(node): State<AppState>, Path(id): Path<String>) -> ApiResult<ProposalView> {
    let id = Hash32::from_hex(&id).map_err(|_| bad("invalid proposal id"))?;
    Ok(Json(
        blocking(&node, move |node| {
            let r = node.chain.read()?;
            let g = global(&r)?;
            let p: Proposal = read_typed(&ReaderAdapter(&r), &state::proposal_key(&id))
                .map_err(|e| ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.0))?
                .ok_or_else(|| not_found("proposal not found"))?;
            Ok(proposal_view(&p, node.chain.tip().height, &g.params, g.circulating(), node.params.network))
        })
        .await?,
    ))
}

async fn gov_params(State(node): State<AppState>) -> ApiResult<serde_json::Value> {
    let g = node.chain.global()?;
    Ok(Json(serde_json::json!({
        "current": g.params,
        "bounds": node.params.gov_bounds,
        "voting_proposals": g.voting.len(),
        "pending_activations": g.pending_activations.len(),
    })))
}

async fn mempool(State(node): State<AppState>) -> ApiResult<serde_json::Value> {
    let m = node.mempool.lock();
    let n = node.params.network;
    let txs: Vec<TxView> = m.txids(100).iter().filter_map(|id| m.get(id)).map(|t| pending_view(&m, t, n)).collect();
    Ok(Json(serde_json::json!({ "count": m.len(), "bytes": m.bytes(), "txs": txs })))
}

async fn peers(State(node): State<AppState>) -> ApiResult<Vec<PeerView>> {
    let peers = node.peers.read();
    Ok(Json(
        peers
            .values()
            .map(|p| PeerView {
                addr: p.addr.to_string(),
                inbound: p.inbound,
                height: p.best_height(),
                user_agent: p.user_agent(),
                connected_secs: p.connected_at.elapsed().as_secs(),
            })
            .collect(),
    ))
}

async fn mining(State(node): State<AppState>) -> ApiResult<MiningView> {
    let m = &node.miner;
    let _ = now_secs();
    Ok(Json(MiningView {
        enabled: m.address.is_some(),
        threads: m.threads,
        address: m.address.map(|a| a.encode(node.params.network)),
        hashrate: m.hashrate(),
        blocks_found: m.blocks_found.load(Ordering::Relaxed),
        signal_proposals: m.signal.clone(),
    }))
}
