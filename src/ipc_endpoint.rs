//! Compatibility export for the platform-neutral local-IPC contract.
//!
//! Product modules should migrate to `platform::contract::ipc`; this shim
//! preserves existing internal call sites while the facade migration lands.

pub(crate) use crate::platform::contract::ipc::*;
