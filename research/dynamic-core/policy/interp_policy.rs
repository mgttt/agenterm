//! Q15 — the POLICY-ENFORCING interpreter backend.
//!
//! It is Q9's eval-core (`interp/interp.rs`) with a policy layer bolted INTO the dispatch
//! loop. §1.1 of the spec forbids modifying the reused core, so this is a SEPARATE file; the
//! very fact that per-instruction checks must live INSIDE the central loop (not beside it) is
//! itself criterion ④'s structural point: the interpreter already HAS the chokepoint every
//! instruction passes through, so a check is one `if` shared by all ops (O(1) code), not code
//! emitted per op (O(ops), the JIT's cost).
//!
//! eval_op below is a VERBATIM copy of Q9 `interp.rs::eval_op` (it is private there and cannot
//! be called from outside). Copying it keeps the ③ byte comparison honest: policy-core minus
//! this baseline = the cost of the checks, nothing else.
//!
//! Discipline (spec §1.1): the policy is a struct of scalars + one region list + one taint
//! bitvector. NO policy DSL, NO permission model, NO load-time verifier. Enough to trip the
//! criteria, no more.

#![allow(dead_code)]

use crate::ir::*;

/// Why execution was refused. Returning this (instead of performing the effect) IS the
/// control point: the interpreter decides, per instruction, whether to proceed.
#[derive(Debug, PartialEq, Eq)]
pub enum Violation {
    OutOfBounds { addr: u64, op: &'static str }, // ① memory
    StepLimit { limit: u64 },                    // ① resource (halting)
    AllocLimit { asked: u64, budget: u64 },      // ① resource (memory)
    IntentDenied { intent: &'static str },       // ① OS surface
    TaintedArg { intent: &'static str },         // ① data-flow
}

/// A known-valid memory region [lo, hi). `tainted` marks data whose provenance the policy
/// wants to track (e.g. bytes read from a file). A load from a tainted region yields a
/// tainted value word.
#[derive(Clone, Copy)]
struct Region {
    lo: u64,
    hi: u64,
    tainted: bool,
}

/// The whole policy. Scalars + a region list + (implicit) per-value taint. This is the entire
/// "mechanism" — deliberately tiny (spec §1.1).
pub struct Policy {
    pub max_steps: u64,       // ① halting: instruction-dispatch ceiling (0 = unlimited)
    pub max_alloc: u64,       // ① memory budget: sum of Alloc bytes (0 = unlimited)
    pub bounds: bool,         // ① check every Load/Store against known regions
    pub allowed: u64,         // ① OS surface: bit i set => Intent #i permitted
    pub taint_reads: bool,    // ① data-flow: mark FileRead buffers tainted
    pub block_tainted_write: bool, // ① data-flow: refuse WriteStdout of a tainted buffer
}

impl Policy {
    /// Allow everything — the "no policy" baseline (behaves like Q9's plain interpreter,
    /// modulo the region bookkeeping needed to answer ①).
    pub fn permissive() -> Self {
        Policy { max_steps: 0, max_alloc: 0, bounds: false, allowed: !0, taint_reads: false, block_tainted_write: false }
    }
    fn intent_bit(i: Intent) -> u64 {
        1u64 << (i as u64)
    }
}

fn intent_name(i: Intent) -> &'static str {
    match i {
        Intent::Alloc => "Alloc",
        Intent::FileOpen => "FileOpen",
        Intent::FileRead => "FileRead",
        Intent::FileClose => "FileClose",
        Intent::WriteStdout => "WriteStdout",
        Intent::SpawnWait => "SpawnWait",
    }
}

/// Policy-enforcing interpreter. Same walk as Q9 `run`, but every memory op and every intent
/// call is gated. Returns Err(Violation) the instant a rule is broken — and produces NO effect
/// past that point (the malicious store never lands, the denied intent never fires).
pub fn run_policy(m: &Module, pol: &Policy) -> Result<u64, Violation> {
    let mut vals = vec![0u64; m.n_vals as usize];
    let mut taint = vec![false; m.n_vals as usize]; // ① data-flow: per-value taint bit
    let rodata_base = m.rodata.as_ptr() as u64;

    // Known regions. rodata is always a valid, untainted region.
    let mut regions: Vec<Region> = vec![Region { lo: rodata_base, hi: rodata_base + m.rodata.len() as u64, tainted: false }];
    let mut alloc_total: u64 = 0;
    let mut steps: u64 = 0;

    let addr_in = |regions: &[Region], a: u64| -> Option<usize> {
        regions.iter().position(|r| a >= r.lo && a < r.hi)
    };

    let mut bi = m.entry as usize;
    loop {
        let blk = &m.blocks[bi];
        for inst in &blk.insts {
            // ① resource (halting): one dispatch = one step. This is the O(1) shared check
            //    that the JIT cannot express without per-op emitted counter code (④).
            if pol.max_steps != 0 {
                steps += 1;
                if steps > pol.max_steps {
                    return Err(Violation::StepLimit { limit: pol.max_steps });
                }
            }
            match inst {
                Inst::Set(d, op) => {
                    // ① memory: gate loads inside the op before the raw deref happens.
                    if pol.bounds {
                        if let Some((a, name)) = load_addr(op, &vals) {
                            if addr_in(&regions, a).is_none() {
                                return Err(Violation::OutOfBounds { addr: a, op: name });
                            }
                        }
                    }
                    vals[*d as usize] = eval_op(op, &vals, rodata_base);
                    // ① data-flow: taint propagation. A load from a tainted region taints the
                    //    result; arithmetic taints if any input is tainted.
                    taint[*d as usize] = taint_of(op, &vals, &taint, &regions, &addr_in);
                }
                Inst::Store8(addr, v) => {
                    let a = vals[*addr as usize];
                    if pol.bounds && addr_in(&regions, a).is_none() {
                        return Err(Violation::OutOfBounds { addr: a, op: "Store8" });
                    }
                    unsafe { *(a as *mut u8) = vals[*v as usize] as u8; }
                }
                Inst::StoreW(addr, v) => {
                    let a = vals[*addr as usize];
                    if pol.bounds && addr_in(&regions, a).is_none() {
                        return Err(Violation::OutOfBounds { addr: a, op: "StoreW" });
                    }
                    unsafe { *(a as *mut u64) = vals[*v as usize]; }
                }
                Inst::Call(d, id, args) => {
                    let intent = m.externs[*id as usize].intent;

                    // ① OS surface: the intent allowlist. THIS is the intent-layer control
                    //    point — and criterion ② is that control ENDS here.
                    if pol.allowed & Policy::intent_bit(intent) == 0 {
                        return Err(Violation::IntentDenied { intent: intent_name(intent) });
                    }

                    let mut buf = [0u64; 3];
                    for (i, v) in args.iter().enumerate() {
                        buf[i] = vals[*v as usize];
                    }

                    // ① data-flow: refuse to let file-derived bytes flow into stdout. The
                    //    buffer's provenance is the region taint (set by FileRead); value
                    //    taint on args covers the direct-value case. NOTE the laundering hole
                    //    proven by read_hash_print: taint is LOST through a table lookup
                    //    (hextab[nibble]) — value-taint cannot follow an index into untainted
                    //    rodata, so a payload that formats file bytes via a lookup writes an
                    //    "untainted" buffer. That is data-flow's real boundary (④/②).
                    if pol.block_tainted_write && intent == Intent::WriteStdout {
                        let buf_tainted = addr_in(&regions, buf[0]).map(|i| regions[i].tainted).unwrap_or(false);
                        if buf_tainted || args.iter().any(|v| taint[*v as usize]) {
                            return Err(Violation::TaintedArg { intent: "WriteStdout" });
                        }
                    }

                    let ret = do_intent(intent, &buf[..args.len()]);

                    // Bookkeeping AFTER the call succeeds: register Alloc regions (with budget)
                    // and taint FileRead buffers. NOTE (criterion ②): for SpawnWait this
                    // returns a child's exit code — the interpreter observed NOTHING the child
                    // did; control was gone the instant do_intent entered kernel32.
                    match intent {
                        Intent::Alloc => {
                            let n = buf[0];
                            if pol.max_alloc != 0 {
                                alloc_total += n;
                                if alloc_total > pol.max_alloc {
                                    return Err(Violation::AllocLimit { asked: n, budget: pol.max_alloc });
                                }
                            }
                            if ret != 0 {
                                regions.push(Region { lo: ret, hi: ret + n, tainted: false });
                            }
                        }
                        Intent::FileRead => {
                            // args = [handle, buf, cap]; ret = bytes read. Mark the buffer's
                            // region tainted from provenance = a file.
                            if pol.taint_reads {
                                let bufaddr = buf[1];
                                if let Some(idx) = addr_in(&regions, bufaddr) {
                                    regions[idx].tainted = true;
                                }
                            }
                        }
                        _ => {}
                    }
                    vals[*d as usize] = ret;
                    taint[*d as usize] = false;
                }
            }
        }
        match &blk.term {
            Term::Br(x) => bi = *x as usize,
            Term::BrCond(c, nz, z) => {
                bi = if vals[*c as usize] != 0 { *nz } else { *z } as usize;
            }
            Term::Ret(v) | Term::Exit(v) => return Ok(vals[*v as usize]),
        }
    }
}

/// If `op` is a load, return (address, op-name) for the bounds check. Kept separate so the
/// hot path reads clearly.
fn load_addr(op: &Op, vals: &[u64]) -> Option<(u64, &'static str)> {
    match op {
        Op::Load8(x) => Some((vals[*x as usize], "Load8")),
        Op::LoadW(x) => Some((vals[*x as usize], "LoadW")),
        _ => None,
    }
}

/// ① data-flow: taint of an op's result.
fn taint_of<F: Fn(&[Region], u64) -> Option<usize>>(
    op: &Op, vals: &[u64], taint: &[bool], regions: &[Region], addr_in: &F,
) -> bool {
    let t = |v: &Val| taint[*v as usize];
    match op {
        Op::Const(_) | Op::Rodata(_) => false,
        Op::Load8(x) | Op::LoadW(x) => {
            // tainted iff the region loaded from is tainted
            addr_in(regions, vals[*x as usize]).map(|i| regions[i].tainted).unwrap_or(false)
        }
        Op::Add(x, y) | Op::Sub(x, y) | Op::Mul(x, y) | Op::Xor(x, y)
        | Op::And(x, y) | Op::Or(x, y) | Op::Ult(x, y) => t(x) || t(y),
        Op::Shl(x, _) | Op::Shr(x, _) => t(x),
    }
}

// ---- VERBATIM copy of Q9 interp.rs::eval_op (private there). ISA-independent, pure u64. ----
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

// ---- OS SEAM — verbatim shape from Q9 interp.rs (L1–L5 live here, unchanged). ----
#[cfg(all(windows, not(policy_measure_core)))]
mod seam {
    #[link(name = "kernel32")]
    extern "system" {
        pub fn VirtualAlloc(addr: *mut u8, size: usize, typ: u32, protect: u32) -> *mut u8;
        pub fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *mut u8, disp: u32, flags: u32, tmpl: *mut u8) -> *mut u8;
        pub fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32;
        pub fn CloseHandle(h: *mut u8) -> i32;
        pub fn GetStdHandle(which: u32) -> *mut u8;
        pub fn WriteFile(h: *mut u8, buf: *const u8, n: u32, wrote: *mut u32, ov: *mut u8) -> i32;
        pub fn CreateProcessA(app: *const u8, cmd: *mut u8, pa: *mut u8, ta: *mut u8, inh: i32, flags: u32, env: *mut u8, dir: *const u8, si: *mut u8, pi: *mut u8) -> i32;
        pub fn WaitForSingleObject(h: *mut u8, ms: u32) -> u32;
        pub fn GetExitCodeProcess(h: *mut u8, code: *mut u32) -> i32;
    }
}

#[cfg(all(windows, not(policy_measure_core)))]
fn do_intent(intent: Intent, args: &[u64]) -> u64 {
    use seam::*;
    unsafe {
        match intent {
            Intent::Alloc => VirtualAlloc(core::ptr::null_mut(), args[0] as usize, 0x3000, 0x04) as u64,
            Intent::FileOpen => CreateFileA(args[0] as *const u8, 0x8000_0000, 1, core::ptr::null_mut(), 3, 0, core::ptr::null_mut()) as u64,
            Intent::FileRead => {
                let mut got: u32 = 0;
                ReadFile(args[0] as *mut u8, args[1] as *mut u8, args[2] as u32, &mut got, core::ptr::null_mut());
                got as u64
            }
            Intent::FileClose => CloseHandle(args[0] as *mut u8) as u64,
            Intent::WriteStdout => {
                let h = GetStdHandle(0xFFFF_FFF5);
                let mut wrote: u32 = 0;
                WriteFile(h, args[0] as *const u8, args[1] as u32, &mut wrote, core::ptr::null_mut());
                wrote as u64
            }
            Intent::SpawnWait => spawn_wait(),
        }
    }
}

#[cfg(all(windows, not(policy_measure_core)))]
unsafe fn spawn_wait() -> u64 {
    use seam::*;
    #[repr(C)]
    struct StartupInfoA { cb: u32, _rest: [u8; 100] }
    #[repr(C)]
    struct ProcessInformation { h_process: *mut u8, h_thread: *mut u8, pid: u32, tid: u32 }
    let mut si: StartupInfoA = core::mem::zeroed();
    si.cb = 104;
    let mut pi: ProcessInformation = core::mem::zeroed();
    let mut cmd: Vec<u8> = b"cmd.exe /c exit 7\0".to_vec();
    CreateProcessA(core::ptr::null(), cmd.as_mut_ptr(), core::ptr::null_mut(), core::ptr::null_mut(),
        0, 0, core::ptr::null_mut(), core::ptr::null(), &mut si as *mut _ as *mut u8, &mut pi as *mut _ as *mut u8);
    let hproc = pi.h_process;
    WaitForSingleObject(hproc, 0xFFFF_FFFF);
    let mut code: u32 = 0;
    GetExitCodeProcess(hproc, &mut code);
    CloseHandle(hproc);
    code as u64
}

// measurement stub: --cfg policy_measure_core collapses the seam so `.text` measures the
// policy eval-core alone (criterion ③ byte split, mirrors Q9's interp_measure_core).
#[cfg(any(not(windows), policy_measure_core))]
fn do_intent(_intent: Intent, _args: &[u64]) -> u64 {
    unsafe { core::hint::unreachable_unchecked() }
}
