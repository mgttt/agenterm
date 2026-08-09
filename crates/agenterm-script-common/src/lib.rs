//! Engine-agnostic scaffolding shared by agenterm's script backends.
//!
//! `crates/agenterm-rh`, `crates/agenterm-lua`, and `crates/agenterm-qjs`
//! each independently arrived at the same manifest shape, report shape,
//! failure taxonomy, and exit-code mapping for `check-many` (lua's and
//! qjs's own doc comments say "aligned with agenterm-rh" — this wasn't a
//! coincidence, it's a deliberate cross-engine contract described in
//! `plan/plan-v0.1.16.md` §1 Rh: "同一套 L2 facade / catalog ... 引擎只换
//! L3 执行后端"). Until now that alignment was maintained by hand — three
//! ~300-600 line files kept in sync by copy-paste-and-compare. This crate
//! makes it structural: one implementation, three thin per-engine adapters
//! that plug in their own checker function and manifest `kind` label.
//!
//! What's unified here: `check_many` (manifest/report/options/driver loop),
//! `corpus_scan` (directory-walk-and-check), `cli` (the shared `check-many`
//! argv parser, slice-based flag helpers, and — for qjs/sql — the whole
//! `check-many`/`corpus-scan` command bodies; an earlier version of this
//! doc predicted CLI parsing was "small enough per-engine that forcing a
//! shared error type isn't worth the indirection", which stopped being true
//! the moment three byte-identical parsers needed a fourth copy), and
//! `test_support` (engine-side contract tests, `test-support` feature).
//! What's deliberately NOT unified: each engine's actual syntax/semantic
//! checker (different signatures, different notions of "project root"
//! support), and pack/qualify (rh's native-codegen pack has no
//! bytecode-fingerprint analog to lua/qjs interpreted bytecode).
//!
//! Future backends (a `sql` engine has been mentioned as a later addition)
//! should be able to adopt `check_many`/`corpus_scan` the same way qjs did
//! from day one, rather than re-deriving the manifest contract from rh's
//! source a fourth time.

pub mod check_many;
pub mod cli;
pub mod corpus_scan;
pub mod hex;
pub mod pack_support;
#[cfg(feature = "test-support")]
pub mod test_support;
