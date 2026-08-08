//! X isolation, half A: a flat blob containing the lowerer. Byte size compared
//! against measure_driver_flat (identical minus the lowerer) yields X = the
//! lowerer's own machine code, in Q0's exact flat-PIC-binary口径. Not executed;
//! only its size is read. IR is empty here (X is codegen, not any payload's data).
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../mem_intrinsics.rs"]
mod mem;
#[path = "../ir.rs"]
mod ir;
#[path = "../lower.rs"]
mod lower;

use abi::Kernel;

static PROBE: [u8; 8] = [ir::OP_IMM, 0, 0, 0, 0, 0, 0, 0];

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    // Make the IR pointer AND length opaque (black_box) so the optimizer cannot fold
    // the decode loop away or prune unused encoder arms — X must reflect the WHOLE
    // lowerer, not just what a fixed IR touches. Never executed; only its size is read.
    let env = [0usize; 8];
    let ptr = core::hint::black_box(PROBE.as_ptr());
    let len = core::hint::black_box(64usize);
    let ir = unsafe { core::slice::from_raw_parts(ptr, len) };
    lower::lower_and_run(unsafe { &*k }, ir, env.as_ptr())
}
