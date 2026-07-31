//! Product-facing IPC facade service.
//!
//! The contract is OS-neutral; compile-time adapter choice is private to
//! [`crate::platform::selected`].

pub(crate) use super::services::ipc::*;
