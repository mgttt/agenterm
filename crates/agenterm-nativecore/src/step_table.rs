//! Linear orchestration step table — ported verbatim (engine unchanged) from
//! `research/dynamic-core/orchestration/step_table.rs` (Q21), reused by Q22's
//! assembly via `#[path]` and here by a plain copy (this crate does not share
//! a `#[path]` boundary with the research tree, since it needs to stand alone
//! as a shippable crate). Zero lines of the control loop below differ from
//! Q21's own file.
//!
//! This module is deliberately host-agnostic: it knows nothing about Win32,
//! `CreateProcessA`, or any specific intent. Everything host-specific (which
//! function `reach_id` N calls, argument layout, `STARTUPINFOA`/
//! `PROCESS_INFORMATION` byte content) lives in `seam.rs`/`declare.rs` as
//! DATA, mirroring the `tables/marshal.rs` split (fixed engine vs table data).

#![allow(dead_code)]

/// Where a call argument's value comes from. A bounded, fixed enum — no
/// variant here encodes "which step to run next", only "where does this
/// call's argument value come from".
pub enum ArgSrc {
    /// A constant baked into the table at author time.
    Const(i64),
    /// The raw i64 return value of an earlier step.
    Slot(usize),
    /// A field read: `*(slots[slot] as *const _) + off`, `width` bytes,
    /// zero/sign-extended to i64. How "extract hProcess from
    /// PROCESS_INFORMATION" becomes an argument source for the next call
    /// instead of a separate "code" step.
    SlotPtrOff(usize, i64, u8),
    /// The address of a freshly-allocated copy of this byte string. Used for
    /// command lines and for STARTUPINFOA/PROCESS_INFORMATION's content.
    Rodata(&'static [u8]),
}

/// One call in a straight-line (branch-free) sequence.
pub struct Step {
    /// Index into the caller-supplied `reach` table of function pointers.
    pub reach_id: usize,
    pub args: &'static [ArgSrc],
    /// Slot that receives this call's raw i64 return value.
    pub out_slot: usize,
    /// After resolving args (which may allocate, for `Rodata`), stash the
    /// resolved *address* of `args[arg_index]` into `slots[dest_slot]`.
    pub capture_args: &'static [(usize, usize)],
    /// After the call returns, read `width` bytes at
    /// `*(resolved(args[arg_index])) + 0` into `slots[dest_slot]`.
    pub read_out: &'static [(usize, usize, u8)],
}

pub struct StepTable {
    pub steps: &'static [Step],
}

pub type Wrapper = unsafe fn(args: &[i64]) -> i64;

/// THE control loop. Every step, no matter which host call it names,
/// executes through these same lines — no `match step_index`, no `if
/// step.reach_id == N`, no jump: the program counter is `for step in
/// table.steps`, i.e. Rust's own linear iteration.
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
            let boxed: Box<[u8]> = bytes.to_vec().into_boxed_slice();
            Box::leak(boxed).as_ptr() as i64
        }
    }
}

unsafe fn read_width(ptr: *const u8, width: u8) -> i64 {
    unsafe {
        match width {
            1 => *ptr as i64,
            4 => *(ptr as *const i32) as i64,
            8 => *(ptr as *const i64),
            _ => unreachable!("read_width: unsupported width {width}"),
        }
    }
}
