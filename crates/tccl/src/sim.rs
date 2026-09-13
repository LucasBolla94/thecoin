//! In-memory blockchain simulator for developing and testing contracts
//! locally (used by `tccl run` and by unit tests). It follows the same rules
//! as the real chain: failed calls revert every change, TCN sent with a call is
//! credited to the contract, `send` moves TCN out of the contract balance.

use crate::error::VmError;
use crate::program::{Program, Value};
use crate::vm::{self, CallContext, Host, Mode};
use crate::{compile, CompileOptions};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_FUEL: u64 = 5_000_000;
pub const SIM_START_BALANCE: u64 = 1_000_000 * 100_000_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimContract {
    pub source: String,
    #[serde(skip)]
    pub program: Option<Program>,
    /// hex key → hex value
    pub storage: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub fields: Vec<(String, Value)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Simulator {
    pub height: u64,
    /// hex address → motes
    pub balances: BTreeMap<String, u64>,
    pub contracts: BTreeMap<String, SimContract>,
    pub nonce: u64,
}

#[derive(Debug)]
pub struct CallResult {
    pub result: Result<Value, VmError>,
    pub fuel_used: u64,
    pub events: Vec<Event>,
}

/// Deterministic address for a named test account (`alice`, `bob`, ...).
pub fn account(name: &str) -> [u8; 20] {
    let h = blake3::hash(format!("tccl-sim-account:{name}").as_bytes());
    let mut a = [0u8; 20];
    a.copy_from_slice(&h.as_bytes()[..20]);
    a
}

struct SimHost<'s> {
    sim: &'s mut Simulator,
    contract: String,
    events: Vec<Event>,
    destroyed: bool,
}

impl Host for SimHost<'_> {
    fn storage_read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, VmError> {
        let c = self.sim.contracts.get(&self.contract).ok_or_else(|| VmError::Host("contract missing".into()))?;
        Ok(c.storage.get(&hex::encode(key)).map(|v| hex::decode(v).expect("valid hex")))
    }

    fn storage_write(&mut self, key: &[u8], value: Option<Vec<u8>>) -> Result<(), VmError> {
        let c = self.sim.contracts.get_mut(&self.contract).ok_or_else(|| VmError::Host("contract missing".into()))?;
        match value {
            Some(v) => c.storage.insert(hex::encode(key), hex::encode(v)),
            None => c.storage.remove(&hex::encode(key)),
        };
        Ok(())
    }

    fn balance(&mut self) -> Result<u64, VmError> {
        Ok(*self.sim.balances.get(&self.contract).unwrap_or(&0))
    }

    fn send(&mut self, to: &[u8; 20], amount: u64) -> Result<(), VmError> {
        let bal = self.sim.balances.entry(self.contract.clone()).or_insert(0);
        if *bal < amount {
            return Err(VmError::InsufficientBalance);
        }
        *bal -= amount;
        let is_contract = self.sim.contracts.contains_key(&hex::encode(to));
        let dest = self.sim.balances.entry(hex::encode(to)).or_insert(if is_contract { 0 } else { SIM_START_BALANCE });
        *dest = dest.checked_add(amount).ok_or(VmError::Overflow)?;
        Ok(())
    }

    fn emit(&mut self, event: &str, fields: Vec<(String, Value)>) -> Result<(), VmError> {
        self.events.push(Event { name: event.to_string(), fields });
        Ok(())
    }

    fn storage_items(&mut self) -> Result<u64, VmError> {
        Ok(self.sim.contracts.get(&self.contract).map_or(0, |c| c.storage.len() as u64))
    }

    fn destroy(&mut self, to: &[u8; 20]) -> Result<(), VmError> {
        let bal = self.sim.balances.remove(&self.contract).unwrap_or(0);
        if bal > 0 {
            let is_contract = self.sim.contracts.contains_key(&hex::encode(to));
            *self.sim.balances.entry(hex::encode(to)).or_insert(if is_contract { 0 } else { SIM_START_BALANCE }) += bal;
        }
        self.destroyed = true;
        Ok(())
    }
}

impl Simulator {
    pub fn new() -> Self {
        Simulator { height: 1, ..Default::default() }
    }

    pub fn load(json: &str) -> Result<Simulator, String> {
        let mut sim: Simulator = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for (addr, c) in sim.contracts.iter_mut() {
            c.program = Some(compile(&c.source, &CompileOptions::default()).map_err(|e| format!("contract {addr}: {e}"))?);
        }
        Ok(sim)
    }

    pub fn save(&self) -> String {
        serde_json::to_string_pretty(self).expect("serializable")
    }

    /// Balance of an address. Test accounts start with 1 000 000 TCN; contracts with 0.
    pub fn balance_of(&self, addr: &[u8; 20]) -> u64 {
        let key = hex::encode(addr);
        match self.balances.get(&key) {
            Some(b) => *b,
            None if self.contracts.contains_key(&key) => 0,
            None => SIM_START_BALANCE,
        }
    }

    fn ensure_account(&mut self, addr: &[u8; 20]) {
        self.balances.entry(hex::encode(addr)).or_insert(SIM_START_BALANCE);
    }

    fn run(&mut self, addr_hex: &str, mode: Mode, function: &str, caller: [u8; 20], args: Vec<Value>, value: u64) -> CallResult {
        let snapshot = self.clone();
        let program = self.contracts[addr_hex].program.clone().expect("compiled");
        if mode != Mode::View {
            self.ensure_account(&caller);
        }
        // Transfer the call value first (reverted on failure).
        if value > 0 {
            let from = self.balances.entry(hex::encode(caller)).or_insert(0);
            if *from < value {
                return CallResult { result: Err(VmError::Host("caller has insufficient balance".into())), fuel_used: 0, events: vec![] };
            }
            *from -= value;
            *self.balances.entry(addr_hex.to_string()).or_insert(0) += value;
        }
        let mut self_address = [0u8; 20];
        self_address.copy_from_slice(&hex::decode(addr_hex).expect("hex"));
        let ctx = CallContext { caller, value, height: self.height, self_address };
        let mut host = SimHost { sim: self, contract: addr_hex.to_string(), events: Vec::new(), destroyed: false };
        let out = vm::execute(&program, mode, function, args, &ctx, &mut host, DEFAULT_FUEL);
        let events = std::mem::take(&mut host.events);
        let destroyed = host.destroyed;
        if out.result.is_err() || mode == Mode::View {
            let keep_height = self.height;
            *self = snapshot;
            self.height = keep_height;
        } else {
            if destroyed {
                self.contracts.remove(addr_hex);
            }
            self.height += 1;
        }
        CallResult { result: out.result, fuel_used: out.fuel_used, events }
    }

    /// Deploys a contract. Returns its address (hex) and the result of `init`.
    pub fn deploy(&mut self, source: &str, deployer: [u8; 20], args: Vec<Value>, value: u64) -> Result<(String, CallResult), String> {
        let program = compile(source, &CompileOptions::default()).map_err(|e| e.to_string())?;
        self.nonce += 1;
        let h = blake3::hash(&[&deployer[..], &self.nonce.to_le_bytes()].concat());
        let addr = hex::encode(&h.as_bytes()[..20]);
        self.contracts.insert(addr.clone(), SimContract { source: source.to_string(), program: Some(program), storage: BTreeMap::new() });
        let mut r = self.run(&addr, Mode::Deploy, "init", deployer, args, value);
        // On-chain deployments also pay for compiling the source.
        r.fuel_used += source.len() as u64 * vm::fuel::COMPILE_PER_BYTE;
        if r.result.is_err() {
            self.contracts.remove(&addr);
        }
        Ok((addr, r))
    }

    pub fn call(&mut self, addr_hex: &str, caller: [u8; 20], function: &str, args: Vec<Value>, value: u64) -> Result<CallResult, String> {
        if !self.contracts.contains_key(addr_hex) {
            return Err(format!("no contract at {addr_hex}"));
        }
        Ok(self.run(addr_hex, Mode::Action, function, caller, args, value))
    }

    pub fn view(&mut self, addr_hex: &str, function: &str, args: Vec<Value>) -> Result<CallResult, String> {
        if !self.contracts.contains_key(addr_hex) {
            return Err(format!("no contract at {addr_hex}"));
        }
        Ok(self.run(addr_hex, Mode::View, function, [0u8; 20], args, 0))
    }

    pub fn program(&self, addr_hex: &str) -> Option<&Program> {
        self.contracts.get(addr_hex).and_then(|c| c.program.as_ref())
    }
}
