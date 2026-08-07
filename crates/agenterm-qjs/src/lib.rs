//! agenterm-qjs — QuickJS-backed script engine, capability-aligned with
//! `agenterm-rh` (see `crates/agenterm-rh`) and, for the host-function
//! boundary specifically, with `agenterm-lua` (see `host.rs`). Wired into
//! the root Cargo workspace (see root `Cargo.toml`'s `[[bin]] agenterm-qjs`
//! entry); `plan/plan-v0.1.16.md` §1 "Rh. 脚本引擎矩阵" /
//! `prd/PRD_02_10_rhai_scripting.md` "Script engine family" are the SSOT
//! for the parity contract and tracked open risks.
//!
//! "Capability alignment" scope:
//! - QJS-M1 (shipped): same CLI verb shape, same typed JSON/exit-code
//!   envelope as `agenterm-rh` for `check`/`eval`/`check-many`.
//! - QJS-M2 (in progress): `__host.fleet_call`/`args_len`/`arg` bound the
//!   same way `agenterm_lua::LuaHostFunctions` binds them, so
//!   `scripts/qjs/lib/fleet.js` can port `scripts/lua/lib/fleet.lua`
//!   near-line-for-line rather than re-deriving the L2 facade shape.
//!   `agenterm::script_backend` wiring (the `try_execute_*_invocation`
//!   family) and `task`/`run`/`pack`/`qualify` verbs are not yet done.
//! - Explicitly NOT in scope, ever: AOT/native codegen (rh-specific).

pub mod check;
pub mod check_many;
pub mod error;
pub mod eval;
pub mod host;

pub use check::check;
pub use check_many::{
    CheckManyManifest, CheckManyOptions, CheckManyReport, ParsedCheckManyCli, parse_check_many_cli,
    read_manifest, run_check_many,
};
pub use error::QjsError;
pub use eval::{EvalOutcome, eval_entry, eval_entry_with_host};
pub use host::QjsHostFunctions;

pub const QJS_VERSION: &str = env!("CARGO_PKG_VERSION");
