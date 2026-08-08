//! Variant B (2 layer) — pure_compute payload blob. Compiled to a flat,
//! position-independent code image; the kernel loads it (①) and jumps in (②),
//! passing the primitive table. Does NOT include the kernel.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../payloads/pure_compute/logic.rs"]
mod payload;

use abi::Kernel;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    payload::run(unsafe { &*k })
}
