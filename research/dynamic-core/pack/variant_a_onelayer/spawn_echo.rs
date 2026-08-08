//! Variant A (1 layer) — spawn_echo. Kernel + spawn adapter + payload statically
//! linked into one artifact. Measures the "+1 capability" cost (criterion ④) on
//! the 1-layer side. See §1.3 upper-bound note.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel;
#[cfg(dc_os = "linux")]
#[path = "../../adapters/linux/spawn.rs"]
mod adapter;
#[cfg(dc_os = "windows")]
#[path = "../../adapters/windows/spawn.rs"]
mod adapter;
#[path = "../../payloads/spawn_echo/logic.rs"]
mod payload;

use abi::Kernel;

#[no_mangle]
pub extern "sysv64" fn agent_main(k: *const Kernel) -> ! {
    payload::run(unsafe { &*k })
}
