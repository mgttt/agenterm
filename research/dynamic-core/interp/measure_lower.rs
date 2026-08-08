//! J2/J4 byte measurement — the Q1 NATIVE LOWERER body, measured with the SAME method as
//! the interpreter (measure_core.rs) so the byte ratio is apples-to-apples.
//!
//! `.text` here = common::lower + lower_op/lower_inst/lower_call + the whole asm.rs x86-64
//! encoder + win64.rs emit_call/emit_spawn. That encoder (the ISA-specific machine-code
//! emitter) is exactly what the interpreter does NOT have — the byte gap is criterion ④.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/lower/asm.rs"]
mod asm;
#[path = "../ir/lower/common.rs"]
mod common;
#[path = "../ir/lower/win64.rs"]
mod win64;

pub fn lower_win(m: &ir::Module) -> Vec<u8> {
    common::set_externs(&m.externs);
    common::lower(m, &win64::Win64)
}
