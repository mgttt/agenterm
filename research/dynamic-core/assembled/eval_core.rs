//! Q22 (assembled) — the EVAL-CORE, copied verbatim in body from `interp/interp.rs`'s
//! `run` + `eval_op` (Q9's ISA-independent half). Only TWO changes from the original,
//! both deliberate integration points, called out explicitly (nothing else touched):
//!
//!   1. `run` takes `&VerifiedModule` instead of `&Module` — this is the ONE-LINE
//!      production change Q19's RESULTS §"Deviations" flagged as "noted, not done":
//!      *"wiring [the construction gate] into `interp::run` in production is a
//!      one-line signature change, noted not done."* Q22 does it. A caller cannot
//!      reach `run` without first passing `verify::verify()` — the un-forgettable
//!      gate is now load-bearing, not merely demonstrated standalone.
//!   2. `do_intent` now takes a third argument, `&SeamCtx` (Q9's original had none —
//!      it went straight to Win32 FFI inline). This is required because `do_intent`
//!      here is TABLE-DRIVEN (seam.rs), not per-intent hardcoded Rust match arms, and
//!      the table-driven marshaller needs a place to keep the resolved stdout handle
//!      etc. across calls. The eval semantics (arithmetic/control-flow/Load/Store) are
//!      completely unchanged from Q9.
//!
//! Everything else — the op/inst/term walk, the wrapping-arithmetic semantics, the
//! zero-ISA-token discipline Q9 measured (criterion ④: grep for register/mnemonic
//! tokens over this file must find NONE in executable code) — is IDENTICAL to Q9.

#![allow(dead_code)]

use crate::ir::*;
use crate::seam::{do_intent, SeamCtx};
use crate::verify::VerifiedModule;

/// Interpret a VERIFIED module and return its Exit/Ret value. Un-forgettable: there is
/// no way to obtain a `&VerifiedModule` except through `verify::verify()` (Q19).
pub fn run(vm: &VerifiedModule, ctx: &SeamCtx) -> u64 {
    let m = vm.module();
    // one machine word per SSA/virtual value. reassignment (loops) just overwrites.
    let mut vals = vec![0u64; m.n_vals as usize];
    // rodata is an opaque byte array owned by the Module; its host address IS the neutral
    // "data base" that Op::Rodata offsets into.
    let rodata_base = m.rodata.as_ptr() as u64;

    let mut bi = m.entry as usize;
    loop {
        let blk = &m.blocks[bi];
        for inst in &blk.insts {
            match inst {
                Inst::Set(d, op) => vals[*d as usize] = eval_op(op, &vals, rodata_base),
                // Store8/StoreW dereference a real host pointer held in a value word —
                // exactly what the lowered `mov [rax], cl` / `mov [rax], rcx` does.
                Inst::Store8(addr, v) => unsafe {
                    *(vals[*addr as usize] as *mut u8) = vals[*v as usize] as u8;
                },
                Inst::StoreW(addr, v) => unsafe {
                    *(vals[*addr as usize] as *mut u64) = vals[*v as usize];
                },
                Inst::Call(d, id, args) => {
                    let intent = m.externs[*id as usize].intent;
                    let a: [u64; 3] = {
                        let mut buf = [0u64; 3];
                        for (i, v) in args.iter().enumerate() {
                            buf[i] = vals[*v as usize];
                        }
                        buf
                    };
                    vals[*d as usize] = do_intent(intent, &a[..args.len()], ctx);
                }
            }
        }
        match &blk.term {
            Term::Br(x) => bi = *x as usize,
            Term::BrCond(c, nz, z) => {
                bi = if vals[*c as usize] != 0 { *nz } else { *z } as usize;
            }
            Term::Ret(v) | Term::Exit(v) => return vals[*v as usize],
        }
    }
}

/// Evaluate one value-producing op. Pure u64 word semantics, wrapping arithmetic to match
/// the ISA's 64-bit wraparound. NOTHING here is ISA- or ABI-specific (Q9 criterion ④,
/// unchanged).
fn eval_op(op: &Op, vals: &[u64], rodata_base: u64) -> u64 {
    let g = |v: &Val| vals[*v as usize];
    match op {
        Op::Const(x) => *x,
        Op::Rodata(off) => rodata_base.wrapping_add(*off as u64),
        Op::Add(x, y) => g(x).wrapping_add(g(y)),
        Op::Sub(x, y) => g(x).wrapping_sub(g(y)),
        Op::Mul(x, y) => g(x).wrapping_mul(g(y)),
        Op::Xor(x, y) => g(x) ^ g(y),
        Op::And(x, y) => g(x) & g(y),
        Op::Or(x, y) => g(x) | g(y),
        Op::Shl(x, s) => g(x).wrapping_shl(*s as u32),
        Op::Shr(x, s) => g(x).wrapping_shr(*s as u32),
        Op::Ult(x, y) => (g(x) < g(y)) as u64,
        Op::Load8(x) => unsafe { *(g(x) as *const u8) as u64 },
        Op::LoadW(x) => unsafe { *(g(x) as *const u64) },
    }
}
