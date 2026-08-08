//! Variant A (1 layer) — read_hash_print. Kernel + Linux adapter + payload all
//! statically linked into one artifact. See §1.3 upper-bound note.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel;
#[path = "../../adapters/linux/readfile.rs"]
mod adapter;
#[path = "../../payloads/read_hash_print/logic.rs"]
mod payload;

use abi::Kernel;

#[no_mangle]
pub extern "sysv64" fn agent_main(k: *const Kernel) -> ! {
    payload::run(unsafe { &*k })
}
