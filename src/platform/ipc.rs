//! Product-facing IPC facade service.
//!
//! The contract is OS-neutral; compile-time adapter choice is private to
//! the public `agenterm-platform` transport facade.

pub(crate) use super::services::ipc::*;
