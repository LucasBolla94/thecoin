//! Execution of TCCL smart contracts inside the state-transition function.
//!
//! * A contract lives at `program_address(creator, nonce)`. Its TCN balance is
//!   the ordinary [`Account`] at that address; its code, metadata and storage
//!   are separate state records.
//! * Every execution runs in a child overlay. If the contract fails (require,
//!   out of fuel, overflow, compile error...) the overlay is discarded: the
//!   call value, storage writes, sends and events are all reverted. The fee was
//!   already charged by [`crate::execution::apply_tx`].
//! * **Storage deposit:** contract state (code + storage) is backed by a
//!   refundable deposit of `storage_deposit_per_kb` per kB. Callers that make a
//!   contract grow pay the difference (up to their `max_deposit`); callers that
//!   free storage get the proportional part back. `destroy(to)` returns the
//!   remaining balance and the whole deposit.

use crate::address::Address;
use crate::contracts::{credit, storage_deposit};
use crate::error::TxError;
use crate::execution::{LogEntry, TxReceipt};
use crate::hash::{tagged_hash, tags, Hash32};
use crate::params::{ChainParams, MAX_EVENTS_PER_CALL, MAX_PROGRAM_BYTES};
use crate::state::{program_code_key, program_meta_key, program_storage_key, Account, Overlay, ProgramMeta, StateError, StateReader};
use tccl::vm::{self, fuel, CallContext, Host, Mode};
use tccl::{Program, Value, VmError};

/// Address of the contract deployed by `creator` with transaction `nonce`.
pub fn program_address(creator: &Address, nonce: u64) -> Address {
    let h = tagged_hash(tags::PROGRAM_ADDRESS, &[&creator.0, &nonce.to_le_bytes()]);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h.0[..20]);
    Address(a)
}

pub fn source_hash(source: &str) -> Hash32 {
    tagged_hash("tccl-source", &[source.as_bytes()])
}

/// Parameters shared by deploy and invoke.
#[derive(Clone, Debug)]
pub struct ProgramCall {
    pub sender: Address,
    pub height: u64,
    pub value: u64,
    pub max_fuel: u64,
    pub max_deposit: u64,
    pub deposit_per_kb: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramOutcome {
    pub success: bool,
    pub error: Option<String>,
    pub fuel_used: u64,
    pub logs: Vec<LogEntry>,
    pub return_value: Option<Value>,
    pub program: Option<Address>,
    pub touched: Vec<Address>,
}

impl ProgramOutcome {
    fn failed(error: impl Into<String>, fuel_used: u64) -> Self {
        ProgramOutcome { success: false, error: Some(error.into()), fuel_used, ..Default::default() }
    }

    /// Copies the outcome into a transaction receipt.
    pub fn fill(self, r: &mut TxReceipt) {
        r.success = self.success;
        r.error = self.error;
        r.fuel_used = self.fuel_used;
        r.logs = self.logs;
        r.return_value = self.return_value;
        if self.success {
            r.program = self.program.or(r.program);
        }
        r.touched.extend(self.touched);
    }
}

struct ChainHost<'s, 'b, R: StateReader + ?Sized> {
    state: &'s mut Overlay<'b, R>,
    addr: Address,
    meta: ProgramMeta,
    logs: Vec<LogEntry>,
    touched: Vec<Address>,
    destroy_to: Option<Address>,
    /// A storage backend failure (not a contract failure): aborts the block.
    fatal: Option<StateError>,
}

impl<R: StateReader + ?Sized> ChainHost<'_, '_, R> {
    fn fatal(&mut self, e: StateError) -> VmError {
        let msg = e.0.clone();
        self.fatal = Some(e);
        VmError::Host(msg)
    }
}

impl<R: StateReader + ?Sized> Host for ChainHost<'_, '_, R> {
    fn storage_read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, VmError> {
        let k = program_storage_key(&self.addr, key);
        self.state.get_raw(&k).map_err(|e| self.fatal(e))
    }

    fn storage_write(&mut self, key: &[u8], value: Option<Vec<u8>>) -> Result<(), VmError> {
        let k = program_storage_key(&self.addr, key);
        let old = self.state.get_raw(&k).map_err(|e| self.fatal(e))?;
        if let Some(o) = &old {
            self.meta.state_bytes = self.meta.state_bytes.saturating_sub((key.len() + o.len()) as u64);
            self.meta.storage_items = self.meta.storage_items.saturating_sub(1);
        }
        match value {
            Some(v) => {
                self.meta.state_bytes += (key.len() + v.len()) as u64;
                self.meta.storage_items += 1;
                self.state.put_raw(k, v);
            }
            None => self.state.delete_raw(k),
        }
        Ok(())
    }

    fn balance(&mut self) -> Result<u64, VmError> {
        let addr = self.addr;
        self.state.account(&addr).map(|a| a.balance).map_err(|e| self.fatal(e))
    }

    fn send(&mut self, to: &[u8; 20], amount: u64) -> Result<(), VmError> {
        let addr = self.addr;
        let mut acc = self.state.account(&addr).map_err(|e| self.fatal(e))?;
        if acc.balance < amount {
            return Err(VmError::InsufficientBalance);
        }
        acc.balance -= amount;
        self.state.put_account(&addr, &acc);
        let to = Address(*to);
        match credit(self.state, &to, amount) {
            Ok(()) => {}
            Err(TxError::State(e)) => return Err(self.fatal(e)),
            Err(_) => return Err(VmError::Overflow),
        }
        self.touched.push(to);
        Ok(())
    }

    fn emit(&mut self, event: &str, fields: Vec<(String, Value)>) -> Result<(), VmError> {
        if self.logs.len() >= MAX_EVENTS_PER_CALL {
            return Err(VmError::TooLarge);
        }
        self.logs.push(LogEntry { contract: self.addr, event: event.to_string(), fields });
        Ok(())
    }

    fn storage_items(&mut self) -> Result<u64, VmError> {
        Ok(self.meta.storage_items)
    }

    fn destroy(&mut self, to: &[u8; 20]) -> Result<(), VmError> {
        self.destroy_to = Some(Address(*to));
        Ok(())
    }
}

enum Debit {
    Done,
    Insufficient { needed: u64, available: u64 },
}

fn debit<R: StateReader + ?Sized>(state: &mut Overlay<'_, R>, who: &Address, amount: u64, height: u64) -> Result<Debit, StateError> {
    if amount == 0 {
        return Ok(Debit::Done);
    }
    let mut acc = state.account(who)?;
    let available = acc.spendable(height);
    if amount > available {
        return Ok(Debit::Insufficient { needed: amount, available });
    }
    acc.balance -= amount;
    state.put_account(who, &acc);
    Ok(Debit::Done)
}

/// Runs a program in `child`. `Ok(Ok(outcome))` means success and the child
/// changes must be kept; `Ok(Err(outcome))` is a contract failure (discard the
/// child); `Err` is a storage failure.
#[allow(clippy::too_many_arguments)]
fn run<R: StateReader + ?Sized>(
    child: &mut Overlay<'_, R>,
    program: &Program,
    mode: Mode,
    function: &str,
    args: Vec<Value>,
    call: &ProgramCall,
    addr: Address,
    meta: ProgramMeta,
    fuel_limit: u64,
) -> Result<Result<ProgramOutcome, ProgramOutcome>, StateError> {
    let is_deploy = mode == Mode::Deploy;
    let old_bytes = if is_deploy { 0 } else { meta.state_bytes };

    if call.value > 0 {
        if let Debit::Insufficient { needed, available } = debit(child, &call.sender, call.value, call.height)? {
            return Ok(Err(ProgramOutcome::failed(format!("insufficient funds for value: need {needed}, spendable {available}"), 0)));
        }
        if let Err(e) = credit(child, &addr, call.value) {
            return match e {
                TxError::State(s) => Err(s),
                other => Ok(Err(ProgramOutcome::failed(other.to_string(), 0))),
            };
        }
    }

    let ctx = CallContext { caller: call.sender.0, value: call.value, height: call.height, self_address: addr.0 };
    let mut host = ChainHost { state: &mut *child, addr, meta, logs: Vec::new(), touched: Vec::new(), destroy_to: None, fatal: None };
    let out = vm::execute(program, mode, function, args, &ctx, &mut host, fuel_limit);
    let ChainHost { mut meta, logs, mut touched, destroy_to, fatal, .. } = host;
    if let Some(e) = fatal {
        return Err(e);
    }
    let ret = match out.result {
        Ok(v) => v,
        Err(e) => return Ok(Err(ProgramOutcome::failed(e.to_string(), out.fuel_used))),
    };
    touched.push(addr);

    if let Some(to) = destroy_to {
        let acc = child.account(&addr)?;
        let total = acc.balance.saturating_add(meta.deposit);
        child.put_account(&addr, &Account { balance: 0, ..acc });
        child.delete_raw(program_meta_key(&addr));
        child.delete_raw(program_code_key(&addr));
        if let Err(e) = credit(child, &to, total) {
            return match e {
                TxError::State(s) => Err(s),
                other => Ok(Err(ProgramOutcome::failed(other.to_string(), out.fuel_used))),
            };
        }
        touched.push(to);
    } else {
        let new_bytes = meta.state_bytes;
        if is_deploy || new_bytes > old_bytes {
            let required = storage_deposit(new_bytes, call.deposit_per_kb);
            let extra = required.saturating_sub(meta.deposit);
            if extra > call.max_deposit {
                return Ok(Err(ProgramOutcome::failed(
                    format!("storage deposit of {extra} motes exceeds max_deposit {} (contract state: {new_bytes} bytes)", call.max_deposit),
                    out.fuel_used,
                )));
            }
            if let Debit::Insufficient { needed, available } = debit(child, &call.sender, extra, call.height)? {
                return Ok(Err(ProgramOutcome::failed(format!("insufficient funds for storage deposit: need {needed}, spendable {available}"), out.fuel_used)));
            }
            meta.deposit += extra;
        } else if new_bytes < old_bytes && old_bytes > 0 && meta.deposit > 0 {
            let refund = ((meta.deposit as u128 * (old_bytes - new_bytes) as u128) / old_bytes as u128) as u64;
            if refund > 0 {
                meta.deposit -= refund;
                if let Err(e) = credit(child, &call.sender, refund) {
                    return match e {
                        TxError::State(s) => Err(s),
                        other => Ok(Err(ProgramOutcome::failed(other.to_string(), out.fuel_used))),
                    };
                }
            }
        }
        child.put(program_meta_key(&addr), &meta);
    }

    Ok(Ok(ProgramOutcome {
        success: true,
        error: None,
        fuel_used: out.fuel_used,
        logs,
        return_value: if ret == Value::Unit { None } else { Some(ret) },
        program: None,
        touched,
    }))
}

/// Deploys a contract. Returns a receipt outcome; `Err` only for storage failures.
pub fn deploy<R: StateReader + ?Sized>(
    p: &ChainParams,
    state: &mut Overlay<'_, R>,
    call: &ProgramCall,
    nonce: u64,
    txid: Hash32,
    source: &str,
    args: Vec<Value>,
) -> Result<ProgramOutcome, TxError> {
    let compile_fuel = source.len() as u64 * fuel::COMPILE_PER_BYTE;
    if compile_fuel > call.max_fuel {
        return Ok(ProgramOutcome::failed("out of fuel while compiling", call.max_fuel));
    }
    let opts = tccl::CompileOptions { address_prefixes: vec![p.hrp().to_string()] };
    let program = match tccl::compile(source, &opts) {
        Ok(prog) => prog,
        Err(e) => return Ok(ProgramOutcome::failed(format!("compile error: {e}"), compile_fuel)),
    };
    let code = program.to_bytes();
    if code.len() > MAX_PROGRAM_BYTES {
        return Ok(ProgramOutcome::failed(format!("compiled program too large ({} bytes, max {MAX_PROGRAM_BYTES})", code.len()), compile_fuel));
    }
    let addr = program_address(&call.sender, nonce);
    if state.program_meta(&addr)?.is_some() || state.account(&addr)?.nonce > 0 {
        return Ok(ProgramOutcome::failed("contract address collision", compile_fuel));
    }
    let meta = ProgramMeta {
        creator: call.sender,
        created_height: call.height,
        deploy_txid: txid,
        source_hash: source_hash(source),
        name: program.name.clone(),
        state_bytes: code.len() as u64,
        storage_items: 0,
        deposit: 0,
    };
    let result = {
        let mut child = Overlay::new(&*state);
        child.put_raw(program_code_key(&addr), code);
        let r = run(&mut child, &program, Mode::Deploy, "init", args, call, addr, meta, call.max_fuel - compile_fuel)?;
        r.map(|out| (out, child.into_changes()))
    };
    Ok(match result {
        Ok((mut out, changes)) => {
            state.absorb(changes);
            out.fuel_used += compile_fuel;
            out.program = Some(addr);
            out
        }
        Err(mut failed) => {
            failed.fuel_used += compile_fuel;
            failed
        }
    })
}

/// Fuel charged to load a program from state.
fn load_fuel(code_len: usize) -> u64 {
    100 + code_len as u64 / 100
}

fn load_program<R: StateReader + ?Sized>(state: &Overlay<'_, R>, addr: &Address) -> Result<Option<(ProgramMeta, Program, usize)>, TxError> {
    let Some(meta) = state.program_meta(addr)? else { return Ok(None) };
    let code = state.get_raw(&program_code_key(addr))?.ok_or_else(|| StateError("program code missing".into()))?;
    let program = Program::from_bytes(&code).map_err(|e| StateError(format!("corrupt program: {e}")))?;
    Ok(Some((meta, program, code.len())))
}

/// Calls an action of a deployed contract.
pub fn invoke<R: StateReader + ?Sized>(state: &mut Overlay<'_, R>, call: &ProgramCall, contract: &Address, function: &str, args: Vec<Value>) -> Result<ProgramOutcome, TxError> {
    let Some((meta, program, code_len)) = load_program(state, contract)? else {
        return Ok(ProgramOutcome::failed(format!("no contract at {}", contract.to_hex()), 0));
    };
    let load = load_fuel(code_len);
    if load > call.max_fuel {
        return Ok(ProgramOutcome::failed("out of fuel while loading the contract", call.max_fuel));
    }
    let result = {
        let mut child = Overlay::new(&*state);
        let r = run(&mut child, &program, Mode::Action, function, args, call, *contract, meta, call.max_fuel - load)?;
        r.map(|out| (out, child.into_changes()))
    };
    Ok(match result {
        Ok((mut out, changes)) => {
            state.absorb(changes);
            out.fuel_used += load;
            out
        }
        Err(mut failed) => {
            failed.fuel_used += load;
            failed
        }
    })
}

/// Read-only call of a `view`. Nothing is written.
pub fn view<R: StateReader + ?Sized>(base: &R, height: u64, contract: &Address, function: &str, args: Vec<Value>, fuel_limit: u64) -> Result<(Result<Value, String>, u64), TxError> {
    let mut ov = Overlay::new(base);
    let Some((meta, program, _)) = load_program(&ov, contract)? else {
        return Ok((Err(format!("no contract at {}", contract.to_hex())), 0));
    };
    let ctx = CallContext { caller: [0u8; 20], value: 0, height, self_address: contract.0 };
    let mut host = ChainHost { state: &mut ov, addr: *contract, meta, logs: Vec::new(), touched: Vec::new(), destroy_to: None, fatal: None };
    let out = vm::execute(&program, Mode::View, function, args, &ctx, &mut host, fuel_limit);
    if let Some(e) = host.fatal {
        return Err(TxError::State(e));
    }
    Ok((out.result.map_err(|e| e.to_string()), out.fuel_used))
}

/// Metadata and interface of a deployed contract.
pub fn describe<R: StateReader + ?Sized>(base: &R, contract: &Address) -> Result<Option<(ProgramMeta, Program)>, TxError> {
    let ov = Overlay::new(base);
    Ok(load_program(&ov, contract)?.map(|(m, p, _)| (m, p)))
}
