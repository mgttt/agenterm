//! agenterm-sql — the FOURTH script-engine backend scaffold, alongside
//! `agenterm-rh` (`crates/agenterm-rh`), `agenterm-lua` (`crates/agenterm-lua`),
//! and `agenterm-qjs` (`crates/agenterm-qjs`). Wired into the root Cargo
//! workspace (see root `Cargo.toml`'s `[[bin]] agenterm-sql` entry, and
//! `src/script_backend.rs`/`src/script_engine.rs` for the
//! `ScriptBackend::Sql`/`SqlEngineBackend` wiring). `plan/design-script-engine-trait.md`
//! §2.6 "第四个后端（sql）需要实现的最小方法集" is the SSOT that scoped this
//! crate before any code existed here; `plan/plan-v0.1.16.md` §1 Rh and
//! `agenterm-script-common`'s own module doc both flagged sql as "user has
//! mentioned it, not started, recorded so nobody collides with the name" —
//! this crate is that "not started" becoming "started, honestly partial".
//!
//! ## Benchmark targets
//!
//! The eventual REAL implementation of this backend is benchmarked against
//! **SQL-92** and **PostgreSQL** — i.e. "does this accept what a
//! SQL-92-conforming or PostgreSQL-flavored query would accept, and reject
//! what neither would". Nothing in this crate today validates against the
//! actual SQL-92 standard text; `check()`'s dialect choice (see `check.rs`'s
//! module doc) is a pragmatic single-dialect approximation of that target,
//! not the target itself.
//!
//! ## What's real vs. placeholder in this pass
//!
//! | Surface | Status |
//! |---|---|
//! | [`check`] | **Real.** `sqlparser` actually parses `source`; syntax errors are real syntax errors, not simulated. |
//! | [`check_many`]/[`corpus_scan`] | **Real**, thin adapters over `agenterm_script_common`'s shared drivers (same drivers rh/lua/qjs use) — no hand-rolled manifest/report logic here. |
//! | [`eval`]/`execute` (see `main.rs`'s CLI stubs for `eval`/`run`/`pack`/`qualify`/`task`) | **Placeholder — fail-closed, not fake.** Every execute-shaped surface returns a typed "not implemented" error explaining exactly why (see `eval.rs`'s module doc), never a silently-wrong success. |
//!
//! ## Open design question (deliberately unresolved, not overlooked)
//!
//! What does `eval`/`execute`ing a `.sql` source actually *run against*?
//! Three genuinely different answers exist and none has been chosen:
//!
//! 1. **An embedded engine** (e.g. an in-process SQLite or DataFusion),
//!    giving sql source its own private, ephemeral database — closest to
//!    how `agenterm-lua`/`agenterm-qjs` each run in their own private VM.
//! 2. **A connection to an external PostgreSQL-compatible database** —
//!    closest to "real" SQL usage, but introduces a network/process
//!    dependency and a credentials/connection-string story none of the
//!    other three engines need.
//! 3. **The host's own state exposed as virtual tables** (fleet inventory,
//!    tab state, script manifests, ... queried *as SQL*) — the most
//!    "agenterm-native" option (SQL as a query language *over* the fleet
//!    facade, not a general-purpose database), and also the furthest from
//!    a solved problem: it would need a whole virtual-table/FDW-shaped
//!    layer that doesn't exist anywhere in this codebase yet.
//!
//! Each implies a different `ScriptInvocationResult.value` shape, a
//! different (or absent) role for `fleet_bridge`, and a different set of
//! new dependencies. Picking one is out of scope for this scaffolding pass;
//! [`eval::eval_entry`] and `src/script_engine.rs`'s `SqlEngineBackend::execute`
//! both fail closed with an error naming this exact open question, rather
//! than defaulting to any one answer silently. See
//! `plan/design-script-engine-trait.md` §2.6 for the design-time
//! prediction this scaffold is validating (or not) against reality — this
//! module doc's "what's real vs. placeholder" table above is the honest
//! answer, reported back regardless of which way it cut.

pub mod check;
pub mod check_many;
pub mod corpus_scan;
pub mod error;
pub mod eval;

pub use check::check;
pub use check_many::{
    CheckManyManifest, CheckManyOptions, CheckManyReport, ParsedCheckManyCli, parse_check_many_cli,
    read_manifest, run_check_many,
};
pub use corpus_scan::{CorpusScanReport, FailedFile as CorpusScanFailedFile, scan_directory};
pub use agenterm_script_common::cli::{find_flag_value, has_flag, positional, require_flag_value};
pub use error::SqlError;
pub use eval::eval_entry;

pub const SQL_VERSION: &str = env!("CARGO_PKG_VERSION");
