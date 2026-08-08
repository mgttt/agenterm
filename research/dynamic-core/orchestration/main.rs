//! Q21 driver — real Windows execution of the linear (branch-free) subset of
//! `SpawnWait` (Q1's L3, Q7's `spawn_boundary()`), driven ENTIRELY from
//! `step_table.rs` DATA through its one fixed control loop.
//!
//! Two runs:
//!  1. SUCCESS path: spawn `cmd.exe /c exit 7`, wait, read exit code, close.
//!     Verifies the linear-dataflow subset (Q7 spawn_boundary items 4 & 5)
//!     tabifies and executes correctly on real `kernel32`.
//!  2. FAILURE-DEMO path: spawn a nonexistent executable. `CreateProcessA`
//!     returns FALSE; the table has no way to skip the subsequent calls, so
//!     it reports a bogus "exit code 0" as if the process had run and
//!     succeeded. This is the concrete, executed demonstration of Q21 ③ —
//!     the exact point where a fixed step table runs out of expressive
//!     power without a conditional-jump primitive (see RESULTS.md).

#[path = "step_table.rs"]
mod step_table;

use step_table::{ArgSrc, Step, StepTable, Wrapper};

#[cfg(windows)]
extern "system" {
    fn CreateProcessA(
        app_name: *const u8,
        cmd_line: *mut u8,
        proc_attr: *mut u8,
        thread_attr: *mut u8,
        inherit: i32,
        flags: u32,
        env: *mut u8,
        cwd: *const u8,
        startup_info: *mut u8,
        proc_info: *mut u8,
    ) -> i32;
    fn WaitForSingleObject(h: *mut u8, millis: u32) -> u32;
    fn GetExitCodeProcess(h: *mut u8, code: *mut u32) -> i32;
    fn CloseHandle(h: *mut u8) -> i32;
    fn GetLastError() -> u32;
}

// ---- reach wrappers: the ONLY per-op code in this file, one thin FFI
// bridge per distinct host call, exactly the shape Q7's reach/symbol layer
// already established as legitimate "fixed" cost (bounded, not O(intents)).
#[cfg(windows)]
unsafe fn w_create_process(a: &[i64]) -> i64 {
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
#[cfg(windows)]
unsafe fn w_wait(a: &[i64]) -> i64 {
    WaitForSingleObject(a[0] as *mut u8, a[1] as u32) as i64
}
#[cfg(windows)]
unsafe fn w_get_exit_code(a: &[i64]) -> i64 {
    GetExitCodeProcess(a[0] as *mut u8, a[1] as *mut u32) as i64
}
#[cfg(windows)]
unsafe fn w_close_handle(a: &[i64]) -> i64 {
    CloseHandle(a[0] as *mut u8) as i64
}

#[cfg(windows)]
const REACH: &[Wrapper] = &[w_create_process, w_wait, w_get_exit_code, w_close_handle];
const R_CREATE: usize = 0;
const R_WAIT: usize = 1;
const R_GETEXIT: usize = 2;
const R_CLOSE: usize = 3;

// ---- DATA: STARTUPINFOA / PROCESS_INFORMATION content. Once L3a's layout
// facts are known (Q7: query-form data, `cb` offset 0, size 104; hProcess
// offset 0 of a 24-byte PROCESS_INFORMATION) the struct *content* to pass
// in is itself a constant byte blob — a rodata table, not code.
const STARTUPINFOA_BYTES: [u8; 104] = {
    let mut b = [0u8; 104];
    // cb = 104 (u32 LE) at offset 0 — the one field CreateProcessA requires
    // the caller to fill in; everything else zero-initialized is valid.
    b[0] = 104;
    b[1] = 0;
    b[2] = 0;
    b[3] = 0;
    b
};
const PROCESS_INFORMATION_BYTES: [u8; 24] = [0u8; 24]; // OS fills this in; zero going in.
const EXITCODE_BUF: [u8; 4] = [0u8; 4];

fn spawn_table(cmdline: &'static [u8]) -> StepTable {
    StepTable {
        steps: Box::leak(Box::new([
            Step {
                reach_id: R_CREATE,
                args: Box::leak(Box::new([
                    ArgSrc::Const(0), // lpApplicationName = NULL
                    ArgSrc::Rodata(cmdline),
                    ArgSrc::Const(0), // proc attr
                    ArgSrc::Const(0), // thread attr
                    ArgSrc::Const(0), // bInheritHandles = FALSE
                    ArgSrc::Const(0), // dwCreationFlags
                    ArgSrc::Const(0), // lpEnvironment
                    ArgSrc::Const(0), // lpCurrentDirectory
                    ArgSrc::Rodata(&STARTUPINFOA_BYTES),
                    ArgSrc::Rodata(&PROCESS_INFORMATION_BYTES),
                ])),
                out_slot: 0, // BOOL success
                capture_args: Box::leak(Box::new([(9usize, 1usize)])), // pi ptr -> slot 1
                read_out: &[],
            },
            Step {
                reach_id: R_WAIT,
                args: Box::leak(Box::new([
                    ArgSrc::SlotPtrOff(1, 0, 8), // hProcess = *(pi+0), Q7 item 4
                    ArgSrc::Const(0xFFFF_FFFFu32 as i64), // INFINITE
                ])),
                out_slot: 2,
                capture_args: &[],
                read_out: &[],
            },
            Step {
                reach_id: R_GETEXIT,
                args: Box::leak(Box::new([
                    ArgSrc::SlotPtrOff(1, 0, 8), // same hProcess dataflow, second use
                    ArgSrc::Rodata(&EXITCODE_BUF),
                ])),
                out_slot: 3, // BOOL success of GetExitCodeProcess itself
                capture_args: &[],
                read_out: Box::leak(Box::new([(1usize, 5usize, 4u8)])), // *(args[1])+0, 4B -> slot 5
            },
            Step {
                reach_id: R_CLOSE,
                args: Box::leak(Box::new([ArgSrc::SlotPtrOff(1, 0, 8)])),
                out_slot: 6,
                capture_args: &[],
                read_out: &[],
            },
        ])),
    }
}

/// FAILURE-DEMO table: identical shape, 3 steps (no Close, to avoid the
/// undefined-behaviour edge of CloseHandle(NULL)). Same fixed engine, same
/// step *shape* — only the DATA (command line) differs, and that data
/// happens to make step 0 fail. Nothing in the table changes to handle
/// that; there is nothing IN THE SCHEMA that could make it change.
fn spawn_table_failure_demo(cmdline: &'static [u8]) -> StepTable {
    StepTable {
        steps: Box::leak(Box::new([
            Step {
                reach_id: R_CREATE,
                args: Box::leak(Box::new([
                    ArgSrc::Const(0),
                    ArgSrc::Rodata(cmdline),
                    ArgSrc::Const(0),
                    ArgSrc::Const(0),
                    ArgSrc::Const(0),
                    ArgSrc::Const(0),
                    ArgSrc::Const(0),
                    ArgSrc::Const(0),
                    ArgSrc::Rodata(&STARTUPINFOA_BYTES),
                    ArgSrc::Rodata(&PROCESS_INFORMATION_BYTES),
                ])),
                out_slot: 0,
                capture_args: Box::leak(Box::new([(9usize, 1usize)])),
                read_out: &[],
            },
            Step {
                reach_id: R_WAIT,
                args: Box::leak(Box::new([
                    ArgSrc::SlotPtrOff(1, 0, 8),
                    ArgSrc::Const(0xFFFF_FFFFu32 as i64),
                ])),
                out_slot: 2,
                capture_args: &[],
                read_out: &[],
            },
            Step {
                reach_id: R_GETEXIT,
                args: Box::leak(Box::new([
                    ArgSrc::SlotPtrOff(1, 0, 8),
                    ArgSrc::Rodata(&EXITCODE_BUF),
                ])),
                out_slot: 3,
                capture_args: &[],
                read_out: Box::leak(Box::new([(1usize, 5usize, 4u8)])),
            },
        ])),
    }
}

#[cfg(windows)]
fn main() {
    println!("== Q21 orchestration-as-data: success path (linear subset) ==");
    let table = spawn_table(b"cmd.exe /c exit 7\0");
    let mut slots = [0i64; 8];
    step_table::run(&table, REACH, &mut slots);
    let create_ok = slots[0] != 0;
    let exit_code = slots[5];
    println!("CreateProcessA success = {create_ok}");
    println!("WaitForSingleObject raw = 0x{:x}", slots[2] as u32);
    println!("exit code (read purely from the step table's DATA-driven field-read) = {exit_code}");
    assert!(create_ok, "CreateProcessA must succeed for the success-path assertion to be meaningful");
    assert_eq!(exit_code, 7, "expected exit code 7 from `cmd.exe /c exit 7`, table-driven end to end");
    println!("PASS: linear 4-step SpawnWait orchestration executed entirely from DATA, zero per-op branch in the control loop.\n");

    println!("== Q21 ③: failure-path demonstration (why the linear-only table breaks) ==");
    let ftable = spawn_table_failure_demo(b"C:\\definitely_missing_xyz_q21.exe\0");
    let mut fslots = [0i64; 8];
    step_table::run(&ftable, REACH, &mut fslots);
    let fcreate_ok = fslots[0] != 0;
    let fexit_code = fslots[5];
    let last_err = unsafe { GetLastError() };
    println!("CreateProcessA success = {fcreate_ok} (GetLastError = {last_err}, expect 2 = ERROR_FILE_NOT_FOUND)");
    println!("WaitForSingleObject raw = 0x{:x} (expect 0xffffffff = WAIT_FAILED, NULL handle)", fslots[2] as u32);
    println!("exit code the table reports anyway = {fexit_code}");
    assert!(!fcreate_ok, "the demo command line must fail to spawn for this to demonstrate the point");
    println!("FINDING: CreateProcessA failed (no process ever ran), yet the linear step table");
    println!("has NO FIELD that can express \"skip Wait/GetExitCode when step 0 failed\" — it");
    println!("reports exit code {fexit_code} as if a process had run and exited cleanly. The table");
    println!("schema has no capability to make step 1/2's execution CONTINGENT on step 0's");
    println!("result. That contingency is exactly what Q21 ③ names as the missing capability.");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Q21 orchestration driver targets Win32 CreateProcessA; not portable to this host by design (matches Q1/Q7 posture: Windows executed, SysV analyzed).");
}
