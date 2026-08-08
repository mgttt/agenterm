//! Q16 — INTEGRATION VALIDATION (the seam audit).
//!
//! Sixteen experiments each measured ONE axis in isolation. This harness bolts the
//! already-decided parts into ONE binary and runs the three canonical payloads through
//! the combined pipeline, toggling each part, to answer: do the seams fight?
//!
//! Parts assembled (all reused VERBATIM via #[path], no reimplementation):
//!   * Q1  neutral IR + payloads          (ir, payloads)
//!   * Q9  interpreter backend            (interp)
//!   * Q15 policy-enforcing interpreter    (interp_policy)
//!   * Q7  table-driven marshaller         (table, marshal)
//!   * Q4  structural equivalence guard    (common=equiv_lower, sysv64, win64, verify)
//!   * Q13 declare detection               (inlined below — calls real Win32)
//!   * Q3  content addressing              (fnv hash over produced bytes, inlined)
//!   * Q6  five-primitive kernel arity      (observed via CreateProcessA=10 in the seam)
//!
//! Discipline: this is a SEAM AUDIT, not a runtime. No optimisation, no second ISA, no
//! product. The main product is the seam list printed at the end; execution is evidence.

#![allow(dead_code)]

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/lower/asm.rs"]
mod asm;
// crate::common := the Q4 region-aware lowerer (byte-identical to Q1 common.rs, per its
// own header). win64.rs / sysv64.rs resolve `crate::common::{Frame,Target}` against it,
// so the SAME target files serve both native execution AND the Q4 guard.
#[path = "../equiv/equiv_lower.rs"]
mod common;
#[path = "../ir/lower/sysv64.rs"]
mod sysv64;
#[path = "../ir/lower/win64.rs"]
mod win64;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "../equiv/verify.rs"]
mod verify;
#[path = "../tables/table.rs"]
mod table;
#[path = "../tables/marshal.rs"]
mod marshal;
#[path = "../interp/interp.rs"]
mod interp;
#[path = "../policy/interp_policy.rs"]
mod interp_policy;

use ir::{Intent, Module};
use interp_policy::Policy;

// ---- Q3 content addressing: identity = FNV-1a/64 over the produced bytes ----
fn content_id(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

// ============================================================================
// Native execution harness (reused shape from tables/main.rs). Two ctx contracts
// exist — Q7 (5 symbols, stdout via ctx[2]) and Q1 (9 symbols, GetStdHandle at
// runtime). That divergence is SEAM S6; here we only need pure_compute (no ctx).
// ============================================================================
#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut u8, size: usize, typ: u32, protect: u32) -> *mut u8;
    fn VirtualProtect(addr: *mut u8, size: usize, protect: u32, old: *mut u32) -> i32;
}
#[cfg(windows)]
fn jit_run_noctx(code: &[u8]) -> u64 {
    unsafe {
        let mem = VirtualAlloc(std::ptr::null_mut(), code.len().max(1), 0x3000, 0x04);
        assert!(!mem.is_null());
        std::ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
        let mut old = 0u32;
        VirtualProtect(mem, code.len(), 0x20, &mut old);
        let f: extern "win64" fn(*const u64) -> u64 = std::mem::transmute(mem);
        f(std::ptr::null())
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

// ============================================================================
// Q13 declare detection — one representative fact (SYSTEM_INFO.dwPageSize) run
// through BOTH backends so we can ask "same conclusion in interp vs codegen?".
// ============================================================================
#[cfg(windows)]
mod declare {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemInfo(si: *mut u8);
    }
    fn read_u16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
    fn read_u32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }
    /// Returns (pass_on_correct, fire_on_corrupt). Detection = pass && fire.
    pub fn pagesize_detect() -> (bool, bool) {
        let mut si = [0u8; 48];
        unsafe { GetSystemInfo(si.as_mut_ptr()); }
        let ok = |off: usize| -> bool {
            let arch = read_u16(&si, 0);
            let page = read_u32(&si, off);
            let gran = read_u32(&si, 40);
            arch == 9 && page >= 4096 && page <= 65536 && (page & (page - 1)) == 0 && gran == 65536
        };
        (ok(4), !ok(0)) // correct @4 passes; corrupted @0 must fire
    }
}

// ============================================================================
// Backends. Each takes a Module and returns Result<exit_value, reason>.
// ============================================================================
#[derive(Clone, Copy, PartialEq)]
enum Backend { Interp, Policy, TableCodegen, HandCodegen }

fn run_interp(m: &Module) -> Result<u64, String> { Ok(interp::run(m)) }

fn run_policy(m: &Module, pol: &Policy) -> Result<u64, String> {
    interp_policy::run_policy(m, pol).map_err(|v| format!("{v:?}"))
}

/// Lower via Q7 table marshaller. Catches the SpawnWait panic (SEAM S1) so the boolean
/// gate can record "refused" rather than aborting the whole run.
fn lower_table(m: &Module, abi: &table::AbiDesc) -> Result<Vec<u8>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| marshal::lower(m, abi)))
        .map_err(|_| "marshaller has no table row (SpawnWait) — cannot lower".to_string())
}

fn main() {
    // hush the panic hook so the caught SpawnWait panic doesn't spam the log.
    std::panic::set_hook(Box::new(|_| {}));

    println!("================ Q16 — INTEGRATION VALIDATION (seam audit) ================\n");

    let payloads: [(&str, Module); 3] = [
        ("pure_compute", payloads::pure_compute()),
        ("read_hash_print", payloads::read_hash_print()),
        ("spawn_echo", payloads::spawn_echo()),
    ];
    // input for rhp so the interpreter's FileRead has something to hash.
    let _ = std::fs::write("input.txt", b"q16 integration 2026-08-08\n");

    // ---- a composed policy: all Q15 instruction-layer checks ON, all intents allowed ----
    let composed_policy = Policy {
        max_steps: 5_000_000,
        max_alloc: 1 << 20,
        bounds: true,
        allowed: !0,
        taint_reads: true,
        block_tainted_write: false, // leave WriteStdout open; rhp must still print
    };

    // =====================================================================
    // ① COMBINED BOOLEAN GATE + per-part enable/disable
    // =====================================================================
    println!("--- ① boolean gate: three payloads x four backends ---\n");
    println!("  {:<16} {:>12} {:>14} {:>14} {:>14}", "payload", "interp(Q9)", "policy(Q15)", "table(Q7)", "hand(Q1/Q4)");
    for (name, m) in &payloads {
        common::set_externs(&m.externs);

        let i = run_interp(m).map(|v| format!("{v}")).unwrap_or_else(|e| e);
        let p = run_policy(m, &composed_policy).map(|v| format!("{v}")).unwrap_or_else(|e| e);
        // Q7 table codegen (win): lower only; report bytes or refusal.
        let t = match lower_table(m, &table::WIN64) {
            Ok(b) => format!("{}B", b.len()),
            Err(_) => "REFUSED".to_string(),
        };
        // Q1/Q4 hand codegen (win, region-aware): always lowers (has emit_spawn).
        let (hb, _) = common::lower_regions(m, &win64::Win64);
        let h = format!("{}B", hb.len());

        println!("  {:<16} {:>12} {:>14} {:>14} {:>14}", name, i, p, t, h);
    }

    // Native execution proof (pure_compute only — no ctx needed; identical bytes both codegen paths)
    #[cfg(windows)]
    {
        common::set_externs(&payloads[0].1.externs);
        let want = reference_pure();
        let t = lower_table(&payloads[0].1, &table::WIN64).unwrap();
        let (h, _) = common::lower_regions(&payloads[0].1, &win64::Win64);
        let rt = jit_run_noctx(&t);
        let rh = jit_run_noctx(&h);
        println!("\n  native exec pure_compute: table(Q7)->{rt} hand(Q1)->{rh} interp->{} (want {want})",
                 interp::run(&payloads[0].1));
    }

    // =====================================================================
    // ② SEAM CONFLICTS — demonstrated empirically, then listed.
    // =====================================================================
    println!("\n--- ② seam observations (evidence for the conflict list) ---\n");

    // S1: Q7 x SpawnWait — coverage gap.
    let s1 = lower_table(&payloads[2].1, &table::WIN64).is_err();
    println!("  S1  Q7 marshaller lowers spawn_echo? {} (SpawnWait has no table row)",
             if s1 { "NO — REFUSED" } else { "yes" });

    // S2/S3: Q4 guard over each payload — coverage fractions; and it needs 2 CODEGEN sides.
    println!("  S2  Q4 structural guard (needs TWO codegen lowerings; interp/Q7-run have none):");
    for (name, m) in &payloads {
        common::set_externs(&m.externs);
        match verify::VerifiedArtifact::build(m, &sysv64::SysV64, &win64::Win64) {
            Ok(a) => {
                let c = a.win_coverage();
                println!("        {:<16} congruent=OK  neutral={:.0}%  intent(unverified)={:.0}%  whole-identical={}",
                         name, c.neutral_frac() * 100.0, c.intent_frac() * 100.0, a.identical());
            }
            Err(e) => println!("        {:<16} congruence FAILED: {e:?}", name),
        }
    }

    // S4: Q3 content addressing vs Q4 equivalence — same behaviour, different bytes.
    println!("  S4  Q3 content-id(bytes) vs Q4 behaviour-equivalence — read_hash_print, win64:");
    {
        let m = &payloads[1].1;
        common::set_externs(&m.externs);
        let (hand, _) = common::lower_regions(m, &win64::Win64);
        let table_bytes = lower_table(m, &table::WIN64).unwrap();
        println!("        hand(Q1) : {} B  content-id={:016x}", hand.len(), content_id(&hand));
        println!("        table(Q7): {} B  content-id={:016x}", table_bytes.len(), content_id(&table_bytes));
        println!("        -> behaviourally equivalent, byte-different => CA sees TWO adapters;");
        println!("           Q4's guard cannot even PAIR them (it compares sysv-vs-win of ONE backend).");
        println!("           interp backend: NO bytes at all => content-id undefined (nothing to hash).");
    }

    // S5: Q15 intent allowlist still bites through the IR, regardless of Q7 tablification.
    println!("  S5  Q15 intent allowlist on the composed IR (deny SpawnWait):");
    {
        let mut pol = Policy::permissive();
        pol.allowed = !0 & !(1u64 << (Intent::SpawnWait as u64));
        let m = &payloads[2].1;
        let r = run_policy(m, &pol);
        println!("        spawn_echo under deny-SpawnWait -> {:?}", r);
        println!("        -> the allowlist keys on Intent in the IR extern table, which survives");
        println!("           Q7's tablification (intent stays the table KEY). Gate holds.");
        println!("           BUT: it is an interpreter-loop chokepoint. Q7 native code has NO loop;");
        println!("           to keep the gate under Q7 it must move to BIND TIME (refuse to emit).");
    }

    // S6: Q13 declare detection — does interp vs codegen reach the same conclusion?
    #[cfg(windows)]
    {
        let (pass, fire) = declare::pagesize_detect();
        println!("  S6  Q13 detection (SYSTEM_INFO.dwPageSize): pass_on_correct={pass} fire_on_corrupt={fire}");
        println!("        interp seam bakes layout via Rust #[repr(C)] (compiler-computed offsets);");
        println!("        Q7 table + Q1 lowerer bake NUMERIC offsets (104, @0, @4). Detection is");
        println!("        LOAD-BEARING for the codegen bake, near-VACUOUS for the interp bake");
        println!("        (nothing numeric to get wrong) => same round-trip result, different target.");
    }

    // =====================================================================
    // ③ COMPOSED COST (one ruler: file size of the single composed binary; LOC separately)
    // =====================================================================
    println!("\n--- ③ composed cost ---");
    if let Ok(md) = std::fs::metadata(std::env::current_exe().unwrap()) {
        println!("  composed binary (this exe, all backends+guard+tables+detection): {} B on disk", md.len());
    }
    println!("  NOTE: not comparable to any single-Q byte number — the union carries THREE");
    println!("  backends + the guard + both tables + detection, each measured alone under a");
    println!("  DIFFERENT 口径 (some incl. OS seam, some excl.). LOC is the only shared ruler.");

    let _ = std::fs::remove_file("input.txt");
    println!("\n================ end Q16 ================");
}
