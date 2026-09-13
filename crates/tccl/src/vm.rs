//! Deterministic interpreter for compiled TCCL programs.
//!
//! * Every operation consumes **fuel**; running out aborts the call.
//! * All arithmetic is checked; errors abort the call and the host reverts
//!   every change made by it.
//! * Memory is bounded: values are limited to 64 KiB, in-memory lists to 4 096
//!   items, call depth to 16.
//! * No floating point, no clock, no randomness: the same call on the same
//!   state always gives the same result on every node.

use crate::ast::BinOp;
use crate::error::VmError;
use crate::ops::{self, MAX_LIST_LEN, MAX_VALUE_BYTES};
use crate::program::*;
use sha2::Digest;

pub const MAX_CALL_DEPTH: usize = 16;
pub const MAX_RING_SIZE: usize = 64;

/// Fuel schedule (consensus-critical).
/// Fuel prices. Calibrated with `tests/fuel_bench.rs` and the node's
/// `fuel_storage_bench.rs` so that every operation costs roughly 20 ns of CPU
/// per unit of fuel on a 2 vCPU VPS: a completely full block (50 M fuel) then
/// executes in about one second in the worst case.
pub mod fuel {
    pub const STMT: u64 = 2;
    pub const EXPR: u64 = 1;
    pub const CALL: u64 = 20;
    pub const PER_32_BYTES: u64 = 1;
    pub const STORAGE_READ: u64 = 250;
    pub const STORAGE_WRITE: u64 = 400;
    pub const STORAGE_WRITE_PER_BYTE: u64 = 4;
    pub const HASH: u64 = 60;
    pub const HASH_PER_64_BYTES: u64 = 20;
    pub const ED25519_VERIFY: u64 = 3_500;
    pub const RING_BASE: u64 = 5_000;
    pub const RING_PER_MEMBER: u64 = 10_000;
    pub const SEND: u64 = 300;
    pub const EMIT: u64 = 100;
    pub const DESTROY: u64 = 1_000;
    /// Charged by the host per byte of source code when deploying.
    pub const COMPILE_PER_BYTE: u64 = 5;
}

/// Execution context of a call.
#[derive(Clone, Debug)]
pub struct CallContext {
    pub caller: [u8; 20],
    /// TCN (in motes) transferred with the call.
    pub value: u64,
    pub height: u64,
    pub self_address: [u8; 20],
}

/// What the blockchain provides to a running contract.
///
/// Keys are contract-local; the host namespaces them per contract and keeps
/// every write in a revertible overlay.
pub trait Host {
    fn storage_read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, VmError>;
    fn storage_write(&mut self, key: &[u8], value: Option<Vec<u8>>) -> Result<(), VmError>;
    /// Contract balance in motes (includes the value of the current call).
    fn balance(&mut self) -> Result<u64, VmError>;
    fn send(&mut self, to: &[u8; 20], amount: u64) -> Result<(), VmError>;
    fn emit(&mut self, event: &str, fields: Vec<(String, Value)>) -> Result<(), VmError>;
    /// Number of storage entries of the contract.
    fn storage_items(&mut self) -> Result<u64, VmError>;
    /// Removes the contract, paying its remaining balance to `to`.
    fn destroy(&mut self, to: &[u8; 20]) -> Result<(), VmError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Deployment: state initial values, then `init` (if any).
    Deploy,
    /// Transaction calling an `action`.
    Action,
    /// Read-only query of a `view`.
    View,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub result: Result<Value, VmError>,
    pub fuel_used: u64,
}

enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
    Halt,
}

// Storage key layout (contract-local).
fn scalar_key(var: u16) -> Vec<u8> {
    let mut k = vec![0u8];
    k.extend_from_slice(&var.to_be_bytes());
    k
}
fn map_key(var: u16, key: &Value) -> Vec<u8> {
    let mut k = vec![1u8];
    k.extend_from_slice(&var.to_be_bytes());
    k.extend_from_slice(&key.key_bytes());
    k
}
fn list_len_key(var: u16) -> Vec<u8> {
    let mut k = vec![2u8];
    k.extend_from_slice(&var.to_be_bytes());
    k
}
fn list_item_key(var: u16, index: u64) -> Vec<u8> {
    let mut k = vec![3u8];
    k.extend_from_slice(&var.to_be_bytes());
    k.extend_from_slice(&index.to_be_bytes());
    k
}

struct Vm<'a, H: Host> {
    program: &'a Program,
    host: &'a mut H,
    ctx: &'a CallContext,
    fuel_left: u64,
    depth: usize,
    read_only: bool,
}

/// Runs `function` of `program`.
///
/// In [`Mode::Deploy`] the function name is ignored and `init` is used.
pub fn execute<H: Host>(
    program: &Program,
    mode: Mode,
    function: &str,
    args: Vec<Value>,
    ctx: &CallContext,
    host: &mut H,
    fuel_limit: u64,
) -> Outcome {
    let mut vm = Vm { program, host, ctx, fuel_left: fuel_limit, depth: 0, read_only: mode == Mode::View };
    let result = vm.run(mode, function, args);
    Outcome { result, fuel_used: fuel_limit - vm.fuel_left }
}

impl<'a, H: Host> Vm<'a, H> {
    fn charge(&mut self, amount: u64) -> Result<(), VmError> {
        match self.fuel_left.checked_sub(amount) {
            Some(left) => {
                self.fuel_left = left;
                Ok(())
            }
            None => {
                self.fuel_left = 0;
                Err(VmError::OutOfFuel)
            }
        }
    }

    fn run(&mut self, mode: Mode, function: &str, args: Vec<Value>) -> Result<Value, VmError> {
        match mode {
            Mode::Deploy => {
                for (i, s) in self.program.states.iter().enumerate() {
                    if let Some(v) = &s.init {
                        self.write_scalar(i as u16, &s.ty, v.clone())?;
                    }
                }
                match self.program.find("init") {
                    Some((idx, f)) => {
                        if self.ctx.value > 0 && !f.payable {
                            return Err(VmError::NotPayable);
                        }
                        self.call_entry(idx, args)
                    }
                    None => {
                        if !args.is_empty() {
                            return Err(VmError::BadArguments("this contract has no init()".into()));
                        }
                        if self.ctx.value > 0 {
                            return Err(VmError::NotPayable);
                        }
                        Ok(Value::Unit)
                    }
                }
            }
            Mode::Action | Mode::View => {
                let (idx, f) = self.program.find(function).ok_or_else(|| VmError::UnknownFunction(function.to_string()))?;
                let expected = if mode == Mode::Action { FnKind::Action } else { FnKind::View };
                if f.kind != expected {
                    return Err(VmError::NotCallable(function.to_string()));
                }
                if self.ctx.value > 0 && !f.payable {
                    return Err(VmError::NotPayable);
                }
                self.call_entry(idx, args)
            }
        }
    }

    fn call_entry(&mut self, idx: u16, args: Vec<Value>) -> Result<Value, VmError> {
        let f = &self.program.functions[idx as usize];
        if args.len() != f.params.len() {
            return Err(VmError::BadArguments(format!("{} expects {} argument(s), got {}", f.name, f.params.len(), args.len())));
        }
        for (i, (a, (name, t))) in args.iter().zip(f.params.iter()).enumerate() {
            if !a.has_type(t) {
                return Err(VmError::BadArguments(format!("argument {} ({name}) must be {t}", i + 1)));
            }
            check_size(a)?;
        }
        self.call(idx, args)
    }

    fn call(&mut self, idx: u16, args: Vec<Value>) -> Result<Value, VmError> {
        self.charge(fuel::CALL)?;
        if self.depth >= MAX_CALL_DEPTH {
            return Err(VmError::CallDepth);
        }
        self.depth += 1;
        let program = self.program;
        let f = &program.functions[idx as usize];
        let mut locals = vec![Value::Unit; f.locals as usize];
        for (i, a) in args.into_iter().enumerate() {
            locals[i] = a;
        }
        let flow = self.block(&f.body, &mut locals)?;
        self.depth -= 1;
        Ok(match flow {
            Flow::Return(v) => v,
            _ => Value::Unit,
        })
    }

    fn block(&mut self, stmts: &[Stmt], locals: &mut Vec<Value>) -> Result<Flow, VmError> {
        for s in stmts {
            match self.stmt(s, locals)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn mutation(&self) -> Result<(), VmError> {
        if self.read_only {
            Err(VmError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn stmt(&mut self, s: &Stmt, locals: &mut Vec<Value>) -> Result<Flow, VmError> {
        self.charge(fuel::STMT)?;
        match s {
            Stmt::SetLocal { slot, value } => {
                let v = self.eval(value, locals)?;
                locals[*slot as usize] = v;
            }
            Stmt::SetState { var, value } => {
                self.mutation()?;
                let v = self.eval(value, locals)?;
                let ty = self.program.states[*var as usize].ty.clone();
                self.write_scalar(*var, &ty, v)?;
            }
            Stmt::SetMap { var, key, value } => {
                self.mutation()?;
                let k = self.eval(key, locals)?;
                let v = self.eval(value, locals)?;
                let Type::Map(_, vt) = &self.program.states[*var as usize].ty else {
                    return Err(VmError::Type("not a map".into()));
                };
                let default = Value::default_for(vt);
                let key = map_key(*var, &k);
                if v == default {
                    self.write(&key, None)?;
                } else {
                    self.write(&key, Some(borsh::to_vec(&v).expect("serializable")))?;
                }
            }
            Stmt::RemoveMap { var, key } => {
                self.mutation()?;
                let k = self.eval(key, locals)?;
                self.write(&map_key(*var, &k), None)?;
            }
            Stmt::SetStateListItem { var, index, value } => {
                self.mutation()?;
                let i = self.eval_int(index, locals)?;
                let v = self.eval(value, locals)?;
                let len = self.list_len(*var)?;
                let idx = check_index(i, len)?;
                self.write(&list_item_key(*var, idx), Some(borsh::to_vec(&v).expect("serializable")))?;
            }
            Stmt::PushStateList { var, value } => {
                self.mutation()?;
                let v = self.eval(value, locals)?;
                let len = self.list_len(*var)?;
                self.write(&list_item_key(*var, len), Some(borsh::to_vec(&v).expect("serializable")))?;
                self.write(&list_len_key(*var), Some((len + 1).to_be_bytes().to_vec()))?;
            }
            Stmt::PopStateList { var } => {
                self.mutation()?;
                self.pop_state_list(*var)?;
            }
            Stmt::SetLocalListItem { slot, index, value } => {
                let i = self.eval_int(index, locals)?;
                let v = self.eval(value, locals)?;
                let Value::List(items) = &mut locals[*slot as usize] else {
                    return Err(VmError::Type("not a list".into()));
                };
                let idx = check_index(i, items.len() as u64)? as usize;
                items[idx] = v;
            }
            Stmt::PushLocalList { slot, value } => {
                let v = self.eval(value, locals)?;
                let Value::List(items) = &mut locals[*slot as usize] else {
                    return Err(VmError::Type("not a list".into()));
                };
                if items.len() >= MAX_LIST_LEN {
                    return Err(VmError::TooLarge);
                }
                self.charge(fuel::EXPR + v.size() as u64 / 32)?;
                items.push(v);
            }
            Stmt::If { cond, then, els } => {
                return if self.eval_bool(cond, locals)? { self.block(then, locals) } else { self.block(els, locals) };
            }
            Stmt::While { cond, body } => {
                while self.eval_bool(cond, locals)? {
                    match self.block(body, locals)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                }
            }
            Stmt::ForRange { slot, start, end, body } => {
                let s = self.eval_int(start, locals)?;
                let e = self.eval_int(end, locals)?;
                let mut i = s;
                while i < e {
                    self.charge(fuel::EXPR)?;
                    locals[*slot as usize] = Value::Int(i);
                    match self.block(body, locals)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                    i += 1;
                }
            }
            Stmt::ForEachLocal { slot, list, body } => {
                let Value::List(items) = self.eval(list, locals)? else {
                    return Err(VmError::Type("not a list".into()));
                };
                for item in items {
                    self.charge(fuel::EXPR)?;
                    locals[*slot as usize] = item;
                    match self.block(body, locals)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                }
            }
            Stmt::ForEachState { slot, var, body } => {
                let len = self.list_len(*var)?;
                for i in 0..len {
                    let item = self.list_get(*var, i)?;
                    locals[*slot as usize] = item;
                    match self.block(body, locals)? {
                        Flow::Break => break,
                        Flow::Normal | Flow::Continue => {}
                        other => return Ok(other),
                    }
                }
            }
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Continue => return Ok(Flow::Continue),
            Stmt::Return(v) => {
                let value = match v {
                    Some(e) => self.eval(e, locals)?,
                    None => Value::Unit,
                };
                return Ok(Flow::Return(value));
            }
            Stmt::Require { cond, message } => {
                if !self.eval_bool(cond, locals)? {
                    let msg = match self.eval(message, locals)? {
                        Value::Text(t) => t,
                        _ => "requirement failed".into(),
                    };
                    return Err(VmError::Require(msg));
                }
            }
            Stmt::Send { to, amount } => {
                self.mutation()?;
                self.charge(fuel::SEND)?;
                let Value::Address(addr) = self.eval(to, locals)? else {
                    return Err(VmError::Type("not an address".into()));
                };
                let amount = self.eval_int(amount, locals)?;
                if amount <= 0 || amount > u64::MAX as i128 {
                    return Err(VmError::BadAmount);
                }
                self.host.send(&addr, amount as u64)?;
            }
            Stmt::Emit { event, args } => {
                self.mutation()?;
                let def = &self.program.events[*event as usize];
                let mut fields = Vec::with_capacity(args.len());
                let mut size = 0usize;
                for (a, (name, _)) in args.iter().zip(def.fields.iter()) {
                    let v = self.eval(a, locals)?;
                    size += v.size();
                    fields.push((name.clone(), v));
                }
                self.charge(fuel::EMIT + size as u64)?;
                self.host.emit(&def.name, fields)?;
            }
            Stmt::Destroy { to } => {
                self.mutation()?;
                self.charge(fuel::DESTROY)?;
                let Value::Address(addr) = self.eval(to, locals)? else {
                    return Err(VmError::Type("not an address".into()));
                };
                // Scalar state variables are cleared automatically; maps and lists
                // cannot be enumerated, so the contract must empty them first.
                let program = self.program;
                for (i, sv) in program.states.iter().enumerate() {
                    if sv.ty.is_scalar() {
                        self.write(&scalar_key(i as u16), None)?;
                    }
                }
                let items = self.host.storage_items()?;
                if items > 0 {
                    return Err(VmError::StorageNotEmpty(items));
                }
                self.host.destroy(&addr)?;
                return Ok(Flow::Halt);
            }
            Stmt::Eval(e) => {
                self.eval(e, locals)?;
            }
        }
        Ok(Flow::Normal)
    }

    fn read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, VmError> {
        self.charge(fuel::STORAGE_READ)?;
        self.host.storage_read(key)
    }

    fn write(&mut self, key: &[u8], value: Option<Vec<u8>>) -> Result<(), VmError> {
        self.mutation()?;
        let bytes = key.len() + value.as_ref().map_or(0, Vec::len);
        self.charge(fuel::STORAGE_WRITE + fuel::STORAGE_WRITE_PER_BYTE * bytes as u64)?;
        self.host.storage_write(key, value)
    }

    fn decode_value(bytes: &[u8]) -> Result<Value, VmError> {
        borsh::from_slice::<Value>(bytes).map_err(|e| VmError::Host(format!("corrupt storage value: {e}")))
    }

    fn write_scalar(&mut self, var: u16, ty: &Type, v: Value) -> Result<(), VmError> {
        if v == Value::default_for(ty) {
            self.write(&scalar_key(var), None)
        } else {
            self.write(&scalar_key(var), Some(borsh::to_vec(&v).expect("serializable")))
        }
    }

    fn list_len(&mut self, var: u16) -> Result<u64, VmError> {
        Ok(match self.read(&list_len_key(var))? {
            Some(b) => u64::from_be_bytes(b.try_into().map_err(|_| VmError::Host("corrupt list length".into()))?),
            None => 0,
        })
    }

    fn list_get(&mut self, var: u16, index: u64) -> Result<Value, VmError> {
        match self.read(&list_item_key(var, index))? {
            Some(b) => Self::decode_value(&b),
            None => Err(VmError::Host("missing list item".into())),
        }
    }

    fn pop_state_list(&mut self, var: u16) -> Result<Value, VmError> {
        let len = self.list_len(var)?;
        if len == 0 {
            return Err(VmError::IndexOutOfBounds { index: -1, len: 0 });
        }
        let v = self.list_get(var, len - 1)?;
        self.write(&list_item_key(var, len - 1), None)?;
        self.write(&list_len_key(var), if len == 1 { None } else { Some((len - 1).to_be_bytes().to_vec()) })?;
        Ok(v)
    }

    fn eval_int(&mut self, e: &Expr, locals: &[Value]) -> Result<i128, VmError> {
        match self.eval(e, locals)? {
            Value::Int(i) => Ok(i),
            _ => Err(VmError::Type("expected int".into())),
        }
    }

    fn eval_bool(&mut self, e: &Expr, locals: &[Value]) -> Result<bool, VmError> {
        match self.eval(e, locals)? {
            Value::Bool(b) => Ok(b),
            _ => Err(VmError::Type("expected bool".into())),
        }
    }

    fn eval_bytes(&mut self, e: &Expr, locals: &[Value]) -> Result<Vec<u8>, VmError> {
        match self.eval(e, locals)? {
            Value::Bytes(b) => Ok(b),
            _ => Err(VmError::Type("expected bytes".into())),
        }
    }

    fn eval(&mut self, e: &Expr, locals: &[Value]) -> Result<Value, VmError> {
        self.charge(fuel::EXPR)?;
        Ok(match e {
            Expr::Const(v) => {
                self.charge(v.size() as u64 / 32 * fuel::PER_32_BYTES)?;
                v.clone()
            }
            Expr::Local(slot) => {
                let v = locals[*slot as usize].clone();
                self.charge(v.size() as u64 / 32 * fuel::PER_32_BYTES)?;
                v
            }
            Expr::State(var) => {
                let ty = &self.program.states[*var as usize].ty;
                match self.read(&scalar_key(*var))? {
                    Some(b) => Self::decode_value(&b)?,
                    None => Value::default_for(ty),
                }
            }
            Expr::MapGet { var, key } => {
                let k = self.eval(key, locals)?;
                let Type::Map(_, vt) = &self.program.states[*var as usize].ty else {
                    return Err(VmError::Type("not a map".into()));
                };
                let default = Value::default_for(vt);
                match self.read(&map_key(*var, &k))? {
                    Some(b) => Self::decode_value(&b)?,
                    None => default,
                }
            }
            Expr::MapHas { var, key } => {
                let k = self.eval(key, locals)?;
                Value::Bool(self.read(&map_key(*var, &k))?.is_some())
            }
            Expr::StateListGet { var, index } => {
                let i = self.eval_int(index, locals)?;
                let len = self.list_len(*var)?;
                let idx = check_index(i, len)?;
                self.list_get(*var, idx)?
            }
            Expr::StateListLen { var } => Value::Int(self.list_len(*var)? as i128),
            Expr::StateListPop { var } => {
                self.mutation()?;
                self.pop_state_list(*var)?
            }
            Expr::List(items) => {
                if items.len() > MAX_LIST_LEN {
                    return Err(VmError::TooLarge);
                }
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval(it, locals)?);
                }
                let v = Value::List(out);
                check_size(&v)?;
                v
            }
            Expr::Index { base, index } => {
                let b = self.eval(base, locals)?;
                let i = self.eval_int(index, locals)?;
                match b {
                    Value::List(items) => {
                        let idx = check_index(i, items.len() as u64)? as usize;
                        items.into_iter().nth(idx).expect("checked index")
                    }
                    Value::Bytes(bytes) => {
                        let idx = check_index(i, bytes.len() as u64)? as usize;
                        Value::Int(bytes[idx] as i128)
                    }
                    _ => return Err(VmError::Type("cannot index".into())),
                }
            }
            Expr::Neg(inner) => Value::Int(self.eval_int(inner, locals)?.checked_neg().ok_or(VmError::Overflow)?),
            Expr::Not(inner) => Value::Bool(!self.eval_bool(inner, locals)?),
            Expr::Binary { op, left, right } => match op {
                BinOp::And => Value::Bool(self.eval_bool(left, locals)? && self.eval_bool(right, locals)?),
                BinOp::Or => Value::Bool(self.eval_bool(left, locals)? || self.eval_bool(right, locals)?),
                _ => {
                    let l = self.eval(left, locals)?;
                    let r = self.eval(right, locals)?;
                    let size = (l.size() + r.size()) as u64;
                    self.charge(size / 32 * fuel::PER_32_BYTES)?;
                    ops::binary(*op, &l, &r)?
                }
            },
            Expr::Call { func, args } => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a, locals)?);
                }
                self.call(*func, vals)?
            }
            Expr::Ctx(c) => match c {
                Ctx::Caller => Value::Address(self.ctx.caller),
                Ctx::Value => Value::Int(self.ctx.value as i128),
                Ctx::Balance => Value::Int(self.host.balance()? as i128),
                Ctx::Height => Value::Int(self.ctx.height as i128),
                Ctx::SelfAddress => Value::Address(self.ctx.self_address),
            },
            Expr::Builtin { f, args } => self.builtin(*f, args, locals)?,
        })
    }

    fn builtin(&mut self, f: Builtin, args: &[Expr], locals: &[Value]) -> Result<Value, VmError> {
        Ok(match f {
            Builtin::Len => match self.eval(&args[0], locals)? {
                Value::List(l) => Value::Int(l.len() as i128),
                Value::Text(t) => Value::Int(t.len() as i128),
                Value::Bytes(b) => Value::Int(b.len() as i128),
                _ => return Err(VmError::Type("len".into())),
            },
            Builtin::Sha256 | Builtin::Blake3 => {
                let data = self.eval_bytes(&args[0], locals)?;
                self.charge(fuel::HASH + fuel::HASH_PER_64_BYTES * (data.len() as u64 / 64))?;
                if f == Builtin::Sha256 {
                    Value::Bytes(sha2::Sha256::digest(&data).to_vec())
                } else {
                    Value::Bytes(blake3::hash(&data).as_bytes().to_vec())
                }
            }
            Builtin::BytesOfInt => Value::Bytes(self.eval_int(&args[0], locals)?.to_be_bytes().to_vec()),
            Builtin::BytesOfAddress => match self.eval(&args[0], locals)? {
                Value::Address(a) => Value::Bytes(a.to_vec()),
                _ => return Err(VmError::Type("address".into())),
            },
            Builtin::BytesOfText => match self.eval(&args[0], locals)? {
                Value::Text(t) => Value::Bytes(t.into_bytes()),
                _ => return Err(VmError::Type("text".into())),
            },
            Builtin::BytesOfBool => Value::Bytes(vec![self.eval_bool(&args[0], locals)? as u8]),
            Builtin::TextOfInt => Value::Text(self.eval_int(&args[0], locals)?.to_string()),
            Builtin::IntOfBytes => {
                let b = self.eval_bytes(&args[0], locals)?;
                if b.len() > 15 {
                    return Err(VmError::BadArguments("to_int() accepts at most 15 bytes".into()));
                }
                let mut acc: i128 = 0;
                for byte in b {
                    acc = (acc << 8) | byte as i128;
                }
                Value::Int(acc)
            }
            Builtin::Min | Builtin::Max => {
                let a = self.eval_int(&args[0], locals)?;
                let b = self.eval_int(&args[1], locals)?;
                Value::Int(if f == Builtin::Min { a.min(b) } else { a.max(b) })
            }
            Builtin::Abs => Value::Int(self.eval_int(&args[0], locals)?.checked_abs().ok_or(VmError::Overflow)?),
            Builtin::Slice => {
                let b = self.eval_bytes(&args[0], locals)?;
                let s = self.eval_int(&args[1], locals)?;
                let e = self.eval_int(&args[2], locals)?;
                if s < 0 || e < s || e > b.len() as i128 {
                    return Err(VmError::IndexOutOfBounds { index: e, len: b.len() as u64 });
                }
                Value::Bytes(b[s as usize..e as usize].to_vec())
            }
            Builtin::VerifyEd25519 => {
                let pk = self.eval_bytes(&args[0], locals)?;
                let msg = self.eval_bytes(&args[1], locals)?;
                let sig = self.eval_bytes(&args[2], locals)?;
                self.charge(fuel::ED25519_VERIFY + msg.len() as u64 / 64)?;
                Value::Bool(verify_ed25519(&pk, &msg, &sig))
            }
            Builtin::RingVerify => {
                let Value::List(ring) = self.eval(&args[0], locals)? else {
                    return Err(VmError::Type("ring".into()));
                };
                let msg = self.eval_bytes(&args[1], locals)?;
                let sig = self.eval_bytes(&args[2], locals)?;
                let key_image = self.eval_bytes(&args[3], locals)?;
                if ring.len() > MAX_RING_SIZE {
                    return Err(VmError::BadArguments(format!("ring larger than {MAX_RING_SIZE} members")));
                }
                self.charge(fuel::RING_BASE + fuel::RING_PER_MEMBER * ring.len() as u64)?;
                let mut keys = Vec::with_capacity(ring.len());
                for k in ring {
                    match k {
                        Value::Bytes(b) if b.len() == 32 => keys.push(<[u8; 32]>::try_from(b.as_slice()).expect("32 bytes")),
                        _ => return Ok(Value::Bool(false)),
                    }
                }
                let ki: [u8; 32] = match key_image.as_slice().try_into() {
                    Ok(k) => k,
                    Err(_) => return Ok(Value::Bool(false)),
                };
                Value::Bool(crate::ring::verify(&msg, &keys, &sig, &ki))
            }
            Builtin::AddressOfKey => {
                let pk = self.eval_bytes(&args[0], locals)?;
                if pk.len() != 32 {
                    return Err(VmError::BadArguments("address_of() needs a 32-byte public key".into()));
                }
                // Same derivation as The Coin addresses: tagged BLAKE3("address").
                let mut h = blake3::Hasher::new();
                h.update(b"TheCoin:address\x00");
                h.update(&pk);
                let mut a = [0u8; 20];
                a.copy_from_slice(&h.finalize().as_bytes()[..20]);
                Value::Address(a)
            }
            Builtin::ZeroAddress => Value::Address([0u8; 20]),
        })
    }
}

fn verify_ed25519(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let (Ok(pk), Ok(sig)) = (<[u8; 32]>::try_from(pk), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&pk) else { return false };
    if vk.is_weak() {
        return false;
    }
    vk.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
}

fn check_index(i: i128, len: u64) -> Result<u64, VmError> {
    if i < 0 || i >= len as i128 {
        return Err(VmError::IndexOutOfBounds { index: i, len });
    }
    Ok(i as u64)
}

fn check_size(v: &Value) -> Result<(), VmError> {
    if v.size() > MAX_VALUE_BYTES {
        return Err(VmError::TooLarge);
    }
    if let Value::List(items) = v {
        if items.len() > MAX_LIST_LEN {
            return Err(VmError::TooLarge);
        }
    }
    Ok(())
}
