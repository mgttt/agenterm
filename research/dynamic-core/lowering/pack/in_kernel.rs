//! Packaging: LOWERER IN THE KERNEL (Q3 §1.3). Variant-A single product =
//! kernel primitives + lowerer + runner + Q0 adapters, all statically linked.
//! The payload ships as IR bytes (env!("DC_IR")). TCB = this whole binary (④).
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel; // primitives + _start/mainCRTStartup + memcpy/memset
#[cfg(dc_os = "linux")]
#[path = "../../adapters/linux/readfile.rs"]
mod fio;
#[cfg(dc_os = "windows")]
#[path = "../../adapters/windows/readfile.rs"]
mod fio;
#[cfg(dc_os = "linux")]
#[path = "../../adapters/linux/spawn.rs"]
mod proc;
#[cfg(dc_os = "windows")]
#[path = "../../adapters/windows/spawn.rs"]
mod proc;
#[path = "../ir.rs"]
mod ir;
#[path = "../lower.rs"]
mod lower;
#[path = "../runner.rs"]
mod runner;

use abi::Kernel;

#[no_mangle]
pub extern "sysv64" fn agent_main(k: *const Kernel) -> ! {
    let ir: &[u8] = include_bytes!(env!("DC_IR"));
    runner::run(unsafe { &*k }, ir)
}
