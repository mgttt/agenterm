//! X isolation, half B (Q10): same flat scaffold as measure_patch_flat but WITHOUT the
//! applier or stencil data. size(patch_flat) - size(this) = X_total.
#![no_std]
#![no_main]
#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../lowering/mem_intrinsics.rs"]
mod mem;
use abi::Kernel;
#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    let _env = [0usize; 8];
    (unsafe { &*k }.exit)(0)
}
