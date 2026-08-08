//! Q12 criterion ③ — the placement / reachability gate, MEASURED.
//!
//! Survey (reference §7.4) claim, transcribed & unverified:
//!   "R_X86_64_PC32 requires the code buffer land within ±2GB of every runtime symbol it
//!    calls; AArch64 CALL26 requires ±128MB. The failure mode is a SILENTLY TRUNCATED
//!    relocation, not an error."
//!
//! This probe confirms the FAILURE MODE on x86_64: emit a `call rel32` whose target is
//! >2GB away, using the EXACT back-patch arithmetic Q2's lower.rs::patch() uses
//! (`let rel = (target - (site+4)) as i32`), and show that:
//!   (a) the emit/back-patch step returns NO error — the `as i32` cast truncates silently;
//!   (b) execution then transfers to the WRONG address (a decoy we plant there returns 99,
//!       or, absent a decoy, an access violation) — never the intended target.
//! A near (<2GB) target with the identical code path runs correctly (42), isolating the
//! cause to reach, not to the mechanism.
//!
//! Clean-room; kernel32 FFI only.

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
    fn FlushInstructionCache(proc: HANDLE, base: *const c_void, size: usize) -> BOOL;
}

const MEM_COMMIT_RESERVE: u32 = 0x3000;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const PAGE_EXECUTE_READ: u32 = 0x20;

fn err() -> u32 {
    unsafe { GetLastError() }
}

// The intended callee: `mov eax, 42 ; ret`.
const TARGET42: [u8; 6] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3];
// The decoy we plant at the TRUNCATED address: `mov eax, 99 ; ret`.
const DECOY99: [u8; 6] = [0xB8, 0x63, 0x00, 0x00, 0x00, 0xC3];

/// Try to commit `size` executable-writable bytes at (or very near) `want`. Returns the
/// actual base or null. We probe a few nearby 64KB-aligned bases because a specific page
/// may be taken.
unsafe fn alloc_near(want: u64, size: usize, protect: u32) -> *mut c_void {
    let base = want & !0xFFFFu64; // 64KB granularity
    for i in 0..64u64 {
        let cand = (base + i * 0x10000) as *mut c_void;
        let p = VirtualAlloc(cand, size, MEM_COMMIT_RESERVE, protect);
        if !p.is_null() {
            return p;
        }
    }
    std::ptr::null_mut()
}

/// Build a source function `S` = [E8 rel32][C3] (call rel32 ; ret). `rel32` is computed
/// with Q2's identical formula and cast, targeting `target`. Returns the S base.
unsafe fn build_source(target: u64) -> (*mut c_void, i32, i64) {
    let s = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_READWRITE);
    assert!(!s.is_null(), "alloc S failed err={}", err());
    let sb = s as u64;
    // opcode E8 at S+0, rel32 field at S+1, next-insn at S+5. site = S+1, site+4 = S+5.
    let site = sb + 1;
    let full_delta: i64 = target as i64 - (site as i64 + 4);
    // === Q2 lower.rs::patch() arithmetic, verbatim: `as i32` — silent truncation. ===
    let rel: i32 = full_delta as i32; // <-- NO error, NO check, even when it doesn't fit
    let code: [u8; 6] = {
        let mut b = [0u8; 6];
        b[0] = 0xE8;
        b[1..5].copy_from_slice(&rel.to_le_bytes());
        b[5] = 0xC3;
        b
    };
    std::ptr::copy_nonoverlapping(code.as_ptr(), s as *mut u8, 6);
    let mut old = 0u32;
    VirtualProtect(s, 4096, PAGE_EXECUTE_READ, &mut old);
    FlushInstructionCache(GetCurrentProcess(), s, 6);
    (s, rel, full_delta)
}

fn run(s: *mut c_void) -> u64 {
    let f: extern "sysv64" fn() -> u64 = unsafe { std::mem::transmute(s) };
    f()
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "near".into());
    println!("=== Q12 ③ placement/reach probe: mode={} ===", mode);

    unsafe {
        match mode.as_str() {
            // NEAR: target within ±2GB. Identical code path; must run and return 42.
            "near" => {
                let t = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_EXECUTE_READWRITE);
                assert!(!t.is_null());
                std::ptr::copy_nonoverlapping(TARGET42.as_ptr(), t as *mut u8, TARGET42.len());
                FlushInstructionCache(GetCurrentProcess(), t, TARGET42.len());
                let (s, rel, delta) = build_source(t as u64);
                println!("  target=0x{:016x}  S=0x{:016x}", t as u64, s as u64);
                println!("  full 64-bit delta = {} (0x{:x}); fits in i32 = {}", delta, delta, delta >= i32::MIN as i64 && delta <= i32::MAX as i64);
                println!("  emitted rel32 = {} (0x{:08x})  -- no error reported by emit", rel, rel as u32);
                let got = run(s);
                println!("  EXECUTED: returned {} (want 42) => {}", got, if got == 42 { "OK, target reached" } else { "WRONG" });
            }

            // FAR: target >2GB away. Same emit path. Plant a decoy at the TRUNCATED address
            // so the mis-jump lands on valid code returning 99 -> proves SILENT wrong jump
            // (no error at emit, no crash, just the wrong callee). If the decoy page is
            // unavailable, run anyway: the truncated call access-violates (still silent@emit).
            "far" => {
                let s_probe = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_READWRITE);
                let s_hint = s_probe as u64;
                // Put the real target ~3GB above S. 3GB > 2GB so rel32 MUST truncate.
                let want_target = s_hint.wrapping_add(0xC000_0000);
                let t = alloc_near(want_target, 4096, PAGE_EXECUTE_READWRITE);
                if t.is_null() {
                    println!("  could not allocate a >2GB-away target on this run; try again.");
                    return;
                }
                std::ptr::copy_nonoverlapping(TARGET42.as_ptr(), t as *mut u8, TARGET42.len());
                FlushInstructionCache(GetCurrentProcess(), t, TARGET42.len());

                let (s, rel, delta) = build_source(t as u64);
                let site = s as u64 + 1;
                // Where the truncated rel32 ACTUALLY points:
                let effective: u64 = (site + 4).wrapping_add(rel as i64 as u64);
                println!("  intended target = 0x{:016x}", t as u64);
                println!("  S               = 0x{:016x}", s as u64);
                println!("  full 64-bit delta = {} (0x{:x}); fits in i32 = {}", delta, delta, delta >= i32::MIN as i64 && delta <= i32::MAX as i64);
                println!("  emitted rel32   = {} (0x{:08x})  -- EMIT REPORTED NO ERROR", rel, rel as u32);
                println!("  effective (truncated) call target = 0x{:016x}", effective);
                println!("  => truncation moved the call by {} bytes away from intent", (t as u64).wrapping_sub(effective) as i64);

                // Plant decoy at the effective (wrong) address so we get a clean, crash-free
                // demonstration of a SILENT wrong jump.
                let decoy = alloc_near(effective & !0xFFFu64, 4096, PAGE_EXECUTE_READWRITE);
                let mut decoy_ok = false;
                if !decoy.is_null() {
                    let db = decoy as u64;
                    let page = effective & !0xFFFu64;
                    if page >= db && (effective + 6) <= db + 4096 {
                        let off = (effective - db) as usize;
                        std::ptr::copy_nonoverlapping(DECOY99.as_ptr(), (decoy as *mut u8).add(off), DECOY99.len());
                        FlushInstructionCache(GetCurrentProcess(), (decoy as *const u8).add(off) as *const c_void, DECOY99.len());
                        decoy_ok = true;
                        println!("  planted decoy `mov eax,99;ret` AT the truncated address 0x{:016x}", effective);
                    }
                }
                if !decoy_ok {
                    println!("  (no decoy planted at truncated address; call will access-violate)");
                }
                println!("  EXECUTING the far call now...");
                let got = run(s);
                println!("  EXECUTED: returned {} => {}", got, if got == 42 {
                    "reached intent (unexpected)"
                } else if got == 99 {
                    "SILENTLY jumped to the WRONG callee (decoy 99), NOT the intended 42"
                } else {
                    "returned garbage from wrong location"
                });
            }
            _ => println!("usage: probe_reach [near|far]"),
        }
    }
    println!("=== done ({}) ===", mode);
}
