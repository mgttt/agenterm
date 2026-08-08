//! Variant B (2 layer) — read_hash_print payload blob (Linux adapter). Compiled to
//! a flat position-independent image; loaded and entered by the kernel. All OS
//! access is via the primitive table, so the blob is ABI-uniform across hosts;
//! only the adapter it links differs per OS.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../adapters/linux/readfile.rs"]
mod adapter;
#[path = "../../payloads/read_hash_print/logic.rs"]
mod payload;

use abi::Kernel;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    payload::run(unsafe { &*k })
}
