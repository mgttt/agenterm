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
//! ## What's real vs. placeholder in this pass (M1, see
//! `plan/design-sql-execution-target.md`)
//!
//! | Surface | Status |
//! |---|---|
//! | [`check`] | **Real.** `sqlparser` actually parses `source`; syntax errors are real syntax errors, not simulated. |
//! | [`check_many`]/[`corpus_scan`] | **Real**, thin adapters over `agenterm_script_common`'s shared drivers (same drivers rh/lua/qjs use) — no hand-rolled manifest/report logic here. |
//! | [`eval::execute_entry`] (CLI: `eval`/`run`) | **Real as of M1.** A private, in-process, ephemeral (`:memory:`) `rusqlite`/SQLite database per call; every `;`-separated statement in `source` runs against it in order; the last result-producing statement's rows are returned as JSON. See `eval.rs`'s module doc for the full value-mapping table and the known `check()`-parses-PostgreSQL-`execute()`-runs-SQLite dialect skew. |
//! | CLI `pack`/`qualify`/`task` | **Still placeholder — fail-closed, not fake.** M1's scope is `eval`/`run` only (design doc §5); these three remain honest "not implemented" stubs, exit `2`. |
//!
//! ## Execution target: DECIDED as of M1 (was an open question pre-M1)
//!
//! `plan/design-sql-execution-target.md` (design doc, rev1) evaluated three
//! candidates — (a) an embedded engine, (b) a connection to an external
//! PostgreSQL-compatible database, (c) the host's own state exposed as
//! virtual tables — and recommended (a), specifically `rusqlite` with the
//! `bundled` feature, with (c) deferred to M2 as an extension of (a) rather
//! than an independent implementation, and (b) not recommended at all. This
//! crate's `execute_entry` (see `eval.rs`) is that recommendation, built.
//! `fleet_bridge` remains unused in M1 (virtual tables are M2 — the design
//! doc's own prediction that "sql 大概率不用 fleet_bridge" held for this
//! phase, same as it did for the pre-M1 placeholder).

pub mod check;
pub mod check_many;
pub mod cli;
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
pub use eval::{ExecuteBudgets, ExecuteOutcome, execute_entry};

pub const SQL_VERSION: &str = env!("CARGO_PKG_VERSION");
