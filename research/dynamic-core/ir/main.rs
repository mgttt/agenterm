//! Driver for the neutral-IR experiment.
//!
//! For each of the three reused payloads it: (1) lowers the SAME neutral IR module
//! through two INDEPENDENT lowerers (SysV x86_64 and Win64) — spec §1.3 — (2) reports
//! emitted byte size, and (3) on Windows JIT-executes the Win64 lowering against the
//! real kernel32 to prove correctness (criterion ①). `pure_compute`, having no OS
//! surface, is byte-identical across both ABIs and is executed as neutrality proof
//! for BOTH sides; the OS-touching payloads execute Win64 only (no WSL for SysV),
//! matching the prior round's honest split.

#[path = "spec/ir.rs"]
mod ir;
#[path = "lower/asm.rs"]
mod asm;
#[path = "lower/common.rs"]
mod common;
#[path = "lower/sysv64.rs"]
mod sysv64;
#[path = "lower/win64.rs"]
mod win64;
#[path = "payloads/payloads.rs"]
mod payloads;

use ir::Module;

// ---- minimal Win32 FFI for JIT execution + stdout redirection ----
#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut u8, size: usize, typ: u32, protect: u32) -> *mut u8;
    fn VirtualProtect(addr: *mut u8, size: usize, protect: u32, old: *mut u32) -> i32;
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
    fn GetStdHandle(which: u32) -> *mut u8;
    fn SetStdHandle(which: u32, h: *mut u8) -> i32;
    fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *mut u8, disp: u32, flags: u32, tmpl: *mut u8) -> *mut u8;
    fn CloseHandle(h: *mut u8) -> i32;
}

#[cfg(windows)]
const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (DWORD)-11

/// Build the ctx that the emitted Win64 code expects: a u64[2] where [0] points at
/// the resolved-symbol table and [1] points at the module's rodata. This is the
/// kernel's job (③ resolution + ① memory) done in the host for the JIT.
#[cfg(windows)]
struct Ctx {
    _bindings: Vec<u64>,
    _rodata: Vec<u8>,
    words: Vec<u64>, // [0]=&bindings, [1]=&rodata
}
#[cfg(windows)]
impl Ctx {
    fn new(rodata: &[u8]) -> Self {
        unsafe {
            let k32 = LoadLibraryA(b"kernel32.dll\0".as_ptr());
            let mut bindings = Vec::new();
            for s in win64::SYMBOLS {
                let mut name: Vec<u8> = s.bytes().collect();
                name.push(0);
                let a = GetProcAddress(k32, name.as_ptr());
                assert!(!a.is_null(), "unresolved symbol {s}");
                bindings.push(a as u64);
            }
            let rodata = rodata.to_vec();
            let mut c = Ctx { _bindings: bindings, _rodata: rodata, words: vec![0, 0] };
            c.words[0] = c._bindings.as_ptr() as u64;
            c.words[1] = c._rodata.as_ptr() as u64;
            c
        }
    }
    fn ptr(&self) -> *const u64 {
        self.words.as_ptr()
    }
}

/// Copy code into RX memory and run it as `fn(ctx) -> u64`.
#[cfg(windows)]
fn jit_run(code: &[u8], ctx: *const u64) -> u64 {
    unsafe {
        const MEM_COMMIT_RESERVE: u32 = 0x3000;
        const PAGE_RW: u32 = 0x04;
        const PAGE_RX: u32 = 0x20;
        let mem = VirtualAlloc(std::ptr::null_mut(), code.len().max(1), MEM_COMMIT_RESERVE, PAGE_RW);
        assert!(!mem.is_null());
        std::ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
        let mut old = 0u32;
        VirtualProtect(mem, code.len(), PAGE_RX, &mut old);
        let f: extern "win64" fn(*const u64) -> u64 = std::mem::transmute(mem);
        f(ctx)
    }
}

/// Run an OS-touching payload with stdout redirected to a temp file so we can verify
/// what it printed. Returns (retval, captured_stdout).
#[cfg(windows)]
fn jit_run_capture(code: &[u8], ctx: *const u64, tmp: &str) -> (u64, Vec<u8>) {
    unsafe {
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const CREATE_ALWAYS: u32 = 2;
        const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
        let mut name: Vec<u8> = tmp.bytes().collect();
        name.push(0);
        let fh = CreateFileA(name.as_ptr(), GENERIC_WRITE, 0, std::ptr::null_mut(), CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut());
        let saved = GetStdHandle(STD_OUTPUT_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, fh);
        let r = jit_run(code, ctx);
        SetStdHandle(STD_OUTPUT_HANDLE, saved);
        CloseHandle(fh);
        let cap = std::fs::read(tmp).unwrap_or_default();
        (r, cap)
    }
}

fn lower_both(m: &Module) -> (Vec<u8>, Vec<u8>) {
    common::set_externs(&m.externs);
    let sysv = common::lower(m, &sysv64::SysV64);
    common::set_externs(&m.externs);
    let win = common::lower(m, &win64::Win64);
    (sysv, win)
}

fn reference_pure() -> u64 {
    let mut acc: u64 = 1469598103934665603;
    let mut i: u64 = 0;
    while i < 1_000_000 {
        acc = acc.wrapping_mul(1099511628211).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc & 0xff
}

fn reference_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn main() {
    println!("== neutral-IR experiment: lower each payload through two ABIs ==\n");

    let pure = payloads::pure_compute();
    let rhp = payloads::read_hash_print();
    let spawn = payloads::spawn_echo();

    let (pure_s, pure_w) = lower_both(&pure);
    let (rhp_s, rhp_w) = lower_both(&rhp);
    let (spawn_s, spawn_w) = lower_both(&spawn);

    // ---- emitted byte sizes (criterion ③ numerator) ----
    println!("emitted code size (bytes)   sysv64   win64   identical?");
    let rows = [
        ("pure_compute", &pure_s, &pure_w),
        ("read_hash_print", &rhp_s, &rhp_w),
        ("spawn_echo", &spawn_s, &spawn_w),
    ];
    for (n, s, w) in rows {
        println!("  {:<20} {:>6}  {:>6}   {}", n, s.len(), w.len(), s == w);
    }
    println!();

    // dump the emitted blobs for third-party inspection
    let _ = std::fs::create_dir_all("out");
    for (n, s, w) in rows {
        let _ = std::fs::write(format!("out/{n}.sysv64.bin"), s);
        let _ = std::fs::write(format!("out/{n}.win64.bin"), w);
    }

    // ---- criterion ①: neutrality via real execution ----
    #[cfg(windows)]
    {
        println!("== criterion ① — execution evidence (Win64 native, real kernel32) ==");

        // pure_compute: no OS, no ctx -> both ABIs identical. Execute (valid for BOTH).
        assert_eq!(pure_s, pure_w, "pure_compute must lower identically for both ABIs");
        let r = jit_run(&pure_w, std::ptr::null());
        let want = reference_pure();
        println!("  pure_compute  -> {:>3}  (expected {}, prior round exit=163)  {}",
                 r, want, if r == want { "OK" } else { "FAIL" });

        // read_hash_print: write the fixed input, run, verify printed hash.
        let input = b"dynamic-core experiment 2026-08-08\n";
        std::fs::write("input.txt", input).unwrap();
        let ctx = Ctx::new(&rhp.rodata);
        let (_r, out) = jit_run_capture(&rhp_w, ctx.ptr(), "rhp_stdout.tmp");
        let want_hex = format!("{:016x}\n", reference_hash(input));
        let got = String::from_utf8_lossy(&out).to_string();
        println!("  read_hash_print -> {:?}  (expected {:?})  {}",
                 got.trim_end(), want_hex.trim_end(),
                 if got == want_hex { "OK" } else { "FAIL" });

        // spawn_echo: run cmd.exe /c exit 7, print exit=07, return 7.
        let ctx2 = Ctx::new(&spawn.rodata);
        let (r2, out2) = jit_run_capture(&spawn_w, ctx2.ptr(), "spawn_stdout.tmp");
        let got2 = String::from_utf8_lossy(&out2).to_string();
        println!("  spawn_echo    -> printed {:?}, ret={}  (expected \"exit=07\", 7)  {}",
                 got2.trim_end(), r2,
                 if got2.trim_end() == "exit=07" && r2 == 7 { "OK" } else { "FAIL" });

        let _ = (std::fs::remove_file("rhp_stdout.tmp"), std::fs::remove_file("spawn_stdout.tmp"));
    }
    #[cfg(not(windows))]
    {
        println!("(non-Windows host: execution skipped; byte measurement only)");
    }

    println!("\n== done ==");
}
