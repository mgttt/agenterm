//! `verify::verify`'s structural gate, one deliberately-malformed `Module`
//! per `IrFault` variant, matching the rigor of `agenterm-nativecore`'s
//! `structural_fault_*` tests (`tests/native_intents.rs`): a minimal
//! hand-built `Module` that triggers exactly one fault class, asserted
//! before any interpretation happens, never through `eval_core::run`.
//!
//! `pack_lifecycle.rs` already covers `ExternIdOutOfRange`,
//! `UnknownOperation`, and the missing-required-param/wrong-type shapes of
//! `ParamsMismatch` — this file does not repeat those, it fills the
//! remaining `IrFault` variants (`NoBlocks`, `EntryOutOfRange`,
//! `ValOutOfRange`, `BlockTargetOutOfRange`) plus the `ParamsMismatch`/
//! `UnknownOperation` shapes `pack_lifecycle.rs` does not exercise
//! (invalid JSON, non-object JSON, an extra undeclared key, a declared-but-
//! unavailable operation, and out-of-declared-bounds integers).

use agenterm_dynacore::ir::{Block, Builder, Inst, Module, Op, Term};
use agenterm_dynacore::verify::{self, IrFault, OperationParamSchema, OperationSchema};

fn demo_catalog() -> Vec<OperationSchema> {
    vec![OperationSchema {
        id: "demo.echo".to_string(),
        available: true,
        parameters: vec![OperationParamSchema {
            name: "n".to_string(),
            value_type: "uint32".to_string(),
            required: true,
            minimum: Some(0),
            maximum: Some(100),
        }],
    }]
}

// ============================================================================
// IR-internal structural faults (no fleet_call / catalog involved at all).
// ============================================================================

#[test]
fn no_blocks_is_rejected() {
    let m = Module {
        name: "empty".into(),
        n_vals: 0,
        blocks: vec![],
        entry: 0,
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::NoBlocks));
}

#[test]
fn entry_out_of_range_is_rejected() {
    let m = Module {
        name: "bad_entry".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![], term: Term::Exit(0) }],
        entry: 7, // only block 0 exists
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::EntryOutOfRange { entry: 7 }));
}

#[test]
fn val_out_of_range_in_terminator_is_rejected() {
    let m = Module {
        name: "bad_val_term".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![], term: Term::Exit(5) }], // val 5 >= n_vals 1
        entry: 0,
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::ValOutOfRange { block: 0, val: 5 }));
}

#[test]
fn val_out_of_range_in_op_operand_is_rejected() {
    // `Set` dest is in range (0), but the op it computes reads val 9, which
    // is not -- the ValOutOfRange check must walk operands, not just dests.
    let m = Module {
        name: "bad_val_operand".into(),
        n_vals: 1,
        blocks: vec![Block {
            insts: vec![Inst::Set(0, Op::Add(9, 0))],
            term: Term::Exit(0),
        }],
        entry: 0,
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::ValOutOfRange { block: 0, val: 9 }));
}

#[test]
fn val_out_of_range_in_brcond_condition_is_rejected() {
    let m = Module {
        name: "bad_val_brcond".into(),
        n_vals: 1,
        blocks: vec![
            Block { insts: vec![], term: Term::BrCond(3, 0, 0) }, // cond val 3 >= n_vals 1
        ],
        entry: 0,
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::ValOutOfRange { block: 0, val: 3 }));
}

#[test]
fn block_target_out_of_range_via_br_is_rejected() {
    let mut b = Builder::new();
    b.term(Term::Br(9)); // no block 9 exists
    let m = b.finish("bad_br_target", 0);
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::BlockTargetOutOfRange { block: 0, target: 9 }));
}

#[test]
fn block_target_out_of_range_via_brcond_nz_arm_is_rejected() {
    let m = Module {
        name: "bad_brcond_nz".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![], term: Term::BrCond(0, 42, 0) }], // nz target 42 does not exist
        entry: 0,
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::BlockTargetOutOfRange { block: 0, target: 42 }));
}

#[test]
fn block_target_out_of_range_via_brcond_z_arm_is_rejected() {
    let m = Module {
        name: "bad_brcond_z".into(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![], term: Term::BrCond(0, 0, 42) }], // z target 42 does not exist
        entry: 0,
        externs: vec![],
    };
    assert_eq!(verify::verify(&m, &[]).err(), Some(IrFault::BlockTargetOutOfRange { block: 0, target: 42 }));
}

// ============================================================================
// UnknownOperation shape pack_lifecycle.rs does not exercise: an id present
// in the catalog but explicitly marked unavailable must be treated exactly
// like an id that is not there at all -- an unavailable operation is not a
// degraded/partial call target, it is a non-target.
// ============================================================================

#[test]
fn unavailable_operation_is_rejected_the_same_as_unknown() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.disabled", "{}");
    b.term(Term::Exit(v));
    let m = b.finish("calls_unavailable_op", 0);

    let catalog = vec![OperationSchema {
        id: "demo.disabled".to_string(),
        available: false,
        parameters: vec![],
    }];
    let err = verify::verify(&m, &catalog).err().expect("an unavailable operation must be rejected");
    assert_eq!(
        err,
        IrFault::UnknownOperation {
            block: 0,
            id: 0,
            operation_id: "demo.disabled".to_string(),
        }
    );
}

// ============================================================================
// ParamsMismatch shapes pack_lifecycle.rs does not exercise.
// ============================================================================

#[test]
fn params_json_that_is_not_valid_json_is_rejected() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "{not valid json"); // truncated/garbage
    b.term(Term::Exit(v));
    let m = b.finish("bad_json_syntax", 0);

    match verify::verify(&m, &demo_catalog()).map(|_| ()) {
        Err(IrFault::ParamsMismatch { reason, .. }) => {
            assert!(reason.contains("not valid JSON"), "got reason {reason:?}");
        }
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

#[test]
fn params_json_that_is_a_json_array_not_an_object_is_rejected() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "[1,2,3]"); // valid JSON, but not an object
    b.term(Term::Exit(v));
    let m = b.finish("bad_json_shape_array", 0);

    match verify::verify(&m, &demo_catalog()).map(|_| ()) {
        Err(IrFault::ParamsMismatch { reason, .. }) => {
            assert!(reason.contains("must be a JSON object"), "got reason {reason:?}");
        }
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

#[test]
fn params_json_with_an_undeclared_extra_key_is_rejected() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "{\"n\":3,\"surprise\":true}"); // "surprise" is not declared
    b.term(Term::Exit(v));
    let m = b.finish("extra_key", 0);

    match verify::verify(&m, &demo_catalog()).map(|_| ()) {
        Err(IrFault::ParamsMismatch { reason, .. }) => {
            assert!(reason.contains("surprise"), "got reason {reason:?}");
        }
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

#[test]
fn params_json_below_declared_minimum_is_rejected() {
    // demo.echo's "n" declares minimum: Some(0) -- a negative value is both
    // below the minimum AND fails the uint32 type check (is_u64() is false
    // for a negative i64), so this specifically targets the >= 0 but still
    // schema-invalid boundary using a schema with a nonzero floor instead.
    let catalog = vec![OperationSchema {
        id: "demo.bounded".to_string(),
        available: true,
        parameters: vec![OperationParamSchema {
            name: "n".to_string(),
            value_type: "uint32".to_string(),
            required: true,
            minimum: Some(10),
            maximum: Some(100),
        }],
    }];
    let mut b = Builder::new();
    let v = b.fleet_call("demo.bounded", "{\"n\":3}"); // 3 < declared minimum 10
    b.term(Term::Exit(v));
    let m = b.finish("below_minimum", 0);

    match verify::verify(&m, &catalog).map(|_| ()) {
        Err(IrFault::ParamsMismatch { reason, .. }) => {
            assert!(reason.contains("below declared minimum"), "got reason {reason:?}");
        }
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

#[test]
fn params_json_above_declared_maximum_is_rejected() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "{\"n\":101}"); // demo.echo's max is 100
    b.term(Term::Exit(v));
    let m = b.finish("above_maximum", 0);

    match verify::verify(&m, &demo_catalog()).map(|_| ()) {
        Err(IrFault::ParamsMismatch { reason, .. }) => {
            assert!(reason.contains("above declared maximum"), "got reason {reason:?}");
        }
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

/// A real, if narrow, finding from auditing `json_type_matches`: the
/// `"uint32"` value_type promises a 32-bit width, and (fixed in this same
/// patch) is now actually checked against `u32::MAX` even when the
/// operation declares no explicit `maximum` -- previously a value that fit
/// in a JSON u64 but overflowed u32 (e.g. `5_000_000_000`) satisfied
/// `is_u64()` and slipped past this exact check with no `maximum` bound to
/// catch it. Every real `uint32` param in the product catalog happens to
/// declare an explicit `maximum` where the bound matters (audited via
/// `src/operations.rs` while writing this test), so this was previously
/// unobserved in practice, not exploited -- still closed here at zero
/// behavior cost for any value that was already in range.
#[test]
fn uint32_param_beyond_u32_max_is_rejected_even_with_no_declared_maximum() {
    let catalog = vec![OperationSchema {
        id: "demo.unbounded_u32".to_string(),
        available: true,
        parameters: vec![OperationParamSchema {
            name: "n".to_string(),
            value_type: "uint32".to_string(),
            required: true,
            minimum: None,
            maximum: None, // deliberately no explicit ceiling
        }],
    }];
    let mut b = Builder::new();
    // Fits in a JSON u64 (and would have passed the old `is_u64()`-only
    // check) but does not fit in a real u32.
    let v = b.fleet_call("demo.unbounded_u32", "{\"n\":5000000000}");
    b.term(Term::Exit(v));
    let m = b.finish("beyond_u32_max", 0);

    match verify::verify(&m, &catalog).map(|_| ()) {
        Err(IrFault::ParamsMismatch { reason, .. }) => {
            assert!(reason.contains("uint32"), "expected the type-mismatch reason to name uint32, got {reason:?}");
        }
        Ok(_) => panic!("verify() accepted a uint32 param value that overflows u32::MAX"),
        Err(other) => panic!("expected ParamsMismatch, got {other:?}"),
    }

    // A value that fits within u32 must still be accepted -- the fix must
    // not have narrowed the type past its own promised width.
    let mut b2 = Builder::new();
    let v2 = b2.fleet_call("demo.unbounded_u32", "{\"n\":4294967295}"); // u32::MAX exactly
    b2.term(Term::Exit(v2));
    let m2 = b2.finish("exactly_u32_max", 0);
    verify::verify(&m2, &catalog).expect("u32::MAX itself must still be accepted as a valid uint32");
}
