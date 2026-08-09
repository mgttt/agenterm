//! agenterm-nativecore — dynacore's actual body: an interpreter that reaches
//! real native OS APIs directly, with no compiler in the loop and no
//! executable memory ever requested. See `plan/design-dynacore-native-core.md`
//! for the full design and `research/dynamic-core/assembled/` (Q22) for the
//! prior research this crate revives with a produce-time contract-arity fix
//! (`verify.rs`'s F1 fix) and a bake-and-detect layout self-check
//! (`declare.rs`, Q13's pattern) added from day one.
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

pub mod declare;
pub mod eval_core;
pub mod ir;
pub mod pack;
pub mod payloads;
pub mod seam;
pub mod step_table;
pub mod store;
pub mod verify;
