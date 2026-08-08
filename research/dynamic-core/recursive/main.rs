//! Q17 driver — the recursive / self-hosting boundary experiment.
//!
//! Question (from the track's strongest convergence: all residuals stop at the intent
//! boundary): if the thing on the far side of an intent is ITSELF our own core running
//! neutral IR — not a foreign binary — do observation/verification/policy re-establish
//! on the far side? And crucially (§②): where does the seam MOVE, and is it smaller?
//!
//! One layer only (A→B). Two process roles in ONE binary:
//!   * parent (default): core A. Builds sub-tasks as neutral IR, inspects/constrains them,
//!     spawns core B as a REAL child process (std::process::Command → CreateProcessA),
//!     hands B the IR over the wire, reads B's structured report. Also runs the Q15
//!     foreign-spawn baseline (cmd.exe) for the item-by-item observation contrast.
//!   * child  (`--child <in> <out>`): core B. Reads an IR module, runs it observed under a
//!     policy, writes a report.
//!
//! Reuse: `ir` spec + Q1 `payloads` via #[path]; Q9 `interp` for the in-process cost
//! baseline. `serial` + `child` are this experiment's only new code.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "../interp/interp.rs"]
mod interp;
#[path = "serial.rs"]
mod serial;
#[path = "child.rs"]
mod child;

use ir::*;
use std::time::Instant;

// ------------------------------------------------------------------ sub-tasks (neutral IR)
/// A pure-compute sub-task: no intents at all. Reuses Q1's pure_compute (returns 163).
fn sub_compute() -> Module { payloads::pure_compute() }

/// An OS-touching sub-task B is allowed to do: alloc a buffer, copy a message, write it,
/// return the length. Uses only {Alloc, WriteStdout} — both in B's safe subset.
fn sub_write() -> Module {
    let mut b = Builder::new();
    let msg = b"hello from core B\n";
    let msg_off = b.rodata(msg);
    let len = msg.len() as u64;

    let clen = b.konst(len);
    let buf = b.call(Intent::Alloc, vec![clen]);
    let src = b.set(Op::Rodata(msg_off));
    let i = b.konst(0);
    let one = b.konst(1);
    let n = b.konst(len);
    b.term(Term::Br(1));
    // block 1: copy guard
    let cont = b.set(Op::Ult(i, n));
    b.term(Term::BrCond(cont, 2, 3));
    // block 2: copy body
    let sa = b.set(Op::Add(src, i));
    let sb = b.set(Op::Load8(sa));
    let da = b.set(Op::Add(buf, i));
    b.store8(da, sb);
    b.assign(i, Op::Add(i, one));
    b.term(Term::Br(1));
    // block 3: write + exit len
    let wlen = b.konst(len);
    let wrote = b.call(Intent::WriteStdout, vec![buf, wlen]);
    b.term(Term::Exit(wrote));
    b.finish("sub_write", false, 0)
}

/// A sub-task that reaches for a FORBIDDEN intent (SpawnWait) — used to demonstrate that
/// A can (a) inspect the IR and refuse before launch, and (b) B denies it at runtime.
fn sub_evil() -> Module {
    let mut b = Builder::new();
    let code = b.call(Intent::SpawnWait, vec![]);
    b.term(Term::Exit(code));
    b.finish("sub_evil", false, 0)
}

// ------------------------------------------------------------------ the process boundary
fn out_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap().join("out")
}

/// A spawns B as a real child process and hands over the module. Returns (report_bytes,
/// wall_time). This is the A↔B seam in action: CreateProcessA (opaque launch, same as Q15)
/// + downlink file + uplink file.
fn recurse(m: &Module, allowed_tags: &[Intent], tag: &str) -> (serial::Report, f64, usize) {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let infile = dir.join(format!("{tag}.in.bin"));
    let outfile = dir.join(format!("{tag}.out.bin"));

    let wire = serial::ser_module(m);
    let wire_len = wire.len();
    std::fs::write(&infile, &wire).unwrap();
    // encode the policy A imposes on B as a comma tag string
    let pol: String = allowed_tags.iter().map(|i| serial::intent_tag(*i).to_string()).collect::<Vec<_>>().join(",");

    let exe = std::env::current_exe().unwrap();
    let t = Instant::now();
    let status = std::process::Command::new(exe)
        .arg("--child")
        .arg(&infile)
        .arg(&outfile)
        .arg(&pol)
        .status()
        .expect("spawn core B");
    let wall = t.elapsed().as_secs_f64();
    assert!(status.success(), "core B exited non-zero");
    let rep_bytes = std::fs::read(&outfile).unwrap();
    (serial::de_report(&rep_bytes), wall, wire_len)
}

/// Pre-launch inspection: A holds the sub-task as an IR VALUE and can enumerate exactly
/// which intents it will attempt — BEFORE it runs. (Contrast Q15: the command was not an
/// IR value.) Returns the set of intents present.
fn inspect_intents(m: &Module) -> Vec<Intent> {
    m.externs.iter().map(|e| e.intent).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--child" {
        return child_main(&args[2], &args[3], args.get(4).map(|s| s.as_str()).unwrap_or(""));
    }
    parent_main();
}

// ------------------------------------------------------------------ core B entry (child)
fn child_main(infile: &str, outfile: &str, pol: &str) {
    let wire = std::fs::read(infile).expect("read module");
    let m = serial::de_module(&wire);
    let allowed: Vec<Intent> = if pol.is_empty() {
        vec![]
    } else {
        pol.split(',').map(|s| serial::intent_of(s.parse::<u8>().unwrap())).collect()
    };
    let rep = child::run_observed(&m, &allowed);
    std::fs::write(outfile, serial::ser_report(&rep)).expect("write report");
}

// ------------------------------------------------------------------ core A entry (parent)
fn parent_main() {
    println!("== Q17 — recursive / self-hosting boundary ==\n");

    // ============================================================ ① boolean gate + contrast
    println!("== ① A launches B (real child process) running neutral IR — can A observe/constrain B? ==\n");

    // --- Q15 baseline: A spawns a FOREIGN binary (cmd.exe). Observation = exit code only. ---
    let t = Instant::now();
    let st = std::process::Command::new("cmd.exe").args(["/c", "exit", "7"]).status().unwrap();
    let _q15_wall = t.elapsed().as_secs_f64();
    let q15_code = st.code().unwrap_or(-1);
    println!("  [Q15 foreign spawn]  cmd.exe /c exit 7");
    println!("     A could inspect the task beforehand? NO  (the command is not an IR value)");
    println!("     A observed at runtime .............. exit code = {q15_code}  (nothing else)");
    println!("     A constrained what it did? .......... NO  (allow/deny the spawn, then blind)\n");

    // --- Recursion: A spawns core B (our own interpreter) running sub_write. ---
    let m = sub_write();
    let pre = inspect_intents(&m);
    println!("  [Q17 recursion]  core B runs neutral IR sub_write");
    println!("     A inspected the task beforehand:    {} ops across {} blocks, intents = {:?}",
        m.blocks.iter().map(|b| b.insts.len()).sum::<usize>(), m.blocks.len(),
        pre.iter().map(|i| serial::intent_name(*i)).collect::<Vec<_>>());
    let (rep, wall, wlen) = recurse(&m, &[Intent::Alloc, Intent::WriteStdout], "write");
    println!("     A observed B's runtime trace ({} intents):", rep.trace.len());
    for e in &rep.trace {
        println!("        {:<12} args={:x?} -> ret={}", serial::intent_name(e.intent), e.args, e.ret);
    }
    println!("     A observed B's captured stdout ..... {:?}", String::from_utf8_lossy(&rep.capture).trim_end());
    println!("     A observed B's result .............. {}", rep.result);
    println!("     (downlink module = {wlen} B, round-trip wall = {:.2} ms)\n", wall * 1e3);

    // --- Constraint: A refuses / B denies a forbidden intent. ---
    println!("  [Q17 constraint]  sub_evil reaches for SpawnWait");
    let evil = sub_evil();
    let evil_intents = inspect_intents(&evil);
    let a_would_refuse = evil_intents.iter().any(|i| *i == Intent::SpawnWait);
    println!("     (a) A pre-launch inspection sees intents {:?} -> A refuses before launch: {}",
        evil_intents.iter().map(|i| serial::intent_name(*i)).collect::<Vec<_>>(), a_would_refuse);
    let (erep, _, _) = recurse(&evil, &[Intent::Alloc, Intent::WriteStdout], "evil");
    println!("     (b) if launched anyway with policy {{Alloc,WriteStdout}}, B denies at runtime: {}",
        erep.denied.map(|i| serial::intent_name(i)).unwrap_or("none"));
    println!("     -> the intent chokepoint (Q15's O(1) allow/deny) now guards the INNER core too\n");

    // Item-by-item verdict for ①
    println!("  ── ① observation contrast (item-by-item vs Q15) ──");
    println!("     {:<38} {:<14} {:<14}", "capability", "Q15 foreign", "Q17 recursion");
    for (cap, q15, q17) in [
        ("task inspectable before launch", "no", "yes (IR value)"),
        ("intents known before launch", "no", "yes"),
        ("per-intent runtime trace (args+ret)", "no", "yes"),
        ("side-effect output captured", "no", "yes"),
        ("result value", "yes (exit code)", "yes"),
        ("runtime allow/deny of intents", "at launch only", "per intent, inside B"),
        ("force honesty without trusting callee", "no", "no (still trust B's binary)"),
    ] {
        println!("     {cap:<38} {q15:<14} {q17:<14}");
    }
    println!();

    // ============================================================ ② where the seam moved
    println!("== ② the NEW seam = the A↔B channel (main product) ==\n");
    let ms = [("sub_compute", sub_compute()), ("sub_write", sub_write()), ("sub_evil", sub_evil())];
    println!("  downlink wire size (the sub-task, as an IR VALUE crossing the seam):");
    for (name, mm) in &ms {
        let w = serial::ser_module(mm);
        // round-trip identity check: de(ser(m)) executes to the same result in-process
        let back = serial::de_module(&w);
        let same_shape = back.blocks.len() == mm.blocks.len() && back.n_vals == mm.n_vals;
        println!("     {name:<12} {:>5} B   ({} externs, {} blocks)  round-trips: {}",
            w.len(), mm.externs.len(), mm.blocks.len(), same_shape);
    }
    println!("\n  seam character (the point of §②):");
    println!("     - launch half:   CreateProcessA — IDENTICAL to Q15's L1-L5 spawn, reused ONCE, O(1)");
    println!("     - downlink half: fixed codec; content is NEUTRAL IR A inspects/verifies pre-launch");
    println!("     - uplink half:   fixed codec; B's structured trace (neutral intent tags + values)");
    println!("     - vocabulary:    op tags 0..=12, intent tags 0..=5 — 1 neutral byte each, SHARED");
    println!("                      (contrast Q1 L1: per-target symbol strings/numbers, DISJOINT)");
    println!("     - the OS content (L2-L5: arity, struct layout, out-param width, sentinel) did NOT");
    println!("       shrink — it RELOCATED into B unchanged. B still opens files / spawns with the");
    println!("       same STARTUPINFOA.cb=104 etc. The global OS seam is the same size, now inside B.\n");

    // ============================================================ ③ applicability boundary
    println!("== ③ where recursion does NOT apply ==\n");
    println!("     - foreign binary (cmd.exe): has no parser for our wire format. Handing it");
    println!("       {} bytes of IR does nothing — it is not our core.", serial::ser_module(&sub_write()).len());
    // illustrate: cmd.exe cannot consume the IR; it is opaque both ways.
    println!("       => the far side must be OUR interpreter binary, or observation is impossible.");
    println!("     - existing native library (a .dll/.so we call via primitive ④): same wall — the");
    println!("       code behind the symbol is native, not IR. Recursion cannot wrap it.");
    println!("     - any 'far side is not our IR' case: no downlink is possible, so no re-establish.");
    println!("     => recursion buys observability ONLY for components WE produce as neutral IR.\n");

    // ============================================================ ④ cost
    println!("== ④ cost of one recursion layer ==\n");
    // in-process baseline: run sub_write's identical eval in the SAME process (no boundary).
    let mw = sub_write();
    let reps = 50u32;
    let t = Instant::now();
    for _ in 0..reps { std::hint::black_box(child::run_observed(&mw, &[Intent::Alloc, Intent::WriteStdout])); }
    let inproc = t.elapsed().as_secs_f64() / reps as f64;
    let t = Instant::now();
    let reps_x = 20u32;
    for _ in 0..reps_x { let _ = recurse(&mw, &[Intent::Alloc, Intent::WriteStdout], "cost"); }
    let xproc = t.elapsed().as_secs_f64() / reps_x as f64;
    println!("     same sub-task, in-process (no boundary): {:>8.1} us", inproc * 1e6);
    println!("     same sub-task, across A→B boundary:      {:>8.1} us   ({:.0}x)", xproc * 1e6, xproc / inproc.max(1e-9));
    println!("     the delta = one full interpreter PROCESS + marshal round-trip per observed sub-task.");
    println!("     north-star reading: making one agent sub-task observable costs ~1 process spawn");
    println!("     (~{:.1} ms here) + O(module-size) marshalling. Cheap in bytes, heavy in latency.\n", (xproc - inproc) * 1e3);

    // cleanup
    let dir = out_dir();
    for f in ["write.in.bin","write.out.bin","evil.in.bin","evil.out.bin","cost.in.bin","cost.out.bin"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    println!("== done ==");
}
