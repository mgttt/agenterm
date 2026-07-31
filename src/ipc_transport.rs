//! Product-neutral IPC framing and server contract.
//!
//! Native endpoint transport mechanics live at `platform::ipc_transport_impl`;
//! this module deliberately exposes the stable product-facing contract only.

#[path = "platform/ipc_transport_impl.rs"]
mod implementation;

pub(crate) use implementation::*;
