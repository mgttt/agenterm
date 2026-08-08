//! X isolation, half B: the SAME flat-blob scaffold as measure_lower_flat but WITHOUT
//! the lowerer. size(measure_lower_flat) - size(this) = X (the lowerer's code alone).
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../mem_intrinsics.rs"]
mod mem;

use abi::Kernel;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    let _env = [0usize; 8];
    (unsafe { &*k }.exit)(0)
}
