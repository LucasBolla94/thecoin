//! Errors of the compiler and the virtual machine.

use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// A compile error with the source position where it was detected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {pos}: {message}")]
pub struct CompileError {
    pub pos: Pos,
    pub message: String,
}

impl CompileError {
    pub fn new(pos: Pos, message: impl Into<String>) -> Self {
        CompileError { pos, message: message.into() }
    }
}

/// Runtime errors. Any error aborts the call and reverts every change it made.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VmError {
    #[error("out of fuel")]
    OutOfFuel,
    #[error("requirement failed: {0}")]
    Require(String),
    #[error("integer overflow")]
    Overflow,
    #[error("division by zero")]
    DivisionByZero,
    #[error("index {index} out of bounds (length {len})")]
    IndexOutOfBounds { index: i128, len: u64 },
    #[error("value too large")]
    TooLarge,
    #[error("call depth limit reached")]
    CallDepth,
    #[error("unknown function '{0}'")]
    UnknownFunction(String),
    #[error("function '{0}' cannot be called this way")]
    NotCallable(String),
    #[error("wrong arguments: {0}")]
    BadArguments(String),
    #[error("function does not accept TCN (not payable)")]
    NotPayable,
    #[error("state cannot be modified in a view")]
    ReadOnly,
    #[error("invalid amount")]
    BadAmount,
    #[error("insufficient contract balance")]
    InsufficientBalance,
    #[error("contract cannot be destroyed while it still has storage ({0} entries)")]
    StorageNotEmpty(u64),
    #[error("host error: {0}")]
    Host(String),
    #[error("internal type error: {0}")]
    Type(String),
}
