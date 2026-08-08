//! Byte measurement — the VERIFIER body, measured with the SAME method as Q9's interp
//! (`rustc -O --crate-type=lib --emit=obj` → `llvm-size .text`) so the number is comparable
//! to Q9's eval-core (1908 B) and to Q4's guard LOC. The IR types are data-only; keeping
//! `verify` crate-public keeps only its `.text` (Builder etc. are dead-code eliminated).

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "verify.rs"]
mod verify;

pub use verify::verify;
