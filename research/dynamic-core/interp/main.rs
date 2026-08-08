//! Q9 driver — interpreter backend vs Q1's native lowering, on the SAME neutral IR.
//!
//! J1 (coverage): run each of the three Q1 payloads through `interp::run` and verify the
//!     SAME acceptance as Q1 (pure->163, rhp->"a49d2cbecc13994f", spawn->"exit=07"/7).
//! J3 (speed):    time `interp::run(m)` vs `jit_run(lower(m, Win64))`, same module, same input.
//!
//! The IR spec, the three payloads, and the Win64 lowerer are REUSED verbatim from ../ir via
//! `#[path]` — no reconstruction (spec §1.1). This file is just harness.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/lower/asm.rs"]
mod asm;
#[path = "../ir/lower/common.rs"]
mod common;
#[path = "../ir/lower/win64.rs"]
mod win64;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "interp.rs"]
mod interp;

use ir::Module;
use std::time::Instant;

// ---- minimal Win32 FFI for the NATIVE baseline (JIT-execute the lowered Win64 code) ----
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
const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;

// Build the ctx the lowered Win64 code expects: u64[2] = [&resolved_symbols, &rodata].
#[cfg(windows)]
struct Ctx {
    _bindings: Vec<u64>,
    _rodata: Vec<u8>,
    words: Vec<u64>,
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

#[cfg(windows)]
fn jit_prepare(code: &[u8]) -> extern "win64" fn(*const u64) -> u64 {
    unsafe {
        let mem = VirtualAlloc(std::ptr::null_mut(), code.len().max(1), 0x3000, 0x04);
        assert!(!mem.is_null());
        std::ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
        let mut old = 0u32;
        VirtualProtect(mem, code.len(), 0x20, &mut old); // PAGE_EXECUTE_READ
        std::mem::transmute(mem)
    }
}

/// Redirect STD_OUTPUT to a temp file, run `f`, restore, return (ret, captured bytes).
#[cfg(windows)]
fn with_captured_stdout<F: FnOnce() -> u64>(tmp: &str, f: F) -> (u64, Vec<u8>) {
    unsafe {
        let mut name: Vec<u8> = tmp.bytes().collect();
        name.push(0);
        let fh = CreateFileA(name.as_ptr(), 0x4000_0000, 0, std::ptr::null_mut(), 2, 0x80, std::ptr::null_mut());
        let saved = GetStdHandle(STD_OUTPUT_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, fh);
        let r = f();
        SetStdHandle(STD_OUTPUT_HANDLE, saved);
        CloseHandle(fh);
        let cap = std::fs::read(tmp).unwrap_or_default();
        (r, cap)
    }
}

fn lower_win(m: &Module) -> Vec<u8> {
    common::set_externs(&m.externs);
    common::lower(m, &win64::Win64)
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
    println!("== Q9: interpreter backend vs native lowering — SAME neutral IR ==\n");

    let pure = payloads::pure_compute();
    let rhp = payloads::read_hash_print();
    let spawn = payloads::spawn_echo();

    #[cfg(windows)]
    {
        let input = b"dynamic-core experiment 2026-08-08\n";
        std::fs::write("input.txt", input).unwrap();
        let want_hex = format!("{:016x}\n", reference_hash(input));

        // ============================ J1 — COVERAGE (interpreter) ============================
        println!("== J1 — coverage: does interp::run pass all three, on the UNCHANGED Q1 IR? ==");

        let r_pure = interp::run(&pure);
        let ok_pure = r_pure == reference_pure();
        println!("  pure_compute     -> {r_pure:>3}   (expected {})   {}", reference_pure(), pass(ok_pure));

        let (_r, out) = with_captured_stdout("rhp_interp.tmp", || interp::run(&rhp));
        let got = String::from_utf8_lossy(&out).to_string();
        let ok_rhp = got == want_hex;
        println!("  read_hash_print  -> {:?}   (expected {:?})   {}", got.trim_end(), want_hex.trim_end(), pass(ok_rhp));

        let (r2, out2) = with_captured_stdout("spawn_interp.tmp", || interp::run(&spawn));
        let got2 = String::from_utf8_lossy(&out2).to_string();
        let ok_spawn = got2.trim_end() == "exit=07" && r2 == 7;
        println!("  spawn_echo       -> printed {:?}, ret={}   (expected \"exit=07\", 7)   {}", got2.trim_end(), r2, pass(ok_spawn));
        let _ = (std::fs::remove_file("rhp_interp.tmp"), std::fs::remove_file("spawn_interp.tmp"));
        println!();

        // ============================ J3 — SPEED (interp vs native) ==========================
        println!("== J3 — speed: interpret vs already-lowered native code (same module, same input) ==");

        // pure_compute: compute-heavy (1M-iter loop). The worst case for interpretation.
        // THREE points, for an honest ratio: (a) optimized native = the real JIT-with-regalloc
        // ceiling (Rust -O), (b) Q1's naive stack-slot lowering = the literal "已降级原生码",
        // (c) the interpreter. Reporting only vs (b) would flatter interp, since (b) is slow.
        let native_pure = jit_prepare(&lower_win(&pure));
        let reps_p = 200u32;
        let t = Instant::now();
        for _ in 0..reps_p { std::hint::black_box(reference_pure()); }
        let opt_p = t.elapsed().as_secs_f64() / reps_p as f64;
        let t = Instant::now();
        for _ in 0..reps_p { std::hint::black_box(native_pure(std::ptr::null())); }
        let nat_p = t.elapsed().as_secs_f64() / reps_p as f64;
        let t = Instant::now();
        for _ in 0..reps_p { std::hint::black_box(interp::run(&pure)); }
        let int_p = t.elapsed().as_secs_f64() / reps_p as f64;
        println!("  pure_compute   opt-native {:>9.3} us | Q1-naive-native {:>9.3} us | interp {:>9.3} us",
                 opt_p * 1e6, nat_p * 1e6, int_p * 1e6);
        println!("                 interp vs Q1-naive-native = {:>6.1}x   |   interp vs opt-native = {:>7.1}x  (compute-bound)",
                 int_p / nat_p, int_p / opt_p);

        // read_hash_print: OS-bound (open/read/close/write dominate the tiny hash loop).
        let native_rhp = jit_prepare(&lower_win(&rhp));
        let ctx_rhp = Ctx::new(&rhp.rodata);
        let reps_r = 200u32;
        let t = Instant::now();
        for _ in 0..reps_r { let _ = with_captured_stdout("rhp_bench_n.tmp", || native_rhp(ctx_rhp.ptr())); }
        let nat_r = t.elapsed().as_secs_f64() / reps_r as f64;
        let t = Instant::now();
        for _ in 0..reps_r { let _ = with_captured_stdout("rhp_bench_i.tmp", || interp::run(&rhp)); }
        let int_r = t.elapsed().as_secs_f64() / reps_r as f64;
        println!("  read_hash_print native {:>10.3} us   interp {:>10.3} us   -> {:>7.1}x  (OS-bound)",
                 nat_r * 1e6, int_r * 1e6, int_r / nat_r);
        let _ = (std::fs::remove_file("rhp_bench_n.tmp"), std::fs::remove_file("rhp_bench_i.tmp"));

        // spawn_echo: process-creation-bound (CreateProcessA dominates everything).
        let native_spawn = jit_prepare(&lower_win(&spawn));
        let ctx_spawn = Ctx::new(&spawn.rodata);
        let reps_s = 30u32;
        let t = Instant::now();
        for _ in 0..reps_s { let _ = with_captured_stdout("sp_bench_n.tmp", || native_spawn(ctx_spawn.ptr())); }
        let nat_s = t.elapsed().as_secs_f64() / reps_s as f64;
        let t = Instant::now();
        for _ in 0..reps_s { let _ = with_captured_stdout("sp_bench_i.tmp", || interp::run(&spawn)); }
        let int_s = t.elapsed().as_secs_f64() / reps_s as f64;
        println!("  spawn_echo     native {:>10.3} us   interp {:>10.3} us   -> {:>7.1}x  (spawn-bound)",
                 nat_s * 1e6, int_s * 1e6, int_s / nat_s);
        let _ = (std::fs::remove_file("sp_bench_n.tmp"), std::fs::remove_file("sp_bench_i.tmp"));

        println!("\n  (reps: pure={reps_p}, rhp={reps_r}, spawn={reps_s}; per-call = total/reps)");
    }
    #[cfg(not(windows))]
    {
        let _ = (&pure, &rhp, &spawn);
        println!("(non-Windows host: execution skipped)");
    }

    println!("\n== done ==");
}

fn pass(b: bool) -> &'static str {
    if b { "OK" } else { "FAIL" }
}
