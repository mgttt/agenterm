//! Q21 — engine half. Pure schema + ONE fixed, op-agnostic control loop.
//!
//! This module is deliberately host-agnostic: it knows nothing about Win32,
//! CreateProcessA, or any specific intent. Everything host-specific (which
//! function `reach_id` N calls, argument layout, the STARTUPINFOA/
//! PROCESS_INFORMATION byte content) lives in `main.rs` as DATA, mirroring
//! the tables/marshal.rs split (fixed engine vs table.rs data).
//!
//! ## What this tests (Q21 §②, the linear subset only)
//! Q7's `spawn_boundary()` marked 5 facts "irreducibly code". Two of them —
//! "extract hProcess from PROCESS_INFORMATION output" and "sequence
//! CreateProcess->Wait->GetExitCode->Close" — are **straight-line dataflow**:
//! no runtime value ever selects which step runs next. This module tests
//! whether that subset tabifies under a genuinely fixed interpreter (no
//! `match` on step identity, no per-op branch, no jump/PC field).
//!
//! ## What this does NOT attempt (see RESULTS.md ③)
//! There is no conditional-jump field here. Adding one is exactly the
//! failure mode this experiment is designed to detect (spec discipline:
//! don't invent a mini bytecode language and call it "data"). The
//! conditional-branch case (fork/pid, sentinel-acting, argv loop) is
//! analyzed, not implemented, in RESULTS.md.

/// Where a call argument's value comes from. A **bounded, fixed** enum (4
/// variants) — matching this is not "per-op code": every one of Q1/Q7's
/// data-driven layers (`abi.reach`, `Ret::*`) already has a small fixed
/// match of this shape. The discipline this experiment enforces is that NO
/// variant here encodes "which step to run next" — only "where does this
/// call's *argument* value come from".
pub enum ArgSrc {
    /// A constant baked into the table at author time.
    Const(i64),
    /// The raw i64 return value of an earlier step.
    Slot(usize),
    /// A field read: `*(slots[slot] as *const _) + off`, `width` bytes,
    /// zero/sign-extended to i64. This is L3a (struct layout, already
    /// established as DATA-via-query by Q7) folded directly into argument
    /// resolution — it is how "extract hProcess from PROCESS_INFORMATION"
    /// (Q7 spawn_boundary item 4) stops being a separate "code" step and
    /// becomes an argument source for the *next* call.
    SlotPtrOff(usize, i64, u8),
    /// The address of a freshly-allocated copy of this byte string. Used
    /// for command lines and for STARTUPINFOA/PROCESS_INFORMATION's
    /// *content* (which, once L3a's layout facts are known, is itself a
    /// constant byte blob — Q7's finding, reused here).
    Rodata(&'static [u8]),
}

/// One call in a straight-line (branch-free) sequence.
pub struct Step {
    /// Index into the caller-supplied `reach` table of function pointers.
    /// DATA, exactly like Q1/Q7's `OpSpec.reach_id` — selecting *which*
    /// host function to call is not a branch on a *runtime value*, it is a
    /// fixed lookup baked in at author time.
    pub reach_id: usize,
    pub args: &'static [ArgSrc],
    /// Slot that receives this call's raw i64 return value.
    pub out_slot: usize,
    /// After resolving args (which may allocate, for `Rodata`), stash the
    /// resolved *address* of `args[arg_index]` into `slots[dest_slot]` —
    /// this is how a later step gets to read back into an out-parameter
    /// buffer this step supplied (e.g. `PROCESS_INFORMATION*`).
    pub capture_args: &'static [(usize, usize)],
    /// After the call returns, read `width` bytes at
    /// `*(resolved(args[arg_index])) + 0` into `slots[dest_slot]` — models
    /// Q7's `Ret::OutParam{width}` (L4), used here for
    /// `GetExitCodeProcess`'s `LPDWORD` out-param.
    pub read_out: &'static [(usize, usize, u8)],
}

pub struct StepTable {
    pub steps: &'static [Step],
}

pub type Wrapper = unsafe fn(args: &[i64]) -> i64;

/// THE control loop under test (Q21 ①②, linear subset). Every step, no
/// matter which host call it names, executes through these same lines.
/// There is no `match step_index`, no `if step.reach_id == N`, no jump: the
/// program counter is `for step in table.steps`, i.e. Rust's own linear
/// iteration — the table supplies zero control-flow decisions.
///
/// Discipline check (third-party, spec §1 of the task): `grep -nE
/// 'match|if ' step_table.rs` inside this function must show only the
/// bounded `ArgSrc` match in `resolve` (data-shape dispatch) — no
/// conditional keyed on step identity or step index.
pub fn run(table: &StepTable, reach: &[Wrapper], slots: &mut [i64]) {
    for step in table.steps {
        let mut resolved: Vec<i64> = Vec::with_capacity(step.args.len());
        for a in step.args {
            resolved.push(resolve(a, slots));
        }
        for &(arg_idx, dest_slot) in step.capture_args {
            slots[dest_slot] = resolved[arg_idx];
        }
        let f = reach[step.reach_id];
        let ret = unsafe { f(&resolved) };
        slots[step.out_slot] = ret;
        for &(arg_idx, dest_slot, width) in step.read_out {
            let ptr = resolved[arg_idx] as *const u8;
            slots[dest_slot] = unsafe { read_width(ptr, width) };
        }
    }
}

fn resolve(a: &ArgSrc, slots: &[i64]) -> i64 {
    match a {
        ArgSrc::Const(v) => *v,
        ArgSrc::Slot(i) => slots[*i],
        ArgSrc::SlotPtrOff(i, off, width) => {
            let ptr = (slots[*i] as *const u8).wrapping_offset(*off as isize);
            unsafe { read_width(ptr, *width) }
        }
        ArgSrc::Rodata(bytes) => {
            // Fresh heap copy each resolution, leaked for the lifetime of
            // this process — fine for a measurement driver. The point under
            // test is that the *bytes* are inert data; the copy mechanism
            // is fixed, generic, and identical for every use site.
            let boxed: Box<[u8]> = bytes.to_vec().into_boxed_slice();
            Box::leak(boxed).as_ptr() as i64
        }
    }
}

unsafe fn read_width(ptr: *const u8, width: u8) -> i64 {
    match width {
        1 => *ptr as i64,
        4 => *(ptr as *const i32) as i64,
        8 => *(ptr as *const i64),
        _ => unreachable!("read_width: unsupported width {width}"),
    }
}
