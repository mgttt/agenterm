//! Q9 — the INTERPRETER backend for the Q1 neutral IR.
//!
//! This is primitive ②'s third implementation (direct-RX / cross-process / **interpret**,
//! per Q8 RESULTS §"对四原语模型的影响" #2). It consumes the SAME `ir::Module` that
//! `ir/lower/common.rs::lower` consumes — no IR change, no per-payload code. Instead of
//! emitting x86-64 and jumping in, it walks the IR op-by-op. That is the whole point: the
//! interpreter is **another backend, not another architecture**.
//!
//! Structure — two clearly separated halves, because criteria ④ and ⑤ hinge on the split:
//!   * `run` + `eval_op`  = the EVAL-CORE. Pure IR semantics. Zero machine code emitted,
//!     zero register names, zero x86-64. This is the ISA-INDEPENDENT part (criterion ④).
//!   * `do_intent`        = the OS SEAM. Win32 FFI to realise the 6 Intents. This is where
//!     Q1's leaks L1–L5 live — and, as criterion ⑤ shows, they live here in EXACTLY the
//!     same shape as in `win64.rs::emit_call`. The seam is independent of execution method.
//!
//! Discipline (spec §1.4): NO threaded dispatch, NO superinstructions, NO inline caching,
//! NO compile cache. A plain `match` over the op enum. Minimal usable, not fast.

#![allow(dead_code)]

use crate::ir::*;

/// Interpret a module and return its Exit/Ret value. `run` receives no ctx: the
/// interpreter holds rodata directly (a real host pointer), and resolves intents itself.
pub fn run(m: &Module) -> u64 {
    // one machine word per SSA/virtual value. reassignment (loops) just overwrites.
    let mut vals = vec![0u64; m.n_vals as usize];
    // rodata is an opaque byte array owned by the Module; its host address IS the neutral
    // "data base" that Op::Rodata offsets into (mirrors ctx[1] in the lowered code).
    let rodata_base = m.rodata.as_ptr() as u64;

    let mut bi = m.entry as usize;
    loop {
        let blk = &m.blocks[bi];
        for inst in &blk.insts {
            match inst {
                Inst::Set(d, op) => vals[*d as usize] = eval_op(op, &vals, rodata_base),
                // Store8/StoreW dereference a real host pointer held in a value word —
                // exactly what the lowered `mov [rax], cl` / `mov [rax], rcx` does.
                Inst::Store8(addr, v) => unsafe {
                    *(vals[*addr as usize] as *mut u8) = vals[*v as usize] as u8;
                },
                Inst::StoreW(addr, v) => unsafe {
                    *(vals[*addr as usize] as *mut u64) = vals[*v as usize];
                },
                Inst::Call(d, id, args) => {
                    let intent = m.externs[*id as usize].intent;
                    let a: [u64; 3] = {
                        let mut buf = [0u64; 3];
                        for (i, v) in args.iter().enumerate() {
                            buf[i] = vals[*v as usize];
                        }
                        buf
                    };
                    vals[*d as usize] = do_intent(intent, &a[..args.len()]);
                }
            }
        }
        match &blk.term {
            Term::Br(x) => bi = *x as usize,
            Term::BrCond(c, nz, z) => {
                bi = if vals[*c as usize] != 0 { *nz } else { *z } as usize;
            }
            // Exit and Ret both yield the value word (the harness reads it). A standalone
            // interpreter would route Exit through the OS exit intent — a few bytes, noted.
            Term::Ret(v) | Term::Exit(v) => return vals[*v as usize],
        }
    }
}

/// Evaluate one value-producing op. Pure u64 word semantics, wrapping arithmetic to match
/// the ISA's 64-bit wraparound. NOTHING here is ISA- or ABI-specific (criterion ④).
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
        // Load8/LoadW dereference a real host pointer (from Alloc/Rodata), same as the
        // lowered `movzx`/`mov r,[r]`.
        Op::Load8(x) => unsafe { *(g(x) as *const u8) as u64 },
        Op::LoadW(x) => unsafe { *(g(x) as *const u64) },
    }
}

// ===========================================================================================
// THE OS SEAM — criterion ⑤. Everything below is target-specific (Win64 + Windows reach).
// Compare line-for-line against `ir/lower/win64.rs::emit_call` / `emit_spawn`: the symbol
// names (L1), the injected constant args (L2), the STARTUPINFOA/PROCESS_INFORMATION layout
// (L3), the 32-bit out-param widths (L4), the sentinel handling (L5) are ALL still here.
// The lowerer places these into registers+stack and calls through resolved pointers; the
// interpreter passes them as Rust FFI args. Different *placement*, identical *content*.
// ===========================================================================================

#[cfg(all(windows, not(interp_measure_core)))]
mod seam {
    // Reach = symbol resolution (primitive ③'s live half on Windows). These 9 symbols are
    // exactly `win64::SYMBOLS`. This list IS leak L1 — it grows per OS capability.
    #[link(name = "kernel32")]
    extern "system" {
        pub fn VirtualAlloc(addr: *mut u8, size: usize, typ: u32, protect: u32) -> *mut u8; // 0
        pub fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *mut u8, disp: u32, flags: u32, tmpl: *mut u8) -> *mut u8; // 1
        pub fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32; // 2
        pub fn CloseHandle(h: *mut u8) -> i32; // 3
        pub fn GetStdHandle(which: u32) -> *mut u8; // 4
        pub fn WriteFile(h: *mut u8, buf: *const u8, n: u32, wrote: *mut u32, ov: *mut u8) -> i32; // 5
        pub fn CreateProcessA(app: *const u8, cmd: *mut u8, pa: *mut u8, ta: *mut u8, inh: i32, flags: u32, env: *mut u8, dir: *const u8, si: *mut u8, pi: *mut u8) -> i32; // 6
        pub fn WaitForSingleObject(h: *mut u8, ms: u32) -> u32; // 7
        pub fn GetExitCodeProcess(h: *mut u8, code: *mut u32) -> i32; // 8
    }
}

/// Realise one OS-neutral Intent on Win64. This is the interpreter's analogue of
/// `win64.rs::emit_call`. Its per-intent constant args and struct layout are IDENTICAL —
/// that identity is the criterion-⑤ result.
#[cfg(all(windows, not(interp_measure_core)))]
fn do_intent(intent: Intent, args: &[u64]) -> u64 {
    use seam::*;
    unsafe {
        match intent {
            // VirtualAlloc(NULL, n, MEM_COMMIT|RESERVE=0x3000, PAGE_READWRITE=0x04)
            Intent::Alloc => {
                VirtualAlloc(core::ptr::null_mut(), args[0] as usize, 0x3000, 0x04) as u64
            }
            // CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, 0, OPEN_EXISTING, 0, 0)
            //   L2: 1 semantic arg (path) -> 7 native args, 6 injected constants.
            Intent::FileOpen => CreateFileA(
                args[0] as *const u8, 0x8000_0000, 1, core::ptr::null_mut(), 3, 0, core::ptr::null_mut(),
            ) as u64,
            // ReadFile(h, buf, cap, &got, NULL); count comes back through a 32-bit out-param.
            //   L4: the out-param is 32 bits; a `u64` read would be wrong.
            Intent::FileRead => {
                let mut got: u32 = 0;
                ReadFile(args[0] as *mut u8, args[1] as *mut u8, args[2] as u32, &mut got, core::ptr::null_mut());
                got as u64
            }
            Intent::FileClose => CloseHandle(args[0] as *mut u8) as u64,
            // h = GetStdHandle(STD_OUTPUT_HANDLE=-11); WriteFile(h, buf, len, &wrote, NULL)
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

/// The known hard bone (Q1 leak L3). The STARTUPINFOA size (104), its `cb` field at offset
/// 0, the PROCESS_INFORMATION `hProcess` at offset 0, and the literal command line are all
/// Win64/OS facts with NO neutral form. In the lowerer they are emitted into a VirtualAlloc'd
/// buffer; here they live in a Rust struct — SAME layout numbers, different syntax.
#[cfg(all(windows, not(interp_measure_core)))]
unsafe fn spawn_wait() -> u64 {
    use seam::*;

    // STARTUPINFOA is 104 bytes on x86_64; cb (offset 0) must equal 104. <-- L3 layout leak
    #[repr(C)]
    struct StartupInfoA {
        cb: u32,
        _rest: [u8; 100], // 104 total. field offsets are the leak; we only touch cb.
    }
    // PROCESS_INFORMATION: hProcess at offset 0 (what we read back). <-- L3 layout leak
    #[repr(C)]
    struct ProcessInformation {
        h_process: *mut u8,
        h_thread: *mut u8,
        pid: u32,
        tid: u32,
    }
    assert_eq!(core::mem::size_of::<StartupInfoA>(), 104);

    let mut si: StartupInfoA = core::mem::zeroed();
    si.cb = 104; // sizeof(STARTUPINFOA)
    let mut pi: ProcessInformation = core::mem::zeroed();

    // The command line must be a writable buffer (CreateProcessA may modify it). <-- Windows
    // string leak (same literal as win64.rs::emit_spawn).
    let mut cmd: Vec<u8> = b"cmd.exe /c exit 7\0".to_vec();

    // CreateProcessA(NULL, cmd, 0,0,0,0,0,0, &si, &pi) — 10 args (L2: SpawnWait has 0
    // semantic args; ALL 10 are injected here).
    CreateProcessA(
        core::ptr::null(),
        cmd.as_mut_ptr(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        0,
        0,
        core::ptr::null_mut(),
        core::ptr::null(),
        &mut si as *mut _ as *mut u8,
        &mut pi as *mut _ as *mut u8,
    );
    let hproc = pi.h_process; // PROCESS_INFORMATION.hProcess at offset 0 <-- L3
    WaitForSingleObject(hproc, 0xFFFF_FFFF);
    let mut code: u32 = 0; // GetExitCodeProcess out-param is 32 bits <-- L4
    GetExitCodeProcess(hproc, &mut code);
    CloseHandle(hproc);
    code as u64
}

// --- measurement stub: with `--cfg interp_measure_core` the OS seam collapses to
//     `unreachable`, so `.text` measures the EVAL-CORE alone (criterion ② byte split). ---
#[cfg(any(not(windows), interp_measure_core))]
fn do_intent(_intent: Intent, _args: &[u64]) -> u64 {
    unsafe { core::hint::unreachable_unchecked() }
}
