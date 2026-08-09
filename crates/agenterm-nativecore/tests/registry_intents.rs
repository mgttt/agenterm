//! Real-machine black-box acceptance tests for `plan/design-dynacore-native-
//! core.md` §9 (the v2 signature-registry path) — reproducing, inside this
//! crate's real source (not the `research/dynamic-core/runtime-intent/`
//! prototype), the S1/S2/S3/S4/S5 findings
//! `research/dynamic-core/runtime-intent/RESULTS.md` measured. Same
//! discipline as `tests/native_intents.rs`: every test here either (a)
//! drives the FULL public pipeline (build IR -> `pack::pack` into a real
//! `store::Store` on disk -> `pack::load` back -> `verify::verify` ->
//! `eval_core::run`) against a REAL kernel32 export, or (b) constructs a
//! deliberately malformed IR and asserts `verify()` rejects it BEFORE any
//! `eval_core::run` call happens. Nothing here mocks the OS.
//!
//! This file is a SEPARATE compilation unit from `tests/native_intents.rs`
//! (Rust integration tests cannot share private helpers across files), so
//! the small pipeline helpers below are duplicated in shape, not imported.

use agenterm_nativecore::eval_core::{self, Termination};
use agenterm_nativecore::ir::{Builder, Term};
use agenterm_nativecore::pack;
use agenterm_nativecore::store::Store;
use agenterm_nativecore::verify::{self, IrFault};

fn run_through_full_pipeline(store: &Store, m: &agenterm_nativecore::ir::Module) -> Result<eval_core::RunOutcome, String> {
    let manifest = pack::pack(store, m).map_err(|e| format!("pack: {e}"))?;
    let loaded = pack::load(store, &manifest)?;
    let verified = verify::verify(&loaded).map_err(|e| format!("verify REJECTED: {e:?}"))?;
    Ok(eval_core::run(&verified))
}

fn verify_through_full_pipeline(store: &Store, m: &agenterm_nativecore::ir::Module) -> Result<(), IrFault> {
    let manifest = pack::pack(store, m).expect("pack must succeed even for a malformed module (packing is not verification)");
    let loaded = pack::load(store, &manifest).expect("load must succeed (bytes round-trip regardless of well-formedness)");
    verify::verify(&loaded).map(|_| ())
}

fn expect_rejected(m: &agenterm_nativecore::ir::Module) -> IrFault {
    match verify::verify(m) {
        Ok(_) => panic!("expected verify() to reject this IR, but it was accepted"),
        Err(fault) => fault,
    }
}

// ============================================================================
// S1/S2 reproduction: a real kernel32 export NONE of the seven compile-time
// Intents bind to, called purely from a registry-backed pack, real result
// matches the real independently-computed oracle. Zero recompilation of the
// seven-intent path was needed to add these symbols -- they are pure data
// (`Builder::call_reg`'s string arguments), not `Intent` variants.
// ============================================================================

#[test]
fn s1_registry_call_muldiv_matches_the_real_oracle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    let mut b = Builder::new();
    let a = b.konst(7);
    let bb = b.konst(11);
    let c = b.konst(5);
    let d = b.call_reg("kernel32.dll", "MulDiv", vec![a, bb, c]);
    b.term(Term::Exit(d));
    let module = b.finish("s1_muldiv", 0);

    let outcome = run_through_full_pipeline(&store, &module).expect("MulDiv(7,11,5) via the registry path must verify and run");
    // independent oracle: MulDiv rounds (a*b)/c to nearest, .5 away from zero.
    let oracle = ((7i64 * 11) as f64 / 5.0).round() as u64;
    assert_eq!(oracle, 15, "sanity on the oracle formula itself");
    assert_eq!(outcome.termination, Termination::Exited(oracle));
    assert_eq!(outcome.registry_calls.len(), 1);
    assert_eq!(outcome.registry_calls[0].symbol, "MulDiv");
    assert_eq!(outcome.registry_calls[0].result, 15);
    assert!(outcome.calls.is_empty(), "must not have touched the seven-intent NativeCallRecord log at all");
}

#[test]
fn s2_registry_call_lstrlena_matches_the_real_oracle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    let mut b = Builder::new();
    let path_off = b.rodata(b"HELLO\0");
    let ptr = b.set(agenterm_nativecore::ir::Op::Rodata(path_off));
    let d = b.call_reg("kernel32.dll", "lstrlenA", vec![ptr]);
    b.term(Term::Exit(d));
    let module = b.finish("s2_lstrlena", 0);

    let outcome = run_through_full_pipeline(&store, &module).expect("lstrlenA(\"HELLO\") via the registry path must verify and run");
    assert_eq!(outcome.termination, Termination::Exited(5), "real lstrlenA(\"HELLO\") == 5");
    assert_eq!(outcome.registry_calls.len(), 1);
    assert_eq!(outcome.registry_calls[0].symbol, "lstrlenA");
}

// ============================================================================
// S3/S4 reproduction: "derive, not declare". Two distinct pack-supplied
// channels could in principle carry a wrong arity for a registry-backed
// call -- (1) the registry extern's OWN declared `nargs` field, and (2) the
// `CallReg` call site's actual `args.len()`. Both are independently checked
// against ground truth that is NEVER the pack's own say-so:
//   - declared nargs is checked against the REGISTRY-derived arity
//     (`RegistryArityMismatch`) -- this is the S4 finding.
//   - the call site's args.len() is checked against the extern's OWN
//     declared nargs (`RegArityMismatch`, IR-internal) -- this is S3's
//     "under-provision caught before execution", one level up.
// ============================================================================

#[test]
fn s4_derive_not_declare_an_internally_consistent_but_registry_wrong_arity_is_still_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    // MulDiv's REAL registry arity is 3. This pack is INTERNALLY CONSISTENT
    // -- it declares nargs=2 AND the call site passes exactly 2 args, so
    // there is no IR-internal disagreement (`RegArityMismatch` would NOT
    // fire). The only thing that can catch this is the registry-DERIVED
    // check, because nothing here disagrees with itself -- exactly Q23 S4's
    // "the author now controls both the IR and the declaration" pattern,
    // reproduced for a registry symbol.
    let mut b = Builder::new();
    let a = b.konst(7);
    let bb = b.konst(11);
    let d = b.call_reg_raw_arity("kernel32.dll", "MulDiv", vec![a, bb], 2);
    b.term(Term::Exit(d));
    let module = b.finish("s4_internally_consistent_lie", 0);

    let err = verify_through_full_pipeline(&store, &module).expect_err("must be rejected by the registry-derived check, not waved through as self-consistent");
    match err {
        IrFault::RegistryArityMismatch { symbol, declared: 2, contract: 3, .. } if symbol == "MulDiv" => {}
        other => panic!("expected RegistryArityMismatch{{ MulDiv, declared:2, contract:3 }}, got {other:?}"),
    }
}

#[test]
fn s3_underprovided_call_site_args_caught_before_execution_ir_internal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    // The extern DECLARES the correct registry arity (3, matching MulDiv's
    // real contract -- the registry-derived pass above would PASS this),
    // but the actual `CallReg` call site only supplies 2 `Val`s. This is
    // the F1 pattern reproduced one level up for the registry path: an IR
    // that under-provides args to a recipe requiring 3.
    let mut b = Builder::new();
    let a = b.konst(7);
    let bb = b.konst(11);
    let d = b.call_reg_raw_arity("kernel32.dll", "MulDiv", vec![a, bb], 3);
    b.term(Term::Exit(d));
    let module = b.finish("s3_underprovided_call_site", 0);

    let err = verify_through_full_pipeline(&store, &module).expect_err("must be rejected before execution");
    match err {
        IrFault::RegArityMismatch { got: 2, want: 3, .. } => {}
        other => panic!("expected RegArityMismatch{{ got:2, want:3 }}, got {other:?}"),
    }
}

// ============================================================================
// S5 reproduction: THE FIX. A pack referencing a symbol NOT in the registry
// is rejected at verify() time -- this is the entire point of §9 (closing
// the hole Q23 found: a single-author pack cannot make verify() trust an
// arity it asserts for a symbol nobody has reviewed).
// ============================================================================

#[test]
fn s5_fix_symbol_not_in_registry_is_rejected_at_verify_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    // OutputDebugStringA is a real kernel32 export but deliberately NOT in
    // SIGNATURE_REGISTRY (registry.rs) -- exactly Q23's residue: a
    // no-contract-in-registry symbol. The pack "honestly" declares 1 arg
    // (matching the real ABI, if anyone bothered to check) -- honesty about
    // the arity must not matter, because the symbol itself was never
    // reviewed.
    let mut b = Builder::new();
    let msg_off = b.rodata(b"hi\0");
    let ptr = b.set(agenterm_nativecore::ir::Op::Rodata(msg_off));
    let d = b.call_reg("kernel32.dll", "OutputDebugStringA", vec![ptr]);
    b.term(Term::Exit(d));
    let module = b.finish("s5_unregistered_symbol", 0);

    let err = verify_through_full_pipeline(&store, &module).expect_err("an unregistered symbol must be rejected, not internally-self-consistency-checked into a pass");
    match &err {
        IrFault::SymbolNotInRegistry { symbol, module, .. } => {
            assert_eq!(symbol, "OutputDebugStringA");
            assert_eq!(module, "kernel32.dll");
        }
        other => panic!("expected SymbolNotInRegistry, got {other:?}"),
    }
    let message = err.to_string();
    assert!(
        message.contains("not in the signature registry, cannot be called this way"),
        "design doc §9.2 criterion 4: the error message must plainly name the registry as the reason, got {message:?}"
    );
}

/// A registry entry with the RIGHT symbol name but the WRONG module must
/// still be rejected -- `SignatureRegistry::lookup` requires an exact
/// `(module, symbol)` match, so claiming a real symbol lives in some other
/// DLL than the one the registry vouches for does not sneak past.
#[test]
fn symbol_in_registry_under_a_different_module_name_is_still_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    let mut b = Builder::new();
    let a = b.konst(7);
    let bb = b.konst(11);
    let c = b.konst(5);
    let d = b.call_reg("not-the-real-kernel32.dll", "MulDiv", vec![a, bb, c]);
    b.term(Term::Exit(d));
    let module = b.finish("wrong_module_name", 0);

    let err = verify_through_full_pipeline(&store, &module).expect_err("must be rejected");
    assert!(matches!(err, IrFault::SymbolNotInRegistry { .. }), "got {err:?}");
}

// ============================================================================
// Negative-arity-lie test: try, honestly, to find a pack-supplied channel
// that smuggles a wrong arity through to `do_registry_call`. Given this IR
// design there are exactly two pack-supplied numbers that could disagree
// with the truth -- `RegExternDecl.nargs` (checked above, S4) and the
// `CallReg` call site's `args.len()` (checked above, S3) -- and both are
// independently gated before `eval_core::run` ever reaches `do_registry_call`.
// This test constructs the WORST case (both wrong at once, in different
// directions) and confirms `verify()` still catches it -- no execution, no
// native call.
// ============================================================================

#[test]
fn negative_no_pack_channel_can_smuggle_a_wrong_arity_past_verify() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    // Registry truth: MulDiv needs 3. This pack declares nargs=1 (a lie
    // against the registry) AND the call site supplies only 1 Val (so the
    // IR-internal check alone would not catch it) -- the ONLY thing that can
    // catch this two-directional lie is the registry-derived pass.
    let mut b = Builder::new();
    let a = b.konst(7);
    let d = b.call_reg_raw_arity("kernel32.dll", "MulDiv", vec![a], 1);
    b.term(Term::Exit(d));
    let module = b.finish("negative_smuggle_attempt", 0);

    let err = verify_through_full_pipeline(&store, &module).expect_err("verify() must reject this before any native call happens");
    match err {
        IrFault::RegistryArityMismatch { declared: 1, contract: 3, .. } => {}
        other => panic!("expected RegistryArityMismatch{{ declared:1, contract:3 }}, got {other:?}"),
    }

    // Honest conclusion, not asserted away: this closes the channel FOR
    // SYMBOLS THAT ARE IN THE REGISTRY. It does not and cannot make the
    // registry's own hand-authored `arity` field self-verifying -- if a
    // human mis-transcribes a real API's arity into `registry.rs`, this
    // mechanism has no way to catch that (same residual trust Q23 named:
    // the registry itself is the human-reviewed second party, and nothing
    // downstream of it can validate it against ground truth Windows does
    // not expose). That is accepted, documented scope (design doc §9.1/9.3
    // and `registry.rs`'s header), not a gap this test is hiding.
}

// ============================================================================
// Structural sanity: a `CallReg` naming a registry-extern id that does not
// exist is rejected the same way `ExternIdOutOfRange` handles the
// seven-intent path's equivalent case.
// ============================================================================

#[test]
fn callreg_naming_a_nonexistent_registry_extern_id_is_rejected() {
    use agenterm_nativecore::ir::{Block, Inst, Module, Term};
    let m = Module {
        name: "bad_reg_id".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![Inst::CallReg(0, 7, vec![])], term: Term::Exit(0) }],
        entry: 0,
        rodata: vec![],
        externs: vec![],
        registry_externs: vec![], // empty -- id 7 cannot exist
    };
    assert!(matches!(expect_rejected(&m), IrFault::RegExternIdOutOfRange { id: 7, .. }));
}

// ============================================================================
// Sanity: the seven-intent path and the registry path can coexist in the
// SAME module, and neither disturbs the other's verification or execution.
// ============================================================================

#[test]
fn seven_intent_and_registry_calls_coexist_in_the_same_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");

    let mut b = Builder::new();
    let cap = b.konst(64);
    let buf = b.call(agenterm_nativecore::ir::Intent::Alloc, vec![cap]); // seven-intent path
    let a = b.konst(7);
    let bb = b.konst(11);
    let c = b.konst(5);
    let muldiv = b.call_reg("kernel32.dll", "MulDiv", vec![a, bb, c]); // registry path
    let _ = buf; // Alloc's return value is not otherwise used by this test
    b.term(Term::Exit(muldiv));
    let module = b.finish("mixed_paths", 0);

    let outcome = run_through_full_pipeline(&store, &module).expect("mixed module must verify and run");
    assert_eq!(outcome.termination, Termination::Exited(15));
    assert_eq!(outcome.calls.len(), 1, "the real Alloc/VirtualAlloc call happened");
    assert_eq!(outcome.registry_calls.len(), 1, "the real MulDiv registry call happened");
}
