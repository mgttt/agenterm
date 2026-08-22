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
use std::process::Command;

/// Modules WASM 1.0 validation rejects: `(name, wasm_hex)`.
const REJECTED: [(&str, &str); 36] = [
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
        "untyped_select_funcref",
        "0061736d010000000105016000017003020100070801046d61696e00000a0b010900d070d07041011b0b",
    ),
    (
        "untyped_select_externref",
        "0061736d010000000105016000016f03020100070801046d61696e00000a0b010900d06fd06f41001b0b",
    ),
    (
        "ref_func_without_declaration",
        "0061736d01000000010802600000600001700303020001070801046d61696e00010a090202000b0400d2000b",
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
    (
        "memory_size_without_memory",
        "0061736d010000000105016000017f03020100070801046d61696e00000a060104003f000b",
    ),
    (
        "memory_grow_without_memory",
        "0061736d010000000105016000017f03020100070801046d61696e00000a08010600410040000b",
    ),
    (
        "load_without_memory",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0901070041002802000b",
    ),
    (
        "memory_copy_without_memory",
        "0061736d0100000001040160000003020100070801046d61696e00000a0e010c00410041004100fc0a00000b",
    ),
    (
        "i32_load_overaligned",
        "0061736d010000000105016000017f030201000503010001070801046d61696e00000a0901070041002803000b",
    ),
    (
        "i64_load_overaligned",
        "0061736d010000000105016000017e030201000503010001070801046d61696e00000a0901070041002904000b",
    ),
    (
        "i32_load8_overaligned",
        "0061736d010000000105016000017f030201000503010001070801046d61696e00000a0901070041002d01000b",
    ),
    (
        "i64_store_overaligned",
        "0061736d01000000010401600000030201000503010001070801046d61696e00000a0b010900410042003704000b",
    ),
    (
        "instruction_after_function_end",
        "0061736d0100000001040160000003020100070801046d61696e00000a050103000b01",
    ),
    (
        "duplicate_else",
        "0061736d0100000001040160000003020100070801046d61696e00000a0b0109004101044005050b0b",
    ),
    (
        "positive_i64_leb_overflow",
        "0061736d010000000105016000017e03020100070801046d61696e00000a0f010d0042808080808080808080010b",
    ),
    (
        "negative_i64_leb_overflow",
        "0061736d010000000105016000017e03020100070801046d61696e00000a0f010d0042ffffffffffffffffff7e0b",
    ),
    ("custom_section_without_name", "0061736d010000000000"),
    (
        "custom_section_truncated_name_length",
        "0061736d010000000001ff",
    ),
    (
        "custom_section_invalid_utf8_name",
        "0061736d01000000000201ff",
    ),
    (
        "load_with_empty_memory_section",
        "0061736d010000000105016000017f03020100050100070801046d61696e00000a0901070041002802000b",
    ),
    (
        "immutable_global_set",
        "0061736d010000000105016000017f030201000606017f0041070b070801046d61696e00000a0a0108004109240023000b",
    ),
];

/// Legal counterparts that must keep loading and running.
const ACCEPTED: [(&str, &str); 14] = [
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
        "typed_select_funcref",
        "0061736d010000000105016000017003020100070801046d61696e00000a0d010b00d070d07041011c01700b",
    ),
    (
        "export_declares_ref_func",
        "0061736d01000000010802600000600001700303020001070c02046d61696e0001016600000a090202000b0400d2000b",
    ),
    (
        "element_declares_ref_func",
        "0061736d010000000105016000017003020100040401700001070801046d61696e0000090501030001000a06010400d2000b",
    ),
    (
        "if_with_else_and_result",
        "0061736d010000000105016000017f03020100070801046d61696e00000a0e010c004101047f41050541060b0b",
    ),
    (
        "call_arg_type_ok",
        "0061736d01000000010a026000017f60017f017f0303020001070801046d61696e00000a10020600412910010b0700200041016a0b",
    ),
    (
        "i64_min_leb_boundary",
        "0061736d010000000105016000017e03020100070801046d61696e00000a0f010d00428080808080808080807f0b",
    ),
    (
        "i64_max_leb_boundary",
        "0061736d010000000105016000017e03020100070801046d61696e00000a0f010d0042ffffffffffffffffff000b",
    ),
    (
        "custom_section_with_opaque_payload",
        "0061736d0100000000040178ff000105016000017f03020100070801046d61696e00000a09010700410141026a0b",
    ),
    (
        "pure_compute_with_empty_memory_section",
        "0061736d010000000105016000017f03020100050100070801046d61696e00000a09010700410141026a0b",
    ),
    (
        "mutable_global_set",
        "0061736d010000000105016000017f030201000606017f0141070b070801046d61696e00000a0a0108004109240023000b",
    ),
];

const WABT_ORACLE: &str = include_str!("fixtures/validate_gate.txt");

fn oracle_rows() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    WABT_ORACLE
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut columns = line.split('|');
            let name = columns.next().expect("load-gate oracle id");
            let verdict = columns.next().expect("load-gate oracle verdict");
            let hex = columns.next().expect("load-gate oracle wasm hex");
            assert!(columns.next().is_none(), "{name}: extra oracle column");
            assert!(matches!(verdict, "reject" | "accept"));
            assert!(hex.len() % 2 == 0 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
            (name, verdict, hex)
        })
}

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn wabt_oracle_fixture_exactly_matches_the_rust_load_gate() {
    let rows: Vec<_> = oracle_rows().collect();
    assert_eq!(rows.len(), REJECTED.len() + ACCEPTED.len());
    for (name, hex) in REJECTED {
        assert!(
            rows.contains(&(name, "reject", hex)),
            "{name}: rejected Rust case missing from WABT oracle"
        );
    }
    for (name, hex) in ACCEPTED {
        assert!(
            rows.contains(&(name, "accept", hex)),
            "{name}: accepted Rust case missing from WABT oracle"
        );
    }
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

#[test]
fn standard_untyped_select_rejects_reference_values() {
    for name in ["untyped_select_funcref", "untyped_select_externref"] {
        let (_, hex) = REJECTED
            .into_iter()
            .find(|(case, _)| *case == name)
            .expect("untyped reference select fixture");
        assert!(
            matches!(
                WasmModule::from_bytes(&bytes(hex)),
                Err(WasmError::Decode(_))
            ),
            "{name}: legacy select must reject reference operands"
        );
    }

    let (_, typed_hex) = ACCEPTED
        .into_iter()
        .find(|(name, _)| *name == "typed_select_funcref")
        .expect("typed reference select fixture");
    let values = eval(&bytes(typed_hex))
        .unwrap_or_else(|error| panic!("typed reference select: {}", error.message()));
    assert!(
        matches!(values.as_slice(), [agenterm_tinyvm::Val::FuncRef(None)]),
        "typed reference select must return one null funcref"
    );
}

#[test]
fn standard_ref_func_declarations_include_function_exports() {
    let (_, undeclared_hex) = REJECTED
        .into_iter()
        .find(|(name, _)| *name == "ref_func_without_declaration")
        .expect("undeclared ref.func fixture");
    assert!(matches!(
        WasmModule::from_bytes(&bytes(undeclared_hex)),
        Err(WasmError::Decode(_))
    ));

    for name in ["export_declares_ref_func", "element_declares_ref_func"] {
        let (_, hex) = ACCEPTED
            .into_iter()
            .find(|(case, _)| *case == name)
            .expect("declared ref.func fixture");
        let values =
            eval(&bytes(hex)).unwrap_or_else(|error| panic!("{name}: {}", error.message()));
        assert!(
            matches!(values.as_slice(), [agenterm_tinyvm::Val::FuncRef(Some(0))]),
            "{name}: ref.func must preserve function index zero"
        );
    }
}

#[test]
fn standard_bytes_require_declared_memory() {
    let no_memory = bytes(ACCEPTED[0].1);
    let instance = WasmModule::from_bytes(&no_memory)
        .unwrap_or_else(|e| panic!("pure compute module: {}", e.message()))
        .instantiate()
        .unwrap_or_else(|e| panic!("instantiate pure compute module: {}", e.message()));
    assert_eq!(instance.memory_pages(), 0);
    assert!(
        instance
            .memory()
            .unwrap_or_else(|error| panic!("memory view: {}", error.message()))
            .is_empty()
    );

    let (_, empty_memory_hex) = ACCEPTED
        .into_iter()
        .find(|(name, _)| *name == "pure_compute_with_empty_memory_section")
        .expect("empty memory vector fixture");
    let empty_memory_instance = WasmModule::from_bytes(&bytes(empty_memory_hex))
        .unwrap_or_else(|e| panic!("empty memory vector: {}", e.message()))
        .instantiate()
        .unwrap_or_else(|e| panic!("instantiate empty memory vector: {}", e.message()));
    assert_eq!(empty_memory_instance.memory_pages(), 0);
    assert!(
        empty_memory_instance
            .memory()
            .unwrap_or_else(|error| panic!("memory view: {}", error.message()))
            .is_empty()
    );

    let (_, empty_memory_load_hex) = REJECTED
        .into_iter()
        .find(|(name, _)| *name == "load_with_empty_memory_section")
        .expect("load with empty memory vector fixture");
    assert!(matches!(
        WasmModule::from_bytes(&bytes(empty_memory_load_hex)),
        Err(WasmError::Decode(
            "validation: memory instruction requires memory"
        ))
    ));

    let active_empty = bytes("0061736d010000000b06010041000b00");
    assert!(matches!(
        WasmModule::from_bytes(&active_empty),
        Err(WasmError::Decode("data segment runs past memory bounds"))
    ));

    let passive = bytes("0061736d010000000b040101012a");
    let passive_instance = WasmModule::from_bytes(&passive)
        .unwrap_or_else(|e| panic!("passive data does not name a memory: {}", e.message()))
        .instantiate()
        .unwrap_or_else(|e| panic!("instantiate passive-data-only module: {}", e.message()));
    assert!(
        passive_instance
            .memory()
            .unwrap_or_else(|error| panic!("memory view: {}", error.message()))
            .is_empty()
    );
}

#[test]
fn standard_memarg_alignment_is_validated_at_load() {
    for (name, hex) in REJECTED
        .into_iter()
        .filter(|(name, _)| name.ends_with("_overaligned"))
    {
        assert!(
            matches!(
                WasmModule::from_bytes(&bytes(hex)),
                Err(WasmError::Decode(
                    "memory alignment exceeds natural alignment"
                ))
            ),
            "{name}: over-aligned memarg must fail at load"
        );
    }
}

#[test]
fn standard_function_expression_structure_is_canonical() {
    for (name, expected) in [
        (
            "instruction_after_function_end",
            "instructions follow function end",
        ),
        ("duplicate_else", "duplicate else in if"),
    ] {
        let (_, hex) = REJECTED
            .into_iter()
            .find(|(candidate, _)| *candidate == name)
            .expect("named malformed fixture");
        assert!(
            matches!(WasmModule::from_bytes(&bytes(hex)), Err(WasmError::Decode(message)) if message == expected),
            "{name}: malformed function structure must fail at load"
        );
    }
}

#[test]
fn standard_i64_leb_rejects_invalid_unused_high_bits() {
    for name in ["positive_i64_leb_overflow", "negative_i64_leb_overflow"] {
        let (_, hex) = REJECTED
            .into_iter()
            .find(|(candidate, _)| *candidate == name)
            .expect("named overflowing fixture");
        assert!(matches!(
            WasmModule::from_bytes(&bytes(hex)),
            Err(WasmError::Decode("signed LEB128 too long"))
        ));
    }
}

#[test]
fn standard_custom_section_name_is_validated_while_opaque_payload_stays_ignored() {
    for (name, expected) in [
        ("custom_section_without_name", "truncated unsigned LEB128"),
        (
            "custom_section_truncated_name_length",
            "truncated unsigned LEB128",
        ),
        (
            "custom_section_invalid_utf8_name",
            "name is not valid UTF-8",
        ),
    ] {
        let (_, hex) = REJECTED
            .into_iter()
            .find(|(candidate, _)| *candidate == name)
            .expect("named malformed custom section fixture");
        assert!(
            matches!(WasmModule::from_bytes(&bytes(hex)), Err(WasmError::Decode(message)) if message == expected),
            "{name}: malformed custom-section name must fail at load"
        );
    }
}

#[test]
fn standard_global_set_requires_a_mutable_declaration() {
    let (_, hex) = REJECTED
        .into_iter()
        .find(|(name, _)| *name == "immutable_global_set")
        .expect("immutable global.set fixture");
    assert!(matches!(
        WasmModule::from_bytes(&bytes(hex)),
        Err(WasmError::Decode("global.set"))
    ));
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

#[test]
fn module_validate_cli_is_static_and_rejects_invalid_bytes() {
    let directory = tempfile::tempdir().expect("temporary module validation directory");
    let valid = directory.path().join("valid-with-trapping-start.wasm");
    let invalid = directory.path().join("invalid.wasm");

    // Structurally valid `(module (func unreachable) (start 0))`. Validation
    // must succeed because this command never instantiates or runs the start
    // function; an execution-based checker would trap.
    std::fs::write(
        &valid,
        bytes("0061736d01000000010401600000030201000801000a05010300000b"),
    )
    .expect("write valid module");
    std::fs::write(&invalid, bytes(REJECTED[0].1)).expect("write invalid module");

    let accepted = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["module", "validate", valid.to_str().expect("valid path")])
        .output()
        .expect("validate legal module");
    assert!(
        accepted.status.success(),
        "static validation failed: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let stdout = String::from_utf8_lossy(&accepted.stdout);
    assert!(stdout.contains("start_function=present"));
    assert!(stdout.contains("standard_features=(mvp-only)"));
    assert!(stdout.contains("OK: standard Wasm module validated without instantiation"));

    let rejected = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "module",
            "validate",
            invalid.to_str().expect("invalid path"),
        ])
        .output()
        .expect("reject invalid module");
    assert!(!rejected.status.success());
    assert!(!String::from_utf8_lossy(&rejected.stderr).trim().is_empty());
}
