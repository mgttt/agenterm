//! Q22 (assembled) — the TABLE-DRIVEN OS SEAM. This is the file the whole assembly
//! exercise is really about: it replaces Q9's per-intent hardcoded `do_intent` (each
//! intent got its own inline Rust match arm calling Win32 FFI directly) with a fixed
//! dispatcher over DATA, in the vocabulary Q7 established (`Arg`/`Ret`/`OpSpec` for a
//! single native call) extended with Q21's vocabulary (`StepTable`/`Step`/`ArgSrc` for
//! a FIXED, branch-free MULTI-call sequence — reused verbatim from
//! `orchestration/step_table.rs`, engine untouched).
//!
//! ## Two mechanisms, chosen per intent by its NATIVE CALL COUNT, not asserted
//!   * `Mechanism::Single`  — exactly one native call (Alloc/FileOpen/FileRead/FileClose).
//!     Modelled on `tables/table.rs`'s `OpSpec`/`Arg`/`Ret`.
//!   * `Mechanism::Linear`  — a FIXED sequence of >1 native calls with no runtime branch
//!     (WriteStdout = GetStdHandle+WriteFile; SpawnWait = Create+Wait+GetExit+Close;
//!     FileWrite = Create+Write+Close). Modelled on `orchestration/step_table.rs`'s
//!     `StepTable`, reused UNMODIFIED (see `#[path]` in main.rs) — this file only
//!     supplies the DATA (the table values) and the REACH wrapper array it walks.
//!
//! Both mechanisms share ONE reach table (`REACH`, 9 entries — same count and same
//! symbol set as Q9's `WIN_SYMBOLS`/`seam::extern` block, cross-checked in RESULTS.md).
//! Sharing the reach table is what makes `FileWrite`'s marginal reach-code cost ZERO:
//! it reuses reach ids 1 (CreateFileA), 5 (WriteFile), 3 (CloseHandle) — every wrapper
//! it needs already exists for FileOpen/WriteStdout/FileClose/SpawnWait's CloseHandle.
//!
//! ## Q20's Kind tag — wired, honestly inert here
//! `TypedArg` carries a `Kind` (Int/Float) alongside every `Arg`, per Q20's finding
//! that float is a DATA extension of the placement axis, not a new primitive. None of
//! the seven intents assembled here (Alloc/FileOpen/FileRead/FileClose/WriteStdout/
//! SpawnWait/FileWrite) has a float argument — every Windows file/process API touched
//! is integer/pointer-only — so every `Kind` in this file is `Kind::Int`. The field is
//! present and load-bearing in the type (not a comment), but genuinely unexercised by
//! `Kind::Float`; RESULTS.md reports this as N/A-and-explained, not padded.

#![allow(dead_code)]

use crate::ir::Intent;
use crate::step_table::{ArgSrc, Step, StepTable, Wrapper};

// ============================================================================
// Reach — one thin FFI bridge per DISTINCT native function (bounded by call
// count, not by intent count — same accounting convention orchestration/RESULTS.md
// uses for its REACH array).
// ============================================================================

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
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
    VirtualAlloc(a[0] as *mut u8, a[1] as usize, a[2] as u32, a[3] as u32) as i64
}
#[cfg(windows)]
unsafe fn w_create_file(a: &[i64]) -> i64 {
    CreateFileA(a[0] as *const u8, a[1] as u32, a[2] as u32, a[3] as *mut u8, a[4] as u32, a[5] as u32, a[6] as *mut u8) as i64
}
#[cfg(windows)]
unsafe fn w_read_file(a: &[i64]) -> i64 {
    ReadFile(a[0] as *mut u8, a[1] as *mut u8, a[2] as u32, a[3] as *mut u32, a[4] as *mut u8) as i64
}
#[cfg(windows)]
unsafe fn w_close_handle(a: &[i64]) -> i64 {
    CloseHandle(a[0] as *mut u8) as i64
}
#[cfg(windows)]
unsafe fn w_get_std_handle(a: &[i64]) -> i64 {
    GetStdHandle(a[0] as u32) as i64
}
#[cfg(windows)]
unsafe fn w_write_file(a: &[i64]) -> i64 {
    WriteFile(a[0] as *mut u8, a[1] as *const u8, a[2] as u32, a[3] as *mut u32, a[4] as *mut u8) as i64
}
#[cfg(windows)]
unsafe fn w_create_process(a: &[i64]) -> i64 {
    CreateProcessA(a[0] as *const u8, a[1] as *mut u8, a[2] as *mut u8, a[3] as *mut u8, a[4] as i32, a[5] as u32, a[6] as *mut u8, a[7] as *const u8, a[8] as *mut u8, a[9] as *mut u8) as i64
}
#[cfg(windows)]
unsafe fn w_wait(a: &[i64]) -> i64 {
    WaitForSingleObject(a[0] as *mut u8, a[1] as u32) as i64
}
#[cfg(windows)]
unsafe fn w_get_exit_code(a: &[i64]) -> i64 {
    GetExitCodeProcess(a[0] as *mut u8, a[1] as *mut u32) as i64
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
// Single-call mechanism — Q7's OpSpec vocabulary, adapted for an INTERPRETER
// (args are already-resolved u64 words, not registers to place).
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Int,
    #[allow(dead_code)]
    Float, // present, unexercised here — see file header
}

#[derive(Clone, Copy)]
pub enum Arg {
    /// the k-th SEMANTIC arg of this IR call (already a resolved u64 word)
    Sem(usize),
    /// an injected target constant (Q7's L2)
    Const(i64),
    /// pointer to a fresh 4-byte out-param scratch cell (Q7's L4 OutPtr)
    OutPtr,
}
#[derive(Clone, Copy)]
pub struct TypedArg {
    pub src: Arg,
    pub kind: Kind,
}
const fn ti(src: Arg) -> TypedArg { TypedArg { src, kind: Kind::Int } }

#[derive(Clone, Copy)]
pub enum Ret {
    Direct,
    OutParam { width: u8 },
}

pub struct OpSpec {
    pub reach_id: usize,
    pub args: &'static [TypedArg],
    pub ret: Ret,
}

pub enum Mechanism {
    Single(OpSpec),
    Linear { table: &'static StepTable, input_slots: &'static [usize], result_slot: usize },
}

static ALLOC_ARGS: [TypedArg; 4] = [
    ti(Arg::Const(0)), ti(Arg::Sem(0)), ti(Arg::Const(0x3000)), ti(Arg::Const(0x04)),
];
static FILEOPEN_ARGS: [TypedArg; 7] = [
    ti(Arg::Sem(0)), ti(Arg::Const(0x8000_0000u32 as i64)), ti(Arg::Const(1)),
    ti(Arg::Const(0)), ti(Arg::Const(3)), ti(Arg::Const(0)), ti(Arg::Const(0)),
];
static FILEREAD_ARGS: [TypedArg; 5] = [
    ti(Arg::Sem(0)), ti(Arg::Sem(1)), ti(Arg::Sem(2)), ti(Arg::OutPtr), ti(Arg::Const(0)),
];
static FILECLOSE_ARGS: [TypedArg; 1] = [ti(Arg::Sem(0))];

static WRITESTDOUT_INPUT_SLOTS: [usize; 2] = [1, 2];
static SPAWN_INPUT_SLOTS: [usize; 0] = [];
static FILEWRITE_INPUT_SLOTS: [usize; 3] = [0, 1, 2];

/// THE DATA — one row per intent. Adding an intent = one match arm HERE naming a
/// Mechanism + (for Linear) a StepTable value below. `do_intent`/`exec_single`/
/// `exec_linear`/`step_table::run` never change (③④'s measured claim).
fn intent_table(intent: Intent) -> Mechanism {
    match intent {
        Intent::Alloc => Mechanism::Single(OpSpec {
            reach_id: R_ALLOC,
            args: &ALLOC_ARGS,
            ret: Ret::Direct,
        }),
        Intent::FileOpen => Mechanism::Single(OpSpec {
            reach_id: R_CREATEFILE,
            args: &FILEOPEN_ARGS,
            ret: Ret::Direct,
        }),
        Intent::FileRead => Mechanism::Single(OpSpec {
            reach_id: R_READFILE,
            args: &FILEREAD_ARGS,
            ret: Ret::OutParam { width: 4 },
        }),
        Intent::FileClose => Mechanism::Single(OpSpec {
            reach_id: R_CLOSEHANDLE,
            args: &FILECLOSE_ARGS,
            ret: Ret::Direct,
        }),
        Intent::WriteStdout => Mechanism::Linear { table: &WRITESTDOUT_TABLE, input_slots: &WRITESTDOUT_INPUT_SLOTS, result_slot: 4 },
        Intent::SpawnWait => Mechanism::Linear { table: &SPAWN_TABLE, input_slots: &SPAWN_INPUT_SLOTS, result_slot: 5 },
        // ---- ASSEMBLED (Q22, new intent): every field below is DATA. Zero lines
        // changed in exec_single/exec_linear/step_table.rs/verify.rs/eval_core.rs. ----
        Intent::FileWrite => Mechanism::Linear { table: &FILEWRITE_TABLE, input_slots: &FILEWRITE_INPUT_SLOTS, result_slot: 5 },
    }
}

fn exec_single(spec: &OpSpec, args: &[u64]) -> u64 {
    let mut outbuf: u32 = 0;
    let native_args: Vec<i64> = spec
        .args
        .iter()
        .map(|a| match a.src {
            Arg::Sem(k) => args[k] as i64,
            Arg::Const(v) => v,
            Arg::OutPtr => &mut outbuf as *mut u32 as i64,
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

/// Kept for the future ctx-word bind-time relocation Q7 used (stdout handle etc.);
/// none of these seven intents need it (WriteStdout resolves the handle via a Linear
/// step instead), so it is an empty marker today — included so the calling
/// convention (`do_intent(intent, args, ctx)`) does not need a second signature
/// change if that optimization is added later.
pub struct SeamCtx;
impl SeamCtx {
    pub fn new() -> Self { SeamCtx }
}

/// THE DISPATCHER — the only function `eval_core::run` calls. Two-way match on the
/// (data-classified) `Mechanism`, never on `Intent` directly past this single lookup.
pub fn do_intent(intent: Intent, args: &[u64], _ctx: &SeamCtx) -> u64 {
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
        args: &[ArgSrc::Const(-11)], // STD_OUTPUT_HANDLE, matches Q9's 0xFFFF_FFF5 as u32
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
// Reused verbatim (same bytes, same shape) from orchestration/main.rs::spawn_table.
// slots: 0=create BOOL, 1=hProcess (captured from pi), 2=wait raw, 3=getexit BOOL,
//        5=exit code (read_out), 6=close BOOL
const SPAWN_CMDLINE: &[u8] = b"cmd.exe /c exit 7\0";
const STARTUPINFOA_BYTES: [u8; 104] = {
    let mut b = [0u8; 104];
    b[0] = 104; // cb = 104 (u32 LE) at offset 0 — L3a, query-form origin (table.rs STARTUPINFOA)
    b
};
const PROCESS_INFORMATION_BYTES: [u8; 24] = [0u8; 24];
const EXITCODE_BUF: [u8; 4] = [0u8; 4];
static SPAWN_STEPS: [Step; 4] = [
    Step {
        reach_id: R_CREATEPROCESS,
        args: &[
            ArgSrc::Const(0), ArgSrc::Rodata(SPAWN_CMDLINE), ArgSrc::Const(0), ArgSrc::Const(0),
            ArgSrc::Const(0), ArgSrc::Const(0), ArgSrc::Const(0), ArgSrc::Const(0),
            ArgSrc::Rodata(&STARTUPINFOA_BYTES), ArgSrc::Rodata(&PROCESS_INFORMATION_BYTES),
        ],
        out_slot: 0,
        capture_args: &[(9usize, 1usize)],
        read_out: &[],
    },
    Step {
        reach_id: R_WAIT,
        args: &[ArgSrc::SlotPtrOff(1, 0, 8), ArgSrc::Const(0xFFFF_FFFFu32 as i64)],
        out_slot: 2,
        capture_args: &[],
        read_out: &[],
    },
    Step {
        reach_id: R_GETEXITCODE,
        args: &[ArgSrc::SlotPtrOff(1, 0, 8), ArgSrc::Rodata(&EXITCODE_BUF)],
        out_slot: 3,
        capture_args: &[],
        read_out: &[(1usize, 5usize, 4u8)],
    },
    Step {
        reach_id: R_CLOSEHANDLE,
        args: &[ArgSrc::SlotPtrOff(1, 0, 8)],
        out_slot: 6,
        capture_args: &[],
        read_out: &[],
    },
];
static SPAWN_TABLE: StepTable = StepTable { steps: &SPAWN_STEPS };

// ---- FileWrite (ASSEMBLED, Q22 new intent): CreateFileA(write) -> WriteFile -> CloseHandle
// slots: 0=path(input),1=buf(input),2=len(input),3=handle,4=WriteFile BOOL,5=bytes written,6=close BOOL
const GENERIC_WRITE: i64 = 0x4000_0000;
const CREATE_ALWAYS: i64 = 2;
const FILE_ATTRIBUTE_NORMAL: i64 = 0x80;
static FILEWRITE_STEPS: [Step; 3] = [
    Step {
        reach_id: R_CREATEFILE, // <- SAME reach fn as FileOpen; only the DATA differs
        args: &[
            ArgSrc::Slot(0),               // path
            ArgSrc::Const(GENERIC_WRITE),
            ArgSrc::Const(0),              // no sharing
            ArgSrc::Const(0),              // sa
            ArgSrc::Const(CREATE_ALWAYS),
            ArgSrc::Const(FILE_ATTRIBUTE_NORMAL),
            ArgSrc::Const(0),              // tmpl
        ],
        out_slot: 3,
        capture_args: &[],
        read_out: &[],
    },
    Step {
        reach_id: R_WRITEFILE, // <- SAME reach fn as WriteStdout's write step
        args: &[ArgSrc::Slot(3), ArgSrc::Slot(1), ArgSrc::Slot(2), ArgSrc::Rodata(&WROTE_BUF4), ArgSrc::Const(0)],
        out_slot: 4,
        capture_args: &[],
        read_out: &[(3usize, 5usize, 4u8)],
    },
    Step {
        reach_id: R_CLOSEHANDLE, // <- SAME reach fn as FileClose/SpawnWait's close step
        args: &[ArgSrc::Slot(3)],
        out_slot: 6,
        capture_args: &[],
        read_out: &[],
    },
];
static FILEWRITE_TABLE: StepTable = StepTable { steps: &FILEWRITE_STEPS };
