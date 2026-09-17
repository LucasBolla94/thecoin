//! Minimal blocking client for the node REST API (`/api/v1`).

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use std::time::Duration;
use thecoin_core::api::*;
use thecoin_core::hash::Hash32;
use thecoin_core::Transaction;

pub struct NodeClient {
    base: String,
    agent: ureq::Agent,
}

impl NodeClient {
    pub fn new(base_url: &str) -> NodeClient {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .user_agent(&format!("thecoin-wallet/{}", thecoin_core::VERSION))
            .build();
        NodeClient { base: base_url.trim_end_matches('/').to_string(), agent }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        match self.agent.get(&url).call() {
            Ok(resp) => Ok(resp.into_json()?),
            Err(ureq::Error::Status(code, resp)) => Err(api_error(code, resp)),
            Err(e) => Err(anyhow!("cannot reach node at {}: {e}", self.base)),
        }
    }

    pub fn status(&self) -> Result<StatusView> {
        self.get("/api/v1/status")
    }

    pub fn fees(&self) -> Result<FeeView> {
        self.get("/api/v1/fees")
    }

    pub fn account(&self, address: &str) -> Result<AccountView> {
        self.get(&format!("/api/v1/address/{address}"))
    }

    pub fn history(&self, address: &str, limit: usize) -> Result<Vec<TxView>> {
        self.get(&format!("/api/v1/address/{address}/txs?limit={limit}"))
    }

    pub fn tx(&self, txid: &Hash32) -> Result<TxView> {
        self.get(&format!("/api/v1/tx/{txid}"))
    }

    pub fn contract(&self, id: &Hash32) -> Result<ContractView> {
        self.get(&format!("/api/v1/contract/{id}"))
    }

    pub fn proposals(&self) -> Result<Vec<ProposalView>> {
        self.get("/api/v1/governance/proposals")
    }

    pub fn proposal(&self, id: &Hash32) -> Result<ProposalView> {
        self.get(&format!("/api/v1/governance/proposal/{id}"))
    }

    pub fn program(&self, address: &str) -> Result<ProgramView> {
        self.get(&format!("/api/v1/program/{address}"))
    }

    /// Hash of the code deployed right now (upgrades are bound to it).
    pub fn program_code_hash(&self, address: &str) -> Result<thecoin_core::hash::Hash32> {
        Ok(self.program(address)?.code_hash)
    }

    pub fn security(&self, amount: u64) -> Result<SecurityView> {
        self.get(&format!("/api/v1/security?amount={amount}"))
    }

    pub fn alerts(&self) -> Result<Vec<DoubleSpendView>> {
        self.get("/api/v1/alerts")
    }

    fn post<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        match self.agent.post(&url).send_json(body) {
            Ok(resp) => Ok(resp.into_json()?),
            Err(ureq::Error::Status(code, resp)) => Err(api_error(code, resp)),
            Err(e) => Err(anyhow!("cannot reach node at {}: {e}", self.base)),
        }
    }

    /// Read-only contract call; arguments are text parsed by the node with the ABI.
    pub fn view(&self, address: &str, function: &str, args: &[String]) -> Result<ViewCallResponse> {
        self.post(&format!("/api/v1/program/{address}/view"), &ViewCallRequest { function: function.to_string(), args: args.to_vec() })
    }

    /// Dry-runs a signed transaction on top of the node's state.
    pub fn simulate(&self, tx: &Transaction) -> Result<SimulateResponse> {
        self.post("/api/v1/tx/simulate", &SubmitTxRequest { tx: hex::encode(tx.to_bytes()) })
    }

    pub fn submit(&self, tx: &Transaction) -> Result<Hash32> {
        let url = format!("{}/api/v1/tx", self.base);
        let body = SubmitTxRequest { tx: hex::encode(tx.to_bytes()) };
        match self.agent.post(&url).send_json(&body) {
            Ok(resp) => Ok(resp.into_json::<SubmitTxResponse>()?.txid),
            Err(ureq::Error::Status(code, resp)) => Err(api_error(code, resp)),
            Err(e) => Err(anyhow!("cannot reach node at {}: {e}", self.base)),
        }
    }
}

fn api_error(code: u16, resp: ureq::Response) -> anyhow::Error {
    match resp.into_json::<ApiError>() {
        Ok(e) => anyhow!("node rejected request ({code}): {}", e.error),
        Err(_) => anyhow!("node returned HTTP {code}"),
    }
}
