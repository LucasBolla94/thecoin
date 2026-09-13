//! Type checker and compiler: syntax tree → [`Program`].
//!
//! Rules enforced here (a contract that breaks any of them is rejected by the
//! network before it can ever run):
//! * every constant, state variable, parameter and local variable declares its type;
//! * every expression has exactly one type; no implicit conversions;
//! * functions with a return type return on every path;
//! * `view` functions cannot change state, send coins, emit events or call
//!   functions that do;
//! * only `payable` functions can receive TCN;
//! * maps exist only as state variables; names cannot be redeclared or shadow
//!   built-in names.

use crate::ast::{self, AssignOp, BinOp, FuncKind, Item, TypeExpr, UnOp};
use crate::error::{CompileError, Pos};
use crate::ops;
use crate::program::*;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_LOCALS: usize = 1_024;
pub const MAX_FUNCTIONS: usize = 256;
pub const MAX_STATE_VARS: usize = 256;
pub const TCN: i128 = 100_000_000;

/// Compiler options.
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Accepted address prefixes for `address("...")` literals.
    pub address_prefixes: Vec<String>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions { address_prefixes: vec!["tc".into(), "tct".into(), "tcr".into()] }
    }
}

type CResult<T> = Result<T, CompileError>;

fn err<T>(pos: Pos, msg: impl Into<String>) -> CResult<T> {
    Err(CompileError::new(pos, msg))
}

const RESERVED: &[&str] = &[
    "caller", "value", "balance", "height", "self", "TCN", "int", "bool", "text", "bytes", "address", "list", "map", "len", "sha256",
    "blake3", "to_bytes", "to_text", "to_int", "min", "max", "abs", "slice", "verify_ed25519", "ring_verify", "address_of", "zero_address",
    "range",
];

struct Sig {
    kind: FuncKind,
    params: Vec<Type>,
    ret: Type,
}

struct FnCtx {
    kind: FuncKind,
    ret: Type,
    scopes: Vec<BTreeMap<String, (u16, Type)>>,
    next_slot: usize,
    loop_depth: usize,
    mutates: bool,
    calls: BTreeSet<u16>,
}

struct Checker<'o> {
    opts: &'o CompileOptions,
    consts: BTreeMap<String, (Type, Value)>,
    states: Vec<StateVar>,
    state_index: BTreeMap<String, u16>,
    events: Vec<EventDef>,
    event_index: BTreeMap<String, u16>,
    sigs: Vec<Sig>,
    fn_index: BTreeMap<String, u16>,
}

pub fn compile(src: &str, opts: &CompileOptions) -> CResult<Program> {
    let contract = crate::parser::parse(src)?;
    let mut c = Checker {
        opts,
        consts: BTreeMap::new(),
        states: Vec::new(),
        state_index: BTreeMap::new(),
        events: Vec::new(),
        event_index: BTreeMap::new(),
        sigs: Vec::new(),
        fn_index: BTreeMap::new(),
    };
    c.consts.insert("TCN".into(), (Type::Int, Value::Int(TCN)));
    c.program(contract)
}

impl<'o> Checker<'o> {
    fn check_new_name(&self, name: &str, pos: Pos) -> CResult<()> {
        if RESERVED.contains(&name) {
            return err(pos, format!("'{name}' is a reserved name"));
        }
        if self.consts.contains_key(name) || self.state_index.contains_key(name) || self.event_index.contains_key(name) || self.fn_index.contains_key(name)
        {
            return err(pos, format!("'{name}' is already declared"));
        }
        Ok(())
    }

    fn resolve_type(&self, t: &TypeExpr, allow_map: bool) -> CResult<Type> {
        Ok(match t {
            TypeExpr::Named(n, pos) => match n.as_str() {
                "int" => Type::Int,
                "bool" => Type::Bool,
                "text" => Type::Text,
                "bytes" => Type::Bytes,
                "address" => Type::Address,
                other => return err(*pos, format!("unknown type '{other}' (types: int, bool, text, bytes, address, list[T], map[K, V])")),
            },
            TypeExpr::List(inner, _) => Type::List(Box::new(self.resolve_type(inner, false)?)),
            TypeExpr::Map(k, v, pos) => {
                if !allow_map {
                    return err(*pos, "maps can only be used as state variables");
                }
                let kt = self.resolve_type(k, false)?;
                if !kt.is_key() {
                    return err(*pos, format!("map keys must be int, bool, text, bytes or address, not {kt}"));
                }
                Type::Map(Box::new(kt), Box::new(self.resolve_type(v, false)?))
            }
        })
    }

    fn program(&mut self, contract: ast::Contract) -> CResult<Program> {
        // Pass 1: constants, state, events, function signatures (in source order).
        let mut decls = Vec::new();
        for item in contract.items {
            match item {
                Item::Const { name, ty, value, pos } => {
                    self.check_new_name(&name, pos)?;
                    let t = self.resolve_type(&ty, false)?;
                    let v = self.const_value(&value, &t)?;
                    self.consts.insert(name, (t, v));
                }
                Item::State { name, ty, init, pos } => {
                    self.check_new_name(&name, pos)?;
                    if self.states.len() >= MAX_STATE_VARS {
                        return err(pos, "too many state variables");
                    }
                    let t = self.resolve_type(&ty, true)?;
                    let init_v = match init {
                        Some(e) => {
                            if !t.is_scalar() {
                                return err(pos, "only int, bool, text, bytes and address state variables can have an initial value");
                            }
                            Some(self.const_value(&e, &t)?)
                        }
                        None => None,
                    };
                    self.state_index.insert(name.clone(), self.states.len() as u16);
                    self.states.push(StateVar { name, ty: t, init: init_v });
                }
                Item::Event { name, fields, pos } => {
                    self.check_new_name(&name, pos)?;
                    let mut fs = Vec::new();
                    for p in fields {
                        fs.push((p.name, self.resolve_type(&p.ty, false)?));
                    }
                    self.event_index.insert(name.clone(), self.events.len() as u16);
                    self.events.push(EventDef { name, fields: fs });
                }
                Item::Func(f) => {
                    if f.kind == FuncKind::Init {
                        if self.fn_index.contains_key("init") {
                            return err(f.pos, "only one init() is allowed");
                        }
                    } else {
                        self.check_new_name(&f.name, f.pos)?;
                    }
                    if self.sigs.len() >= MAX_FUNCTIONS {
                        return err(f.pos, "too many functions");
                    }
                    if f.payable && !matches!(f.kind, FuncKind::Action | FuncKind::Init) {
                        return err(f.pos, "only action and init can be payable");
                    }
                    let mut params = Vec::new();
                    let mut seen = BTreeSet::new();
                    for p in &f.params {
                        if RESERVED.contains(&p.name.as_str()) || self.consts.contains_key(&p.name) || self.state_index.contains_key(&p.name) {
                            return err(p.pos, format!("parameter name '{}' is reserved or already declared", p.name));
                        }
                        if !seen.insert(p.name.clone()) {
                            return err(p.pos, format!("duplicate parameter '{}'", p.name));
                        }
                        params.push(self.resolve_type(&p.ty, false)?);
                    }
                    let ret = match &f.ret {
                        Some(t) => self.resolve_type(t, false)?,
                        None => Type::Unit,
                    };
                    if f.kind == FuncKind::View && ret == Type::Unit {
                        return err(f.pos, "a view must declare a return type ('-> type')");
                    }
                    if f.kind == FuncKind::Init && ret != Type::Unit {
                        return err(f.pos, "init cannot return a value");
                    }
                    self.fn_index.insert(f.name.clone(), self.sigs.len() as u16);
                    self.sigs.push(Sig { kind: f.kind, params, ret });
                    decls.push(f);
                }
            }
        }
        if !decls.iter().any(|f| matches!(f.kind, FuncKind::Action | FuncKind::View | FuncKind::Init)) {
            return err(contract.pos, "a contract needs at least one init, action or view");
        }

        // Pass 2: bodies.
        let mut functions = Vec::new();
        let mut call_graph = Vec::new();
        for f in &decls {
            let (func, calls) = self.function(f)?;
            functions.push(func);
            call_graph.push(calls);
        }

        // Transitive mutation analysis.
        loop {
            let mut changed = false;
            for i in 0..functions.len() {
                if !functions[i].mutates && call_graph[i].iter().any(|c| functions[*c as usize].mutates) {
                    functions[i].mutates = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for (f, decl) in functions.iter().zip(&decls) {
            if f.kind == FnKind::View && f.mutates {
                return err(decl.pos, format!("view '{}' changes state, sends TCN or emits events (directly or through a fn it calls)", f.name));
            }
        }

        Ok(Program { version: LANGUAGE_VERSION, name: contract.name, states: self.states.clone(), events: self.events.clone(), functions })
    }

    /// Evaluates a compile-time constant expression.
    fn const_value(&self, e: &ast::Expr, expected: &Type) -> CResult<Value> {
        let (v, t) = self.const_eval(e)?;
        if &t != expected {
            return err(e.pos(), format!("expected a {expected} value, found {t}"));
        }
        Ok(v)
    }

    fn const_eval(&self, e: &ast::Expr) -> CResult<(Value, Type)> {
        let pos = e.pos();
        match e {
            ast::Expr::Int(v, _) => Ok((Value::Int(*v), Type::Int)),
            ast::Expr::Bool(b, _) => Ok((Value::Bool(*b), Type::Bool)),
            ast::Expr::Text(s, _) => Ok((Value::Text(s.clone()), Type::Text)),
            ast::Expr::Bytes(b, _) => Ok((Value::Bytes(b.clone()), Type::Bytes)),
            ast::Expr::Name(n, _) => match self.consts.get(n) {
                Some((t, v)) => Ok((v.clone(), t.clone())),
                None => err(pos, format!("'{n}' is not a constant (constant values can only use literals and other constants)")),
            },
            ast::Expr::Call(name, args, _) if name == "address" => self.address_literal(args, pos).map(|v| (v, Type::Address)),
            ast::Expr::Unary(UnOp::Neg, inner, _) => {
                let (v, t) = self.const_eval(inner)?;
                match v {
                    Value::Int(i) => Ok((Value::Int(i.checked_neg().ok_or_else(|| CompileError::new(pos, "overflow"))?), t)),
                    _ => err(pos, "'-' needs an int"),
                }
            }
            ast::Expr::Unary(UnOp::Not, inner, _) => match self.const_eval(inner)? {
                (Value::Bool(b), t) => Ok((Value::Bool(!b), t)),
                _ => err(pos, "'not' needs a bool"),
            },
            ast::Expr::Binary(op, l, r, _) => {
                let (lv, lt) = self.const_eval(l)?;
                let (rv, rt) = self.const_eval(r)?;
                let t = self.binary_type(*op, &lt, &rt, pos)?;
                let v = ops::binary(*op, &lv, &rv).map_err(|e| CompileError::new(pos, format!("constant expression: {e}")))?;
                Ok((v, t))
            }
            _ => err(pos, "constant values can only use literals, other constants, arithmetic and address(\"...\")"),
        }
    }

    fn address_literal(&self, args: &[ast::Expr], pos: Pos) -> CResult<Value> {
        let [ast::Expr::Text(s, _)] = args else {
            return err(pos, "address(...) takes one text literal, e.g. address(\"tc1...\")");
        };
        let checked = bech32::primitives::decode::CheckedHrpstring::new::<bech32::Bech32m>(s).map_err(|_| CompileError::new(pos, "invalid address literal"))?;
        let hrp = checked.hrp().to_lowercase();
        if !self.opts.address_prefixes.iter().any(|p| *p == hrp) {
            return err(pos, format!("address prefix '{hrp}' is not valid on this network"));
        }
        let bytes: Vec<u8> = checked.byte_iter().collect();
        let arr: [u8; 20] = bytes.try_into().map_err(|_| CompileError::new(pos, "invalid address length"))?;
        Ok(Value::Address(arr))
    }

    fn binary_type(&self, op: BinOp, l: &Type, r: &Type, pos: Pos) -> CResult<Type> {
        match op {
            BinOp::Add => match (l, r) {
                (Type::Int, Type::Int) => Ok(Type::Int),
                (Type::Text, Type::Text) => Ok(Type::Text),
                (Type::Bytes, Type::Bytes) => Ok(Type::Bytes),
                _ => err(pos, format!("cannot add {l} and {r}")),
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if *l == Type::Int && *r == Type::Int {
                    Ok(Type::Int)
                } else {
                    err(pos, format!("arithmetic needs int operands, found {l} and {r}"))
                }
            }
            BinOp::Eq | BinOp::Ne => {
                if l != r {
                    return err(pos, format!("cannot compare {l} with {r}"));
                }
                if !l.is_scalar() {
                    return err(pos, format!("cannot compare values of type {l}"));
                }
                Ok(Type::Bool)
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if *l == Type::Int && *r == Type::Int {
                    Ok(Type::Bool)
                } else {
                    err(pos, format!("ordering comparisons need int operands, found {l} and {r}"))
                }
            }
            BinOp::And | BinOp::Or => {
                if *l == Type::Bool && *r == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    err(pos, format!("'and'/'or' need bool operands, found {l} and {r}"))
                }
            }
        }
    }

    fn function(&self, f: &ast::FuncDecl) -> CResult<(Function, BTreeSet<u16>)> {
        let sig = &self.sigs[self.fn_index[&f.name] as usize];
        let mut ctx = FnCtx { kind: f.kind, ret: sig.ret.clone(), scopes: vec![BTreeMap::new()], next_slot: 0, loop_depth: 0, mutates: false, calls: BTreeSet::new() };
        for (p, t) in f.params.iter().zip(&sig.params) {
            self.declare_local(&mut ctx, &p.name, t.clone(), p.pos)?;
        }
        let body = self.block(&mut ctx, &f.body)?;
        if sig.ret != Type::Unit && !always_returns(&body) {
            return err(f.pos, format!("function '{}' must return a {} on every path", f.name, sig.ret));
        }
        let kind = match f.kind {
            FuncKind::Init => FnKind::Init,
            FuncKind::Action => FnKind::Action,
            FuncKind::View => FnKind::View,
            FuncKind::Fn => FnKind::Internal,
        };
        let params = f.params.iter().zip(&sig.params).map(|(p, t)| (p.name.clone(), t.clone())).collect();
        Ok((
            Function { name: f.name.clone(), kind, payable: f.payable, params, ret: sig.ret.clone(), locals: ctx.next_slot as u16, mutates: ctx.mutates, body },
            ctx.calls,
        ))
    }

    fn declare_local(&self, ctx: &mut FnCtx, name: &str, t: Type, pos: Pos) -> CResult<u16> {
        if RESERVED.contains(&name) {
            return err(pos, format!("'{name}' is a reserved name"));
        }
        if self.consts.contains_key(name) || self.state_index.contains_key(name) || self.fn_index.contains_key(name) || self.event_index.contains_key(name) {
            return err(pos, format!("'{name}' is already declared at contract level"));
        }
        if ctx.scopes.iter().any(|s| s.contains_key(name)) {
            return err(pos, format!("variable '{name}' is already declared"));
        }
        if ctx.next_slot >= MAX_LOCALS {
            return err(pos, "too many local variables");
        }
        let slot = ctx.next_slot as u16;
        ctx.next_slot += 1;
        ctx.scopes.last_mut().expect("scope").insert(name.to_string(), (slot, t));
        Ok(slot)
    }

    fn lookup_local(&self, ctx: &FnCtx, name: &str) -> Option<(u16, Type)> {
        ctx.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn block(&self, ctx: &mut FnCtx, stmts: &[ast::Stmt]) -> CResult<Vec<Stmt>> {
        ctx.scopes.push(BTreeMap::new());
        let mut out = Vec::with_capacity(stmts.len());
        for s in stmts {
            out.push(self.stmt(ctx, s)?);
        }
        ctx.scopes.pop();
        Ok(out)
    }

    fn require_mutable(&self, ctx: &mut FnCtx, pos: Pos, what: &str) -> CResult<()> {
        if ctx.kind == FuncKind::View {
            return err(pos, format!("a view cannot {what}"));
        }
        ctx.mutates = true;
        Ok(())
    }

    fn expect_type(&self, ctx: &mut FnCtx, e: &ast::Expr, t: &Type) -> CResult<Expr> {
        if let (ast::Expr::List(items, _), Type::List(_)) = (e, t) {
            if items.is_empty() {
                return Ok(Expr::List(Vec::new()));
            }
        }
        let (x, xt) = self.expr(ctx, e)?;
        if &xt != t {
            return err(e.pos(), format!("expected {t}, found {xt}"));
        }
        Ok(x)
    }

    fn stmt(&self, ctx: &mut FnCtx, s: &ast::Stmt) -> CResult<Stmt> {
        match s {
            ast::Stmt::Let { name, ty, value, pos } => {
                let t = self.resolve_type(ty, false)?;
                let v = self.expect_type(ctx, value, &t)?;
                let slot = self.declare_local(ctx, name, t, *pos)?;
                Ok(Stmt::SetLocal { slot, value: v })
            }
            ast::Stmt::Assign { target, op, value, pos } => self.assign(ctx, target, *op, value, *pos),
            ast::Stmt::If { branches, els, .. } => {
                let mut else_part = match els {
                    Some(b) => self.block(ctx, b)?,
                    None => Vec::new(),
                };
                for (cond, body) in branches.iter().rev() {
                    let c = self.expect_type(ctx, cond, &Type::Bool)?;
                    let then = self.block(ctx, body)?;
                    else_part = vec![Stmt::If { cond: c, then, els: else_part }];
                }
                Ok(else_part.pop().expect("at least one branch"))
            }
            ast::Stmt::While { cond, body, .. } => {
                let c = self.expect_type(ctx, cond, &Type::Bool)?;
                ctx.loop_depth += 1;
                let b = self.block(ctx, body)?;
                ctx.loop_depth -= 1;
                Ok(Stmt::While { cond: c, body: b })
            }
            ast::Stmt::ForRange { var, start, end, body, pos } => {
                let s = self.expect_type(ctx, start, &Type::Int)?;
                let e = self.expect_type(ctx, end, &Type::Int)?;
                ctx.scopes.push(BTreeMap::new());
                let slot = self.declare_local(ctx, var, Type::Int, *pos)?;
                ctx.loop_depth += 1;
                let b = self.block(ctx, body)?;
                ctx.loop_depth -= 1;
                ctx.scopes.pop();
                Ok(Stmt::ForRange { slot, start: s, end: e, body: b })
            }
            ast::Stmt::ForEach { var, iter, body, pos } => {
                // State list?
                if let ast::Expr::Name(n, _) = iter {
                    if self.lookup_local(ctx, n).is_none() {
                        if let Some(&var_idx) = self.state_index.get(n) {
                            let Type::List(inner) = self.states[var_idx as usize].ty.clone() else {
                                return err(*pos, format!("cannot iterate over state variable '{n}' (only lists)"));
                            };
                            ctx.scopes.push(BTreeMap::new());
                            let slot = self.declare_local(ctx, var, *inner, *pos)?;
                            ctx.loop_depth += 1;
                            let b = self.block(ctx, body)?;
                            ctx.loop_depth -= 1;
                            ctx.scopes.pop();
                            return Ok(Stmt::ForEachState { slot, var: var_idx, body: b });
                        }
                    }
                }
                let (list, lt) = self.expr(ctx, iter)?;
                let Type::List(inner) = lt else {
                    return err(*pos, format!("'for ... in' needs a list or range(start, end), found {lt}"));
                };
                ctx.scopes.push(BTreeMap::new());
                let slot = self.declare_local(ctx, var, *inner, *pos)?;
                ctx.loop_depth += 1;
                let b = self.block(ctx, body)?;
                ctx.loop_depth -= 1;
                ctx.scopes.pop();
                Ok(Stmt::ForEachLocal { slot, list, body: b })
            }
            ast::Stmt::Break(pos) | ast::Stmt::Continue(pos) => {
                if ctx.loop_depth == 0 {
                    return err(*pos, "break/continue outside of a loop");
                }
                Ok(if matches!(s, ast::Stmt::Break(_)) { Stmt::Break } else { Stmt::Continue })
            }
            ast::Stmt::Return(v, pos) => match (v, &ctx.ret) {
                (None, Type::Unit) => Ok(Stmt::Return(None)),
                (None, t) => err(*pos, format!("this function must return a {t}")),
                (Some(_), Type::Unit) => err(*pos, "this function does not return a value (declare '-> type')"),
                (Some(e), t) => {
                    let t = t.clone();
                    Ok(Stmt::Return(Some(self.expect_type(ctx, e, &t)?)))
                }
            },
            ast::Stmt::Require(cond, msg, pos) => {
                let c = self.expect_type(ctx, cond, &Type::Bool)?;
                let m = match msg {
                    Some(m) => self.expect_type(ctx, m, &Type::Text)?,
                    None => Expr::Const(Value::Text(format!("requirement at line {} failed", pos.line))),
                };
                Ok(Stmt::Require { cond: c, message: m })
            }
            ast::Stmt::Send(to, amount, pos) => {
                self.require_mutable(ctx, *pos, "send TCN")?;
                let t = self.expect_type(ctx, to, &Type::Address)?;
                let a = self.expect_type(ctx, amount, &Type::Int)?;
                Ok(Stmt::Send { to: t, amount: a })
            }
            ast::Stmt::Emit(name, args, pos) => {
                self.require_mutable(ctx, *pos, "emit events")?;
                let Some(&idx) = self.event_index.get(name) else {
                    return err(*pos, format!("unknown event '{name}'"));
                };
                let fields = self.events[idx as usize].fields.clone();
                if fields.len() != args.len() {
                    return err(*pos, format!("event '{name}' has {} fields, {} given", fields.len(), args.len()));
                }
                let mut out = Vec::new();
                for (a, (_, t)) in args.iter().zip(fields.iter()) {
                    out.push(self.expect_type(ctx, a, t)?);
                }
                Ok(Stmt::Emit { event: idx, args: out })
            }
            ast::Stmt::Destroy(to, pos) => {
                if ctx.kind != FuncKind::Action {
                    return err(*pos, "destroy() can only be used inside an action");
                }
                self.require_mutable(ctx, *pos, "destroy the contract")?;
                let t = self.expect_type(ctx, to, &Type::Address)?;
                Ok(Stmt::Destroy { to: t })
            }
            ast::Stmt::Pass(_) => Ok(Stmt::Eval(Expr::Const(Value::Unit))),
            ast::Stmt::Expr(e, pos) => {
                // Statement-only methods.
                if let ast::Expr::Method(base, m, args, mpos) = e {
                    if let Some(st) = self.method_stmt(ctx, base, m, args, *mpos)? {
                        return Ok(st);
                    }
                }
                let (x, t) = self.expr(ctx, e)?;
                let allowed = matches!(x, Expr::Call { .. } | Expr::StateListPop { .. });
                if !allowed {
                    return err(*pos, format!("this expression ({t}) does nothing as a statement"));
                }
                Ok(Stmt::Eval(x))
            }
        }
    }

    fn method_stmt(&self, ctx: &mut FnCtx, base: &ast::Expr, m: &str, args: &[ast::Expr], pos: Pos) -> CResult<Option<Stmt>> {
        let ast::Expr::Name(n, _) = base else { return Ok(None) };
        match m {
            "push" => {
                let [arg] = args else { return err(pos, "push takes one argument") };
                if let Some((slot, Type::List(inner))) = self.lookup_local(ctx, n) {
                    let v = self.expect_type(ctx, arg, &inner)?;
                    return Ok(Some(Stmt::PushLocalList { slot, value: v }));
                }
                if let Some(&var) = self.state_index.get(n) {
                    if let Type::List(inner) = self.states[var as usize].ty.clone() {
                        self.require_mutable(ctx, pos, "change state")?;
                        let v = self.expect_type(ctx, arg, &inner)?;
                        return Ok(Some(Stmt::PushStateList { var, value: v }));
                    }
                }
                err(pos, format!("'{n}' is not a list"))
            }
            "remove" => {
                let [arg] = args else { return err(pos, "remove takes one argument (the key)") };
                if let Some(&var) = self.state_index.get(n) {
                    if let Type::Map(k, _) = self.states[var as usize].ty.clone() {
                        if self.lookup_local(ctx, n).is_none() {
                            self.require_mutable(ctx, pos, "change state")?;
                            let key = self.expect_type(ctx, arg, &k)?;
                            return Ok(Some(Stmt::RemoveMap { var, key }));
                        }
                    }
                }
                err(pos, format!("'{n}' is not a map"))
            }
            "pop" => {
                if !args.is_empty() {
                    return err(pos, "pop takes no arguments");
                }
                if let (None, Some(&var)) = (self.lookup_local(ctx, n), self.state_index.get(n)) {
                    if matches!(self.states[var as usize].ty, Type::List(_)) {
                        self.require_mutable(ctx, pos, "change state")?;
                        return Ok(Some(Stmt::PopStateList { var }));
                    }
                }
                err(pos, "pop() is only available on state lists")
            }
            _ => Ok(None),
        }
    }

    fn assign(&self, ctx: &mut FnCtx, target: &ast::Expr, op: AssignOp, value: &ast::Expr, pos: Pos) -> CResult<Stmt> {
        let combine = |current: Expr, rhs: Expr| -> Expr {
            let bop = match op {
                AssignOp::Set => return rhs,
                AssignOp::Add => BinOp::Add,
                AssignOp::Sub => BinOp::Sub,
                AssignOp::Mul => BinOp::Mul,
            };
            Expr::Binary { op: bop, left: Box::new(current), right: Box::new(rhs) }
        };
        let check_op = |t: &Type| -> CResult<()> {
            if op != AssignOp::Set && *t != Type::Int && !(op == AssignOp::Add && matches!(t, Type::Text | Type::Bytes)) {
                return err(pos, format!("compound assignment is not defined for {t}"));
            }
            Ok(())
        };
        match target {
            ast::Expr::Name(n, npos) => {
                if let Some((slot, t)) = self.lookup_local(ctx, n) {
                    check_op(&t)?;
                    let rhs = self.expect_type(ctx, value, &t)?;
                    return Ok(Stmt::SetLocal { slot, value: combine(Expr::Local(slot), rhs) });
                }
                if let Some(&var) = self.state_index.get(n) {
                    let t = self.states[var as usize].ty.clone();
                    if !t.is_scalar() {
                        return err(*npos, format!("cannot assign a whole {t}; change its items instead"));
                    }
                    self.require_mutable(ctx, pos, "change state")?;
                    check_op(&t)?;
                    let rhs = self.expect_type(ctx, value, &t)?;
                    return Ok(Stmt::SetState { var, value: combine(Expr::State(var), rhs) });
                }
                if self.consts.contains_key(n) {
                    return err(*npos, format!("'{n}' is a constant and cannot be changed"));
                }
                err(*npos, format!("unknown variable '{n}'"))
            }
            ast::Expr::Index(base, index, ipos) => {
                if let ast::Expr::Name(n, _) = base.as_ref() {
                    if let Some((slot, Type::List(inner))) = self.lookup_local(ctx, n) {
                        check_op(&inner)?;
                        let i = self.expect_type(ctx, index, &Type::Int)?;
                        let rhs = self.expect_type(ctx, value, &inner)?;
                        let current = Expr::Index { base: Box::new(Expr::Local(slot)), index: Box::new(i.clone()) };
                        return Ok(Stmt::SetLocalListItem { slot, index: i, value: combine(current, rhs) });
                    }
                    if self.lookup_local(ctx, n).is_none() {
                        if let Some(&var) = self.state_index.get(n) {
                            match self.states[var as usize].ty.clone() {
                                Type::Map(k, v) => {
                                    self.require_mutable(ctx, pos, "change state")?;
                                    check_op(&v)?;
                                    let key = self.expect_type(ctx, index, &k)?;
                                    let rhs = self.expect_type(ctx, value, &v)?;
                                    let current = Expr::MapGet { var, key: Box::new(key.clone()) };
                                    return Ok(Stmt::SetMap { var, key, value: combine(current, rhs) });
                                }
                                Type::List(inner) => {
                                    self.require_mutable(ctx, pos, "change state")?;
                                    check_op(&inner)?;
                                    let i = self.expect_type(ctx, index, &Type::Int)?;
                                    let rhs = self.expect_type(ctx, value, &inner)?;
                                    let current = Expr::StateListGet { var, index: Box::new(i.clone()) };
                                    return Ok(Stmt::SetStateListItem { var, index: i, value: combine(current, rhs) });
                                }
                                _ => {}
                            }
                        }
                    }
                }
                err(*ipos, "only items of lists and maps can be assigned with [ ]")
            }
            other => err(other.pos(), "cannot assign to this expression"),
        }
    }

    fn expr(&self, ctx: &mut FnCtx, e: &ast::Expr) -> CResult<(Expr, Type)> {
        let pos = e.pos();
        match e {
            ast::Expr::Int(v, _) => Ok((Expr::Const(Value::Int(*v)), Type::Int)),
            ast::Expr::Bool(b, _) => Ok((Expr::Const(Value::Bool(*b)), Type::Bool)),
            ast::Expr::Text(s, _) => {
                if s.len() > ops::MAX_VALUE_BYTES {
                    return err(pos, "text literal too long");
                }
                Ok((Expr::Const(Value::Text(s.clone())), Type::Text))
            }
            ast::Expr::Bytes(b, _) => Ok((Expr::Const(Value::Bytes(b.clone())), Type::Bytes)),
            ast::Expr::Name(n, _) => {
                if let Some((slot, t)) = self.lookup_local(ctx, n) {
                    return Ok((Expr::Local(slot), t));
                }
                if let Some((t, v)) = self.consts.get(n) {
                    return Ok((Expr::Const(v.clone()), t.clone()));
                }
                if let Some(&var) = self.state_index.get(n) {
                    let t = self.states[var as usize].ty.clone();
                    return match t {
                        Type::Map(_, _) => err(pos, format!("'{n}' is a map; use {n}[key] or {n}.has(key)")),
                        Type::List(_) => err(pos, format!("'{n}' is a state list; use {n}[i], len({n}) or 'for x in {n}'")),
                        _ => Ok((Expr::State(var), t)),
                    };
                }
                let ctxv = match n.as_str() {
                    "caller" => (Ctx::Caller, Type::Address),
                    "value" => (Ctx::Value, Type::Int),
                    "balance" => (Ctx::Balance, Type::Int),
                    "height" => (Ctx::Height, Type::Int),
                    "self" => (Ctx::SelfAddress, Type::Address),
                    _ => return err(pos, format!("unknown name '{n}'")),
                };
                if ctxv.0 == Ctx::Value && ctx.kind == FuncKind::View {
                    return err(pos, "'value' is not available in a view");
                }
                Ok((Expr::Ctx(ctxv.0), ctxv.1))
            }
            ast::Expr::List(items, _) => {
                if items.is_empty() {
                    return err(pos, "an empty list needs a known type, e.g. 'let xs: list[int] = []'");
                }
                let (first, t) = self.expr(ctx, &items[0])?;
                let mut out = vec![first];
                for it in &items[1..] {
                    out.push(self.expect_type(ctx, it, &t)?);
                }
                if out.len() > ops::MAX_LIST_LEN {
                    return err(pos, "list literal too long");
                }
                Ok((Expr::List(out), Type::List(Box::new(t))))
            }
            ast::Expr::Unary(UnOp::Neg, inner, _) => Ok((Expr::Neg(Box::new(self.expect_type(ctx, inner, &Type::Int)?)), Type::Int)),
            ast::Expr::Unary(UnOp::Not, inner, _) => Ok((Expr::Not(Box::new(self.expect_type(ctx, inner, &Type::Bool)?)), Type::Bool)),
            ast::Expr::Binary(op, l, r, _) => {
                let (lx, lt) = self.expr(ctx, l)?;
                let (rx, rt) = self.expr(ctx, r)?;
                let t = self.binary_type(*op, &lt, &rt, pos)?;
                Ok((Expr::Binary { op: *op, left: Box::new(lx), right: Box::new(rx) }, t))
            }
            ast::Expr::Index(base, index, _) => {
                if let ast::Expr::Name(n, _) = base.as_ref() {
                    if self.lookup_local(ctx, n).is_none() {
                        if let Some(&var) = self.state_index.get(n) {
                            match self.states[var as usize].ty.clone() {
                                Type::Map(k, v) => {
                                    let key = self.expect_type(ctx, index, &k)?;
                                    return Ok((Expr::MapGet { var, key: Box::new(key) }, *v));
                                }
                                Type::List(inner) => {
                                    let i = self.expect_type(ctx, index, &Type::Int)?;
                                    return Ok((Expr::StateListGet { var, index: Box::new(i) }, *inner));
                                }
                                t => return err(pos, format!("cannot index a {t}")),
                            }
                        }
                    }
                }
                let (b, bt) = self.expr(ctx, base)?;
                let i = self.expect_type(ctx, index, &Type::Int)?;
                match bt {
                    Type::List(inner) => Ok((Expr::Index { base: Box::new(b), index: Box::new(i) }, *inner)),
                    Type::Bytes => Ok((Expr::Index { base: Box::new(b), index: Box::new(i) }, Type::Int)),
                    t => err(pos, format!("cannot index a {t}")),
                }
            }
            ast::Expr::Method(base, m, args, _) => {
                if let ast::Expr::Name(n, _) = base.as_ref() {
                    if self.lookup_local(ctx, n).is_none() {
                        if let Some(&var) = self.state_index.get(n) {
                            let t = self.states[var as usize].ty.clone();
                            match (m.as_str(), &t, args.as_slice()) {
                                ("has", Type::Map(k, _), [key]) => {
                                    let kx = self.expect_type(ctx, key, k)?;
                                    return Ok((Expr::MapHas { var, key: Box::new(kx) }, Type::Bool));
                                }
                                ("len", Type::List(_), []) => return Ok((Expr::StateListLen { var }, Type::Int)),
                                ("pop", Type::List(inner), []) => {
                                    self.require_mutable(ctx, pos, "change state")?;
                                    return Ok((Expr::StateListPop { var }, (**inner).clone()));
                                }
                                _ => return err(pos, format!("'{n}' ({t}) has no method '{m}' with these arguments")),
                            }
                        }
                    }
                }
                let (b, bt) = self.expr(ctx, base)?;
                match (m.as_str(), &bt, args.is_empty()) {
                    ("len", Type::List(_) | Type::Text | Type::Bytes, true) => Ok((Expr::Builtin { f: Builtin::Len, args: vec![b] }, Type::Int)),
                    _ => err(pos, format!("{bt} has no method '{m}' usable here")),
                }
            }
            ast::Expr::Call(name, args, _) => self.call(ctx, name, args, pos),
        }
    }

    fn call(&self, ctx: &mut FnCtx, name: &str, args: &[ast::Expr], pos: Pos) -> CResult<(Expr, Type)> {
        let n = args.len();
        let arity = |want: usize| -> CResult<()> {
            if n != want {
                return err(pos, format!("{name}() takes {want} argument(s), {n} given"));
            }
            Ok(())
        };
        let b = |f: Builtin, a: Vec<Expr>| Expr::Builtin { f, args: a };
        match name {
            "address" => return Ok((Expr::Const(self.address_literal(args, pos)?), Type::Address)),
            "len" => {
                arity(1)?;
                if let ast::Expr::Name(v, _) = &args[0] {
                    if self.lookup_local(ctx, v).is_none() {
                        if let Some(&var) = self.state_index.get(v) {
                            if matches!(self.states[var as usize].ty, Type::List(_)) {
                                return Ok((Expr::StateListLen { var }, Type::Int));
                            }
                        }
                    }
                }
                let (x, t) = self.expr(ctx, &args[0])?;
                if !matches!(t, Type::List(_) | Type::Text | Type::Bytes) {
                    return err(pos, format!("len() needs a list, text or bytes, found {t}"));
                }
                return Ok((b(Builtin::Len, vec![x]), Type::Int));
            }
            "sha256" | "blake3" => {
                arity(1)?;
                let (x, t) = self.expr(ctx, &args[0])?;
                let x = match t {
                    Type::Bytes => x,
                    Type::Text => b(Builtin::BytesOfText, vec![x]),
                    other => return err(pos, format!("{name}() needs bytes or text, found {other}")),
                };
                return Ok((b(if name == "sha256" { Builtin::Sha256 } else { Builtin::Blake3 }, vec![x]), Type::Bytes));
            }
            "to_bytes" => {
                arity(1)?;
                let (x, t) = self.expr(ctx, &args[0])?;
                let f = match t {
                    Type::Int => Builtin::BytesOfInt,
                    Type::Address => Builtin::BytesOfAddress,
                    Type::Text => Builtin::BytesOfText,
                    Type::Bool => Builtin::BytesOfBool,
                    Type::Bytes => return Ok((x, Type::Bytes)),
                    other => return err(pos, format!("to_bytes() does not accept {other}")),
                };
                return Ok((b(f, vec![x]), Type::Bytes));
            }
            "to_text" => {
                arity(1)?;
                let x = self.expect_type(ctx, &args[0], &Type::Int)?;
                return Ok((b(Builtin::TextOfInt, vec![x]), Type::Text));
            }
            "to_int" => {
                arity(1)?;
                let x = self.expect_type(ctx, &args[0], &Type::Bytes)?;
                return Ok((b(Builtin::IntOfBytes, vec![x]), Type::Int));
            }
            "min" | "max" => {
                arity(2)?;
                let a = self.expect_type(ctx, &args[0], &Type::Int)?;
                let c = self.expect_type(ctx, &args[1], &Type::Int)?;
                return Ok((b(if name == "min" { Builtin::Min } else { Builtin::Max }, vec![a, c]), Type::Int));
            }
            "abs" => {
                arity(1)?;
                let a = self.expect_type(ctx, &args[0], &Type::Int)?;
                return Ok((b(Builtin::Abs, vec![a]), Type::Int));
            }
            "slice" => {
                arity(3)?;
                let x = self.expect_type(ctx, &args[0], &Type::Bytes)?;
                let s = self.expect_type(ctx, &args[1], &Type::Int)?;
                let e = self.expect_type(ctx, &args[2], &Type::Int)?;
                return Ok((b(Builtin::Slice, vec![x, s, e]), Type::Bytes));
            }
            "verify_ed25519" => {
                arity(3)?;
                let mut a = Vec::new();
                for x in args {
                    a.push(self.expect_type(ctx, x, &Type::Bytes)?);
                }
                return Ok((b(Builtin::VerifyEd25519, a), Type::Bool));
            }
            "ring_verify" => {
                arity(4)?;
                let ring = self.expect_type(ctx, &args[0], &Type::List(Box::new(Type::Bytes)))?;
                let mut a = vec![ring];
                for x in &args[1..] {
                    a.push(self.expect_type(ctx, x, &Type::Bytes)?);
                }
                return Ok((b(Builtin::RingVerify, a), Type::Bool));
            }
            "address_of" => {
                arity(1)?;
                let x = self.expect_type(ctx, &args[0], &Type::Bytes)?;
                return Ok((b(Builtin::AddressOfKey, vec![x]), Type::Address));
            }
            "zero_address" => {
                arity(0)?;
                return Ok((b(Builtin::ZeroAddress, vec![]), Type::Address));
            }
            "range" => return err(pos, "range() can only be used in 'for i in range(start, end)'"),
            _ => {}
        }
        let Some(&idx) = self.fn_index.get(name) else {
            return err(pos, format!("unknown function '{name}'"));
        };
        let sig = &self.sigs[idx as usize];
        if sig.kind != FuncKind::Fn {
            return err(pos, format!("'{name}' is an entry point; only 'fn' helpers can be called from code"));
        }
        if sig.params.len() != n {
            return err(pos, format!("{name}() takes {} argument(s), {n} given", sig.params.len()));
        }
        let mut out = Vec::new();
        for (a, t) in args.iter().zip(sig.params.iter()) {
            out.push(self.expect_type(ctx, a, t)?);
        }
        ctx.calls.insert(idx);
        Ok((Expr::Call { func: idx, args: out }, sig.ret.clone()))
    }
}

fn always_returns(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If { then, els, .. }) => always_returns(then) && always_returns(els),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Program {
        compile(src, &CompileOptions::default()).unwrap_or_else(|e| panic!("{e}\n{src}"))
    }

    fn fails(src: &str, needle: &str) {
        match compile(src, &CompileOptions::default()) {
            Ok(_) => panic!("expected error containing '{needle}'"),
            Err(e) => assert!(e.message.contains(needle), "error '{}' does not contain '{needle}'", e.message),
        }
    }

    #[test]
    fn compiles_valid_contract() {
        let p = ok(r#"
contract Bank
const MIN: int = 2 * TCN
state balances: map[address, int]
state owners: list[address]
state count: int = 5
event Deposit(who: address, amount: int)

action deposit() payable:
    require value >= MIN, "too small"
    balances[caller] += value
    owners.push(caller)
    emit Deposit(caller, value)

action withdraw(amount: int):
    require balances[caller] >= amount, "not enough"
    balances[caller] -= amount
    send(caller, amount)

view total() -> int:
    let sum: int = 0
    for who in owners:
        sum += balances[who]
    return sum

view doubled(x: int) -> int:
    return twice(x)

fn twice(x: int) -> int:
    if x > 0:
        return x * 2
    else:
        return 0
"#);
        assert_eq!(p.functions.len(), 5);
        assert!(p.functions.iter().find(|f| f.name == "deposit").unwrap().mutates);
        assert!(!p.functions.iter().find(|f| f.name == "twice").unwrap().mutates);
        assert_eq!(p.abi().len(), 4);
    }

    #[test]
    fn rejects_bad_contracts() {
        fails("contract A\nview f() -> int:\n    let x: int = true\n    return x\n", "expected int");
        fails("contract A\nview f() -> int:\n    if true:\n        return 1\n", "every path");
        fails("contract A\nstate n: int\nview f() -> int:\n    n = 2\n    return n\n", "view cannot");
        fails("contract A\nstate n: int\nview f() -> int:\n    return g()\nfn g() -> int:\n    n = 1\n    return n\n", "changes state");
        fails("contract A\nview f() -> int:\n    return y\n", "unknown name");
        fails("contract A\nview f(caller: int) -> int:\n    return 1\n", "reserved");
        fails("contract A\naction f():\n    let m: map[int, int] = 1\n", "maps can only");
        fails("contract A\nconst C: int = 1\naction f():\n    C = 2\n", "constant");
        fails("contract A\naction f():\n    break\n", "outside of a loop");
        fails("contract A\nview f() -> bool:\n    return 1 == \"a\"\n", "cannot compare");
        fails("contract A\nview f() payable -> int:\n    return 1\n", "expected ':'");
        fails("contract A\naction f():\n    1 + 2\n", "does nothing");
        fails("contract A\naction f():\n    g()\naction g():\n    pass\n", "entry point");
        fails("contract A\nconst X: address = address(\"tc1notvalid\")\naction f():\n    pass\n", "invalid address");
    }
}
