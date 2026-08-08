//! Packaging: LOWERER OUT OF THE KERNEL (Q3 §1.3). The lowerer + runner + Q0
//! adapters + IR are compiled into a flat PIC blob, loaded by the UNCHANGED Q0
//! minimal four-primitive kernel (load_and_run). The blob receives the primitive
//! table `k`, builds env, lowers the IR to native code, and enters it — a second
//! mmap+jump stage on top of the loader's. TCB = only the minimal kernel (④).
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../mem_intrinsics.rs"]
mod mem; // blob has no kernel linked -> carries its own memcpy/memset
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
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    // Inline include_bytes! (a &[u8; N] to an anonymous rodata array) — accessed
    // RIP-relatively, NO stored fat pointer. A `static IR: &[u8]` would embed an
    // absolute data pointer needing a load-time relocation the flat copied-and-jumped
    // blob never receives -> access violation. See RESULTS deviation note.
    let ir: &[u8] = include_bytes!(env!("DC_IR"));
    runner::run(unsafe { &*k }, ir)
}
