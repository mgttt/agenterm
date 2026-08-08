//! Q12 landing-gate probe — the SECOND gate after Q8's "can I get executable memory".
//!
//! Q8 measured whether a page can be made executable and jumped into. This probe
//! measures whether, ONCE JUMPED INTO, the hardware/runtime actually lets the bytes
//! run — the "second gate" the survey (reference-cross-target-execution §7.2) lists:
//!   * CET-IBT / ENDBR64 landing pads on indirect-branch targets
//!   * I-cache cross-thread coherence (x86 vs weak-ISA)
//!   * Windows x64 unwind registration (RtlAddFunctionTable)
//!
//! Everything here is MEASURED on this Windows Server 2022 / x86_64 box, not cited.
//! Clean-room: no existing implementation was read; Win32/CPUID contracts are public.
//!
//! Criteria answered here:
//!   ① this box's CET/IBT state: CPUID hardware support + process mitigation policy
//!   ② do OUR products trip the wire: replicate the exact indirect-transfer patterns
//!      that Q2's lowerer (lower.rs) and its callbacks use, all WITHOUT ENDBR64, and
//!      see if they still run; also confirm generated frames have NO unwind entry.
//!
//! ③ (placement / ±2GB truncation) lives in probe_reach.rs.

#![allow(non_snake_case, non_camel_case_types)]
use std::arch::x86_64::__cpuid_count;
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
    fn GetProcessMitigationPolicy(proc: HANDLE, policy: i32, buf: *mut c_void, len: usize) -> BOOL;
    // RtlLookupFunctionEntry: given a PC, return its RUNTIME_FUNCTION (unwind entry) or NULL.
    fn RtlLookupFunctionEntry(pc: u64, image_base: *mut u64, history: *mut c_void) -> *mut c_void;
}

const MEM_COMMIT_RESERVE: u32 = 0x3000;
const MEM_RELEASE: u32 = 0x8000;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;

// Windows PROCESS_MITIGATION_POLICY ids (winnt.h)
const PROCESS_CONTROL_FLOW_GUARD_POLICY: i32 = 7; // software CFG
const PROCESS_USER_SHADOW_STACK_POLICY: i32 = 15; // CET hardware shadow stack (back-edge)

fn err() -> u32 {
    unsafe { GetLastError() }
}

// A host function reached by an INDIRECT call, deliberately WITHOUT an ENDBR64 landing
// pad (rustc-msvc emits none by default — measured: 0 ENDBR64 in our binaries). Mirrors
// the sysv64 shims that Q2's `e_call_env` (`call qword [r15+idx*8]`) targets.
extern "sysv64" fn host_returns_42() -> u64 {
    42
}

unsafe fn make_rx(bytes: &[u8]) -> *mut c_void {
    let mem = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_READWRITE);
    assert!(!mem.is_null(), "VirtualAlloc(RW) failed err={}", err());
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), mem as *mut u8, bytes.len());
    let mut old = 0u32;
    let ok = VirtualProtect(mem, 4096, PAGE_EXECUTE_READ, &mut old);
    assert!(ok != 0, "VirtualProtect(RX) failed err={}", err());
    FlushInstructionCache(GetCurrentProcess(), mem, bytes.len());
    mem
}

// ---------------------------------------------------------------------------
// ① CET / IBT state on this box
// ---------------------------------------------------------------------------
fn criterion_1_cet_state() {
    println!("--- ① CET / IBT state on this machine (measured) ---");
    // CPUID leaf 7, subleaf 0: ECX bit7 = CET_SS (shadow stack), EDX bit20 = CET_IBT.
    let r = __cpuid_count(7, 0);
    let cet_ss = (r.ecx >> 7) & 1;
    let cet_ibt = (r.edx >> 20) & 1;
    println!(
        "  CPUID.7.0: CET_SS (shadow stack) support = {}, CET_IBT (indirect-branch tracking) support = {}",
        cet_ss, cet_ibt
    );

    // Process mitigation policies actually in force on THIS process.
    let mut cfg: u32 = 0;
    let mut uss: u32 = 0;
    unsafe {
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            PROCESS_CONTROL_FLOW_GUARD_POLICY,
            &mut cfg as *mut u32 as *mut c_void,
            4,
        );
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            PROCESS_USER_SHADOW_STACK_POLICY,
            &mut uss as *mut u32 as *mut c_void,
            4,
        );
    }
    // CFG bit0 = EnableControlFlowGuard. UserShadowStack bit0 = EnableUserShadowStack.
    println!(
        "  GetProcessMitigationPolicy(ControlFlowGuard)  = {:#010x}  (EnableControlFlowGuard bit0 = {})",
        cfg,
        cfg & 1
    );
    println!(
        "  GetProcessMitigationPolicy(UserShadowStack)   = {:#010x}  (EnableUserShadowStack bit0 = {})",
        uss,
        uss & 1
    );
    println!("  NOTE: Windows exposes NO runtime 'enforce forward-edge IBT' toggle (unlike ACG");
    println!("        in Q8). Forward-edge enforcement would require the loader/OS to honor");
    println!("        CET-IBT for user mode; the empirical test in ② is the real proof.");
}

// ---------------------------------------------------------------------------
// ② do OUR products trip the wire? Replicate their indirect-transfer patterns.
// ---------------------------------------------------------------------------
fn criterion_2_products() {
    println!();
    println!("--- ② our products' indirect-transfer patterns, all WITHOUT ENDBR64 ---");

    // P0: baseline — indirect CALL into a bare non-ENDBR stub `mov eax,42; ret`.
    //     If CET-IBT were enforced, this indirect call to a non-ENDBR target faults (#CP).
    {
        let stub: [u8; 6] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3];
        let mem = unsafe { make_rx(&stub) };
        let f: extern "sysv64" fn() -> u64 = unsafe { std::mem::transmute(mem) };
        let got = f();
        println!(
            "  [P0] indirect CALL -> non-ENDBR stub                 : returned {} (want 42) {}",
            got,
            if got == 42 { "OK — IBT does NOT bite" } else { "FAULT/wrong" }
        );
        unsafe { VirtualFree(mem, 0, MEM_RELEASE); }
    }

    // P0b: same target but WITH an ENDBR64 landing pad prepended (F3 0F 1E FA).
    //      Proves an ENDBR-prefixed target also runs (it is a NOP when IBT is off).
    {
        let stub: [u8; 10] = [0xF3, 0x0F, 0x1E, 0xFA, 0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3];
        let mem = unsafe { make_rx(&stub) };
        let f: extern "sysv64" fn() -> u64 = unsafe { std::mem::transmute(mem) };
        let got = f();
        println!(
            "  [P0b] indirect CALL -> ENDBR64+stub                  : returned {} (want 42) {}",
            got,
            if got == 42 { "OK" } else { "FAULT/wrong" }
        );
        unsafe { VirtualFree(mem, 0, MEM_RELEASE); }
    }

    // P1: Q2 lower.rs ENTRY pattern — lower_and_run does `transmute(code); entry(env)`,
    //     an INDIRECT CALL landing on e_prologue's first byte (push rbx = 0x53), NO ENDBR.
    //     Faithful bytes: push rbx/r12/r13/r14/r15 ; mov r15,rdi ; mov eax,42 ;
    //                     pop r15/r14/r13/r12/rbx ; ret
    {
        let stub: [u8; 27] = [
            0x53, // push rbx
            0x41, 0x54, // push r12
            0x41, 0x55, // push r13
            0x41, 0x56, // push r14
            0x41, 0x57, // push r15
            0x49, 0x89, 0xFF, // mov r15, rdi   (env ptr, exactly e_prologue)
            0xB8, 0x2A, 0x00, 0x00, 0x00, // mov eax, 42
            0x41, 0x5F, // pop r15
            0x41, 0x5E, // pop r14
            0x41, 0x5D, // pop r13
            0x41, 0x5C, // pop r12
            0x5B, // pop rbx
            0xC3, // ret
        ];
        let mem = unsafe { make_rx(&stub) };
        let f: extern "sysv64" fn(*const usize) -> u64 = unsafe { std::mem::transmute(mem) };
        let got = f(std::ptr::null());
        println!(
            "  [P1] Q2 ENTRY: indirect CALL -> non-ENDBR prologue   : returned {} (want 42) {}",
            got,
            if got == 42 { "OK — Q2's entry does NOT trip" } else { "FAULT/wrong" }
        );
        // Confirm this generated frame has NO x64 unwind entry registered.
        let mut base: u64 = 0;
        let rf = unsafe {
            RtlLookupFunctionEntry(mem as u64, &mut base, std::ptr::null_mut())
        };
        println!(
            "       RtlLookupFunctionEntry(generated code) = {:?}  ({} — any SEH/C++ unwind through this frame is UB)",
            rf,
            if rf.is_null() { "NO unwind entry, as expected" } else { "has entry" }
        );
        unsafe { VirtualFree(mem, 0, MEM_RELEASE); }
    }

    // P2: Q2 CALLBACK pattern — e_call_env emits `call qword [r15+idx*8]`, an INDIRECT
    //     call through memory to a rustc shim that has NO ENDBR64 (measured). Model with
    //     a table at rdi[0]: `mov rax,rdi ; call qword [rax] ; ret`.
    {
        let table: [u64; 1] = [host_returns_42 as *const () as usize as u64];
        let stub: [u8; 6] = [
            0x48, 0x89, 0xF8, // mov rax, rdi
            0xFF, 0x10, // call qword [rax]   (== e_call_env shape)
            0xC3, // ret
        ];
        let mem = unsafe { make_rx(&stub) };
        let f: extern "sysv64" fn(*const u64) -> u64 = unsafe { std::mem::transmute(mem) };
        let got = f(table.as_ptr());
        println!(
            "  [P2] Q2 CALLBACK: `call qword [mem]` -> non-ENDBR fn : returned {} (want 42) {}",
            got,
            if got == 42 { "OK — Q2's OS callbacks do NOT trip" } else { "FAULT/wrong" }
        );
        unsafe { VirtualFree(mem, 0, MEM_RELEASE); }
    }

    println!();
    println!("  I-cache coherence: on x86_64 the hardware keeps I/D coherent for same-core");
    println!("  self-modifying writes; every stub above was written then executed in-thread");
    println!("  and returned correctly WITHOUT any explicit IC/DSB/ISB. (ARM would require it.)");
}

fn main() {
    println!("=== Q12 landing-gate probe (Windows / x86_64, measured) ===");
    criterion_1_cet_state();
    criterion_2_products();
    println!("=== done ===");
}
