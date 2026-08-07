//! agenterm-qjs — QuickJS-backed script engine, capability-aligned with
//! `agenterm-rh` (see `crates/agenterm-rh`). Standalone crate for now — see
//! the `[workspace]` header comment in Cargo.toml for why, and
//! `plan/plan-v0.1.16.md` §1 "Rh. 脚本引擎矩阵" / `prd/PRD_02_10_rhai_scripting.md`
//! "Script engine family" for the parity contract and tracked open risks.
//!
//! "Capability alignment" scope (QJS-M1): same CLI verb shape, same typed
//! JSON/exit-code envelope as `agenterm-rh` for `check`/`eval`/`check-many`.
//! Explicitly NOT in scope here: AOT/native codegen (rh-specific), L2
//! facade/fleet wiring (QJS-M2), root-workspace integration.

pub mod check;
pub mod check_many;
pub mod error;
pub mod eval;

pub use check::check;
pub use check_many::{
    CheckManyManifest, CheckManyOptions, CheckManyReport, ParsedCheckManyCli, parse_check_many_cli,
    read_manifest, run_check_many,
};
pub use error::QjsError;
pub use eval::{EvalOutcome, eval_entry};

pub const QJS_VERSION: &str = env!("CARGO_PKG_VERSION");
