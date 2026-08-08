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
//! What's unified here: `check_many` (manifest/report/options/driver loop)
//! and `corpus_scan` (directory-walk-and-check). What's deliberately NOT
//! unified: each engine's actual syntax/semantic checker (different
//! signatures, different notions of "project root" support), pack/qualify
//! (rh's native-codegen pack has no bytecode-fingerprint analog to lua/qjs
//! interpreted bytecode), and CLI argv parsing (small enough per-engine
//! that forcing a shared error type isn't worth the indirection).
//!
//! Future backends (a `sql` engine has been mentioned as a later addition)
//! should be able to adopt `check_many`/`corpus_scan` the same way qjs did
//! from day one, rather than re-deriving the manifest contract from rh's
//! source a fourth time.

pub mod check_many;
pub mod cli;
pub mod corpus_scan;
pub mod hex;
