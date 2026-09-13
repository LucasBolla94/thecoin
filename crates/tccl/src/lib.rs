//! # TCCL — The Coin Cloud Language
//!
//! A small, statically typed, indentation-based language for smart contracts
//! on The Coin, and the deterministic virtual machine that runs them.
//!
//! ```text
//! contract Counter
//!
//! state count: int
//!
//! action increment(by: int):
//!     require by > 0, "by must be positive"
//!     count += by
//!
//! view get() -> int:
//!     return count
//! ```
//!
//! * [`compile`] — source → type-checked [`Program`] (rejects anything ambiguous)
//! * [`vm::execute`] — runs a program against a [`vm::Host`] with fuel metering
//! * [`ring`] — linkable ring signatures used by privacy contracts
//! * [`abi`] — argument parsing and JSON rendering
//! * [`sim`] — in-memory host for local testing (`tccl run`)
//!
//! The compiler is part of the consensus rules: a deployment transaction
//! carries source code, and every node compiles it identically.

pub mod abi;
pub mod ast;
pub mod checker;
pub mod error;
pub mod lexer;
pub mod ops;
pub mod parser;
pub mod program;
pub mod ring;
pub mod sim;
pub mod vm;

pub use checker::{compile, CompileOptions};
pub use error::{CompileError, VmError};
pub use program::{Program, Type, Value};
