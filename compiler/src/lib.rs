//! CoreX compiler.
//!
//! Pipeline: source -> lexer -> parser -> AST -> semantic analysis -> LLVM IR
//! (textual, LLVM 20) -> object file (llc) -> ELF executable (cc driver).
//!
//! Stage boundaries are strictly one-directional: every later stage consumes
//! only the output of the previous one. `driver` is the only module that talks
//! to external processes.

pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod driver;
pub mod lexer;
pub mod lineedit;
pub mod mangle;
pub mod parser;
pub mod repl;
pub mod semantic;
pub mod token;
