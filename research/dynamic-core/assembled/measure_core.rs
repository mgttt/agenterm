//! Byte measurement harness, SAME method as `interp/measure_core.rs` (Q9) and
//! `verify/measure_core.rs` (Q19): `rustc -O --crate-type=lib --emit=obj` then
//! `llvm-size` Berkeley `.text`, std + default panic, unstripped. This isolates the
//! ENGINE (eval-core + table-driven seam + IR verifier + orchestration step-table
//! engine, all reused/composed) from the demo driver's std::fs/println!/store
//! plumbing in `main.rs`, so it is directly comparable to Q9's 3177 B and Q19's 634 B
//! figures (same tool, same target, same std/panic posture).
#![allow(dead_code)]

#[path = "ir.rs"]
mod ir;
#[path = "../verify/verify.rs"]
mod verify;
#[path = "../orchestration/step_table.rs"]
mod step_table;
#[path = "seam.rs"]
mod seam;
#[path = "eval_core.rs"]
mod eval_core;

pub fn touch() -> u64 {
    // reference every public symbol so none of it is dead-code-eliminated away
    // before `.text` is measured (same discipline as Q9's measure_core.rs).
    let ctx = seam::SeamCtx::new();
    let _ = seam::do_intent;
    let _ = eval_core::run;
    let _ = verify::verify;
    let _ = ctx;
    0
}
