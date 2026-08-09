//! dynacore — v1 skeleton for `plan/design-dynacore-logic-pack.md`.
//!
//! An interpret-only neutral IR, a produce-time well-formedness verification
//! gate, and a content-addressed pack store, whose only host-call primitive
//! is `fleet_call(operation_id, params_json)`. This crate is a mechanism
//! crate with no AgenTerm product name (same posture as
//! `crates/agenterm-platform`): it does not depend on the root `agenterm`
//! crate's `OPERATION_CATALOG` directly — `verify::OperationSchema` is a
//! generic mirror of it, built once by the product-side host binding
//! (`src/script_dynacore_host.rs`) and passed in as data.
//!
//! Ported from `research/dynamic-core/assembled/` (Q22's assembly of
//! Q1/Q3/Q7/Q9/Q18/Q19/Q21) per the design doc's §2 non-goals: no codegen/JIT
//! backend (`eval_core.rs` is the only execution path — `ir/`, `lowering/`,
//! `isa/` were not read, let alone ported), no native OS call surface (every
//! Win32-bound `Intent` variant Q22 assembled — `Alloc`/`FileOpen`/
//! `FileRead`/`FileClose`/`WriteStdout`/`SpawnWait`/`FileWrite` — is cut and
//! replaced by the single `FleetCall` intent), and no runtime name->hash
//! discovery service (`store::Store` only ever takes a hash the caller
//! already has; `pack::PackManifest` is where that hash is recorded).

pub mod eval_core;
pub mod ir;
pub mod pack;
pub mod store;
pub mod verify;
