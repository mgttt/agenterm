//! The load gate: `from_bytes` proves a module before handing it out.
//!
//! Independent of the interpreter's own tests. Every row below is a module the
//! WASM 1.0 validation rules reject. The gate this file guards has four parts:
//!
//! - a bad module fails `from_bytes` with `Decode` — there is no `Module` to
//!   invoke, so nothing can reach the interpreter;
//! - the same bytes must not `eval` to `Ok`, and must not be caught by an
//!   execution-time `Trap` standing in for the missing load check;
//! - a legal module still loads and still runs;
//! - nothing here aborts the process.

use agenterm_tinyvm::{WasmError, WasmModule, eval};

/// Modules WASM 1.0 validation rejects: `(name, wasm_hex)`.
const REJECTED: [(&str, &str); 16] = [
    (
        "empty_stack_add",
        "0061736d010000000105016000017f03020100070801046d61696e00000a050103006a0b",
    ),
    (
        "f32_used_as_i32",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0c010a00430000c03f41016a0b",
    ),
    (
        "local_index_out_of_range",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0601040020630b",
    ),
    (
        "call_index_out_of_range",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0601040010630b",
    ),
    (
        "br_label_out_of_range",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0801060041010c090b",
    ),
    (
        "global_index_out_of_range",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0601040023000b",
    ),
    (
        "call_indirect_type_out_of_range",
        "0061736d010000000105016000017f03020100040401700001070801046d61696e00000907010041000b01000a0901070041001109000b",
    ),
    (
        "select_arms_differ",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0b0109004101420241011b0b",
    ),
    (
        "body_without_end",
        "0061736d010000000105016000017f03020100070801046d61696e00000a050103004107",
    ),
    (
        "block_leaves_value",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0b010900024041010b41020b",
    ),
    (
        "if_without_else_with_result",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0b0109004101047f41050b0b",
    ),
    (
        "function_leaves_extra_value",
        "0061736d010000000105016000017f03020100070801046d61696e00000a08010600410141020b",
    ),
    (
        "br_table_targets_disagree",
        "0061736d010000000105016000017f03020100070801046d61696e00000a14011200027f0240410141000e0100010b41030b0b",
    ),
    (
        "local_set_wrong_type",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0c010a01017f4201210041000b",
    ),
    (
        "store_value_type_mismatch",
        "0061736d010000000105016000017f03020100070801046d61696e00000a10010e004100430000803f36000041000b",
    ),
    (
        "call_arg_type_mismatch",
        "0061736d01000000010a026000017f60017f017f0303020001070801046d61696e00000a0d020600420110010b040020000b",
    ),
];

/// Legal counterparts that must keep loading and running.
const ACCEPTED: [(&str, &str); 6] = [
    (
        "add_two_consts",
        "0061736d010000000105016000017f03020100070801046d61696e00000a09010700410141026a0b",
    ),
    (
        "local_index_in_range",
        "0061736d010000000105016000017f03020100070801046d61696e00000a08010601017f20000b",
    ),
    (
        "global_index_in_range",
        "0061736d010000000105016000017f030201000606017f0041070b070801046d61696e00000a0601040023000b",
    ),
    (
        "select_arms_agree",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0b0109004101410241011b0b",
    ),
    (
        "if_with_else_and_result",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0e010c004101047f41050541060b0b",
    ),
    (
        "call_arg_type_ok",
        "0061736d01000000010a026000017f60017f017f0303020001070801046d61696e00000a10020600412910010b0700200041016a0b",
    ),
];

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn invalid_modules_fail_at_load_not_at_run() {
    for (name, hex) in REJECTED {
        let wasm = bytes(hex);
        match WasmModule::from_bytes(&wasm) {
            Err(WasmError::Decode(_)) => {}
            Err(WasmError::Trap(msg)) => {
                panic!("{name}: load must not lean on an execution trap ({msg})")
            }
            Ok(_) => panic!("{name}: invalid module produced an invokable Module"),
        }
    }
}

#[test]
fn the_same_bytes_never_eval_to_ok() {
    for (name, hex) in REJECTED {
        let wasm = bytes(hex);
        match eval(&wasm) {
            Err(WasmError::Decode(_)) => {}
            Err(WasmError::Trap(msg)) => {
                panic!("{name}: eval fell through to a run-time trap ({msg})")
            }
            Ok(vals) => panic!("{name}: invalid module evaluated to {} values", vals.len()),
        }
    }
}

#[test]
fn legal_modules_still_load_and_run() {
    for (name, hex) in ACCEPTED {
        let wasm = bytes(hex);
        let module = WasmModule::from_bytes(&wasm)
            .unwrap_or_else(|e| panic!("{name}: legal module rejected: {}", e.message()));
        let _ = module.export_index("main");
        match eval(&wasm) {
            Ok(vals) => assert_eq!(vals.len(), 1, "{name}: expected one result"),
            Err(e) => panic!("{name}: legal module failed to run: {}", e.message()),
        }
    }
}

/// The whole point of validating before executing: a rejected module must not
/// have produced a `Module`, so there is nothing left that could be invoked.
#[test]
fn rejection_leaves_nothing_invokable() {
    for (name, hex) in REJECTED {
        let wasm = bytes(hex);
        assert!(
            WasmModule::from_bytes(&wasm).is_err(),
            "{name}: rejected bytes must not yield a Module"
        );
    }
}
