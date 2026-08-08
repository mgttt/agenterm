//! Q10 packaging: applier IN the kernel (variant-A single product) = kernel primitives
//! + copy-and-patch applier + stencil DATA + runner + Q0 adapters, statically linked.
//! Payload ships as IR bytes. TCB = this whole binary (criterion ③/④).
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel;
#[cfg(dc_os = "windows")]
#[path = "../../adapters/windows/readfile.rs"]
mod fio;
#[cfg(dc_os = "windows")]
#[path = "../../adapters/windows/spawn.rs"]
mod proc;
#[cfg(dc_os = "linux")]
#[path = "../../adapters/linux/readfile.rs"]
mod fio;
#[cfg(dc_os = "linux")]
#[path = "../../adapters/linux/spawn.rs"]
mod proc;
#[path = "../ir.rs"]
mod ir;
#[path = "../out/stencils_gen.rs"]
mod stencils_gen;
#[path = "../patch.rs"]
mod patch;
#[path = "../runner.rs"]
mod runner;

use abi::Kernel;

#[no_mangle]
pub extern "sysv64" fn agent_main(k: *const Kernel) -> ! {
    let ir: &[u8] = include_bytes!(env!("DC_IR"));
    runner::run(unsafe { &*k }, ir)
}
