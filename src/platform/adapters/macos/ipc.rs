//! macOS IPC adapter identity.
//!
//! The Unix mechanism is shared privately with Linux, while this adapter keeps
//! the macOS selection point explicit for capability reporting and future
//! Cocoa-specific integration.

#[path = "../linux/ipc.rs"]
mod unix_mechanism;

pub(crate) use unix_mechanism::*;
