//! Q15 driver — is the interpreter a POLICY-ENFORCEMENT point for agent-produced code?
//!
//! J1 (①): each policy class gets a malicious IR payload the interpreter must refuse, WITH a
//!         permissive-vs-strict contrast proving the check is not a no-op.
//! J2 (②): the intent-boundary escape — allow SpawnWait, show the interpreter has ZERO control
//!         over the child, and that the dangerous datum (the command) is not even an IR value.
//! J3 (③): per-instruction overhead (policy interp vs Q9 plain interp on pure_compute) + size.
//! J4 (④): JIT contrast — which policies are O(1) shared code vs O(ops) emitted guard bytes.
//!
//! Reuses Q1 IR + payloads + Q9 interpreter verbatim via #[path]. This file is harness only.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "../interp/interp.rs"]
mod interp; // Q9 plain interpreter — the ③ baseline
#[path = "interp_policy.rs"]
mod interp_policy;
#[path = "evil.rs"]
mod evil;

use interp_policy::{run_policy, Policy, Violation};
use ir::{Intent, Module};
use std::time::Instant;

fn strict() -> Policy {
    Policy {
        max_steps: 100_000_000,
        max_alloc: 1 << 20, // 1 MiB budget
        bounds: true,
        allowed: !0,
        taint_reads: true,
        block_tainted_write: true,
    }
}

fn allow_only(bits: &[Intent]) -> u64 {
    let mut a = 0u64;
    for i in bits { a |= 1u64 << (*i as u64); }
    a
}

fn show(label: &str, r: &Result<u64, Violation>) {
    match r {
        Ok(v) => println!("      {label:<28} -> Ok({v})"),
        Err(e) => println!("      {label:<28} -> REFUSED: {e:?}"),
    }
}

fn main() {
    println!("== Q15: is the interpreter a policy-enforcement point? ==\n");

    #[cfg(windows)]
    {
        std::fs::write("input.txt", b"dynamic-core experiment 2026-08-08\n").unwrap();
    }

    // ============================ J1 — ① CAN it enforce, per class ============================
    println!("== J1 — ① per-class enforcement (each malicious IR must be refused) ==\n");

    // ①memory — out-of-bounds store
    println!("  [①memory] oob_store: Alloc(8) then Store8 at base+64");
    let m = evil::oob_store();
    let permissive = { let mut p = Policy::permissive(); p.bounds = false; p };
    let strict_bounds = { let mut p = Policy::permissive(); p.bounds = true; p };
    show("bounds OFF (agent wins)", &run_policy(&m, &permissive));
    show("bounds ON  (refused)", &run_policy(&m, &strict_bounds));

    // ①resource halting — infinite loop (NEVER run unbounded; that hangs)
    println!("\n  [①resource:halting] infinite_loop: spins forever");
    let m = evil::infinite_loop();
    let mut p = Policy::permissive(); p.max_steps = 1_000_000;
    show("step-limit 1e6 (refused)", &run_policy(&m, &p));
    println!("      (no-limit case not run: it never terminates — that IS the point)");

    // ①resource memory — huge alloc
    println!("\n  [①resource:memory] huge_alloc: Alloc(1 TiB)");
    let m = evil::huge_alloc();
    let mut p = Policy::permissive(); p.max_alloc = 1 << 20;
    show("no budget (call attempted)", &run_policy(&m, &Policy::permissive()));
    show("1 MiB budget (refused)", &run_policy(&m, &p));

    // ①OS surface — spawn denied
    println!("\n  [①OS-surface] spawner: only act is SpawnWait");
    let m = evil::spawner();
    let mut deny = Policy::permissive(); deny.allowed = allow_only(&[Intent::Alloc, Intent::WriteStdout]);
    show("SpawnWait allowed", &run_policy(&m, &Policy::permissive()));
    show("SpawnWait denied (refused)", &run_policy(&m, &deny));

    // ①data-flow positive — echo file to stdout
    println!("\n  [①data-flow +] echo_file: FileRead -> WriteStdout verbatim (exfiltration)");
    let m = evil::echo_file();
    let mut p = Policy::permissive(); p.taint_reads = true; p.block_tainted_write = true;
    show("taint OFF (bytes leak)", &run_policy(&m, &Policy::permissive()));
    show("taint ON  (refused)", &run_policy(&m, &p));

    // ①data-flow LAUNDERING — read_hash_print launders taint through a table lookup
    println!("\n  [①data-flow -] read_hash_print: file bytes -> hash -> hex via lookup table");
    let m = payloads::read_hash_print();
    show("taint ON (NOT refused!)", &run_policy(&m, &p));
    println!("      ^ file-derived output still writes: value-taint is LOST through hextab[nibble].");
    println!("        Data-flow enforcement is PARTIAL — this is its real boundary (④/②).");

    // sanity: honest payloads still pass strict policy
    println!("\n  [sanity] honest payloads under STRICT policy:");
    show("pure_compute", &run_policy(&payloads::pure_compute(), &strict()));
    println!("      (read_hash_print/spawn_echo touch OS + are covered above)");

    // ============================ J2 — ② control ENDS at the intent boundary ==================
    println!("\n== J2 — ② is there any control PAST the intent boundary? (main criterion) ==\n");
    let sp = evil::spawner();
    println!("  spawner IR facts the interpreter can see:");
    println!("      rodata bytes ............. {}", sp.rodata.len());
    println!("      SpawnWait extern nargs ... {}", sp.externs.iter().find(|e| e.intent == Intent::SpawnWait).map(|e| e.nargs).unwrap_or(0));
    println!("      -> the command 'cmd.exe /c exit 7' is NOT an IR value; it lives in the seam");
    println!("         BELOW the IR. Taint/allowlist/bounds cannot reach it: there is no Val to gate.");
    #[cfg(windows)]
    {
        let r = run_policy(&sp, &Policy::permissive());
        println!("  allow SpawnWait -> child ran, interpreter saw ONLY: {r:?}");
        println!("      the interpreter did NOT observe/constrain a single thing the child did.");
        println!("      Verdict ②: control point is the intent ALLOW/DENY gate. Past it: NONE.");
    }

    // ============================ J3 — ③ cost of per-instruction checks =======================
    println!("\n== J3 — ③ per-instruction overhead + size ==\n");
    #[cfg(windows)]
    {
        let pure = payloads::pure_compute();
        let reps = 200u32;
        // Q9 plain interpreter (baseline)
        let t = Instant::now();
        for _ in 0..reps { std::hint::black_box(interp::run(&pure)); }
        let base = t.elapsed().as_secs_f64() / reps as f64;
        // policy interpreter, worst case: step-count + bounds + taint every instruction
        let pol = strict();
        let t = Instant::now();
        for _ in 0..reps { std::hint::black_box(run_policy(&pure, &pol).unwrap()); }
        let strict_t = t.elapsed().as_secs_f64() / reps as f64;
        println!("  pure_compute (1M-iter loop):");
        println!("      Q9 plain interp .......... {:>8.3} us", base * 1e6);
        println!("      policy interp (all on) ... {:>8.3} us   -> {:.2}x", strict_t * 1e6, strict_t / base);
        println!("  (③ size: run size_delta.sh — policy eval-core .text vs Q9's 1908 B)");
    }

    // ============================ J4 — ④ JIT contrast =========================================
    println!("\n== J4 — ④ same policies on the JIT route (structural, byte-costed) ==\n");
    let pure = payloads::pure_compute();
    let rhp = payloads::read_hash_print();
    let n_mem_ops_pure = count_mem_ops(&pure);
    let n_mem_ops_rhp = count_mem_ops(&rhp);
    let n_intent_rhp = count_intents(&rhp);
    println!("  memory-op counts (Load/Store): pure={n_mem_ops_pure}, read_hash_print={n_mem_ops_rhp}");
    println!("  intent calls in read_hash_print: {n_intent_rhp}");
    println!();
    println!("  class            | interpreter cost      | JIT cost");
    println!("  -----------------|-----------------------|-------------------------------------------");
    println!("  memory-bounds    | 1 `if` in Load/Store  | ~10-15 B guard EMITTED per mem op (cmp+jae)");
    println!("                   |  arm = O(1) code      |  = O(mem-ops) bytes, or a load-time verifier");
    println!("  resource:halting | 1 counter in loop     | counter increment EMITTED per block/backedge");
    println!("                   |  = O(1) code          |  = O(blocks) bytes (or watchdog thread)");
    println!("  resource:memory  | 1 add+cmp at Alloc    | 1 add+cmp at the Alloc call site");
    println!("                   |  = O(1)               |  = O(1)  <- SAME (both gate the call)");
    println!("  OS-surface       | allowlist bit at Call | allowlist bit at the resolved-call site");
    println!("                   |  = O(1)               |  = O(1)  <- SAME (both gate the call)");
    println!("  data-flow        | taint bitvec, partial | taint regs EMITTED per op, or verifier");
    println!("                   |  = O(1) code, partial  |  = O(ops) bytes, SAME partial coverage");
    println!();
    println!("  Structural read: the two INTENT-boundary policies (OS-surface, memory-budget) cost");
    println!("  O(1) on BOTH routes — same chokepoint. The PER-INSTRUCTION policies (bounds, halting,");
    println!("  data-flow) are O(1) shared code for the interpreter (it already has the dispatch loop)");
    println!("  but O(ops) EMITTED bytes for the JIT — or a load-time verifier (§4.1 eBPF: 20,065 lines).");

    println!("\n== done ==");
}

fn count_mem_ops(m: &Module) -> usize {
    let mut n = 0;
    for b in &m.blocks {
        for i in &b.insts {
            match i {
                ir::Inst::Set(_, op) => if matches!(op, ir::Op::Load8(_) | ir::Op::LoadW(_)) { n += 1; },
                ir::Inst::Store8(_, _) | ir::Inst::StoreW(_, _) => n += 1,
                _ => {}
            }
        }
    }
    n
}

fn count_intents(m: &Module) -> usize {
    let mut n = 0;
    for b in &m.blocks {
        for i in &b.insts {
            if let ir::Inst::Call(_, _, _) = i { n += 1; }
        }
    }
    n
}
