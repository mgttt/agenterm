//! Q8 measurement harness — Windows/x86_64 executable-memory policy probe.
//!
//! Answers, BY MEASUREMENT on the local machine, whether primitives ①(mem RW↔RX)
//! and ②(jump into that memory) actually work, under two policies:
//!   * `default` — the process's inherited default mitigation policy
//!   * `acg`     — after enabling Arbitrary Code Guard (ProcessDynamicCodePolicy,
//!                 ProhibitDynamicCode) on THIS process at runtime
//!
//! Three independent code-execution mechanisms are exercised each run:
//!   M1  VirtualAlloc(RW) -> write -> VirtualProtect(RX) -> jump   (the Q0 kernel path)
//!   M2  VirtualAlloc(RWX) directly -> write -> jump               (one-shot RWX)
//!   M3  CreateFileMapping(PAGE_EXECUTE_RW, pagefile) -> MapView(FILE_MAP_EXECUTE)
//!       -> write -> jump                                          (section-object route)
//!
//! Each step prints its result and GetLastError on failure. No external crates.
//! Clean-room: no existing implementation was read; Win32 contracts are public.

#![allow(non_snake_case, non_camel_case_types)]
use std::ffi::c_void;

type HANDLE = *mut c_void;
type BOOL = i32;

#[link(name = "kernel32")]
extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, typ: u32, protect: u32) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> BOOL;
    fn VirtualFree(addr: *mut c_void, size: usize, typ: u32) -> BOOL;
    fn GetLastError() -> u32;
    fn GetCurrentProcess() -> HANDLE;
    fn FlushInstructionCache(proc: HANDLE, base: *const c_void, size: usize) -> BOOL;
    fn SetProcessMitigationPolicy(policy: i32, buf: *mut c_void, len: usize) -> BOOL;
    fn GetProcessMitigationPolicy(proc: HANDLE, policy: i32, buf: *mut c_void, len: usize) -> BOOL;
    fn CreateFileMappingA(
        file: HANDLE,
        attrs: *mut c_void,
        protect: u32,
        maxhi: u32,
        maxlo: u32,
        name: *const u8,
    ) -> HANDLE;
    fn MapViewOfFile(map: HANDLE, access: u32, offhi: u32, offlo: u32, len: usize) -> *mut c_void;
    fn UnmapViewOfFile(base: *const c_void) -> BOOL;
    fn CloseHandle(h: HANDLE) -> BOOL;
}

const MEM_COMMIT_RESERVE: u32 = 0x3000;
const MEM_RELEASE: u32 = 0x8000;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const FILE_MAP_WRITE: u32 = 0x0002;
const FILE_MAP_EXECUTE: u32 = 0x0020;
const INVALID_HANDLE_VALUE: HANDLE = usize::MAX as HANDLE;
const PROCESS_MITIGATION_DYNAMIC_CODE_POLICY: i32 = 2;

// x86_64: `mov eax, 0x2A ; ret`  -> returns 42
const STUB: [u8; 6] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3];
const WANT: u64 = 42;

fn err() -> u32 {
    unsafe { GetLastError() }
}

/// Try to enable ACG on this process at runtime. Returns Ok(()) or the error code.
fn enable_acg() -> Result<(), u32> {
    // PROCESS_MITIGATION_DYNAMIC_CODE_POLICY = one DWORD of bit-flags.
    // bit0 = ProhibitDynamicCode.
    let mut val: u32 = 1;
    let ok = unsafe {
        SetProcessMitigationPolicy(
            PROCESS_MITIGATION_DYNAMIC_CODE_POLICY,
            &mut val as *mut u32 as *mut c_void,
            4,
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(err())
    }
}

fn acg_state() -> u32 {
    let mut val: u32 = 0;
    unsafe {
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            PROCESS_MITIGATION_DYNAMIC_CODE_POLICY,
            &mut val as *mut u32 as *mut c_void,
            4,
        );
    }
    val
}

unsafe fn run_stub(mem: *mut c_void) -> u64 {
    FlushInstructionCache(GetCurrentProcess(), mem, STUB.len());
    let f: extern "C" fn() -> u64 = std::mem::transmute(mem);
    f()
}

/// M1: VirtualAlloc(RW) -> write -> VirtualProtect(RX) -> jump. (the Q0 kernel path)
fn m1_rw_then_rx() {
    println!("  [M1] VirtualAlloc(RW) -> VirtualProtect(RX) -> jump  (Q0 kernel path)");
    unsafe {
        let mem = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_READWRITE);
        if mem.is_null() {
            println!("       FAIL: VirtualAlloc(RW) returned NULL, err={}", err());
            return;
        }
        std::ptr::copy_nonoverlapping(STUB.as_ptr(), mem as *mut u8, STUB.len());
        let mut old = 0u32;
        let ok = VirtualProtect(mem, 4096, PAGE_EXECUTE_READ, &mut old);
        if ok == 0 {
            println!(
                "       FAIL: VirtualProtect(->RX) returned FALSE, err={}  <-- RW->RX flip refused",
                err()
            );
            VirtualFree(mem, 0, MEM_RELEASE);
            return;
        }
        let got = run_stub(mem);
        println!("       OK: jumped in, returned {} (want {})", got, WANT);
        VirtualFree(mem, 0, MEM_RELEASE);
        let _ = got == WANT;
    }
}

/// M2: VirtualAlloc(RWX) directly -> write -> jump.
fn m2_direct_rwx() {
    println!("  [M2] VirtualAlloc(RWX) directly -> write -> jump");
    unsafe {
        let mem = VirtualAlloc(
            std::ptr::null_mut(),
            4096,
            MEM_COMMIT_RESERVE,
            PAGE_EXECUTE_READWRITE,
        );
        if mem.is_null() {
            println!(
                "       FAIL: VirtualAlloc(RWX) returned NULL, err={}  <-- executable alloc refused",
                err()
            );
            return;
        }
        std::ptr::copy_nonoverlapping(STUB.as_ptr(), mem as *mut u8, STUB.len());
        let got = run_stub(mem);
        println!("       OK: jumped in, returned {} (want {})", got, WANT);
        VirtualFree(mem, 0, MEM_RELEASE);
    }
}

/// M3: section object mapped executable (pagefile-backed, no disk).
fn m3_section() {
    println!("  [M3] CreateFileMapping(EXECUTE_RW,pagefile) -> MapView(EXECUTE) -> jump");
    unsafe {
        let hmap = CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            std::ptr::null_mut(),
            PAGE_EXECUTE_READWRITE,
            0,
            4096,
            std::ptr::null(),
        );
        if hmap.is_null() {
            println!("       FAIL: CreateFileMapping(EXECUTE_RW) NULL, err={}", err());
            return;
        }
        let view = MapViewOfFile(hmap, FILE_MAP_WRITE | FILE_MAP_EXECUTE, 0, 0, 4096);
        if view.is_null() {
            println!(
                "       FAIL: MapViewOfFile(FILE_MAP_EXECUTE) NULL, err={}  <-- exec section view refused",
                err()
            );
            CloseHandle(hmap);
            return;
        }
        std::ptr::copy_nonoverlapping(STUB.as_ptr(), view as *mut u8, STUB.len());
        let got = run_stub(view);
        println!("       OK: jumped in, returned {} (want {})", got, WANT);
        UnmapViewOfFile(view);
        CloseHandle(hmap);
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "default".into());
    println!("=== Q8 Windows executable-memory probe: mode={} ===", mode);

    if mode == "acg" {
        match enable_acg() {
            Ok(()) => println!(
                "ACG: SetProcessMitigationPolicy(DynamicCode, ProhibitDynamicCode=1) SUCCEEDED at runtime; policy now = {:#x}",
                acg_state()
            ),
            Err(e) => {
                println!(
                    "ACG: SetProcessMitigationPolicy FAILED at runtime, err={} (policy = {:#x}). \
                     Cannot self-enable; would need creation-time attribute.",
                    e,
                    acg_state()
                );
            }
        }
    } else {
        println!("policy (DynamicCode) at start = {:#x}", acg_state());
    }

    m1_rw_then_rx();
    m2_direct_rwx();
    m3_section();
    println!("=== done ({}) ===", mode);
}
