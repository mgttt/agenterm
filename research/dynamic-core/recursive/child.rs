//! Q17 — core B: the OBSERVED interpreter that runs inside the child process.
//!
//! This is the same eval-core as Q9 `interp::run` (the arithmetic is replicated because
//! Q9's `eval_op` is a private `fn` — noted as a deviation in RESULTS), with TWO additions
//! that are the whole point of the recursion experiment:
//!   1. every OS Intent it executes is RECORDED (args + return) into a trace, and every
//!      WriteStdout is CAPTURED — this is the observation channel A gets that Q15's spawn
//!      did not (Q15: A saw only `Ok(7)`).
//!   2. a POLICY set of allowed intents — if the module reaches for a forbidden intent
//!      (e.g. SpawnWait), B refuses and reports the denial. This is A CONSTRAINING B.
//!
//! The seam here is deliberately a SAFE subset: Alloc (host heap) + WriteStdout (captured).
//! FileOpen/Read/Close/SpawnWait are gated off by default so the demo never touches the
//! filesystem or spawns a grandchild. That gate IS the constraint result — it is the same
//! O(1) intent chokepoint Q15 measured, now applied by the *inner* core.

use crate::ir::*;
use crate::serial::{Report, TraceEntry};

/// Run `m` under core B, observing every intent. `allowed` is the policy A imposed.
pub fn run_observed(m: &Module, allowed: &[Intent]) -> Report {
    let mut vals = vec![0u64; m.n_vals as usize];
    let rodata_base = m.rodata.as_ptr() as u64;
    let mut trace: Vec<TraceEntry> = Vec::new();
    let mut capture: Vec<u8> = Vec::new();

    let mut bi = m.entry as usize;
    // step budget: B is another core, so it enforces Q15's halting bound on itself.
    let mut steps: u64 = 0;
    let step_limit: u64 = 200_000_000;
    loop {
        let blk = &m.blocks[bi];
        for inst in &blk.insts {
            steps += 1;
            if steps > step_limit {
                return Report { result: u64::MAX, capture, trace, denied: None };
            }
            match inst {
                Inst::Set(d, op) => vals[*d as usize] = eval_op(op, &vals, rodata_base),
                Inst::Store8(a, v) => unsafe { *(vals[*a as usize] as *mut u8) = vals[*v as usize] as u8; },
                Inst::StoreW(a, v) => unsafe { *(vals[*a as usize] as *mut u64) = vals[*v as usize]; },
                Inst::Call(d, id, args) => {
                    let intent = m.externs[*id as usize].intent;
                    let mut a = Vec::with_capacity(args.len());
                    for v in args { a.push(vals[*v as usize]); }
                    // ---- the constraint chokepoint (O(1), same as Q15) ----
                    if !allowed.contains(&intent) {
                        return Report { result: u64::MAX, capture, trace, denied: Some(intent) };
                    }
                    let ret = do_intent(intent, &a, &mut capture);
                    trace.push(TraceEntry { intent, args: a, ret });
                    vals[*d as usize] = ret;
                }
            }
        }
        match &blk.term {
            Term::Br(x) => bi = *x as usize,
            Term::BrCond(c, nz, z) => bi = if vals[*c as usize] != 0 { *nz } else { *z } as usize,
            Term::Ret(v) | Term::Exit(v) => {
                return Report { result: vals[*v as usize], capture, trace, denied: None };
            }
        }
    }
}

// replicated from Q9 interp::eval_op (private there). Pure u64 word semantics.
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

/// SAFE intent seam: Alloc (host heap, leaked) + WriteStdout (captured into `cap`).
/// Everything else is intentionally unimplemented — it is gated off by `allowed` above,
/// so it is never reached in the demo. That gate is the constraint, not this stub.
fn do_intent(intent: Intent, args: &[u64], cap: &mut Vec<u8>) -> u64 {
    match intent {
        Intent::Alloc => {
            let n = args[0] as usize;
            let v = vec![0u8; n.max(1)].into_boxed_slice();
            Box::leak(v).as_mut_ptr() as u64
        }
        Intent::WriteStdout => {
            let (buf, len) = (args[0] as *const u8, args[1] as usize);
            let slice = unsafe { std::slice::from_raw_parts(buf, len) };
            cap.extend_from_slice(slice);
            len as u64
        }
        other => panic!("intent {other:?} reached seam but is not in the safe subset (should be gated)"),
    }
}
