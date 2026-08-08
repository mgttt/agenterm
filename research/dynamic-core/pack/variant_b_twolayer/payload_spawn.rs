//! Variant B (2 layer) — spawn_echo payload blob. Compiled to a flat
//! position-independent image; the FROZEN kernel loads it (①) and jumps in (②).
//! Adding this capability in 2-layer = shipping one more blob file against the
//! unchanged kernel; programs that do not spawn never load it. Does NOT include
//! the kernel.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
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
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    payload::run(unsafe { &*k })
}
