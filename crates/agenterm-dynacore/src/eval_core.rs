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
//!
//! **Step limit** (v1.1 product hardening, ported from
//! `research/dynamic-core/policy/interp_policy.rs`'s ① halting check, Q15):
//! a well-formed, trusted-source pack can still contain a real bug (a
//! back-edge that never falls out of a loop) that would otherwise wedge this
//! function forever — an unbounded interpreter loop is a host-thread-hang
//! safety gap, not a "who do we trust" question `verify()` could ever catch
//! (a back-edge is perfectly well-formed IR). `run_with_step_limit` counts
//! once per block dispatched (see its body for why once-per-block, not
//! once-per-instruction like Q15's own policy layer, is the right chokepoint
//! for *this* IR) and aborts with `Termination::StepLimitExceeded` instead of
//! looping forever. `run` is `run_with_step_limit` at `DEFAULT_MAX_STEPS` —
//! every caller gets the safety net by construction, not by opting in.

use crate::ir::*;
use crate::verify::VerifiedModule;

/// One `fleet_call` this run made, in program order.
#[derive(Clone, Debug)]
pub struct FleetCallRecord {
    pub operation_id: String,
    pub params_json: String,
    pub result: Result<String, String>,
}

/// How a run ended. Kept a distinct variant from any `Exit`/`Ret` word (never
/// e.g. a magic sentinel value) so a caller cannot mistake "the pack finished
/// and returned N" for "the pack was forcibly cut off" — the two mean
/// completely different things to an embedder deciding whether to trust the
/// result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Termination {
    /// A `Ret`/`Exit` terminator was reached normally; the word is its value.
    Exited(u64),
    /// The step budget (`run_with_step_limit`'s `max_steps`) was exhausted
    /// before any terminator was reached. The run was forcibly aborted —
    /// there is no result word, only whatever `FleetCall`s already completed
    /// before the cutoff (still present in `RunOutcome::calls`).
    StepLimitExceeded,
}

/// The outcome of interpreting a verified module: how it ended, plus every
/// fleet call it made along the way (even on `StepLimitExceeded` — calls that
/// already completed before the cutoff are real and reported, not discarded).
#[derive(Clone, Debug)]
pub struct RunOutcome {
    pub termination: Termination,
    pub calls: Vec<FleetCallRecord>,
}

impl RunOutcome {
    /// The `Exit`/`Ret` word, or `None` if the run was step-limited instead
    /// of completing normally. Convenience for callers that only care about
    /// the happy path (most tests) without matching on `termination`
    /// themselves.
    pub fn result(&self) -> Option<u64> {
        match self.termination {
            Termination::Exited(value) => Some(value),
            Termination::StepLimitExceeded => None,
        }
    }
}

/// Default step ceiling for `run`. Host-fixed, not pack-declarable — see this
/// module's doc and `run_with_step_limit`'s doc for why. 1,000,000 block
/// dispatches: the same order of magnitude Q15's own negative test used
/// (`research/dynamic-core/policy/main.rs`'s `infinite_loop` case ran at
/// 1_000_000), generous for dynacore's actual target shape (small
/// fleet-call-dense workflow/routing packs — design doc §2: "不是计算密集内循环")
/// while still a hard backstop: at Q15's measured 1.47× per-instruction-check
/// overhead (this crate's per-*block* check is cheaper still, since dynacore
/// has no memory ops to gate per instruction), 1,000,000 block dispatches is
/// sub-second on real hardware even in the worst case of a pure control-flow
/// spin with no fleet calls at all.
pub const DEFAULT_MAX_STEPS: u64 = 1_000_000;

/// Interpret a VERIFIED module, calling `bridge(operation_id, params_json)`
/// for every `FleetCall` it executes, and return its `Exit`/`Ret` value plus
/// the full call log. Bounded at `DEFAULT_MAX_STEPS` — see this module's doc.
pub fn run(vm: &VerifiedModule, bridge: &dyn Fn(&str, &str) -> Result<String, String>) -> RunOutcome {
    run_with_step_limit(vm, bridge, DEFAULT_MAX_STEPS)
}

/// `run`, with a caller-chosen step ceiling instead of `DEFAULT_MAX_STEPS`.
/// `max_steps == 0` means unlimited (kept for tests/embedders that
/// deliberately want the pre-v1.1 unbounded behavior; `run`'s public default
/// never passes 0). This is a HOST-side knob, not something a pack can
/// declare for itself: the caller of this function is, by construction, the
/// trusted embedder deciding how long it is willing to block its own thread
/// — there is no `IrFault`/`verify()` gate needed here the way there would be
/// if the limit were read out of pack bytes, because a pack has no way to
/// reach this parameter at all.
pub fn run_with_step_limit(
    vm: &VerifiedModule,
    bridge: &dyn Fn(&str, &str) -> Result<String, String>,
    max_steps: u64,
) -> RunOutcome {
    let m = vm.module();
    // one machine word per SSA/virtual value. reassignment (loops) just overwrites.
    let mut vals = vec![0u64; m.n_vals as usize];
    let mut calls = Vec::new();

    let mut bi = m.entry as usize;
    let mut steps: u64 = 0;
    loop {
        // O(1) chokepoint (Q15 ①, `interp_policy.rs::run_policy`'s halting
        // check): one counter increment/compare per block dispatched, i.e.
        // once per jump/terminator followed. A straight-line module reaches
        // its Ret/Exit on the very first pass through this point and never
        // comes back; the ONLY way this outer loop iterates more than once
        // at all is a Br/BrCond back-edge, and every back-edge re-enters
        // here. Q15's own policy layer counted per-*instruction* because its
        // IR also had per-instruction memory ops to gate (Load8/Store8); this
        // IR doesn't (`ir.rs`'s header: raw-memory ops were cut, not kept),
        // so a block's `insts` cannot loop on their own — counting block
        // dispatches bounds total work just as surely, for less overhead.
        steps += 1;
        if max_steps != 0 && steps > max_steps {
            return RunOutcome {
                termination: Termination::StepLimitExceeded,
                calls,
            };
        }
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
                    termination: Termination::Exited(vals[*v as usize]),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::verify;

    fn no_op_bridge(_: &str, _: &str) -> Result<String, String> {
        panic!("a pure control-flow loop must never reach fleet_call")
    }

    /// A single block that branches to itself forever — well-formed IR
    /// (`verify()` accepts it: the back-edge target exists, nothing is
    /// out-of-range), but would hang `run` forever without a step limit.
    /// This is the exact product safety gap task 1 closes.
    fn infinite_loop_module() -> Module {
        let mut b = Builder::new();
        b.term(Term::Br(0));
        b.finish("infinite_loop", 0)
    }

    #[test]
    fn step_limit_aborts_an_infinite_loop_instead_of_hanging() {
        let module = infinite_loop_module();
        let verified = verify(&module, &[]).expect("a self-loop is structurally well-formed");
        let bridge: &dyn Fn(&str, &str) -> Result<String, String> = &no_op_bridge;
        let outcome = run_with_step_limit(&verified, bridge, 1_000);
        assert_eq!(outcome.termination, Termination::StepLimitExceeded);
        assert!(outcome.calls.is_empty());
        assert_eq!(outcome.result(), None, "a step-limited run has no Exit/Ret word");
    }

    #[test]
    fn default_run_also_bounds_an_infinite_loop() {
        let module = infinite_loop_module();
        let verified = verify(&module, &[]).expect("a self-loop is structurally well-formed");
        let bridge: &dyn Fn(&str, &str) -> Result<String, String> = &no_op_bridge;
        let started = std::time::Instant::now();
        let outcome = run(&verified, bridge);
        assert_eq!(outcome.termination, Termination::StepLimitExceeded);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "DEFAULT_MAX_STEPS must abort promptly, not just eventually"
        );
    }

    #[test]
    fn step_limit_does_not_disturb_a_normally_completing_run() {
        let mut b = Builder::new();
        let one = b.konst(1);
        let two = b.konst(2);
        let sum = b.set(Op::Add(one, two));
        b.term(Term::Exit(sum));
        let module = b.finish("adds_within_budget", 0);
        let verified = verify(&module, &[]).expect("well-formed");
        let bridge: &dyn Fn(&str, &str) -> Result<String, String> = &no_op_bridge;
        let outcome = run_with_step_limit(&verified, bridge, 1);
        assert_eq!(outcome.termination, Termination::Exited(3));
        assert_eq!(outcome.result(), Some(3));
    }
}
