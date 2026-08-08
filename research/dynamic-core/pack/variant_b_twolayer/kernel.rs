//! Variant B (2 layer) — the frozen kernel/loader. Its code is identical no matter
//! which payload runs; the payload blob is embedded via `env!("DC_BLOB")` purely as
//! a packaging choice (a file-loading loader would keep the blob external at the
//! cost of a few extra syscalls). The kernel-only size = binary size - blob size.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel;

// Entry (_start) and blob embedding live in kernel.rs under cfg(dc_variant="b").
