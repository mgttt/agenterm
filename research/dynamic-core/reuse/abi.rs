//! Reuse experiment — the Kernel primitive table (same four-primitive ABI as Q0).
//! Included by BOTH the loader and every blob (adapters + payloads). Contains no
//! platform semantics: only the four primitives. Calling convention is fixed to
//! sysv64 on every OS so one blob is ABI-identical across hosts.
#![allow(dead_code)]

use core::panic::PanicInfo;

/// The dynamic core. Exactly four primitives (plus process-exit bootstrap).
#[repr(C)]
pub struct Kernel {
    // ① memory
    pub mem_alloc: extern "sysv64" fn(size: usize) -> *mut u8,
    pub mem_protect: extern "sysv64" fn(ptr: *mut u8, size: usize, exec: bool),
    // ③ reachability
    pub raw_syscall: extern "sysv64" fn(
        n: usize,
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        e: usize,
        f: usize,
    ) -> isize,
    pub sym: extern "sysv64" fn(module: *const u8, name: *const u8) -> *mut u8,
    // ④ call
    pub call: extern "sysv64" fn(addr: *mut u8, nargs: usize, args: *const usize) -> usize,
    // process control (bootstrap; part of ② domain)
    pub exit: extern "sysv64" fn(code: usize) -> !,
}

// Single panic handler for each crate graph (loader binary AND every blob).
#[panic_handler]
fn dc_panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
