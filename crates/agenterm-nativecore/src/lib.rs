//! agenterm-nativecore — dynacore's actual body: an interpreter that reaches
//! real native OS APIs directly, with no compiler in the loop and no
//! executable memory ever requested. See `plan/design-dynacore-native-core.md`
//! for the full design and `research/dynamic-core/assembled/` (Q22) for the
//! prior research this crate revives with a produce-time contract-arity fix
//! (`verify.rs`'s F1 fix) and a bake-and-detect layout self-check
//! (`declare.rs`, Q13's pattern) added from day one.
//!
//! Deliberately NOT a member of the root workspace and NOT depended on by
//! the `agenterm` product crate (see `Cargo.toml`'s `[workspace]` table and
//! the design doc §5 acceptance criterion 1) — this crate proves the
//! mechanism stands on its own; wiring it into the product is a later round.
//!
//! x86_64/Windows only (design doc §3). Most of this crate's real
//! functionality (`declare`, `seam`, and the native-call parts of
//! `eval_core`) is `#[cfg(windows)]`-gated at the FFI boundary; `ir`,
//! `verify`, `pack`, and `store` are host-independent and compile/test on
//! any target, but this crate is only ever RUN for real on Windows.

pub mod declare;
pub mod eval_core;
pub mod ir;
pub mod pack;
pub mod payloads;
pub mod seam;
pub mod step_table;
pub mod store;
pub mod verify;
