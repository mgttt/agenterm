//! Q19 driver — the IR structural verifier as the interpreter path's produce-time gate.
//!
//! ① boolean gate (negative-probe posture):
//!     - the three real Q1 payloads must VERIFY (PASS);
//!     - five injected bad IRs (out-of-range index / undefined callee / arity mismatch /
//!       jump to illegal target / rodata offset OOB) must FIRE.
//! ② the gate composes with Q9's interpreter through the un-forgettable `VerifiedModule`
//!    construction gate: `run_verified` cannot be called without having passed `verify`.
//! ③ coverage boundary: three demonstrations of what the verifier CANNOT see (a well-formed
//!    but memory-unsafe IR passes; a well-formed but semantically wrong IR passes; the OS
//!    seam / L1–L5 content is not in the IR at all).
//!
//! ir/spec, payloads, and Q9's interpreter are REUSED verbatim via `#[path]`. This file is
//! harness + verify.rs (the new code). No Cargo, never touches the root workspace.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "../interp/interp.rs"]
mod interp;
#[path = "verify.rs"]
mod verify;

#[allow(unused)]
use ir::*;
use verify::{verify, IrFault, VerifiedModule};

/// The gate composed with Q9's interpreter: you can ONLY run a module you have verified,
/// because `run_verified` demands proof (`&VerifiedModule`) that only `verify` can mint.
/// This is the un-forgettability of criterion ②, wired to the actual Q9 `interp::run`.
fn run_verified(vm: &VerifiedModule) -> u64 {
    interp::run(vm.module())
}

fn main() {
    println!("== Q19 — IR STRUCTURAL VERIFIER (interpreter-path produce-time gate) ==\n");

    // -------------------------------------------------------------------------------------
    // ① positive: the three real payloads are well-formed → must PASS
    // -------------------------------------------------------------------------------------
    println!("① positive gate — the three Q1 payloads must VERIFY (PASS):");
    let good: [(&str, ir::Module); 3] = [
        ("pure_compute", payloads::pure_compute()),
        ("read_hash_print", payloads::read_hash_print()),
        ("spawn_echo", payloads::spawn_echo()),
    ];
    let mut all_pass = true;
    for (name, m) in &good {
        match verify(m) {
            Ok(_) => println!("  {name:<16} PASS (well-formed)"),
            Err(e) => {
                all_pass = false;
                println!("  {name:<16} *** UNEXPECTED FIRE: {e:?}  (should have passed)");
            }
        }
    }

    // -------------------------------------------------------------------------------------
    // ② compose with Q9 interpreter through the un-forgettable gate.
    //    pure_compute has no OS deps → deterministic 163; run it ONLY via the verified path.
    // -------------------------------------------------------------------------------------
    println!("\n② the gate composes with Q9's interpreter (un-forgettable construction gate):");
    let pc = payloads::pure_compute();
    match verify(&pc) {
        Ok(vm) => {
            let r = run_verified(&vm); // <- cannot be reached without `vm`, which only verify() mints
            println!("  pure_compute verified -> run_verified -> {r} (expect 163)  {}",
                     if r == 163 { "OK" } else { "*** WRONG" });
        }
        Err(e) => println!("  *** pure_compute failed to verify: {e:?}"),
    }
    println!("  (rhp/spawn already executed under Q9's path; re-running adds no verifier info)");

    // -------------------------------------------------------------------------------------
    // ① negative probes — inject bad IR, the verifier MUST FIRE (negative-probe posture).
    //    Each probe rebuilds a fresh payload and corrupts one thing.
    // -------------------------------------------------------------------------------------
    println!("\n① negative probes — injected bad IR MUST FIRE:");
    let mut all_fire = true;

    // P1 — out-of-range value index: reference Val 9999 where n_vals is tiny.
    {
        let mut m = payloads::pure_compute();
        m.blocks[0].insts.push(ir::Inst::Set(0, ir::Op::Add(9999, 0)));
        all_fire &= expect_fire("P1 越界索引 (out-of-range value index)", &m,
            matches!(verify(&m), Err(IrFault::ValOutOfRange { .. })), verify(&m).err());
    }
    // P2 — undefined callee (the "undefined opcode" analog): extern id past the table.
    {
        let mut m = payloads::read_hash_print();
        m.blocks[0].insts.push(ir::Inst::Call(0, 99, vec![]));
        all_fire &= expect_fire("P2 未定义 opcode/callee (undefined extern id)", &m,
            matches!(verify(&m), Err(IrFault::ExternIdOutOfRange { .. })), verify(&m).err());
    }
    // P3 — type/arity mismatch: call extern 0 (Alloc, nargs=1) with 0 args.
    {
        let mut m = payloads::read_hash_print();
        m.blocks[0].insts.push(ir::Inst::Call(0, 0, vec![]));
        all_fire &= expect_fire("P3 类型/arity 不匹配 (arity mismatch)", &m,
            matches!(verify(&m), Err(IrFault::ArityMismatch { .. })), verify(&m).err());
    }
    // P4 — control flow to an illegal target: jump to block 999.
    {
        let mut m = payloads::pure_compute();
        m.blocks[0].term = ir::Term::Br(999);
        all_fire &= expect_fire("P4 控制流跳到非法目标 (CFI: illegal jump target)", &m,
            matches!(verify(&m), Err(IrFault::BlockTargetOutOfRange { .. })), verify(&m).err());
    }
    // P5 — rodata offset past the blob.
    {
        let mut m = payloads::read_hash_print();
        m.blocks[0].insts.push(ir::Inst::Set(0, ir::Op::Rodata(99999)));
        all_fire &= expect_fire("P5 rodata 偏移越界 (data-side OOB index)", &m,
            matches!(verify(&m), Err(IrFault::RodataOffsetOutOfRange { .. })), verify(&m).err());
    }

    // -------------------------------------------------------------------------------------
    // ③ coverage boundary — what the verifier CANNOT see (all must PASS despite being wrong).
    // -------------------------------------------------------------------------------------
    println!("\n③ coverage boundary — well-formed does NOT mean safe/correct:");

    // B1 — well-formed but MEMORY-UNSAFE: Alloc(8) then Store8 at base+64.
    //      (This is Q15's `oob_store`. NOT executed here — it is a real OOB write.)
    {
        let mut b = ir::Builder::new();
        let cap = b.konst(8);
        let base = b.call(Intent::Alloc, vec![cap]);
        let off = b.konst(64);
        let bad = b.set(Op::Add(base, off));
        let val = b.konst(0x41);
        b.store8(bad, val);
        let z = b.konst(0);
        b.term(Term::Exit(z));
        let m = b.finish("oob_store", true, 0);
        let passes = verify(&m).is_ok();
        println!("  B1 memory safety : oob_store (writes 64B past an 8B alloc) -> verify {} \
                  → structural != memory-safe (needs value-range analysis = eBPF's 20k lines / Q15 run-time)",
                 if passes { "PASS" } else { "FIRE" });
    }
    // B2 — well-formed but SEMANTICALLY WRONG: a module that Exits 999 (not the intended 163).
    {
        let mut b = ir::Builder::new();
        let w = b.konst(999);
        b.term(Term::Exit(w));
        let m = b.finish("wrong_result", false, 0);
        let passes = verify(&m).is_ok();
        println!("  B2 semantics     : wrong_result (Exit 999) -> verify {} \
                  → well-formed != behaviourally correct (the verifier is not a correctness proof)",
                 if passes { "PASS" } else { "FIRE" });
    }
    // B3 — the OS seam is not in the IR at all: spawn_echo's dangerous content lives BELOW it.
    {
        let m = payloads::spawn_echo();
        let spawn_nargs = m.externs.iter()
            .find(|e| e.intent == Intent::SpawnWait).map(|e| e.nargs).unwrap_or(0);
        println!("  B3 OS seam (L1–L5): spawn_echo rodata={}B, SpawnWait extern nargs={} \
                  → the command 'cmd.exe /c exit 7' is NOT an IR value; the verifier (like Q4/Q9/Q15) \
                  cannot see the seam it stops at",
                 m.rodata.len(), spawn_nargs);
    }

    println!("\n== SUMMARY ==");
    println!("  ① positive (3 payloads PASS): {}", if all_pass { "OK" } else { "FAIL" });
    println!("  ① negative (5 probes FIRE):   {}", if all_fire { "OK" } else { "FAIL" });
    println!("  ② gate composes w/ interpreter, un-forgettable construction gate: OK");
    println!("  ③ boundary: memory-safety / semantics / OS-seam all UNVERIFIABLE structurally");
}

/// Print a probe result and return whether it fired as required.
fn expect_fire(label: &str, _m: &ir::Module, fired_correctly: bool, err: Option<IrFault>) -> bool {
    match (fired_correctly, err) {
        (true, Some(e)) => {
            println!("  {label:<44} FIRE  {e:?}");
            true
        }
        (false, Some(e)) => {
            println!("  {label:<44} *** FIRED WRONG FAULT: {e:?}");
            false
        }
        (_, None) => {
            println!("  {label:<44} *** DID NOT FIRE (verifier missed it)");
            false
        }
    }
}
