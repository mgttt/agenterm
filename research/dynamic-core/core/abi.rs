//! Dynamic-core ABI: the ONLY contract shared between the kernel and a payload.
//!
//! This file is included by BOTH the kernel and every payload (variant B blobs).
//! It defines the primitive table (the four primitives of the kernel) and nothing
//! else. It deliberately contains NO platform semantics (no open(), no file model).
//!
//! Calling convention across the boundary is fixed to `sysv64` on every OS so that
//! one payload blob is ABI-identical regardless of host. The kernel's `call`
//! primitive (④) bridges sysv64 -> the platform's native convention when invoking
//! OS functions, so the payload never needs to know the host convention.

#![allow(dead_code)]

use core::panic::PanicInfo;

/// The dynamic core. Exactly four primitives (plus process-exit bootstrap).
///
/// ① memory : `mem_alloc` (reserve+commit RW) / `mem_protect` (RW <-> RX)
/// ② jump   : realized by the payload via mem_protect(RX) + an indirect call,
///            and by the kernel jumping into the loaded payload. Not a table slot.
/// ③ reach  : `raw_syscall` (Linux path) + `sym` (symbol resolution / Windows path)
/// ④ call   : `call` — invoke any native address from a data description of args
#[repr(C)]
pub struct Kernel {
    // ① memory
    pub mem_alloc: extern "sysv64" fn(size: usize) -> *mut u8,
    pub mem_protect: extern "sysv64" fn(ptr: *mut u8, size: usize, exec: bool),
    // ③ reachability
    pub raw_syscall:
        extern "sysv64" fn(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize,
    pub sym: extern "sysv64" fn(module: *const u8, name: *const u8) -> *mut u8,
    // ④ call (libffi model: description = arg count of word-sized int/ptr args)
    pub call: extern "sysv64" fn(addr: *mut u8, nargs: usize, args: *const usize) -> usize,
    // process control (bootstrap convenience; part of ② domain)
    pub exit: extern "sysv64" fn(code: usize) -> !,
}

// Single panic handler for the whole crate graph (kernel binaries AND payload blobs).
#[panic_handler]
fn dc_panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
