//! Q8 measurement harness part 2 — conditional / alternative paths under ACG.
//!
//! T1: does code made RX *before* ACG is enabled still execute *after*? (the
//!     "generate-up-front, then lock down" pattern — a one-shot AOT alternative)
//! T2: ProcessDynamicCodePolicy with AllowThreadOptOut=1, then opt the current
//!     thread out via SetThreadInformation(ThreadDynamicCodePolicy, ALLOW).
//!     Does the RW->RX flip then succeed on the opted-out thread? (a documented
//!     "conditional yes": ACG process-wide + a designated JIT thread)
//!
//! No external crates. Clean-room; Win32 contracts are public.

#![allow(non_snake_case)]
use std::ffi::c_void;
type HANDLE = *mut c_void;
type BOOL = i32;

#[link(name = "kernel32")]
extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, typ: u32, protect: u32) -> *mut c_void;
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> BOOL;
    fn GetLastError() -> u32;
    fn GetCurrentProcess() -> HANDLE;
    fn GetCurrentThread() -> HANDLE;
    fn FlushInstructionCache(proc: HANDLE, base: *const c_void, size: usize) -> BOOL;
    fn SetProcessMitigationPolicy(policy: i32, buf: *mut c_void, len: usize) -> BOOL;
    fn SetThreadInformation(th: HANDLE, class: i32, buf: *mut c_void, len: u32) -> BOOL;
}

const MEM_COMMIT_RESERVE: u32 = 0x3000;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;
const PROCESS_MITIGATION_DYNAMIC_CODE_POLICY: i32 = 2;
const THREAD_DYNAMIC_CODE_POLICY: i32 = 2;
const THREAD_DYNAMIC_CODE_ALLOW: u32 = 1;

const STUB: [u8; 6] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3]; // mov eax,42 ; ret

fn err() -> u32 {
    unsafe { GetLastError() }
}

unsafe fn make_rx() -> Option<*mut c_void> {
    let mem = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_READWRITE);
    if mem.is_null() {
        println!("       VirtualAlloc(RW) NULL err={}", err());
        return None;
    }
    std::ptr::copy_nonoverlapping(STUB.as_ptr(), mem as *mut u8, STUB.len());
    let mut old = 0u32;
    if VirtualProtect(mem, 4096, PAGE_EXECUTE_READ, &mut old) == 0 {
        println!("       VirtualProtect(->RX) FALSE err={}", err());
        return None;
    }
    Some(mem)
}

unsafe fn run(mem: *mut c_void) -> u64 {
    FlushInstructionCache(GetCurrentProcess(), mem, STUB.len());
    let f: extern "C" fn() -> u64 = std::mem::transmute(mem);
    f()
}

fn enable_acg(flags: u32) -> Result<(), u32> {
    let mut v = flags;
    let ok = unsafe {
        SetProcessMitigationPolicy(
            PROCESS_MITIGATION_DYNAMIC_CODE_POLICY,
            &mut v as *mut u32 as *mut c_void,
            4,
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(err())
    }
}

fn t1_pre_acg_rx_survives() {
    println!("[T1] RX made BEFORE ACG, executed AFTER ACG:");
    unsafe {
        let mem = match make_rx() {
            Some(m) => m,
            None => {
                println!("     setup failed");
                return;
            }
        };
        let before = run(mem);
        println!("     before ACG: returned {}", before);
        // ProhibitDynamicCode=1 only.
        match enable_acg(1) {
            Ok(()) => println!("     ACG enabled (ProhibitDynamicCode=1)"),
            Err(e) => {
                println!("     enable_acg failed err={}", e);
                return;
            }
        }
        let after = run(mem);
        println!(
            "     after ACG: returned {}  => pre-existing RX still executes: {}",
            after,
            after == 42
        );
    }
}

fn t2_thread_opt_out() {
    println!("[T2] ACG with AllowThreadOptOut, current thread opts out, then RW->RX flip:");
    // ProhibitDynamicCode=1 | AllowThreadOptOut=1  => bits 0 and 1 => 0b11 = 3
    match enable_acg(0b11) {
        Ok(()) => println!("     ACG enabled (ProhibitDynamicCode=1 | AllowThreadOptOut=1)"),
        Err(e) => {
            println!("     enable_acg failed err={}", e);
            return;
        }
    }
    // Sanity: without opting out, a flip should be blocked.
    unsafe {
        let mem = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_READWRITE);
        if !mem.is_null() {
            std::ptr::copy_nonoverlapping(STUB.as_ptr(), mem as *mut u8, STUB.len());
            let mut old = 0u32;
            let ok = VirtualProtect(mem, 4096, PAGE_EXECUTE_READ, &mut old);
            println!(
                "     pre-opt-out flip: {} (err={})",
                if ok != 0 { "SUCCEEDED (unexpected)" } else { "blocked (expected)" },
                if ok != 0 { 0 } else { err() }
            );
        }
    }
    // Opt the current thread out.
    let mut allow: u32 = THREAD_DYNAMIC_CODE_ALLOW;
    let ok = unsafe {
        SetThreadInformation(
            GetCurrentThread(),
            THREAD_DYNAMIC_CODE_POLICY,
            &mut allow as *mut u32 as *mut c_void,
            4,
        )
    };
    if ok == 0 {
        println!("     SetThreadInformation(ThreadDynamicCodePolicy, ALLOW) FAILED err={}", err());
        return;
    }
    println!("     current thread opted out (SetThreadInformation ALLOW ok)");
    unsafe {
        match make_rx() {
            Some(mem) => {
                let got = run(mem);
                println!(
                    "     post-opt-out RW->RX flip + jump: returned {}  => conditional path works: {}",
                    got,
                    got == 42
                );
            }
            None => println!("     post-opt-out flip STILL blocked"),
        }
    }
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "t1".into());
    println!("=== Q8 conditional-path probe: {} ===", which);
    match which.as_str() {
        "t1" => t1_pre_acg_rx_survives(),
        "t2" => t2_thread_opt_out(),
        _ => println!("unknown"),
    }
    println!("=== done ===");
}
