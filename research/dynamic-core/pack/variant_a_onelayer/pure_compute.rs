//! Variant A (1 layer) — pure_compute. Kernel + payload statically linked into one
//! artifact. NOTE (spec §1.3): even variant A here routes through the Kernel table,
//! so its size is an UPPER BOUND on true 1-layer cost (a real 1-layer build could
//! inline the primitives and drop the indirection).
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel;
#[path = "../../payloads/pure_compute/logic.rs"]
mod payload;

use abi::Kernel;

#[no_mangle]
pub extern "sysv64" fn agent_main(k: *const Kernel) -> ! {
    payload::run(unsafe { &*k })
}
