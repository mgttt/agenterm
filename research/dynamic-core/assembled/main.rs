//! Q22 — ASSEMBLED MINIMAL REFERENCE CORE. One binary, real machine (Windows Server
//! 2022 / x86_64), interpreter path only (no codegen — see RESULTS.md for why that
//! side-steps Q16's S2 rather than re-triggering it).
//!
//! Six parts, all reused from this track's own prior experiments (see the `#[path]`
//! list below and the per-file header comments for what is verbatim vs new glue):
//!   1. Interpreted execution as the default strategy   — `eval_core.rs`  (Q9/Q12)
//!   2. IR well-formedness verification (produce-time)   — `verify.rs`    (Q19, #[path])
//!   3. Table-driven OS-call marshalling                 — `seam.rs`      (Q7 vocabulary)
//!   4. `call`'s float/int Kind tag on the placement axis — `seam.rs::Kind` (Q20)
//!   5. Content addressing + build-time-pinned discovery — `store.rs`     (Q3 + Q18)
//!   6. Linear orchestration step table                  — `step_table.rs` (Q21, #[path])
//!
//! Reproduce:
//!   cd research/dynamic-core/assembled
//!   mkdir out 2>$null; cd out
//!   [IO.File]::WriteAllText("$PWD\input.txt","dynamic-core experiment 2026-08-08`n")
//!   rustc --edition 2021 -O -A dead_code ../main.rs -o assembled.exe
//!   ./assembled.exe

#[path = "ir.rs"]
mod ir;
#[path = "../verify/verify.rs"]
mod verify;
#[path = "../orchestration/step_table.rs"]
mod step_table;
#[path = "seam.rs"]
mod seam;
#[path = "eval_core.rs"]
mod eval_core;
#[path = "ir_ser.rs"]
mod ir_ser;
#[path = "store.rs"]
mod store;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "extra_payloads.rs"]
mod extra_payloads;

use ir::Module;
use store::Store;

fn pack(st: &Store, name: &str, m: &Module) -> String {
    let bytes = ir_ser::serialize(m);
    st.pack(name, &bytes)
}

/// LOAD PHASE: read bytes from the content store (fresh from disk, no reuse of the
/// in-memory Module the pack phase built), deserialize, and run the well-formedness
/// gate. Returns Err(reason) instead of a VerifiedModule/Module on any failure.
fn load_and_verify<'a>(st: &Store, name: &'a str) -> Result<verify::VerifiedModule<'static>, String> {
    let bytes = st.load(name).ok_or_else(|| format!("{name}: store/manifest read failed"))?;
    let module = ir_ser::deserialize(&bytes).ok_or_else(|| format!("{name}: deserialize failed (corrupt bytes)"))?;
    let module: &'static Module = Box::leak(Box::new(module));
    verify::verify(module).map_err(|f| format!("{name}: REJECTED by verify() — {f:?}"))
}

fn main() {
    let mut all_ok = true;
    let st = Store::open(".");
    let ctx = seam::SeamCtx::new();

    println!("== Q22 assembled minimal reference core ==\n");

    // ---------------------------------------------------------------
    // PACK PHASE (simulates build time): construct each payload's IR,
    // serialize it, and write it into the content-addressed store under
    // a build-time-pinned manifest (property 1 + Q18's verdict).
    // ---------------------------------------------------------------
    println!("-- pack phase (content-addressed store, build-time-pinned manifests) --");
    let h1 = pack(&st, "pure_compute", &payloads::pure_compute());
    let h2 = pack(&st, "read_hash_print", &payloads::read_hash_print());
    let h3 = pack(&st, "spawn_echo", &payloads::spawn_echo());
    let h4 = pack(&st, "filewrite_demo", &extra_payloads::filewrite_demo());
    let h5 = pack(&st, "bad_ir_demo", &extra_payloads::bad_ir_demo());
    let h6 = pack(&st, "mismatched_arity_demo", &extra_payloads::mismatched_arity_demo());
    println!("  pure_compute           -> store/{h1}.bin");
    println!("  read_hash_print        -> store/{h2}.bin");
    println!("  spawn_echo             -> store/{h3}.bin");
    println!("  filewrite_demo         -> store/{h4}.bin   (NEW intent, Q22)");
    println!("  bad_ir_demo            -> store/{h5}.bin   (deliberately malformed)");
    println!("  mismatched_arity_demo  -> store/{h6}.bin   (deliberately seam-hostile)\n");

    // ---------------------------------------------------------------
    // LOAD PHASE (simulates run time): read the manifest -> hash -> blob
    // from disk, deserialize, and pass through verify::verify BEFORE any
    // execution (property 2). Then execute the ones that pass, via the
    // table-driven interpreter (properties 3/4/5/6).
    // ---------------------------------------------------------------
    println!("-- load phase: content-addressed load -> IR verify gate -> interpret --\n");

    // ① pure_compute
    match load_and_verify(&st, "pure_compute") {
        Ok(vm) => {
            let r = eval_core::run(&vm, &ctx);
            let ok = r == 163;
            println!("[pure_compute]      verify=PASS  run={r}  expect=163  {}", if ok { "OK" } else { "FAIL" });
            all_ok &= ok;
        }
        Err(e) => { println!("[pure_compute]      UNEXPECTED REJECT: {e}"); all_ok = false; }
    }

    // ② read_hash_print (needs input.txt in CWD, 35 bytes, same fixture as Q1/Q9/Q19)
    match load_and_verify(&st, "read_hash_print") {
        Ok(vm) => {
            let r = eval_core::run(&vm, &ctx);
            let ok = r == 0;
            println!("[read_hash_print]   verify=PASS  exit={r}  expect=0 (hash printed above)  {}", if ok { "OK" } else { "FAIL" });
            all_ok &= ok;
        }
        Err(e) => { println!("[read_hash_print]   UNEXPECTED REJECT: {e}"); all_ok = false; }
    }

    // ③ spawn_echo (Q21 linear step table)
    match load_and_verify(&st, "spawn_echo") {
        Ok(vm) => {
            let r = eval_core::run(&vm, &ctx);
            let ok = r == 7;
            println!("[spawn_echo]        verify=PASS  exit={r}  expect=7 (\"exit=07\" printed above)  {}", if ok { "OK" } else { "FAIL" });
            all_ok &= ok;
        }
        Err(e) => { println!("[spawn_echo]        UNEXPECTED REJECT: {e}"); all_ok = false; }
    }

    // ④ filewrite_demo (the NEW intent)
    match load_and_verify(&st, "filewrite_demo") {
        Ok(vm) => {
            let r = eval_core::run(&vm, &ctx);
            let written = std::fs::read("dc_assembled_filewrite_out.txt").unwrap_or_default();
            let expect = b"hello from Q22 FileWrite\n";
            let ok = r == 25 && written == expect;
            println!(
                "[filewrite_demo]    verify=PASS  bytes_written={r}  expect=25  file_content_matches={}  {}",
                written == expect,
                if ok { "OK" } else { "FAIL" }
            );
            all_ok &= ok;
        }
        Err(e) => { println!("[filewrite_demo]    UNEXPECTED REJECT: {e}"); all_ok = false; }
    }

    // ⑤ bad_ir_demo — MUST be rejected by verify() before any execution.
    match load_and_verify(&st, "bad_ir_demo") {
        Ok(_) => { println!("[bad_ir_demo]       FAIL: verify() ACCEPTED malformed IR (should have rejected)"); all_ok = false; }
        Err(e) => { println!("[bad_ir_demo]       verify=REJECTED as expected -> {e}  OK"); }
    }

    // ⑥ mismatched_arity_demo — passes verify() (self-consistent IR: declared nargs ==
    // call-site arg count) but violates the SEAM TABLE's own arity assumption for
    // FileWrite (3 args). This is the seam-audit finding: verify.rs checks IR-internal
    // consistency, not IR x seam-table cross-consistency; nothing in this assembly
    // plays the role Q7's `validate()` played for its own OpSpec table. Caught here
    // via catch_unwind so the demo reports it instead of crashing.
    match load_and_verify(&st, "mismatched_arity_demo") {
        Ok(vm) => {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| eval_core::run(&vm, &ctx)));
            match result {
                Ok(r) => { println!("[mismatched_arity]  UNEXPECTED: ran to completion, r={r} (expected a panic)"); all_ok = false; }
                Err(_) => { println!("[mismatched_arity]  verify=PASS (self-consistent) but interpreter PANICS at the seam boundary -- SEAM GAP (see RESULTS.md ②), not a crash in this harness's accounting"); }
            }
        }
        Err(e) => { println!("[mismatched_arity]  UNEXPECTED REJECT at verify(): {e}"); all_ok = false; }
    }

    println!("\n== RESULT: {} ==", if all_ok { "ALL CANONICAL PAYLOADS PASS" } else { "SOME CHECKS FAILED" });
    std::process::exit(if all_ok { 0 } else { 1 });
}
