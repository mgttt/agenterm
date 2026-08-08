//! Q4 driver — the equivalence invariant as a structural guard.
//!
//! For each reused Q1 payload it: (1) builds a `VerifiedArtifact`, which runs the
//! congruence invariant on the critical path (spec §1.1); (2) reports region coverage
//! (criterion ②); (3) on Windows JIT-executes — but ONLY via `VerifiedArtifact::
//! win_bytes()`, so execution is impossible unless the invariant passed. Then a
//! NEGATIVE test injects a mutant into a Neutral region and shows `build_from` REFUSES
//! (criterion ① — the guard bites; no artifact, no bytes).
//!
//! Reuse: `ir/spec/ir.rs`, `ir/lower/asm.rs`, `ir/lower/{sysv64,win64}.rs`,
//! `ir/payloads/payloads.rs` are pulled in verbatim via `#[path]`. Only `mod common`
//! (equiv_lower.rs) is a region-recording copy, and `verify.rs` is new.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/lower/asm.rs"]
mod asm;
#[path = "equiv_lower.rs"]
mod common;
#[path = "../ir/lower/sysv64.rs"]
mod sysv64;
#[path = "../ir/lower/win64.rs"]
mod win64;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "verify.rs"]
mod verify;

use ir::Module;
use verify::{lower_one, Coverage, VerifiedArtifact};

// ---- minimal Win32 FFI for JIT execution (same posture as Q1 main.rs) ----
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
            let mut c = Ctx { _bindings: bindings, _rodata: rodata.to_vec(), words: vec![0, 0] };
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
fn jit_run(code: &[u8], ctx: *const u64) -> u64 {
    unsafe {
        let mem = VirtualAlloc(std::ptr::null_mut(), code.len().max(1), 0x3000, 0x04);
        assert!(!mem.is_null());
        std::ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
        let mut old = 0u32;
        VirtualProtect(mem, code.len(), 0x20, &mut old);
        let f: extern "win64" fn(*const u64) -> u64 = std::mem::transmute(mem);
        f(ctx)
    }
}
#[cfg(windows)]
fn jit_run_capture(code: &[u8], ctx: *const u64, tmp: &str) -> (u64, Vec<u8>) {
    unsafe {
        let mut name: Vec<u8> = tmp.bytes().collect();
        name.push(0);
        let fh = CreateFileA(name.as_ptr(), 0x4000_0000, 0, std::ptr::null_mut(), 2, 0x80, std::ptr::null_mut());
        let saved = GetStdHandle(STD_OUTPUT_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, fh);
        let r = jit_run(code, ctx);
        SetStdHandle(STD_OUTPUT_HANDLE, saved);
        CloseHandle(fh);
        let cap = std::fs::read(tmp).unwrap_or_default();
        (r, cap)
    }
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

fn build(m: &Module) -> VerifiedArtifact {
    common::set_externs(&m.externs);
    // NOTE: build() lowers both halves and runs the congruence invariant. If it
    // returned Err we would have NO artifact and could not proceed to execute.
    VerifiedArtifact::build(m, &sysv64::SysV64, &win64::Win64)
        .expect("congruence invariant must hold for the honest lowerers")
}

fn pct(x: f64) -> String {
    format!("{:>5.1}%", x * 100.0)
}

fn print_coverage(name: &str, va: &VerifiedArtifact) {
    let sc = va.sysv_coverage();
    let wc = va.win_coverage();
    println!(
        "  {:<16} identical={:<5}  | sysv total={:>4}  win total={:>4}",
        name, va.identical(), sc.total, wc.total
    );
    let row = |tag: &str, c: Coverage| {
        println!(
            "      {:<9} neutral(A)={} struct(A′)={} mechanics={} intent(leak)={}  [n={} c={} f={} x={} i={}]",
            tag,
            pct(c.neutral_frac()),
            pct(c.struct_frac()),
            pct(c.mechanics_frac()),
            pct(c.intent_frac()),
            c.neutral, c.control, c.frame, c.ctx, c.intent,
        );
    };
    row("sysv", sc);
    row("win", wc);
}

fn main() {
    println!("== Q4: equivalence as a STRUCTURAL invariant ==\n");

    let pure = payloads::pure_compute();
    let rhp = payloads::read_hash_print();
    let spawn = payloads::spawn_echo();

    // ---- ①: build through the gate (invariant runs here) + ②: coverage ----
    println!("-- criterion ① (gate ran, all passed) + ② (region coverage) --");
    let va_pure = build(&pure);
    let va_rhp = build(&rhp);
    let va_spawn = build(&spawn);
    print_coverage("pure_compute", &va_pure);
    print_coverage("read_hash_print", &va_rhp);
    print_coverage("spawn_echo", &va_spawn);
    println!();

    // dump blobs for inspection
    let _ = std::fs::create_dir_all("out");
    for (n, va) in [("pure_compute", &va_pure), ("read_hash_print", &va_rhp), ("spawn_echo", &va_spawn)] {
        let _ = std::fs::write(format!("out/{n}.sysv64.bin"), va.sysv_bytes());
        let _ = std::fs::write(format!("out/{n}.win64.bin"), va.win_bytes());
    }

    // ---- ① negative test: the guard BITES on a mutated Neutral region ----
    println!("-- criterion ① (negative): mutate a Neutral byte -> build MUST refuse --");
    {
        common::set_externs(&rhp.externs);
        let sysv = lower_one(&rhp, &sysv64::SysV64);
        let mut win = lower_one(&rhp, &win64::Win64);
        // find the first Neutral region and flip one of its bytes on the win side
        let nr = win.regions.iter().find(|r| matches!(r.kind, common::RegionKind::Neutral)).copied();
        match nr {
            Some(r) => {
                let victim = r.start;
                win.bytes[victim] ^= 0xFF;
                match VerifiedArtifact::build_from(sysv, win) {
                    Ok(_) => println!("  FAIL: build accepted a mutated neutral region (guard did NOT bite)"),
                    Err(e) => println!("  OK: build refused — {:?} — no artifact, no bytes to run", e),
                }
            }
            None => println!("  (no neutral region to mutate?!)"),
        }
    }
    println!();

    // ---- execution ONLY through the gate (criterion ①: bytes require a VerifiedArtifact) ----
    #[cfg(windows)]
    {
        println!("-- execution evidence (Win64; bytes obtained only via VerifiedArtifact) --");
        let r = jit_run(va_pure.win_bytes(), std::ptr::null());
        let want = reference_pure();
        println!("  pure_compute  -> {} (expect {})  {}", r, want, if r == want { "OK" } else { "FAIL" });

        let input = b"dynamic-core experiment 2026-08-08\n";
        std::fs::write("input.txt", input).unwrap();
        let ctx = Ctx::new(&rhp.rodata);
        let (_r, out) = jit_run_capture(va_rhp.win_bytes(), ctx.ptr(), "rhp_stdout.tmp");
        let want_hex = format!("{:016x}\n", reference_hash(input));
        let got = String::from_utf8_lossy(&out).to_string();
        println!("  read_hash_print -> {:?} (expect {:?})  {}", got.trim_end(), want_hex.trim_end(), if got == want_hex { "OK" } else { "FAIL" });

        let ctx2 = Ctx::new(&spawn.rodata);
        let (r2, out2) = jit_run_capture(va_spawn.win_bytes(), ctx2.ptr(), "spawn_stdout.tmp");
        let got2 = String::from_utf8_lossy(&out2).to_string();
        println!("  spawn_echo    -> printed {:?} ret={} (expect \"exit=07\", 7)  {}", got2.trim_end(), r2, if got2.trim_end() == "exit=07" && r2 == 7 { "OK" } else { "FAIL" });
        let _ = (std::fs::remove_file("rhp_stdout.tmp"), std::fs::remove_file("spawn_stdout.tmp"));
    }
    #[cfg(not(windows))]
    {
        println!("(non-Windows host: execution skipped; structural coverage measured only)");
    }

    println!("\n== done ==");
}
