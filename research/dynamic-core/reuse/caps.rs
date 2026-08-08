//! Reuse experiment — the capability table filled by a file-I/O ADAPTER blob and
//! consumed by a PAYLOAD blob. This is the reuse boundary under test: the payload
//! sees only these function pointers, never the adapter's code directly, so one
//! adapter blob can back many payloads.
//!
//! Every entry takes `*const Kernel` as its first arg, so the adapter functions are
//! stateless: the caller (payload) supplies the primitive table each call. That lets
//! the adapter blob and payload blob live in separate mappings.
#![allow(dead_code)]

use crate::abi::Kernel;

#[repr(C)]
pub struct FileCaps {
    pub read_file:
        extern "sysv64" fn(k: *const Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize,
    pub write_stdout: extern "sysv64" fn(k: *const Kernel, buf: *const u8, len: usize),
}
