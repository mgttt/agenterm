//! Table-driven OS seam — ported from `research/dynamic-core/assembled/seam.rs`
//! (Q22), binding all seven `Intent`s directly to real Win32 APIs. Two
//! mechanisms, chosen per intent by its NATIVE CALL COUNT:
//!   * `Mechanism::Single` — exactly one native call
//!     (`Alloc`/`FileOpen`/`FileRead`/`FileClose`), Q7's `OpSpec` vocabulary.
//!   * `Mechanism::Linear` — a fixed, branch-free sequence of >1 native calls
//!     (`WriteStdout`/`SpawnWait`/`FileWrite`), Q21's `StepTable`
//!     (`step_table.rs`, engine unchanged from Q21/Q22).
//!
//! Difference from Q22's version: `SpawnWait`'s `STARTUPINFOA`/
//! `PROCESS_INFORMATION` byte content comes from `declare.rs`'s
//! self-checked offsets/builders (`build_startupinfoa_bytes`,
//! `startupinfoa_offset::*`, `process_information_offset::*`), not a second,
//! separately-baked copy — the design doc's F1-adjacent instruction ("这类
//! 结构体布局用 Q13 的'烤了就验'模式，不是裸烤") applies to the actual seam
//! binding, not only to a test fixture off to the side.

#![allow(dead_code)]

use crate::declare::{process_information_offset, startupinfoa_offset, STARTUPINFOA_SIZE};
use crate::ir::Intent;
use crate::step_table::{ArgSrc, Step, StepTable, Wrapper};

// ============================================================================
// Reach — one thin FFI bridge per DISTINCT native function.
// ============================================================================

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(addr: *mut u8, size: usize, typ: u32, protect: u32) -> *mut u8;
    fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *mut u8, disp: u32, flags: u32, tmpl: *mut u8) -> *mut u8;
    fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32;
    fn CloseHandle(h: *mut u8) -> i32;
    fn GetStdHandle(which: u32) -> *mut u8;
    fn WriteFile(h: *mut u8, buf: *const u8, n: u32, wrote: *mut u32, ov: *mut u8) -> i32;
    fn CreateProcessA(app: *const u8, cmd: *mut u8, pa: *mut u8, ta: *mut u8, inh: i32, flags: u32, env: *mut u8, dir: *const u8, si: *mut u8, pi: *mut u8) -> i32;
    fn WaitForSingleObject(h: *mut u8, ms: u32) -> u32;
    fn GetExitCodeProcess(h: *mut u8, code: *mut u32) -> i32;
}

#[cfg(windows)]
unsafe fn w_virtual_alloc(a: &[i64]) -> i64 {
    unsafe { VirtualAlloc(a[0] as *mut u8, a[1] as usize, a[2] as u32, a[3] as u32) as i64 }
}
#[cfg(windows)]
unsafe fn w_create_file(a: &[i64]) -> i64 {
    unsafe { CreateFileA(a[0] as *const u8, a[1] as u32, a[2] as u32, a[3] as *mut u8, a[4] as u32, a[5] as u32, a[6] as *mut u8) as i64 }
}
#[cfg(windows)]
unsafe fn w_read_file(a: &[i64]) -> i64 {
    unsafe { ReadFile(a[0] as *mut u8, a[1] as *mut u8, a[2] as u32, a[3] as *mut u32, a[4] as *mut u8) as i64 }
}
#[cfg(windows)]
unsafe fn w_close_handle(a: &[i64]) -> i64 {
    unsafe { CloseHandle(a[0] as *mut u8) as i64 }
}
#[cfg(windows)]
unsafe fn w_get_std_handle(a: &[i64]) -> i64 {
    unsafe { GetStdHandle(a[0] as u32) as i64 }
}
#[cfg(windows)]
unsafe fn w_write_file(a: &[i64]) -> i64 {
    unsafe { WriteFile(a[0] as *mut u8, a[1] as *const u8, a[2] as u32, a[3] as *mut u32, a[4] as *mut u8) as i64 }
}
#[cfg(windows)]
unsafe fn w_create_process(a: &[i64]) -> i64 {
    unsafe {
        CreateProcessA(
            a[0] as *const u8,
            a[1] as *mut u8,
            a[2] as *mut u8,
            a[3] as *mut u8,
            a[4] as i32,
            a[5] as u32,
            a[6] as *mut u8,
            a[7] as *const u8,
            a[8] as *mut u8,
            a[9] as *mut u8,
        ) as i64
    }
}
#[cfg(windows)]
unsafe fn w_wait(a: &[i64]) -> i64 {
    unsafe { WaitForSingleObject(a[0] as *mut u8, a[1] as u32) as i64 }
}
#[cfg(windows)]
unsafe fn w_get_exit_code(a: &[i64]) -> i64 {
    unsafe { GetExitCodeProcess(a[0] as *mut u8, a[1] as *mut u32) as i64 }
}

#[cfg(windows)]
pub const REACH: &[Wrapper] = &[
    w_virtual_alloc,   // 0
    w_create_file,     // 1  <- reused by FileOpen (read) AND FileWrite (write)
    w_read_file,       // 2
    w_close_handle,    // 3  <- reused by FileClose AND SpawnWait AND FileWrite
    w_get_std_handle,  // 4
    w_write_file,      // 5  <- reused by WriteStdout AND FileWrite
    w_create_process,  // 6
    w_wait,            // 7
    w_get_exit_code,   // 8
];
const R_ALLOC: usize = 0;
const R_CREATEFILE: usize = 1;
const R_READFILE: usize = 2;
const R_CLOSEHANDLE: usize = 3;
const R_GETSTDHANDLE: usize = 4;
const R_WRITEFILE: usize = 5;
const R_CREATEPROCESS: usize = 6;
const R_WAIT: usize = 7;
const R_GETEXITCODE: usize = 8;

// ============================================================================
// Single-call mechanism (Q7 vocabulary, adapted for an interpreter: args are
// already-resolved u64 words, not registers to place).
// ============================================================================

#[derive(Clone, Copy)]
pub enum Arg {
    /// the k-th SEMANTIC arg of this IR call (already a resolved u64 word)
    Sem(usize),
    /// an injected target constant
    Const(i64),
    /// pointer to a fresh 4-byte out-param scratch cell
    OutPtr,
}

#[derive(Clone, Copy)]
pub enum Ret {
    Direct,
    OutParam { width: u8 },
}

pub struct OpSpec {
    pub reach_id: usize,
    pub args: &'static [Arg],
    pub ret: Ret,
}

pub enum Mechanism {
    Single(OpSpec),
    Linear { table: &'static StepTable, input_slots: &'static [usize], result_slot: usize },
}

static ALLOC_ARGS: [Arg; 4] = [Arg::Const(0), Arg::Sem(0), Arg::Const(0x3000), Arg::Const(0x04)];
static FILEOPEN_ARGS: [Arg; 7] = [
    Arg::Sem(0),
    Arg::Const(0x8000_0000u32 as i64),
    Arg::Const(1),
    Arg::Const(0),
    Arg::Const(3),
    Arg::Const(0),
    Arg::Const(0),
];
static FILEREAD_ARGS: [Arg; 5] = [Arg::Sem(0), Arg::Sem(1), Arg::Sem(2), Arg::OutPtr, Arg::Const(0)];
static FILECLOSE_ARGS: [Arg; 1] = [Arg::Sem(0)];

static WRITESTDOUT_INPUT_SLOTS: [usize; 2] = [1, 2];
static SPAWN_INPUT_SLOTS: [usize; 0] = [];
static FILEWRITE_INPUT_SLOTS: [usize; 3] = [0, 1, 2];

/// THE DATA — one row per intent (matches `Intent::contract_arity()` 1:1;
/// `verify.rs` guarantees any IR that reaches here already agrees with these
/// arities, so `exec_single`/`exec_linear` never need to bounds-check `args`
/// themselves — the produce-time gate is the ONLY place that responsibility
/// lives, by construction).
fn intent_table(intent: Intent) -> Mechanism {
    match intent {
        Intent::Alloc => Mechanism::Single(OpSpec { reach_id: R_ALLOC, args: &ALLOC_ARGS, ret: Ret::Direct }),
        Intent::FileOpen => Mechanism::Single(OpSpec { reach_id: R_CREATEFILE, args: &FILEOPEN_ARGS, ret: Ret::Direct }),
        Intent::FileRead => Mechanism::Single(OpSpec { reach_id: R_READFILE, args: &FILEREAD_ARGS, ret: Ret::OutParam { width: 4 } }),
        Intent::FileClose => Mechanism::Single(OpSpec { reach_id: R_CLOSEHANDLE, args: &FILECLOSE_ARGS, ret: Ret::Direct }),
        Intent::WriteStdout => Mechanism::Linear { table: &WRITESTDOUT_TABLE, input_slots: &WRITESTDOUT_INPUT_SLOTS, result_slot: 4 },
        Intent::SpawnWait => Mechanism::Linear { table: &SPAWN_TABLE, input_slots: &SPAWN_INPUT_SLOTS, result_slot: 5 },
        Intent::FileWrite => Mechanism::Linear { table: &FILEWRITE_TABLE, input_slots: &FILEWRITE_INPUT_SLOTS, result_slot: 5 },
    }
}

fn exec_single(spec: &OpSpec, args: &[u64]) -> u64 {
    let mut outbuf: u32 = 0;
    let native_args: Vec<i64> = spec
        .args
        .iter()
        .map(|a| match a {
            Arg::Sem(k) => args[*k] as i64,
            Arg::Const(v) => *v,
            Arg::OutPtr => std::ptr::addr_of_mut!(outbuf) as i64,
        })
        .collect();
    let raw = unsafe { REACH[spec.reach_id](&native_args) };
    match spec.ret {
        Ret::Direct => raw as u64,
        Ret::OutParam { width: 4 } => outbuf as u64,
        Ret::OutParam { .. } => raw as u64,
    }
}

fn exec_linear(table: &'static StepTable, args: &[u64], input_slots: &'static [usize], result_slot: usize) -> u64 {
    let mut slots = [0i64; 16];
    for (i, &slot) in input_slots.iter().enumerate() {
        slots[slot] = args[i] as i64;
    }
    crate::step_table::run(table, REACH, &mut slots);
    slots[result_slot] as u64
}

/// THE DISPATCHER — the only function `eval_core::run` calls.
pub fn do_intent(intent: Intent, args: &[u64]) -> u64 {
    match intent_table(intent) {
        Mechanism::Single(spec) => exec_single(&spec, args),
        Mechanism::Linear { table, input_slots, result_slot } => exec_linear(table, args, input_slots, result_slot),
    }
}

// ============================================================================
// Linear (Q21 step-table) DATA — WriteStdout, SpawnWait, FileWrite.
// ============================================================================

const WROTE_BUF4: [u8; 4] = [0u8; 4];

// ---- WriteStdout: GetStdHandle(STD_OUTPUT_HANDLE) -> WriteFile ----
// slots: 0=stdout handle, 1=buf(input), 2=len(input), 3=WriteFile BOOL, 4=bytes written
static WRITESTDOUT_STEPS: [Step; 2] = [
    Step {
        reach_id: R_GETSTDHANDLE,
        args: &[ArgSrc::Const(-11)], // STD_OUTPUT_HANDLE
        out_slot: 0,
        capture_args: &[],
        read_out: &[],
    },
    Step {
        reach_id: R_WRITEFILE,
        args: &[ArgSrc::Slot(0), ArgSrc::Slot(1), ArgSrc::Slot(2), ArgSrc::Rodata(&WROTE_BUF4), ArgSrc::Const(0)],
        out_slot: 3,
        capture_args: &[],
        read_out: &[(3usize, 4usize, 4u8)],
    },
];
static WRITESTDOUT_TABLE: StepTable = StepTable { steps: &WRITESTDOUT_STEPS };

// ---- SpawnWait: CreateProcessA -> WaitForSingleObject -> GetExitCodeProcess -> CloseHandle
// STARTUPINFOA/PROCESS_INFORMATION content: `declare.rs`'s self-checked
// builder/offsets, not a second baked copy (see this file's header).
// slots: 0=create BOOL, 1=hProcess (captured from pi), 2=wait raw, 3=getexit BOOL,
//        5=exit code (read_out), 6=close BOOL
const SPAWN_CMDLINE: &[u8] = b"cmd.exe /c exit 7\0";
static STARTUPINFOA_BYTES: [u8; STARTUPINFOA_SIZE] = {
    // `build_startupinfoa_bytes()` (declare.rs) is not `const fn` (it uses
    // `copy_from_slice`), so a `static` initializer cannot call it directly;
    // this block duplicates its BODY, not its offset knowledge -- the
    // offset constant itself (`startupinfoa_offset::CB`) is still imported
    // from declare.rs below, so a future offset correction only has one
    // place to change, and this block picks it up automatically.
    let mut b = [0u8; STARTUPINFOA_SIZE];
    let off = startupinfoa_offset::CB;
    let bytes = (STARTUPINFOA_SIZE as u32).to_le_bytes();
    b[off] = bytes[0];
    b[off + 1] = bytes[1];
    b[off + 2] = bytes[2];
    b[off + 3] = bytes[3];
    b
};
const PROCESS_INFORMATION_BYTES: [u8; 24] = [0u8; 24];
const EXITCODE_BUF: [u8; 4] = [0u8; 4];
static SPAWN_STEPS: [Step; 4] = [
    Step {
        reach_id: R_CREATEPROCESS,
        args: &[
            ArgSrc::Const(0),
            ArgSrc::Rodata(SPAWN_CMDLINE),
            ArgSrc::Const(0),
            ArgSrc::Const(0),
            ArgSrc::Const(0),
            ArgSrc::Const(0),
            ArgSrc::Const(0),
            ArgSrc::Const(0),
            ArgSrc::Rodata(&STARTUPINFOA_BYTES),
            ArgSrc::Rodata(&PROCESS_INFORMATION_BYTES),
        ],
        out_slot: 0,
        capture_args: &[(9usize, 1usize)],
        read_out: &[],
    },
    Step {
        reach_id: R_WAIT,
        args: &[ArgSrc::SlotPtrOff(1, process_information_offset::H_PROCESS as i64, 8), ArgSrc::Const(0xFFFF_FFFFu32 as i64)],
        out_slot: 2,
        capture_args: &[],
        read_out: &[],
    },
    Step {
        reach_id: R_GETEXITCODE,
        args: &[ArgSrc::SlotPtrOff(1, process_information_offset::H_PROCESS as i64, 8), ArgSrc::Rodata(&EXITCODE_BUF)],
        out_slot: 3,
        capture_args: &[],
        read_out: &[(1usize, 5usize, 4u8)],
    },
    Step {
        reach_id: R_CLOSEHANDLE,
        args: &[ArgSrc::SlotPtrOff(1, process_information_offset::H_PROCESS as i64, 8)],
        out_slot: 6,
        capture_args: &[],
        read_out: &[],
    },
];
static SPAWN_TABLE: StepTable = StepTable { steps: &SPAWN_STEPS };

// ---- FileWrite: CreateFileA(write) -> WriteFile -> CloseHandle
// slots: 0=path(input),1=buf(input),2=len(input),3=handle,4=WriteFile BOOL,5=bytes written,6=close BOOL
const GENERIC_WRITE: i64 = 0x4000_0000;
const CREATE_ALWAYS: i64 = 2;
const FILE_ATTRIBUTE_NORMAL: i64 = 0x80;
static FILEWRITE_STEPS: [Step; 3] = [
    Step {
        reach_id: R_CREATEFILE, // same reach fn as FileOpen; only the DATA differs
        args: &[
            ArgSrc::Slot(0),
            ArgSrc::Const(GENERIC_WRITE),
            ArgSrc::Const(0),
            ArgSrc::Const(0),
            ArgSrc::Const(CREATE_ALWAYS),
            ArgSrc::Const(FILE_ATTRIBUTE_NORMAL),
            ArgSrc::Const(0),
        ],
        out_slot: 3,
        capture_args: &[],
        read_out: &[],
    },
    Step {
        reach_id: R_WRITEFILE, // same reach fn as WriteStdout's write step
        args: &[ArgSrc::Slot(3), ArgSrc::Slot(1), ArgSrc::Slot(2), ArgSrc::Rodata(&WROTE_BUF4), ArgSrc::Const(0)],
        out_slot: 4,
        capture_args: &[],
        read_out: &[(3usize, 5usize, 4u8)],
    },
    Step {
        reach_id: R_CLOSEHANDLE, // same reach fn as FileClose/SpawnWait's close step
        args: &[ArgSrc::Slot(3)],
        out_slot: 6,
        capture_args: &[],
        read_out: &[],
    },
];
static FILEWRITE_TABLE: StepTable = StepTable { steps: &FILEWRITE_STEPS };
