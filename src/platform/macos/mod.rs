//! macOS native platform adapter (`prd/PRD_02_20_native_platform.md`).
//!
//! Ownership: macOS agent only. Shared contracts remain primary-owned.
//!
//! This module stays undeclared until primary freezes `src/platform/mod.rs`.
//! The private helpers below preserve Apple-native behavior without inventing
//! shared event or capability types.
//!
//! Contract revision implemented by this scaffold: none yet.

#![cfg(target_os = "macos")]

pub(crate) mod input;
pub(crate) mod scale;
pub(crate) mod toolbar;
