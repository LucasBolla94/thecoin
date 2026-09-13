//! Compiled program format ("TCCL bytecode").
//!
//! The compiler turns source code into a fully resolved, type-checked tree:
//! every name is replaced by a slot or index, every operation is known to be
//! well-typed. This structure is Borsh-encoded and stored on-chain; nodes
//! execute it directly. **The encoding is consensus-critical** — enum variants
//! may only be appended.

use crate::ast::BinOp;
use borsh::{BorshDeserialize, BorshSerialize};
use std::fmt;

pub const LANGUAGE_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
pub enum Type {
    Int,
    Bool,
    Text,
    Bytes,
    Address,
    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    /// No value (functions without a return type).
    Unit,
}

impl Type {
    pub fn is_scalar(&self) -> bool {
        matches!(self, Type::Int | Type::Bool | Type::Text | Type::Bytes | Type::Address)
    }

    /// Types usable as map keys.
    pub fn is_key(&self) -> bool {
        self.is_scalar()
    }
}

impl std::str::FromStr for Type {
    type Err = String;

    /// Parses the textual form produced by `Display` (`int`, `list[bytes]`, `map[address, int]`).
    fn from_str(s: &str) -> Result<Type, String> {
        let s = s.trim();
        Ok(match s {
            "int" => Type::Int,
            "bool" => Type::Bool,
            "text" => Type::Text,
            "bytes" => Type::Bytes,
            "address" => Type::Address,
            "nothing" => Type::Unit,
            _ => {
                if let Some(inner) = s.strip_prefix("list[").and_then(|x| x.strip_suffix(']')) {
                    Type::List(Box::new(inner.parse()?))
                } else if let Some(inner) = s.strip_prefix("map[").and_then(|x| x.strip_suffix(']')) {
                    let mut depth = 0;
                    let split = inner
                        .char_indices()
                        .find(|(_, c)| {
                            match c {
                                '[' => depth += 1,
                                ']' => depth -= 1,
                                ',' if depth == 0 => return true,
                                _ => {}
                            }
                            false
                        })
                        .map(|(i, _)| i)
                        .ok_or_else(|| format!("invalid map type '{s}'"))?;
                    Type::Map(Box::new(inner[..split].parse()?), Box::new(inner[split + 1..].parse()?))
                } else {
                    return Err(format!("unknown type '{s}'"));
                }
            }
        })
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => f.write_str("int"),
            Type::Bool => f.write_str("bool"),
            Type::Text => f.write_str("text"),
            Type::Bytes => f.write_str("bytes"),
            Type::Address => f.write_str("address"),
            Type::List(t) => write!(f, "list[{t}]"),
            Type::Map(k, v) => write!(f, "map[{k}, {v}]"),
            Type::Unit => f.write_str("nothing"),
        }
    }
}

/// A runtime value.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Int(i128),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Address([u8; 20]),
    List(Vec<Value>),
    Unit,
}

impl Value {
    pub fn default_for(t: &Type) -> Value {
        match t {
            Type::Int => Value::Int(0),
            Type::Bool => Value::Bool(false),
            Type::Text => Value::Text(String::new()),
            Type::Bytes => Value::Bytes(Vec::new()),
            Type::Address => Value::Address([0u8; 20]),
            Type::List(_) => Value::List(Vec::new()),
            Type::Map(_, _) | Type::Unit => Value::Unit,
        }
    }

    pub fn has_type(&self, t: &Type) -> bool {
        match (self, t) {
            (Value::Int(_), Type::Int)
            | (Value::Bool(_), Type::Bool)
            | (Value::Text(_), Type::Text)
            | (Value::Bytes(_), Type::Bytes)
            | (Value::Address(_), Type::Address)
            | (Value::Unit, Type::Unit) => true,
            (Value::List(items), Type::List(inner)) => items.iter().all(|v| v.has_type(inner)),
            _ => false,
        }
    }

    /// Canonical bytes used as a storage key component.
    pub fn key_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("value serializes")
    }

    /// Approximate size in bytes (for limits and fuel).
    pub fn size(&self) -> usize {
        match self {
            Value::Int(_) => 16,
            Value::Bool(_) | Value::Unit => 1,
            Value::Text(s) => s.len(),
            Value::Bytes(b) => b.len(),
            Value::Address(_) => 20,
            Value::List(items) => 4 + items.iter().map(Value::size).sum::<usize>(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FnKind {
    Init,
    Action,
    View,
    Internal,
}

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct StateVar {
    pub name: String,
    pub ty: Type,
    /// Constant initial value set at deployment (already evaluated).
    pub init: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct EventDef {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct Function {
    pub name: String,
    pub kind: FnKind,
    pub payable: bool,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
    /// Number of local slots (parameters first).
    pub locals: u16,
    /// Whether the function (transitively) changes state, sends coins or emits events.
    pub mutates: bool,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct Program {
    pub version: u16,
    pub name: String,
    pub states: Vec<StateVar>,
    pub events: Vec<EventDef>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Ctx {
    Caller,
    Value,
    Balance,
    Height,
    SelfAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Builtin {
    Len,
    Sha256,
    Blake3,
    BytesOfInt,
    BytesOfAddress,
    BytesOfText,
    BytesOfBool,
    TextOfInt,
    Min,
    Max,
    Abs,
    Slice,
    VerifyEd25519,
    RingVerify,
    AddressOfKey,
    ZeroAddress,
    IntOfBytes,
}

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub enum Stmt {
    SetLocal { slot: u16, value: Expr },
    SetState { var: u16, value: Expr },
    SetMap { var: u16, key: Expr, value: Expr },
    RemoveMap { var: u16, key: Expr },
    SetStateListItem { var: u16, index: Expr, value: Expr },
    PushStateList { var: u16, value: Expr },
    PopStateList { var: u16 },
    SetLocalListItem { slot: u16, index: Expr, value: Expr },
    PushLocalList { slot: u16, value: Expr },
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    ForRange { slot: u16, start: Expr, end: Expr, body: Vec<Stmt> },
    ForEachLocal { slot: u16, list: Expr, body: Vec<Stmt> },
    ForEachState { slot: u16, var: u16, body: Vec<Stmt> },
    Break,
    Continue,
    Return(Option<Expr>),
    Require { cond: Expr, message: Expr },
    Send { to: Expr, amount: Expr },
    Emit { event: u16, args: Vec<Expr> },
    Destroy { to: Expr },
    Eval(Expr),
}

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub enum Expr {
    Const(Value),
    Local(u16),
    State(u16),
    MapGet { var: u16, key: Box<Expr> },
    MapHas { var: u16, key: Box<Expr> },
    StateListGet { var: u16, index: Box<Expr> },
    StateListLen { var: u16 },
    StateListPop { var: u16 },
    List(Vec<Expr>),
    Index { base: Box<Expr>, index: Box<Expr> },
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Call { func: u16, args: Vec<Expr> },
    Builtin { f: Builtin, args: Vec<Expr> },
    Ctx(Ctx),
}

/// Public interface of a function (for wallets, explorers and the API).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AbiFunction {
    pub name: String,
    pub kind: FnKind,
    pub payable: bool,
    pub params: Vec<(String, String)>,
    pub returns: String,
}

impl Program {
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("program serializes")
    }

    pub fn from_bytes(b: &[u8]) -> Result<Program, std::io::Error> {
        Program::try_from_slice(b)
    }

    pub fn find(&self, name: &str) -> Option<(u16, &Function)> {
        self.functions.iter().enumerate().find(|(_, f)| f.name == name).map(|(i, f)| (i as u16, f))
    }

    pub fn abi(&self) -> Vec<AbiFunction> {
        self.functions
            .iter()
            .filter(|f| f.kind != FnKind::Internal)
            .map(|f| AbiFunction {
                name: f.name.clone(),
                kind: f.kind,
                payable: f.payable,
                params: f.params.iter().map(|(n, t)| (n.clone(), t.to_string())).collect(),
                returns: f.ret.to_string(),
            })
            .collect()
    }
}
