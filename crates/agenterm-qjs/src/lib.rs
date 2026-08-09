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
//! - QJS-M2 (shipped): `__host.fleet_call`/`args_len`/`arg` bound the
//!   same way `agenterm_lua::LuaHostFunctions` binds them, so
//!   `scripts/qjs/lib/fleet.js` can port `scripts/lua/lib/fleet.lua`
//!   near-line-for-line rather than re-deriving the L2 facade shape;
//!   `agenterm::script_backend` wiring (the `try_execute_*_invocation`
//!   family) covers `check`/`eval`/`run` for the real worker/task dispatch
//!   path (`src/script_worker.rs`'s `execute_inner`).
//! - QJS-M3 (this module set): `pack`/`qualify` — see `compile.rs`/
//!   `pack.rs`/`qualify.rs`. `pack` captures a *real* QuickJS bytecode
//!   fingerprint (`Module::declare().write()`, rquickjs's only public
//!   bytecode surface) but execution still re-parses from stored source —
//!   see `pack.rs`'s module doc for exactly why that's not yet a full
//!   bytecode load+exec path, and what would be needed to close that gap.
//!   CLI `pack`/`qualify`/`run`/`hash` verbs live in `main.rs`.
//! - QJS-M4 (this module): `corpus_scan.rs` — batch `check()` over a
//!   directory tree of `.js`/`.mjs` files, same shape as
//!   `agenterm_lua::corpus_scan` (which is itself the shape rh's
//!   `corpus.rs`/`corpus-scan` verb established). CLI `corpus-scan`/
//!   `run-smoke` verbs live in `main.rs`.
//! - Explicitly NOT in scope, ever: AOT/native codegen (rh-specific) — so
//!   rh's `caller-inventory`/`compile`/`transpile`/`--worker`/
//!   `--internal-incremental-finalize` verbs (all native-codegen tooling)
//!   have no qjs equivalent and never will.
//! - Still open (not this pass, tracked): project-level multi-file
//!   `import` support in `check`/`eval`/`pack` (see `check.rs`'s "known
//!   gap" note — verified by hand this session to be a bigger gap than
//!   "unvalidated": `import` currently fails differently, and for
//!   different reasons, in `check` vs. `eval`/`run`/`pack`/`qualify`; see
//!   `plan/plan-v0.1.16.md` §1 Rh, QJS-M3 for the repro and the design
//!   question it raises); `--framed-worker` (lua ships one but nothing in
//!   the codebase spawns either engine's, so its value is unclear — not
//!   added here either, tracked not silently dropped); a `task` CLI verb
//!   — `agenterm-lua`'s is an informative stub pointing at the root
//!   `agenterm` binary's `task` command, and `agenterm-qjs`'s mirrors
//!   that, since real task dispatch already goes through
//!   `agenterm::script_backend` (root binary, verified end-to-end via a
//!   real `task run` this session — see plan doc), not through this
//!   crate's own binary.

pub mod check;
pub mod check_many;
pub mod cli;
pub mod compile;
pub mod corpus_scan;
pub mod error;
pub mod eval;
pub mod eval_module;
pub mod host;
pub mod manifest;
pub mod module_resolver;
pub mod module_sniff;
pub mod pack;
pub mod pack_module;
pub mod qualify;

pub use agenterm_script_common::cli::{find_flag_value, has_flag, positional, require_flag_value};
pub use check::{check, check_with_project_validation};
pub use check_many::{
    CheckManyManifest, CheckManyOptions, CheckManyReport, ParsedCheckManyCli, parse_check_many_cli,
    read_manifest, run_check_many,
};
pub use compile::{compile_qjs, hash_source};
pub use corpus_scan::{CorpusScanReport, FailedFile as CorpusScanFailedFile, scan_directory};
pub use error::QjsError;
pub use eval::{EvalOutcome, eval_entry, eval_entry_with_host};
pub use eval_module::eval_module_entry_with_host;
pub use host::QjsHostFunctions;
pub use manifest::QjsPackManifest;
pub use module_resolver::ProjectModuleResolver;
pub use module_sniff::wants_module_mode;
pub use pack::{QjsPack, build_pack_dir, is_pack_dir};
pub use pack_module::{
    QjsModulePack, QjsModulePackManifest, QjsModuleQualificationReceipt, build_module_pack_dir,
    discover_import_graph, qualify_module_pack_dir,
};
pub use qualify::{QjsQualificationReceipt, qualify_pack_dir};

pub const QJS_VERSION: &str = env!("CARGO_PKG_VERSION");
