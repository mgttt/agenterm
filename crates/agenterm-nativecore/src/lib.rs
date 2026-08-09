//! agenterm-nativecore — **ARCHIVED (2026-08-09), see `plan/design-dynacore-native-core.md`'s
//! header.** An interpreter that reaches real native OS APIs directly, with no compiler in the
//! loop and no executable memory ever requested. This crate is NOT "dynacore" — that name
//! belongs permanently to `crates/agenterm-dynacore` (a settled decision, not a placeholder;
//! the two crates are unrelated, do not share IR/Op definitions, and neither depends on the
//! other). Code here stays (tested, opt-in, zero cost when unconfigured) but receives no
//! further investment. See `plan/design-dynacore-native-core.md` for the full design and
//! `research/dynamic-core/assembled/` (Q22) for the prior research this crate revives with a
//! produce-time contract-arity fix (`verify.rs`'s F1 fix) and a bake-and-detect layout
//! self-check (`declare.rs`, Q13's pattern) added from day one.
//!
//! Joined the root workspace and the `agenterm` product crate's dependency
//! graph in the product-integration round (design doc §7) — see
//! `src/script_nativecore_pack.rs`/`src/script_backend.rs`'s
//! `try_execute_nativecore_pack_invocation` for the product entry points.
//!
//! x86_64/Windows only (design doc §3). Most of this crate's real
//! functionality (`declare`, `seam`, and the native-call parts of
//! `eval_core`) is `#[cfg(windows)]`-gated at the FFI boundary; `ir`,
//! `verify`, `pack`, and `store` are host-independent and compile/test on
//! any target, but this crate is only ever RUN for real on Windows.
//!
//! **v2 (design doc §9):** `registry` adds a second, additional call path —
//! a small, human-reviewed, compiled-in table of native symbols a pack may
//! reference BY NAME (not one of the seven compile-time `Intent`s in `ir`).
//! `verify` and `seam` grow a parallel registry-backed check/dispatch
//! alongside (not instead of) the seven-intent path; see `registry`'s own
//! header for the exact scope and what this deliberately does not do.

pub mod declare;
pub mod eval_core;
pub mod ir;
pub mod pack;
pub mod payloads;
pub mod registry;
pub mod seam;
pub mod step_table;
pub mod store;
pub mod verify;
