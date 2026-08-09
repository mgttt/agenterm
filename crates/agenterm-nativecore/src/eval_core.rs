//! Interpreter — the op/inst/term walk copied in body from
//! `research/dynamic-core/assembled/eval_core.rs` (Q9's ISA-independent half,
//! assembled by Q22), with `do_intent` routed through THIS crate's `seam.rs`
//! (all seven Win32-bound intents, no `FleetBridgeFn` abstraction — that
//! shape belongs to the logic pack, see `ir.rs`'s header for why the two IRs
//! don't unify).
//!
//! **Step limit** (design doc §2 item, ported from the SAME pattern
//! `crates/agenterm-dynacore/src/eval_core.rs` already applied to the logic
//! pack, itself ported from `research/dynamic-core/policy/interp_policy.rs`'s
//! ① halting check, Q15): a well-formed IR can still contain a real infinite
//! loop (a back-edge that never falls out), which would otherwise wedge this
//! function — and the host thread calling it — forever. `verify()` cannot
//! catch this (a self-loop is structurally perfectly well-formed). The same
//! `Termination::{Exited, StepLimitExceeded}` design is used here, counted
//! once per block dispatched (this IR CAN loop within a single block's own
//! `insts` no more than the logic pack's can — its `Inst`s are still a
//! straight-line list executed once per block entry — so once-per-block is
//! the right, cheap chokepoint here too).

use crate::ir::*;
use crate::seam::{do_intent, do_registry_call};
use crate::verify::VerifiedModule;

/// One native `Intent` call this run made, in program order — useful for
/// tests/embedders that want to observe the full round trip, not just the
/// final word.
#[derive(Clone, Debug)]
pub struct NativeCallRecord {
    pub intent: Intent,
    pub args: Vec<u64>,
    pub result: u64,
}

/// One registry-backed call this run made, in program order — v2 (design
/// doc §9), the `Inst::CallReg` analogue of `NativeCallRecord`. Kept as a
/// SEPARATE record type (not folded into `NativeCallRecord`) because
/// `Intent` is a closed, compile-time enum and a registry call's callee is a
/// runtime symbol name, not an `Intent` variant.
#[derive(Clone, Debug)]
pub struct RegistryCallRecord {
    pub module: String,
    pub symbol: String,
    pub args: Vec<u64>,
    pub result: u64,
}

/// How a run ended. A distinct type from any `Exit`/`Ret` word (never a
/// magic sentinel), so a caller cannot mistake "the pack finished and
/// returned N" for "the pack was forcibly cut off".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Termination {
    /// A `Ret`/`Exit` terminator was reached normally; the word is its value.
    Exited(u64),
    /// The step budget was exhausted before any terminator was reached. Any
    /// native calls that already completed before the cutoff are still real
    /// and reported (in `RunOutcome::calls`), not discarded.
    StepLimitExceeded,
}

#[derive(Clone, Debug)]
pub struct RunOutcome {
    pub termination: Termination,
    pub calls: Vec<NativeCallRecord>,
    /// v2 (design doc §9) — SEPARATE from `calls` above; empty for every run
    /// of a module that only uses the seven compile-time intents.
    pub registry_calls: Vec<RegistryCallRecord>,
}

impl RunOutcome {
    /// The `Exit`/`Ret` word, or `None` if step-limited instead of completing.
    pub fn result(&self) -> Option<u64> {
        match self.termination {
            Termination::Exited(value) => Some(value),
            Termination::StepLimitExceeded => None,
        }
    }
}

/// Default step ceiling for `run`. NOT set to Q15's/the logic pack's
/// `1_000_000` verbatim: this crate's own `payloads::pure_compute` (the
/// canonical, legitimate, no-native-calls reference payload — same mixing
/// loop Q1/Q9 used) dispatches its loop block exactly 1_000_000 times plus 2
/// (entry + exit blocks) = 1,000,002 block dispatches to reach `Exit(163)`
/// normally. A cap of 1_000_000 would clip that REAL payload as if it were
/// an infinite loop — discovered by running this crate's own black-box test
/// suite, not asserted. 2_000_000 gives pure_compute ~2x headroom while
/// still being a hard, real backstop (measured sub-second on this box, same
/// order of magnitude as Q15's own per-instruction-check overhead numbers).
pub const DEFAULT_MAX_STEPS: u64 = 2_000_000;

/// Interpret a VERIFIED module and return its Exit/Ret value plus the native
/// call log. Bounded at `DEFAULT_MAX_STEPS`.
pub fn run(vm: &VerifiedModule) -> RunOutcome {
    run_with_step_limit(vm, DEFAULT_MAX_STEPS)
}

/// `run`, with a caller-chosen step ceiling. `max_steps == 0` means
/// unlimited (kept for tests that deliberately want the pre-hardening
/// behavior; `run`'s public default never passes 0). Host-side knob only —
/// a pack has no way to reach this parameter, by construction (same
/// reasoning as the logic pack's identically-shaped function).
pub fn run_with_step_limit(vm: &VerifiedModule, max_steps: u64) -> RunOutcome {
    let m = vm.module();
    // one machine word per SSA/virtual value. reassignment (loops) just overwrites.
    let mut vals = vec![0u64; m.n_vals as usize];
    // rodata is an opaque byte array owned by the Module; its host address IS
    // the neutral "data base" that Op::Rodata offsets into.
    let rodata_base = m.rodata.as_ptr() as u64;
    let mut calls = Vec::new();
    let mut registry_calls = Vec::new();

    let mut bi = m.entry as usize;
    let mut steps: u64 = 0;
    loop {
        // O(1) chokepoint (Q15 ①): one counter increment/compare per block
        // dispatched. A straight-line module reaches Ret/Exit on its first
        // pass and never comes back; the only way this outer loop iterates
        // more than once is a Br/BrCond back-edge, and every back-edge
        // re-enters here.
        steps += 1;
        if max_steps != 0 && steps > max_steps {
            return RunOutcome { termination: Termination::StepLimitExceeded, calls, registry_calls };
        }
        let blk = &m.blocks[bi];
        for inst in &blk.insts {
            match inst {
                Inst::Set(d, op) => vals[*d as usize] = eval_op(op, &vals, rodata_base),
                Inst::Store8(addr, v) => unsafe {
                    *(vals[*addr as usize] as *mut u8) = vals[*v as usize] as u8;
                },
                Inst::StoreW(addr, v) => unsafe {
                    *(vals[*addr as usize] as *mut u64) = vals[*v as usize];
                },
                Inst::Call(d, id, args) => {
                    let intent = m.externs[*id as usize].intent;
                    let arg_words: Vec<u64> = args.iter().map(|v| vals[*v as usize]).collect();
                    // verify() already proved arg_words.len() matches both
                    // this extern's declared nargs AND intent.contract_arity()
                    // -- seam.rs's tables never read out of bounds here.
                    let result = do_intent(intent, &arg_words);
                    vals[*d as usize] = result;
                    calls.push(NativeCallRecord { intent, args: arg_words, result });
                }
                Inst::CallReg(d, id, args) => {
                    let reg_extern = &m.registry_externs[*id as usize];
                    let arg_words: Vec<u64> = args.iter().map(|v| vals[*v as usize]).collect();
                    // verify() already proved arg_words.len() matches both
                    // this registry extern's declared nargs AND the
                    // registry-DERIVED contract for its symbol (v2, design
                    // doc §9) -- do_registry_call never re-checks arity.
                    let result = do_registry_call(&reg_extern.module, &reg_extern.symbol, &arg_words);
                    vals[*d as usize] = result;
                    registry_calls.push(RegistryCallRecord {
                        module: reg_extern.module.clone(),
                        symbol: reg_extern.symbol.clone(),
                        args: arg_words,
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
                return RunOutcome { termination: Termination::Exited(vals[*v as usize]), calls, registry_calls };
            }
        }
    }
}

/// Evaluate one value-producing op. Pure u64 word semantics, wrapping
/// arithmetic to match ISA 64-bit wraparound. Nothing here is ISA/ABI
/// specific (Q9 criterion ④, unchanged).
fn eval_op(op: &Op, vals: &[u64], rodata_base: u64) -> u64 {
    let g = |v: &Val| vals[*v as usize];
    match op {
        Op::Const(x) => *x,
        Op::Rodata(off) => rodata_base.wrapping_add(u64::from(*off)),
        Op::Add(x, y) => g(x).wrapping_add(g(y)),
        Op::Sub(x, y) => g(x).wrapping_sub(g(y)),
        Op::Mul(x, y) => g(x).wrapping_mul(g(y)),
        Op::Xor(x, y) => g(x) ^ g(y),
        Op::And(x, y) => g(x) & g(y),
        Op::Or(x, y) => g(x) | g(y),
        Op::Shl(x, s) => g(x).wrapping_shl(u32::from(*s)),
        Op::Shr(x, s) => g(x).wrapping_shr(u32::from(*s)),
        Op::Ult(x, y) => u64::from(g(x) < g(y)),
        Op::Load8(x) => unsafe { *(g(x) as *const u8) as u64 },
        Op::LoadW(x) => unsafe { *(g(x) as *const u64) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::verify;

    fn infinite_loop_module() -> Module {
        let mut b = Builder::new();
        b.term(Term::Br(0));
        b.finish("infinite_loop", 0)
    }

    /// The step limit must actually abort a real infinite loop rather than
    /// hanging the calling (test) thread forever. This is the acceptance
    /// test for design doc §5 item 4 ("一个真实的无限循环 pack，在步数上限下
    /// 被及时打断，不挂死宿主线程"), run for real, not just asserted.
    #[test]
    fn step_limit_aborts_an_infinite_loop_instead_of_hanging() {
        let module = infinite_loop_module();
        let verified = verify(&module).expect("a self-loop is structurally well-formed");
        let outcome = run_with_step_limit(&verified, 1_000);
        assert_eq!(outcome.termination, Termination::StepLimitExceeded);
        assert!(outcome.calls.is_empty());
        assert_eq!(outcome.result(), None);
    }

    #[test]
    fn default_run_also_bounds_an_infinite_loop_promptly() {
        let module = infinite_loop_module();
        let verified = verify(&module).expect("a self-loop is structurally well-formed");
        let started = std::time::Instant::now();
        let outcome = run(&verified);
        assert_eq!(outcome.termination, Termination::StepLimitExceeded);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "DEFAULT_MAX_STEPS must abort promptly on a real machine, not just eventually"
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
        let verified = verify(&module).expect("well-formed");
        let outcome = run_with_step_limit(&verified, 1);
        assert_eq!(outcome.termination, Termination::Exited(3));
        assert_eq!(outcome.result(), Some(3));
    }
}
