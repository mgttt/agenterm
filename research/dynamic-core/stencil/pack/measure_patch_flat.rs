//! X isolation, half A (Q10): a flat blob = the copy-and-patch APPLIER + the STENCIL
//! DATA. size(this) - size(measure_driver_flat) = X_total = applier code + stencil data
//! in Q2's exact flat-PIC口径 (directly comparable to Q2's X=3003 B). Not executed;
//! only its size is read. The IR is an opaque probe so no arm/stencil is DCE'd.
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../lowering/mem_intrinsics.rs"]
mod mem;
#[path = "../ir.rs"]
mod ir;
#[path = "../out/stencils_gen.rs"]
mod stencils_gen;
#[path = "../patch.rs"]
mod patch;

use abi::Kernel;

static PROBE: [u8; 8] = [ir::OP_IMM, 0, 0, 0, 0, 0, 0, 0];

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    let env = [0usize; 8];
    let ptr = core::hint::black_box(PROBE.as_ptr());
    let len = core::hint::black_box(64usize);
    let ir = unsafe { core::slice::from_raw_parts(ptr, len) };
    patch::lower_and_run(unsafe { &*k }, ir, env.as_ptr())
}
