//! Interpreter — ported from `research/dynamic-core/assembled/eval_core.rs`
//! (Q9's ISA-independent half, assembled by Q22). The op/inst/term walk and
//! wrapping-u64 arithmetic are unchanged. What Q22 called `do_intent` — a
//! table-driven dispatcher over `seam.rs`'s Win32 FFI wrappers (cut per the
//! design doc §2) — is replaced by exactly one call through the
//! `FleetBridgeFn` shape `src/script_rh_host.rs` already established for the
//! three script engines: `Fn(&str, &str) -> Result<String, String>`. Every
//! call is recorded (operation_id, params_json, result) so a caller — the
//! host binding or a test — can observe the full round trip, not just the
//! final word.
//!
//! `run` takes `&VerifiedModule`, not `&Module`: exactly the one-line
//! production change Q19's own RESULTS flagged as "noted, not done" and Q22
//! closed (its F5). A caller cannot reach `run` without first passing
//! `verify::verify()`.

use crate::ir::*;
use crate::verify::VerifiedModule;

/// One `fleet_call` this run made, in program order.
#[derive(Clone, Debug)]
pub struct FleetCallRecord {
    pub operation_id: String,
    pub params_json: String,
    pub result: Result<String, String>,
}

/// The outcome of interpreting a verified module: its `Exit`/`Ret` word, plus
/// every fleet call it made along the way.
#[derive(Clone, Debug)]
pub struct RunOutcome {
    pub result: u64,
    pub calls: Vec<FleetCallRecord>,
}

/// Interpret a VERIFIED module, calling `bridge(operation_id, params_json)`
/// for every `FleetCall` it executes, and return its `Exit`/`Ret` value plus
/// the full call log.
pub fn run(vm: &VerifiedModule, bridge: &dyn Fn(&str, &str) -> Result<String, String>) -> RunOutcome {
    let m = vm.module();
    // one machine word per SSA/virtual value. reassignment (loops) just overwrites.
    let mut vals = vec![0u64; m.n_vals as usize];
    let mut calls = Vec::new();

    let mut bi = m.entry as usize;
    loop {
        let blk = &m.blocks[bi];
        for inst in &blk.insts {
            match inst {
                Inst::Set(d, op) => vals[*d as usize] = eval_op(op, &vals),
                Inst::FleetCall(d, id) => {
                    // verify() already proved `id` is in range and the
                    // extern's operation_id/params_json are catalog-valid.
                    let extern_decl = &m.externs[*id as usize];
                    let result = bridge(&extern_decl.operation_id, &extern_decl.params_json);
                    vals[*d as usize] = u64::from(result.is_ok());
                    calls.push(FleetCallRecord {
                        operation_id: extern_decl.operation_id.clone(),
                        params_json: extern_decl.params_json.clone(),
                        result,
                    });
                }
            }
        }
        match &blk.term {
            Term::Br(x) => bi = *x as usize,
            Term::BrCond(c, nz, z) => {
                bi = if vals[*c as usize] != 0 { *nz } else { *z } as usize;
            }
            Term::Ret(v) | Term::Exit(v) => {
                return RunOutcome {
                    result: vals[*v as usize],
                    calls,
                };
            }
        }
    }
}

/// Evaluate one value-producing op. Pure u64 word semantics, wrapping
/// arithmetic. Nothing here is ISA- or ABI-specific (Q9 criterion ④,
/// unchanged by this port).
fn eval_op(op: &Op, vals: &[u64]) -> u64 {
    let g = |v: &Val| vals[*v as usize];
    match op {
        Op::Const(x) => *x,
        Op::Add(x, y) => g(x).wrapping_add(g(y)),
        Op::Sub(x, y) => g(x).wrapping_sub(g(y)),
        Op::Mul(x, y) => g(x).wrapping_mul(g(y)),
        Op::Xor(x, y) => g(x) ^ g(y),
        Op::And(x, y) => g(x) & g(y),
        Op::Or(x, y) => g(x) | g(y),
        Op::Shl(x, s) => g(x).wrapping_shl(u32::from(*s)),
        Op::Shr(x, s) => g(x).wrapping_shr(u32::from(*s)),
        Op::Ult(x, y) => u64::from(g(x) < g(y)),
    }
}
