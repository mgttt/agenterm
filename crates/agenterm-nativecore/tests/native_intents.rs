//! Real-machine black-box acceptance tests for
//! `plan/design-dynacore-native-core.md` §5. Every test in this file either
//! (a) drives the FULL public pipeline (build IR -> `pack::pack` into a real
//! `store::Store` on disk -> `pack::load` back -> `verify::verify` ->
//! `eval_core::run`), exercising real Win32 APIs through `seam.rs`, or (b)
//! constructs a deliberately malformed IR and asserts `verify()` rejects it
//! BEFORE any `eval_core::run` call happens at all. Nothing here mocks the
//! OS; every native intent in this file that reaches `eval_core::run` makes
//! a real Win32 call.

use agenterm_nativecore::eval_core::{self, Termination};
use agenterm_nativecore::ir::{Block, Builder, Intent, Module, Op, Term};
use agenterm_nativecore::pack;
use agenterm_nativecore::payloads;
use agenterm_nativecore::store::Store;
use agenterm_nativecore::verify::{self, IrFault};

/// Drive a `Module` through the full production pipeline: serialize it into
/// a real content-addressed store on disk, load the bytes back (never reuse
/// the in-memory value the caller built), verify, and — only if verify
/// passes — run it. This is deliberately the SAME shape a real embedder
/// would use, not a shortcut that skips the store/reload round trip.
fn run_through_full_pipeline(store: &Store, m: &Module) -> Result<eval_core::RunOutcome, String> {
    let manifest = pack::pack(store, m).map_err(|e| format!("pack: {e}"))?;
    let loaded = pack::load(store, &manifest)?;
    let verified = verify::verify(&loaded).map_err(|e| format!("verify REJECTED: {e:?}"))?;
    Ok(eval_core::run(&verified))
}

/// Like `run_through_full_pipeline`, but returns the `verify()` failure
/// itself instead of turning it into a generic error string — for tests
/// that need to assert on the specific `IrFault` variant.
fn verify_through_full_pipeline(store: &Store, m: &Module) -> Result<(), IrFault> {
    let manifest = pack::pack(store, m).expect("pack must succeed even for a malformed module (packing is not verification)");
    let loaded = pack::load(store, &manifest).expect("load must succeed (bytes round-trip regardless of well-formedness)");
    verify::verify(&loaded).map(|_| ())
}

/// `VerifiedModule` deliberately has no `Debug` impl (Q19/Q22's un-forgettable
/// construction-gate discipline: nothing about it should be inspectable
/// except by calling `.module()`), so `Result::unwrap_err` can't be used
/// directly on `verify::verify`'s return value. This helper does the
/// equivalent for tests.
fn expect_rejected(m: &Module) -> IrFault {
    match verify::verify(m) {
        Ok(_) => panic!("expected verify() to reject this IR, but it was accepted"),
        Err(fault) => fault,
    }
}

// ============================================================================
// Acceptance criterion 2: seven native intents, real machine, the same three
// payload semantics Q22 used (pure_compute / read_hash_print / spawn_echo),
// plus filewrite_demo for the seventh intent (FileWrite) none of the three
// named payloads exercises.
// ============================================================================

#[test]
fn pure_compute_runs_through_the_full_pipeline_with_no_os_calls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let outcome = run_through_full_pipeline(&store, &payloads::pure_compute()).expect("pure_compute must verify and run");
    assert_eq!(outcome.termination, Termination::Exited(163), "same independent reference value the whole research track has used");
    assert!(outcome.calls.is_empty(), "pure_compute must make ZERO native calls -- if this is not neutral, the thesis dies (design doc §0)");
}

/// `read_hash_print` (Alloc/FileOpen/FileRead/FileClose/WriteStdout) and
/// `spawn_echo` (Alloc/WriteStdout/SpawnWait) both write to the REAL
/// process stdout through `seam.rs`'s `GetStdHandle`/`WriteFile` -- captured
/// here by temporarily swapping STD_OUTPUT_HANDLE to a real pipe (Win32
/// `SetStdHandle`), the same "redirect and read back" technique
/// `declare.rs`'s self-checks use, applied at the process level instead of
/// via `STARTUPINFOA`. Bundled into ONE test (not split across parallel
/// `#[test]` fns) because swapping the process-wide stdout handle is not
/// safe to do concurrently with another test doing the same thing.
#[test]
fn read_hash_print_and_spawn_echo_and_filewrite_run_for_real() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let original_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(dir.path()).expect("chdir into isolated tempdir");

    // Fixed fixture, independent FNV-1a/64 reference computed here (NOT
    // reusing payloads.rs's constants) so this test is not self-proving.
    // The exact byte count is not load-bearing (read_hash_print's Alloc cap
    // is 65536, far larger than this fixture) -- unlike Q1/Q9/Q19's own
    // 35-byte fixture, whose length WAS pinned by their own test harness.
    let fixture: &[u8] = b"dynamic-core nativecore test 2026\n";
    assert_eq!(fixture.len(), 34);
    std::fs::write("input.txt", fixture).expect("write input.txt fixture");
    let expected_hash = independent_fnv1a64_hex(fixture);

    let (read_hash_outcome, stdout1) =
        capture_real_stdout(|| run_through_full_pipeline(&store, &payloads::read_hash_print()).expect("read_hash_print must verify and run"));
    assert_eq!(read_hash_outcome.termination, Termination::Exited(0));
    assert!(
        stdout1.contains(&expected_hash),
        "expected FNV-1a/64 hex {expected_hash:?} of the fixture in captured stdout, got {stdout1:?}"
    );
    assert_eq!(
        read_hash_outcome.calls.iter().filter(|c| c.intent == Intent::FileOpen).count(),
        1,
        "must have actually opened input.txt via a real CreateFileA"
    );

    let (spawn_outcome, stdout2) = capture_real_stdout(|| run_through_full_pipeline(&store, &payloads::spawn_echo()).expect("spawn_echo must verify and run"));
    assert_eq!(spawn_outcome.termination, Termination::Exited(7), "real cmd.exe child spawned via CreateProcessA, exits 7");
    assert!(stdout2.contains("exit=07"), "expected literal 'exit=07' from the real child's exit code, got {stdout2:?}");
    assert_eq!(
        spawn_outcome.calls.iter().filter(|c| c.intent == Intent::SpawnWait).count(),
        1,
        "must have actually spawned a real child process"
    );

    // FileWrite: the seventh intent, not exercised by the three payloads above.
    let out_name = "nativecore_filewrite_test_out.txt";
    let outcome = run_through_full_pipeline(&store, &payloads::filewrite_demo(out_name)).expect("filewrite_demo must verify and run");
    assert_eq!(outcome.termination, Termination::Exited(22), "WriteFile must report all 22 bytes written");
    let written = std::fs::read(out_name).expect("the file must actually exist on disk");
    assert_eq!(written, b"hello from nativecore\n");
    assert_eq!(outcome.calls.iter().filter(|c| c.intent == Intent::FileWrite).count(), 1);

    std::env::set_current_dir(original_cwd).expect("restore cwd");
}

fn independent_fnv1a64_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3); // canonical FNV-1a/64 prime, 0x100000001b3
    }
    format!("{h:016x}")
}

/// Redirect the REAL process STD_OUTPUT_HANDLE to a pipe for the duration of
/// `f`, restore it afterward, and return `f`'s result plus everything
/// written to the pipe. This intercepts raw Win32 `WriteFile` calls
/// (`seam.rs`'s path) -- it does NOT rely on Rust's `print!`/`io::stdout()`
/// capturing, which never sees these writes at all.
#[cfg(windows)]
fn capture_real_stdout<T>(f: impl FnOnce() -> T) -> (T, String) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut u8;
        fn SetStdHandle(which: u32, h: *mut u8) -> i32;
        fn CreatePipe(read_h: *mut *mut u8, write_h: *mut *mut u8, sa: *mut u8, size: u32) -> i32;
        fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32;
        fn CloseHandle(h: *mut u8) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // -11 as u32
    unsafe {
        let mut read_h: *mut u8 = std::ptr::null_mut();
        let mut write_h: *mut u8 = std::ptr::null_mut();
        assert_ne!(CreatePipe(&mut read_h, &mut write_h, std::ptr::null_mut(), 0), 0, "CreatePipe failed");
        let original = GetStdHandle(STD_OUTPUT_HANDLE);
        assert_ne!(SetStdHandle(STD_OUTPUT_HANDLE, write_h), 0, "SetStdHandle failed");

        let result = f();

        SetStdHandle(STD_OUTPUT_HANDLE, original);
        CloseHandle(write_h); // our copy must close so ReadFile below sees EOF

        let mut collected = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut got: u32 = 0;
            let r = ReadFile(read_h, buf.as_mut_ptr(), buf.len() as u32, &mut got, std::ptr::null_mut());
            if r == 0 || got == 0 {
                break;
            }
            collected.extend_from_slice(&buf[..got as usize]);
        }
        CloseHandle(read_h);
        (result, String::from_utf8_lossy(&collected).to_string())
    }
}

// ============================================================================
// Acceptance criterion 3(a): Q19's structural fault classes are rejected
// before execution.
// ============================================================================

#[test]
fn bad_ir_extern_id_out_of_range_is_rejected_before_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let err = verify_through_full_pipeline(&store, &payloads::bad_ir_demo()).expect_err("must be rejected");
    assert!(matches!(err, IrFault::ExternIdOutOfRange { id: 99, .. }), "got {err:?}");
}

#[test]
fn structural_fault_no_blocks_is_rejected() {
    let m = Module { name: "no_blocks".into(), n_vals: 0, blocks: vec![], entry: 0, rodata: vec![], externs: vec![] };
    assert_eq!(expect_rejected(&m), IrFault::NoBlocks);
}

#[test]
fn structural_fault_entry_out_of_range_is_rejected() {
    let m = Module {
        name: "bad_entry".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![], term: Term::Exit(0) }],
        entry: 5,
        rodata: vec![],
        externs: vec![],
    };
    assert_eq!(expect_rejected(&m), IrFault::EntryOutOfRange { entry: 5 });
}

#[test]
fn structural_fault_val_out_of_range_is_rejected() {
    let m = Module {
        name: "bad_val".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![], term: Term::Exit(7) }], // val 7 >= n_vals 1
        entry: 0,
        rodata: vec![],
        externs: vec![],
    };
    assert_eq!(expect_rejected(&m), IrFault::ValOutOfRange { block: 0, val: 7 });
}

#[test]
fn structural_fault_block_target_out_of_range_is_rejected() {
    let mut b = Builder::new();
    b.term(Term::Br(9)); // no block 9 exists
    let m = b.finish("bad_target", 0);
    assert_eq!(expect_rejected(&m), IrFault::BlockTargetOutOfRange { block: 0, target: 9 });
}

#[test]
fn structural_fault_rodata_offset_out_of_range_is_rejected() {
    let mut b = Builder::new();
    let _ = b.rodata(b"hi"); // 2 bytes, valid offsets 0..=2
    let bad = b.set(Op::Rodata(99));
    b.term(Term::Exit(bad));
    let m = b.finish("bad_rodata", 0);
    assert_eq!(expect_rejected(&m), IrFault::RodataOffsetOutOfRange { block: 0, off: 99 });
}

#[test]
fn structural_fault_arity_mismatch_ir_internal_is_rejected() {
    // Call site passes 2 args to an extern honestly declared (via
    // call_raw_arity) with nargs=1 -- Q19's ORIGINAL IR-internal check.
    let mut b = Builder::new();
    let one = b.konst(1);
    let two = b.konst(2);
    let d = b.call_raw_arity(Intent::FileClose, vec![one, two], 1);
    b.term(Term::Exit(d));
    let m = b.finish("bad_arity_internal", 0);
    assert!(matches!(expect_rejected(&m), IrFault::ArityMismatch { got: 2, want: 1, .. }));
}

// ============================================================================
// Acceptance criterion 3(b): THE F1 fix. A native-call-contract error (an
// IR that is self-consistent by Q19's original rule but disagrees with the
// intent's OWN contract arity) must be rejected too -- this is the direct,
// deliberately-constructed reproduction the design doc demands, not a bare
// assertion that "the verifier now checks this".
// ============================================================================

#[test]
fn f1_fix_spawnwait_arg_count_mismatch_is_rejected_before_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    // SpawnWait's real contract is 0 args (Intent::contract_arity); this IR
    // self-consistently declares AND calls it with 1 -- Q19's original
    // ArityMismatch check does NOT fire (got == want == 1). Only the F1 fix
    // (IntentArityMismatch, cross-checking against contract_arity) catches it.
    let err = verify_through_full_pipeline(&store, &payloads::spawnwait_arity_mismatch_demo()).expect_err("must be rejected by the F1 fix");
    match err {
        IrFault::IntentArityMismatch { intent: Intent::SpawnWait, declared: 1, contract: 0, .. } => {}
        other => panic!("expected IntentArityMismatch{{ SpawnWait, declared:1, contract:0 }}, got {other:?}"),
    }
}

/// The exact case `research/dynamic-core/assembled/RESULTS.md` §② F1
/// demonstrated as a PANIC (`mismatched_arity_demo`, a `FileWrite` call with
/// 1 of its 3 real args) -- reproduced here to prove it is now a `verify()`
/// rejection, not a runtime panic reading out of bounds in `seam.rs`.
#[test]
fn f1_fix_filewrite_arg_count_mismatch_is_rejected_before_execution_not_a_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        verify_through_full_pipeline(&store, &payloads::filewrite_arity_mismatch_demo())
    }));
    let err = match result {
        Ok(Err(e)) => e,
        Ok(Ok(())) => panic!("verify() must reject this IR, not accept it"),
        Err(_) => panic!("verify() itself must not panic -- rejection must happen before any seam code runs"),
    };
    match err {
        IrFault::IntentArityMismatch { intent: Intent::FileWrite, declared: 1, contract: 3, .. } => {}
        other => panic!("expected IntentArityMismatch{{ FileWrite, declared:1, contract:3 }}, got {other:?}"),
    }
}

// ============================================================================
// Acceptance criterion 4: a real infinite-loop pack, driven through the
// FULL public pipeline (not eval_core.rs's own internal unit test), is
// stopped by the step limit and does not hang the host thread.
// ============================================================================

#[test]
fn infinite_loop_pack_is_stopped_by_the_step_limit_without_hanging_the_host_thread() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let mut b = Builder::new();
    b.term(Term::Br(0)); // a block that branches to itself forever
    let infinite = b.finish("nativecore_infinite_loop", 0);

    let manifest = pack::pack(&store, &infinite).expect("pack");
    let loaded = pack::load(&store, &manifest).expect("load");
    let verified = verify::verify(&loaded).expect("a self-loop is structurally well-formed -- verify() cannot and must not catch this");

    let started = std::time::Instant::now();
    let outcome = eval_core::run(&verified); // DEFAULT_MAX_STEPS, the real production default
    let elapsed = started.elapsed();

    assert_eq!(outcome.termination, Termination::StepLimitExceeded);
    assert!(outcome.calls.is_empty());
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "the step limit must abort promptly on a real machine, not just eventually -- took {elapsed:?}"
    );
}

/// Same infinite loop, but proven to not hang the CALLING thread by racing
/// it against a join timeout on a SEPARATE thread -- if `run` ever regressed
/// to unbounded looping, this test itself would time out instead of hanging
/// forever silently.
#[test]
fn infinite_loop_pack_join_with_explicit_timeout_never_blocks_forever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let mut b = Builder::new();
    b.term(Term::Br(0));
    let infinite = b.finish("nativecore_infinite_loop_threaded", 0);
    let manifest = pack::pack(&store, &infinite).expect("pack");

    let store_root = dir.path().to_path_buf();
    let manifest_for_thread = manifest.clone();
    let handle = std::thread::spawn(move || {
        let store = Store::open(&store_root).expect("reopen store on worker thread");
        let loaded = pack::load(&store, &manifest_for_thread).expect("load");
        let verified = verify::verify(&loaded).expect("well-formed");
        eval_core::run(&verified)
    });

    // Poll with a hard deadline instead of a blocking join() so a genuine
    // regression (an unbounded loop) fails this test instead of hanging the
    // whole test binary.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if handle.is_finished() {
            let outcome = handle.join().expect("worker thread must not panic");
            assert_eq!(outcome.termination, Termination::StepLimitExceeded);
            return;
        }
        assert!(std::time::Instant::now() < deadline, "worker thread did not finish within 10s -- the step limit is not bounding it");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// ============================================================================
// Sanity: verify() applied twice in a row to the same store-round-tripped
// module is deterministic (no hidden mutable state in `Intent::contract_arity`
// or the structural walk).
// ============================================================================

#[test]
fn verification_and_execution_are_deterministic_across_repeated_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let outcome1 = run_through_full_pipeline(&store, &payloads::pure_compute()).expect("first run");
    let outcome2 = run_through_full_pipeline(&store, &payloads::pure_compute()).expect("second run");
    assert_eq!(outcome1.termination, outcome2.termination);
    assert_eq!(outcome1.termination, Termination::Exited(163));

    // Also prove content addressing is genuinely deterministic: packing the
    // same module twice must yield the SAME hash (not just the same result).
    let (m1, _) = pack::build_manifest(&payloads::pure_compute());
    let (m2, _) = pack::build_manifest(&payloads::pure_compute());
    assert_eq!(m1.hash, m2.hash);
}
