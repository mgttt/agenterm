//! AgenTerm endpoint and workspace policy for macOS.

#[path = "../linux/ipc.rs"]
mod unix_policy;

pub(crate) use unix_policy::*;
