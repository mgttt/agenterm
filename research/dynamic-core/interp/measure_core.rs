//! J2 byte measurement — the INTERPRETER body.
//!
//! Compiled as a lib to an object; `.text` is extracted with rust-objcopy and its size read.
//!   * default build            -> `.text` = eval-core + OS seam (do_intent) = full interpreter
//!   * `--cfg interp_measure_core` -> do_intent collapses to `unreachable`, so `.text` ~= eval-core
//!     alone. seam bytes = full - core.
//! No Cargo, no workspace. See RESULTS.md for the exact commands.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "interp.rs"]
mod interp;

// keep `run` (and thus eval_op + do_intent) reachable so codegen emits it into the object.
pub use interp::run;
