//! Syntax tree produced by the parser (before type checking).

use crate::error::Pos;

#[derive(Clone, Debug, PartialEq)]
pub enum TypeExpr {
    Named(String, Pos),
    List(Box<TypeExpr>, Pos),
    Map(Box<TypeExpr>, Box<TypeExpr>, Pos),
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub name: String,
    pub pos: Pos,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Const { name: String, ty: TypeExpr, value: Expr, pos: Pos },
    State { name: String, ty: TypeExpr, init: Option<Expr>, pos: Pos },
    Event { name: String, fields: Vec<Param>, pos: Pos },
    Func(FuncDecl),
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub pos: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuncKind {
    Init,
    Action,
    View,
    Fn,
}

#[derive(Clone, Debug)]
pub struct FuncDecl {
    pub kind: FuncKind,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub payable: bool,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let { name: String, ty: TypeExpr, value: Expr, pos: Pos },
    Assign { target: Expr, op: AssignOp, value: Expr, pos: Pos },
    If { branches: Vec<(Expr, Vec<Stmt>)>, els: Option<Vec<Stmt>>, pos: Pos },
    While { cond: Expr, body: Vec<Stmt>, pos: Pos },
    ForRange { var: String, start: Expr, end: Expr, body: Vec<Stmt>, pos: Pos },
    ForEach { var: String, iter: Expr, body: Vec<Stmt>, pos: Pos },
    Break(Pos),
    Continue(Pos),
    Return(Option<Expr>, Pos),
    Require(Expr, Option<Expr>, Pos),
    Send(Expr, Expr, Pos),
    Emit(String, Vec<Expr>, Pos),
    Destroy(Expr, Pos),
    Pass(Pos),
    Expr(Expr, Pos),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i128, Pos),
    Bool(bool, Pos),
    Text(String, Pos),
    Bytes(Vec<u8>, Pos),
    Name(String, Pos),
    List(Vec<Expr>, Pos),
    Unary(UnOp, Box<Expr>, Pos),
    Binary(BinOp, Box<Expr>, Box<Expr>, Pos),
    Call(String, Vec<Expr>, Pos),
    Method(Box<Expr>, String, Vec<Expr>, Pos),
    Index(Box<Expr>, Box<Expr>, Pos),
}

impl Expr {
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Int(_, p)
            | Expr::Bool(_, p)
            | Expr::Text(_, p)
            | Expr::Bytes(_, p)
            | Expr::Name(_, p)
            | Expr::List(_, p)
            | Expr::Unary(_, _, p)
            | Expr::Binary(_, _, _, p)
            | Expr::Call(_, _, p)
            | Expr::Method(_, _, _, p)
            | Expr::Index(_, _, p) => *p,
        }
    }
}
