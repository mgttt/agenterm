use agenterm_tinyvm::{
    Limits, Val, ValueType, WasmError, WasmExternReference, WasmGlobal, WasmMemory, WasmModule,
    WasmStore, WasmTable,
};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn section(module: &mut Vec<u8>, id: u8, payload: &[u8]) {
    assert!(payload.len() < 128);
    module.extend_from_slice(&[id, payload.len() as u8]);
    module.extend_from_slice(payload);
}

fn name(bytes: &mut Vec<u8>, value: &str) {
    assert!(value.len() < 128);
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

fn passive_data_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x00]);
    section(&mut wasm, 3, &[0x01, 0x00]);
    section(&mut wasm, 5, &[0x01, 0x00, 0x01]);
    section(&mut wasm, 12, &[0x01]);
    section(
        &mut wasm,
        10,
        &[
            0x01, 0x0F, 0x00, 0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x08, 0x00, 0x00, 0xFC,
            0x09, 0x00, 0x0B,
        ],
    );
    section(
        &mut wasm,
        11,
        &[0x01, 0x01, 0x05, b'h', b'e', b'l', b'l', b'o'],
    );
    wasm
}

fn passive_elem_module() -> Vec<u8> {
    fn body(code: &mut Vec<u8>, instructions: &[u8]) {
        code.push((instructions.len() + 1) as u8);
        code.push(0);
        code.extend_from_slice(instructions);
    }

    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut wasm,
        1,
        &[
            0x03, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x00, 0x60, 0x00, 0x01, 0x7F, 0x60, 0x01, 0x7F,
            0x01, 0x7F,
        ],
    );
    section(&mut wasm, 3, &[0x05, 0x00, 0x01, 0x01, 0x02, 0x00]);
    section(&mut wasm, 4, &[0x01, 0x70, 0x00, 0x04]);
    section(&mut wasm, 9, &[0x01, 0x01, 0x00, 0x02, 0x01, 0x02]);
    let mut code = vec![0x05];
    body(
        &mut code,
        &[
            0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0C, 0x00, 0x00, 0xFC, 0x0D, 0x00, 0x0B,
        ],
    );
    body(&mut code, &[0x41, 0x2A, 0x0B]);
    body(&mut code, &[0x41, 0x07, 0x0B]);
    body(&mut code, &[0x20, 0x00, 0x11, 0x01, 0x00, 0x0B]);
    body(
        &mut code,
        &[
            0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0E, 0x00, 0x00, 0x0B,
        ],
    );
    section(&mut wasm, 10, &code);
    wasm
}

fn multi_result_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x02, 0x7F, 0x7E]);
    section(&mut wasm, 3, &[0x01, 0x00]);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x00]);
    section(
        &mut wasm,
        10,
        &[0x01, 0x06, 0x00, 0x41, 0x2A, 0x42, 0x07, 0x0B],
    );
    wasm
}

fn typed_host_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut wasm,
        1,
        &[
            0x02, // two types
            0x60, 0x03, 0x7E, 0x7D, 0x7C, 0x03, 0x7C, 0x7E, 0x7D, 0x60, 0x00, 0x03, 0x7C, 0x7E,
            0x7D,
        ],
    );
    section(
        &mut wasm,
        2,
        &[
            0x01, 0x04, b'h', b'o', b's', b't', 0x03, b'm', b'i', b'x', 0x00, 0x00,
        ],
    );
    section(&mut wasm, 3, &[0x01, 0x01]);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x01]);
    section(
        &mut wasm,
        10,
        &[
            0x01, 0x14, 0x00, // one body, 20 bytes, no locals
            0x42, 0x28, // i64.const 40
            0x43, 0x00, 0x00, 0xC0, 0x3F, // f32.const 1.5
            0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x40, // f64.const 2.5
            0x10, 0x00, 0x0B, // call imported host.mix; end
        ],
    );
    wasm
}

fn funcref_host_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x70]);
    section(
        &mut wasm,
        2,
        &[
            0x01, 0x04, b'h', b'o', b's', b't', 0x03, b'r', b'e', b'f', 0x00, 0x00,
        ],
    );
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x00]);
    wasm
}

fn externref_host_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut wasm,
        1,
        &[0x02, 0x60, 0x01, 0x6F, 0x01, 0x6F, 0x60, 0x00, 0x01, 0x7F],
    );
    let mut imports = vec![0x01];
    name(&mut imports, "host");
    name(&mut imports, "identity");
    imports.extend_from_slice(&[0x00, 0x00]);
    section(&mut wasm, 2, &imports);
    section(&mut wasm, 3, &[0x02, 0x00, 0x01]);
    section(&mut wasm, 6, &[0x01, 0x6F, 0x01, 0xD0, 0x6F, 0x0B]);
    let mut exports = vec![0x03];
    for (field, kind, index) in [("pass", 0, 1), ("null", 0, 2), ("saved", 3, 0)] {
        name(&mut exports, field);
        exports.extend_from_slice(&[kind, index]);
    }
    section(&mut wasm, 7, &exports);
    section(
        &mut wasm,
        10,
        &[
            0x02, 0x06, 0x00, 0x20, 0x00, 0x10, 0x00, 0x0B, 0x05, 0x00, 0xD0, 0x6F, 0xD1, 0x0B,
        ],
    );
    wasm
}

fn funcref_module() -> Vec<u8> {
    fn body(code: &mut Vec<u8>, instructions: &[u8]) {
        code.push((instructions.len() + 1) as u8);
        code.push(0);
        code.extend_from_slice(instructions);
    }

    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(&mut wasm, 3, &[0x05, 0x00, 0x00, 0x00, 0x00, 0x00]);
    section(&mut wasm, 4, &[0x01, 0x70, 0x01, 0x01, 0x05]);
    section(&mut wasm, 9, &[0x01, 0x05, 0x70, 0x01, 0xD2, 0x00, 0x0B]);
    let mut code = vec![0x05];
    body(&mut code, &[0x41, 0x2A, 0x0B]);
    body(
        &mut code,
        &[
            0x41, 0x00, 0xD2, 0x00, 0x26, 0x00, 0x41, 0x00, 0x11, 0x00, 0x00, 0x0B,
        ],
    );
    body(&mut code, &[0x41, 0x00, 0x25, 0x00, 0xD1, 0x0B]);
    body(&mut code, &[0xD0, 0x70, 0x41, 0x02, 0xFC, 0x0F, 0x00, 0x0B]);
    body(
        &mut code,
        &[
            0x41, 0x01, 0xD2, 0x00, 0x41, 0x02, 0xFC, 0x11, 0x00, 0x41, 0x02, 0x25, 0x00, 0xD1,
            0x45, 0x0B,
        ],
    );
    section(&mut wasm, 10, &code);
    wasm
}

fn explicit_table_expression_elem_module(table_index: u8) -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(&mut wasm, 3, &[0x02, 0x00, 0x00]);
    section(&mut wasm, 4, &[0x01, 0x70, 0x00, 0x01]);
    section(
        &mut wasm,
        9,
        &[
            0x01,
            0x06,
            table_index,
            0x41,
            0x00,
            0x0B,
            0x70,
            0x01,
            0xD2,
            0x00,
            0x0B,
        ],
    );
    section(
        &mut wasm,
        10,
        &[
            0x02, 0x04, 0x00, 0x41, 0x2A, 0x0B, 0x07, 0x00, 0x41, 0x00, 0x11, 0x00, 0x00, 0x0B,
        ],
    );
    wasm
}

fn multi_table_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(&mut wasm, 3, &[0x03, 0x00, 0x00, 0x00]);
    section(
        &mut wasm,
        4,
        &[0x02, 0x70, 0x01, 0x01, 0x03, 0x70, 0x01, 0x02, 0x04],
    );
    section(
        &mut wasm,
        7,
        &[
            0x02, 0x03, b'r', b'u', b'n', 0x00, 0x02, 0x01, b't', 0x01, 0x01,
        ],
    );
    section(
        &mut wasm,
        9,
        &[
            0x03, 0x00, 0x41, 0x00, 0x0B, 0x01, 0x00, 0x02, 0x01, 0x41, 0x01, 0x0B, 0x00, 0x01,
            0x01, 0x01, 0x00, 0x01, 0x00,
        ],
    );
    section(
        &mut wasm,
        10,
        &[
            0x03, 0x04, 0x00, 0x41, 0x2A, 0x0B, 0x04, 0x00, 0x41, 0x07, 0x0B, 0x69, 0x01, 0x01,
            0x7F, 0x41, 0x00, 0x11, 0x00, 0x00, 0x21, 0x00, 0x41, 0x01, 0x11, 0x00, 0x01, 0x20,
            0x00, 0x6A, 0x21, 0x00, 0x41, 0x00, 0x41, 0x00, 0x25, 0x00, 0x26, 0x01, 0x41, 0x00,
            0x11, 0x00, 0x01, 0x20, 0x00, 0x6A, 0x21, 0x00, 0x41, 0x00, 0x41, 0x01, 0x41, 0x01,
            0xFC, 0x0E, 0x00, 0x01, 0x41, 0x00, 0x11, 0x00, 0x00, 0x20, 0x00, 0x6A, 0x21, 0x00,
            0x41, 0x00, 0x41, 0x00, 0x41, 0x01, 0xFC, 0x0C, 0x02, 0x01, 0xFC, 0x0D, 0x02, 0x41,
            0x00, 0x11, 0x00, 0x01, 0x20, 0x00, 0x6A, 0x21, 0x00, 0x41, 0x00, 0xD0, 0x70, 0x41,
            0x00, 0xFC, 0x11, 0x01, 0xD0, 0x70, 0x41, 0x01, 0xFC, 0x0F, 0x00, 0x20, 0x00, 0x6A,
            0xFC, 0x10, 0x00, 0x6A, 0x0B,
        ],
    );
    wasm
}

fn tail_call_module() -> Vec<u8> {
    // WABT-equivalent standard bytes for tests/fixtures/tail-call-v1.wat. The
    // deep self-tail-call is deliberately far beyond the ordinary call-depth
    // ceiling; return_call must replace the activation instead of growing the
    // native Rust stack.
    vec![
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0A, 0x02, 0x60, 0x01, 0x7F, 0x01,
        0x7F, 0x60, 0x00, 0x01, 0x7F, 0x03, 0x05, 0x04, 0x00, 0x00, 0x00, 0x01, 0x04, 0x04, 0x01,
        0x70, 0x00, 0x01, 0x07, 0x07, 0x01, 0x03, b'r', b'u', b'n', 0x00, 0x03, 0x09, 0x07, 0x01,
        0x00, 0x41, 0x00, 0x0B, 0x01, 0x01, 0x0A, 0x35, 0x04, 0x13, 0x00, 0x20, 0x00, 0x45, 0x04,
        0x7F, 0x41, 0xE4, 0x00, 0x05, 0x20, 0x00, 0x41, 0x01, 0x6B, 0x12, 0x00, 0x0B, 0x0B, 0x07,
        0x00, 0x20, 0x00, 0x41, 0x2B, 0x6A, 0x0B, 0x09, 0x00, 0x20, 0x00, 0x41, 0x00, 0x13, 0x00,
        0x00, 0x0B, 0x0D, 0x00, 0x41, 0xA0, 0x8D, 0x06, 0x10, 0x00, 0x41, 0x00, 0x10, 0x02, 0x6A,
        0x0B,
    ]
}

fn host_tail_call_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut wasm,
        1,
        &[0x02, 0x60, 0x01, 0x7F, 0x01, 0x7F, 0x60, 0x00, 0x01, 0x7F],
    );
    section(
        &mut wasm,
        2,
        &[
            0x01, 0x04, b'h', b'o', b's', b't', 0x08, b'p', b'l', b'u', b's', b'_', b'o', b'n',
            b'e', 0x00, 0x00,
        ],
    );
    section(&mut wasm, 3, &[0x02, 0x00, 0x01]);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x02]);
    section(
        &mut wasm,
        10,
        &[
            0x02, 0x06, 0x00, 0x20, 0x00, 0x12, 0x00, 0x0B, 0x06, 0x00, 0x41, 0x29, 0x10, 0x01,
            0x0B,
        ],
    );
    wasm
}

fn mismatched_tail_result_module(indirect: bool, table_index: u8) -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut wasm,
        1,
        &[0x02, 0x60, 0x00, 0x01, 0x7E, 0x60, 0x00, 0x01, 0x7F],
    );
    section(&mut wasm, 3, &[0x02, 0x00, 0x01]);
    if indirect {
        section(&mut wasm, 4, &[0x01, 0x70, 0x00, 0x01]);
        section(&mut wasm, 9, &[0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, 0x00]);
    }
    let caller = if indirect {
        vec![0x00, 0x41, 0x00, 0x13, 0x00, table_index, 0x0B]
    } else {
        vec![0x00, 0x12, 0x00, 0x0B]
    };
    let mut code = vec![0x02, 0x04, 0x00, 0x42, 0x00, 0x0B];
    code.push(caller.len() as u8);
    code.extend_from_slice(&caller);
    section(&mut wasm, 10, &code);
    wasm
}

fn assert_copy_fill_semantics() {
    let mut module = WasmModule::new();
    let copy = must_ok(
        module.add_function(
            3,
            0,
            0,
            &[
                0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0A, 0x00, 0x00, 0x0B,
            ],
        ),
        "decode standard memory.copy",
    );
    let fill = must_ok(
        module.add_function(
            3,
            0,
            0,
            &[0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0B, 0x00, 0x0B],
        ),
        "decode standard memory.fill",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate module");
    must_ok(instance.memory_mut(), "memory mut")[0..8].copy_from_slice(b"abcdefgh");

    must_ok(instance.invoke(copy, &[2, 0, 6]), "overlap-safe copy");
    assert_eq!(&must_ok(instance.memory(), "memory")[0..8], b"ababcdef");
    must_ok(instance.invoke(fill, &[1, 0x1234, 3]), "low-byte fill");
    assert_eq!(&must_ok(instance.memory(), "memory")[0..8], b"a444cdef");
}

fn only_i32(values: Vec<Val>) -> i32 {
    match values.as_slice() {
        [Val::I32(value)] => *value,
        _ => panic!("expected one i32 result"),
    }
}

fn only_i64(values: Vec<Val>) -> i64 {
    match values.as_slice() {
        [Val::I64(value)] => *value,
        _ => panic!("expected one i64 result"),
    }
}

fn module_with_i32_global_initializer(expression: &[u8]) -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(&mut wasm, 3, &[0x01, 0x00]);
    let mut globals = vec![0x01, 0x7F, 0x00];
    globals.extend_from_slice(expression);
    section(&mut wasm, 6, &globals);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x00]);
    section(&mut wasm, 10, &[0x01, 0x04, 0x00, 0x23, 0x00, 0x0B]);
    wasm
}

fn imported_i32_globals_module(initializer: &[u8]) -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(
        &mut wasm,
        2,
        &[
            0x02, 0x04, b'h', b'o', b's', b't', 0x04, b'b', b'a', b's', b'e', 0x03, 0x7F, 0x00,
            0x04, b'h', b'o', b's', b't', 0x07, b'c', b'o', b'u', b'n', b't', b'e', b'r', 0x03,
            0x7F, 0x01,
        ],
    );
    section(&mut wasm, 3, &[0x01, 0x00]);
    let mut globals = vec![0x01, 0x7F, 0x00];
    globals.extend_from_slice(initializer);
    section(&mut wasm, 6, &globals);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x00]);
    section(
        &mut wasm,
        10,
        &[
            0x01, 0x0B, 0x00, 0x23, 0x01, 0x23, 0x00, 0x6A, 0x24, 0x01, 0x23, 0x02, 0x0B,
        ],
    );
    wasm
}

fn exported_i32_globals_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut wasm,
        6,
        &[
            0x02, 0x7F, 0x00, 0x41, 0x03, 0x0B, 0x7F, 0x01, 0x41, 0x0A, 0x0B,
        ],
    );
    section(
        &mut wasm,
        7,
        &[
            0x02, 0x04, b'b', b'a', b's', b'e', 0x03, 0x00, 0x07, b'c', b'o', b'u', b'n', b't',
            b'e', b'r', 0x03, 0x01,
        ],
    );
    wasm
}

#[test]
fn standard_imported_globals_bind_types_and_share_mutation() {
    let bytes = imported_i32_globals_module(&[0x23, 0x00, 0x41, 0x02, 0x6A, 0x0B]);
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load global imports");
    assert_eq!(module.global_imports().len(), 2);
    assert!(module.global_imports()[0].value_type == ValueType::I32);
    assert!(!module.global_imports()[0].mutable);
    assert!(module.global_imports()[1].mutable);

    let base = WasmGlobal::new(Val::I32(3), false);
    let counter = WasmGlobal::new(Val::I32(10), true);
    must_ok(
        module.bind_global_import("host", "base", &base),
        "bind base",
    );
    must_ok(
        module.bind_global_import("host", "counter", &counter),
        "bind counter",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate global imports");
    assert_eq!(
        only_i32(must_ok(instance.invoke_by_name("run", &[]), "run")),
        5
    );
    assert!(matches!(counter.value(), Val::I32(13)));

    let wrong_mutability = WasmGlobal::new(Val::I32(0), false);
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "reload global imports");
    assert!(matches!(
        module.bind_global_import("host", "counter", &wrong_mutability),
        Err(WasmError::Trap("global binding type"))
    ));
    assert!(matches!(
        base.set(Val::I32(4)),
        Err(WasmError::Trap("global binding type"))
    ));

    let mutable_const = imported_i32_globals_module(&[0x23, 0x01, 0x0B]);
    assert!(matches!(
        WasmModule::from_bytes(&mutable_const),
        Err(WasmError::Decode("const expr global index"))
    ));
}

#[test]
fn standard_exported_global_handle_links_sibling_instances() {
    let provider = must_ok(
        must_ok(
            WasmModule::from_bytes(&exported_i32_globals_module()),
            "load global provider",
        )
        .instantiate(),
        "instantiate global provider",
    );
    let base = provider
        .exported_global_handle("base")
        .expect("base export");
    let counter = provider
        .exported_global_handle("counter")
        .expect("counter export");
    assert!(!base.is_mutable());
    assert!(counter.is_mutable());

    let bytes = imported_i32_globals_module(&[0x23, 0x00, 0x41, 0x02, 0x6A, 0x0B]);
    let mut consumer = must_ok(WasmModule::from_bytes(&bytes), "load global consumer");
    must_ok(
        consumer.bind_global_import("host", "base", &base),
        "link base export",
    );
    must_ok(
        consumer.bind_global_import("host", "counter", &counter),
        "link counter export",
    );
    let mut consumer = must_ok(consumer.instantiate(), "instantiate global consumer");
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("run", &[]),
            "run linked consumer",
        )),
        5
    );
    assert!(matches!(
        provider.exported_global("counter"),
        Some(Val::I32(13))
    ));
}

fn imported_memory_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(
        &mut wasm,
        2,
        &[
            0x01, 0x04, b'h', b'o', b's', b't', 0x03, b'r', b'a', b'm', 0x02, 0x01, 0x01, 0x03,
        ],
    );
    section(&mut wasm, 3, &[0x01, 0x00]);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'u', b'n', 0x00, 0x00]);
    section(
        &mut wasm,
        10,
        &[0x01, 0x07, 0x00, 0x41, 0x00, 0x2D, 0x00, 0x00, 0x0B],
    );
    section(&mut wasm, 11, &[0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, b'A']);
    wasm
}

fn exported_memory_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 5, &[0x01, 0x01, 0x01, 0x03]);
    section(&mut wasm, 7, &[0x01, 0x03, b'r', b'a', b'm', 0x02, 0x00]);
    wasm
}

#[test]
fn standard_imported_memory_binds_limits_and_shares_store_identity() {
    let bytes = imported_memory_module();
    let unbound = must_ok(WasmModule::from_bytes(&bytes), "load unbound memory");
    assert!(matches!(
        unbound.instantiate(),
        Err(WasmError::Trap("unbound imported memory"))
    ));

    let memory = must_ok(WasmMemory::new(1, Some(3)), "allocate imported memory");
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load memory import");
    assert_eq!(module.memory_imports().len(), 1);
    assert_eq!(module.memory_imports()[0].min, 1);
    assert_eq!(module.memory_imports()[0].max, Some(3));
    must_ok(
        module.bind_memory_import("host", "ram", &memory),
        "bind imported memory",
    );
    let mut first = must_ok(module.instantiate(), "instantiate memory import");
    assert!(matches!(
        must_ok(first.invoke_by_name("run", &[]), "read active data").as_slice(),
        [Val::I32(65)]
    ));
    must_ok(memory.view_mut(), "host write")[0] = 66;
    assert!(matches!(
        must_ok(first.invoke_by_name("run", &[]), "read host write").as_slice(),
        [Val::I32(66)]
    ));

    let too_small = must_ok(WasmMemory::new(0, Some(3)), "allocate too-small memory");
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "reload memory import");
    assert!(matches!(
        module.bind_memory_import("host", "ram", &too_small),
        Err(WasmError::Trap("memory binding limits"))
    ));
    let unbounded = must_ok(WasmMemory::new(1, None), "allocate unbounded memory");
    assert!(matches!(
        module.bind_memory_import("host", "ram", &unbounded),
        Err(WasmError::Trap("memory binding limits"))
    ));
}

#[test]
fn standard_exported_memory_handle_links_without_copying_bytes() {
    let mut provider = must_ok(
        must_ok(
            WasmModule::from_bytes(&exported_memory_module()),
            "load memory provider",
        )
        .instantiate(),
        "instantiate memory provider",
    );
    must_ok(provider.exported_memory_mut("ram"), "provider memory").expect("ram export")[0] = 80;
    let memory = must_ok(
        provider.exported_memory_handle("ram"),
        "resolve memory export",
    )
    .expect("ram handle");
    assert_eq!(must_ok(memory.view(), "handle view")[0], 80);

    let mut consumer = must_ok(
        WasmModule::from_bytes(&imported_memory_module()),
        "load memory consumer",
    );
    must_ok(
        consumer.bind_memory_import("host", "ram", &memory),
        "link memory export",
    );
    let mut consumer = must_ok(consumer.instantiate(), "instantiate memory consumer");
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("run", &[]),
            "read linked active data",
        )),
        65
    );
    assert_eq!(
        must_ok(provider.exported_memory("ram"), "provider memory").expect("ram export")[0],
        65
    );
    drop(provider);
    must_ok(memory.view_mut(), "host write")[0] = 66;
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("run", &[]),
            "read linked host write",
        )),
        66
    );
}

#[test]
fn standard_resource_exports_are_resolved_by_name() {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 4, &[0x01, 0x70, 0x00, 0x02]);
    section(&mut wasm, 5, &[0x01, 0x00, 0x01]);
    section(
        &mut wasm,
        6,
        &[
            0x02, 0x7F, 0x01, 0x41, 0x07, 0x0B, 0x7E, 0x00, 0x42, 0x09, 0x0B,
        ],
    );
    section(
        &mut wasm,
        7,
        &[
            0x04, 0x01, b't', 0x01, 0x00, 0x03, b'm', b'e', b'm', 0x02, 0x00, 0x07, b'c', b'o',
            b'u', b'n', b't', b'e', b'r', 0x03, 0x00, 0x05, b'f', b'i', b'x', b'e', b'd', 0x03,
            0x01,
        ],
    );
    section(&mut wasm, 11, &[0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, b'A']);

    let module = must_ok(WasmModule::from_bytes(&wasm), "load resource exports");
    assert_eq!(module.table_export_index("t"), Some(0));
    assert_eq!(module.memory_export_index("mem"), Some(0));
    assert_eq!(module.global_export_index("counter"), Some(0));
    assert_eq!(module.global_export_index("fixed"), Some(1));
    let mut instance = must_ok(module.instantiate(), "instantiate resource exports");
    assert_eq!(instance.exported_table_elements("t"), Some(2));
    assert_eq!(
        must_ok(instance.exported_memory("mem"), "exported memory").map(|memory| memory[0]),
        Some(b'A')
    );
    must_ok(instance.exported_memory_mut("mem"), "exported memory mut").expect("exported memory")
        [1] = b'B';
    assert_eq!(
        &must_ok(instance.exported_memory("mem"), "exported memory").expect("memory")[..2],
        b"AB"
    );
    assert!(matches!(
        instance.exported_global("counter"),
        Some(Val::I32(7))
    ));
    must_ok(
        instance.set_exported_global("counter", Val::I32(11)),
        "set mutable exported global",
    );
    assert!(matches!(
        instance.exported_global("counter"),
        Some(Val::I32(11))
    ));
    assert!(matches!(
        instance.set_exported_global("fixed", Val::I64(10)),
        Err(WasmError::Trap("global binding type"))
    ));
    assert!(must_ok(instance.exported_memory("missing"), "missing memory").is_none());
}

fn exported_funcref_table_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(&mut wasm, 3, &[0x01, 0x00]);
    section(&mut wasm, 4, &[0x01, 0x70, 0x01, 0x01, 0x03]);
    let mut exports = vec![0x01];
    name(&mut exports, "dispatch");
    exports.extend_from_slice(&[0x01, 0x00]);
    section(&mut wasm, 7, &exports);
    section(&mut wasm, 9, &[0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, 0x00]);
    section(&mut wasm, 10, &[0x01, 0x04, 0x00, 0x41, 0x2A, 0x0B]);
    wasm
}

fn imported_funcref_dispatch_module() -> Vec<u8> {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    let mut imports = vec![0x01];
    name(&mut imports, "host");
    name(&mut imports, "dispatch");
    imports.extend_from_slice(&[0x01, 0x70, 0x01, 0x01, 0x03]);
    section(&mut wasm, 2, &imports);
    section(&mut wasm, 3, &[0x01, 0x00]);
    let mut exports = vec![0x01];
    name(&mut exports, "run");
    exports.extend_from_slice(&[0x00, 0x00]);
    section(&mut wasm, 7, &exports);
    section(
        &mut wasm,
        10,
        &[0x01, 0x07, 0x00, 0x41, 0x00, 0x11, 0x00, 0x00, 0x0B],
    );
    wasm
}

#[test]
fn standard_exported_table_handle_links_through_the_same_store() {
    let mut provider = must_ok(
        must_ok(
            WasmModule::from_bytes(&exported_funcref_table_module()),
            "load table provider",
        )
        .instantiate(),
        "instantiate table provider",
    );
    let table = must_ok(
        provider.exported_table_handle("dispatch"),
        "resolve table export",
    )
    .expect("dispatch table export");
    assert_eq!(table.len(), 1);

    let mut consumer = must_ok(
        WasmModule::from_bytes(&imported_funcref_dispatch_module()),
        "load table consumer",
    );
    must_ok(
        consumer.bind_table_import("host", "dispatch", &table),
        "link table export",
    );
    let mut consumer = must_ok(consumer.instantiate(), "instantiate table consumer");
    drop(provider);
    assert!(matches!(
        must_ok(consumer.invoke_by_name("run", &[]), "linked indirect call").as_slice(),
        [Val::I32(42)]
    ));
    assert_eq!(
        must_ok(table.is_null(0), "linked table visibility"),
        Some(false)
    );
}

#[test]
fn standard_imported_tables_decode_before_store_binding_exists() {
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    let mut imports = vec![0x01];
    name(&mut imports, "host");
    name(&mut imports, "dispatch");
    imports.extend_from_slice(&[0x01, 0x70, 0x01, 0x01, 0x03]);
    section(&mut wasm, 2, &imports);
    section(&mut wasm, 4, &[0x01, 0x70, 0x00, 0x01]);
    let mut exports = vec![0x02];
    name(&mut exports, "dispatch");
    exports.extend_from_slice(&[0x01, 0x00]);
    name(&mut exports, "local");
    exports.extend_from_slice(&[0x01, 0x01]);
    section(&mut wasm, 7, &exports);

    let module = must_ok(
        WasmModule::from_bytes_with(
            &wasm,
            Limits {
                max_table_elems: 2,
                ..Limits::default()
            },
        ),
        "decode table import",
    );
    assert_eq!(module.table_imports().len(), 1);
    assert_eq!(module.table_imports()[0].module, "host");
    assert_eq!(module.table_imports()[0].field, "dispatch");
    assert_eq!(module.table_imports()[0].min, 1);
    assert_eq!(module.table_imports()[0].max, Some(3));
    assert_eq!(module.table_export_index("dispatch"), Some(0));
    assert_eq!(module.table_export_index("local"), Some(1));
    assert!(matches!(
        module.instantiate(),
        Err(WasmError::Trap("unbound imported table"))
    ));

    let store = WasmStore::new();
    let table = must_ok(store.create_table(1, Some(3)), "allocate imported table");
    let mut module = must_ok(
        WasmModule::from_bytes_with(
            &wasm,
            Limits {
                max_table_elems: 2,
                ..Limits::default()
            },
        ),
        "reload table import",
    );
    must_ok(
        module.bind_table_import("host", "dispatch", &table),
        "bind imported table",
    );
    let instance = must_ok(module.instantiate(), "instantiate bound table");
    assert_eq!(instance.table_count(), 2);
    assert_eq!(instance.table_elements(), 2);
    assert_eq!(instance.exported_table_elements("dispatch"), Some(1));

    let too_small = must_ok(WasmTable::new(0, Some(3)), "allocate small table");
    let mut module = must_ok(WasmModule::from_bytes(&wasm), "reload for bad binding");
    assert!(matches!(
        module.bind_table_import("host", "dispatch", &too_small),
        Err(WasmError::Trap("table binding limits"))
    ));
    let unbounded = must_ok(WasmTable::new(1, None), "allocate unbounded table");
    assert!(matches!(
        module.bind_table_import("host", "dispatch", &unbounded),
        Err(WasmError::Trap("table binding limits"))
    ));

    assert!(matches!(
        WasmModule::from_bytes_with(
            &wasm,
            Limits {
                max_table_elems: 1,
                ..Limits::default()
            }
        ),
        Err(WasmError::Trap("table size"))
    ));
}

#[test]
fn standard_extended_const_executes_and_rejects_invalid_expression_stacks() {
    let wasm = module_with_i32_global_initializer(&[
        0x41, 40, 0x41, 5, 0x6A, 0x41, 3, 0x6B, 0x41, 1, 0x6C, 0x0B,
    ]);
    let module = must_ok(WasmModule::from_bytes(&wasm), "load extended const module");
    let mut instance = must_ok(module.instantiate(), "instantiate extended const module");
    assert_eq!(
        only_i32(must_ok(instance.invoke_by_name("run", &[]), "read global")),
        42
    );

    let underflow = module_with_i32_global_initializer(&[0x41, 1, 0x6A, 0x0B]);
    assert!(matches!(
        WasmModule::from_bytes(&underflow),
        Err(WasmError::Decode("const expr operand stack"))
    ));

    let wrong_type = module_with_i32_global_initializer(&[0x41, 1, 0x42, 2, 0x6A, 0x0B]);
    assert!(matches!(
        WasmModule::from_bytes(&wrong_type),
        Err(WasmError::Decode("const expr type mismatch"))
    ));

    let extra_result = module_with_i32_global_initializer(&[0x41, 1, 0x41, 2, 0x0B]);
    assert!(matches!(
        WasmModule::from_bytes(&extra_result),
        Err(WasmError::Decode("const expr result arity"))
    ));

    let unavailable_global = module_with_i32_global_initializer(&[0x23, 0x00, 0x0B]);
    assert!(matches!(
        WasmModule::from_bytes(&unavailable_global),
        Err(WasmError::Decode("const expr global index"))
    ));
}

#[test]
fn standard_sign_extension_proposal_executes() {
    let mut module = WasmModule::new();
    let i32_extend8 = must_ok(
        module.add_function(1, 0, 1, &[0x20, 0x00, 0xC0, 0x0B]),
        "decode i32.extend8_s",
    );
    let i32_extend16 = must_ok(
        module.add_function(1, 0, 1, &[0x20, 0x00, 0xC1, 0x0B]),
        "decode i32.extend16_s",
    );
    let i64_extend8 = must_ok(
        module.add_function(1, 0, 1, &[0x20, 0x00, 0xC2, 0x0B]),
        "decode i64.extend8_s",
    );
    let i64_extend16 = must_ok(
        module.add_function(1, 0, 1, &[0x20, 0x00, 0xC3, 0x0B]),
        "decode i64.extend16_s",
    );
    let i64_extend32 = must_ok(
        module.add_function(1, 0, 1, &[0x20, 0x00, 0xC4, 0x0B]),
        "decode i64.extend32_s",
    );

    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(i32_extend8, &[Val::I32(0x80)]),
            "run i32.extend8_s"
        )),
        -128
    );
    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(i32_extend16, &[Val::I32(0x8000)]),
            "run i32.extend16_s"
        )),
        -32768
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(i64_extend8, &[Val::I64(0x80)]),
            "run i64.extend8_s"
        )),
        -128
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(i64_extend16, &[Val::I64(0x8000)]),
            "run i64.extend16_s"
        )),
        -32768
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(i64_extend32, &[Val::I64(0x8000_0000)]),
            "run i64.extend32_s"
        )),
        i64::from(i32::MIN)
    );
}

#[test]
fn standard_nontrapping_conversion_proposal_saturates() {
    fn conversion(module: &mut WasmModule, subopcode: u8) -> usize {
        must_ok(
            module.add_function(1, 0, 1, &[0x20, 0x00, 0xFC, subopcode, 0x0B]),
            "decode trunc_sat conversion",
        )
    }

    let mut module = WasmModule::new();
    let functions: Vec<_> = (0..=7)
        .map(|subopcode| conversion(&mut module, subopcode))
        .collect();

    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(functions[0], &[Val::F32(f32::NAN)]),
            "NaN to signed i32"
        )),
        0
    );
    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(functions[1], &[Val::F32(f32::INFINITY)]),
            "+infinity to unsigned i32"
        )),
        -1
    );
    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(functions[2], &[Val::F64(f64::NEG_INFINITY)]),
            "-infinity to signed i32"
        )),
        i32::MIN
    );
    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(functions[3], &[Val::F64(-42.75)]),
            "negative to unsigned i32"
        )),
        0
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(functions[4], &[Val::F32(f32::INFINITY)]),
            "+infinity to signed i64"
        )),
        i64::MAX
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(functions[5], &[Val::F32(f32::NAN)]),
            "NaN to unsigned i64"
        )),
        0
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(functions[6], &[Val::F64(-42.75)]),
            "finite signed i64 truncation"
        )),
        -42
    );
    assert_eq!(
        only_i64(must_ok(
            module.invoke_val(functions[7], &[Val::F64(f64::INFINITY)]),
            "+infinity to unsigned i64"
        )),
        -1
    );
}

#[test]
fn standard_multi_value_proposal_executes() {
    let bytes = multi_result_module();
    let mut instance = must_ok(
        must_ok(WasmModule::from_bytes(&bytes), "load multi-result module").instantiate(),
        "instantiate multi-result module",
    );
    let results = must_ok(
        instance.invoke_by_name("run", &[]),
        "invoke multi-result export",
    );
    assert!(matches!(results.as_slice(), [Val::I32(42), Val::I64(7)]));

    let mut invalid = bytes;
    let i64_const = invalid
        .windows(2)
        .position(|window| window == [0x42, 0x07])
        .expect("i64.const 7 in fixture");
    invalid.splice(i64_const..i64_const + 2, []);
    let shortened_len = invalid.len();
    invalid[shortened_len - 7] -= 2; // code-section payload length
    invalid[shortened_len - 5] -= 2; // function-body length
    assert!(WasmModule::from_bytes(&invalid).is_err());

    let mut invalid_start = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut invalid_start, 1, &[0x01, 0x60, 0x00, 0x02, 0x7F, 0x7E]);
    section(&mut invalid_start, 3, &[0x01, 0x00]);
    section(&mut invalid_start, 8, &[0x00]);
    section(
        &mut invalid_start,
        10,
        &[0x01, 0x06, 0x00, 0x41, 0x2A, 0x42, 0x07, 0x0B],
    );
    assert!(
        WasmModule::from_bytes(&invalid_start).is_err(),
        "a standard start function must have no parameters and no results"
    );
}

#[test]
fn standard_multi_value_s33_block_type_index_64_executes() {
    let mut module = WasmModule::new();
    for _ in 0..64 {
        module.add_type(0, 0);
    }
    assert_eq!(module.add_type(1, 1), 64);
    let function = must_ok(
        module.add_function(
            0,
            0,
            1,
            &[
                0x41, 0x2A, // i32.const 42
                0x02, 0xC0, 0x00, // block type[64], encoded as positive s33
                0x0B, 0x0B,
            ],
        ),
        "decode block type index 64",
    );
    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(function, &[]),
            "run block type index 64"
        )),
        42
    );

    let noncanonical_i32 = must_ok(
        module.add_function(
            0,
            0,
            1,
            &[
                0x41, 0x2A, // i32.const 42
                0x02, 0xFF, 0x7F, // block i32 with valid sign-extended s33
                0x0B, 0x0B,
            ],
        ),
        "decode sign-extended inline block type",
    );
    assert_eq!(
        only_i32(must_ok(
            module.invoke_val(noncanonical_i32, &[]),
            "run sign-extended inline block type"
        )),
        42
    );

    assert!(
        module
            .add_function(0, 0, 0, &[0x02, 0x80, 0x80, 0x80, 0x80, 0x10, 0x0B, 0x0B])
            .is_err(),
        "an s33 block type must sign-extend its unused high payload bits"
    );
}

#[test]
fn standard_funcref_table_profile_executes_with_instance_semantics() {
    let bytes = funcref_module();
    let module_a = must_ok(WasmModule::from_bytes(&bytes), "load funcref module A");
    let module_b = must_ok(WasmModule::from_bytes(&bytes), "load funcref module B");
    let mut instance_a = must_ok(module_a.instantiate(), "instantiate funcref module A");
    let mut instance_b = must_ok(module_b.instantiate(), "instantiate funcref module B");

    assert_eq!(must_ok(instance_a.invoke(2, &[]), "A starts null"), vec![1]);
    assert_eq!(must_ok(instance_b.invoke(2, &[]), "B starts null"), vec![1]);
    assert_eq!(
        must_ok(instance_a.invoke(1, &[]), "A table.set/call"),
        vec![42]
    );
    assert_eq!(must_ok(instance_a.invoke(2, &[]), "A is non-null"), vec![0]);
    assert_eq!(
        must_ok(instance_b.invoke(2, &[]), "B remains independent"),
        vec![1]
    );

    assert_eq!(must_ok(instance_a.invoke(3, &[]), "A grow 1 to 3"), vec![1]);
    assert_eq!(must_ok(instance_a.invoke(4, &[]), "A table.fill"), vec![1]);
    assert_eq!(
        must_ok(instance_b.invoke(3, &[]), "B independently grows"),
        vec![1]
    );
    assert_eq!(must_ok(instance_a.invoke(3, &[]), "A grow 3 to 5"), vec![3]);
    assert_eq!(
        must_ok(instance_a.invoke(3, &[]), "A declared maximum"),
        vec![-1]
    );

    let mut undeclared = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut undeclared, 1, &[0x01, 0x60, 0x00, 0x01, 0x7F]);
    section(&mut undeclared, 3, &[0x01, 0x00]);
    section(
        &mut undeclared,
        10,
        &[0x01, 0x07, 0x00, 0xD2, 0x00, 0x1A, 0x41, 0x00, 0x0B],
    );
    assert!(WasmModule::from_bytes(&undeclared).is_err());

    let explicit = explicit_table_expression_elem_module(0);
    let mut explicit = must_ok(
        must_ok(
            WasmModule::from_bytes(&explicit),
            "load flag-6 element segment",
        )
        .instantiate(),
        "instantiate flag-6 element segment",
    );
    assert_eq!(
        must_ok(explicit.invoke(1, &[]), "call flag-6 initialized funcref"),
        vec![42]
    );
    assert!(
        WasmModule::from_bytes(&explicit_table_expression_elem_module(1)).is_err(),
        "the single-table profile must reject a nonzero explicit table index"
    );
}

#[test]
fn standard_funcref_bulk_work_traps_before_mutation() {
    use agenterm_tinyvm::Limits;

    let mut module = WasmModule::new_with_limits(Limits {
        max_steps: 6,
        max_table_elems: 128,
        ..Limits::default()
    });
    module.add_table(64);
    let fill = must_ok(
        module.add_function(
            0,
            0,
            0,
            &[
                0x41, 0x00, 0xD0, 0x70, 0x41, 0xC0, 0x00, 0xFC, 0x11, 0x00, 0x0B,
            ],
        ),
        "decode metered table.fill",
    );
    let grow = must_ok(
        module.add_function(0, 0, 1, &[0xD0, 0x70, 0x41, 0x20, 0xFC, 0x0F, 0x00, 0x0B]),
        "decode metered table.grow",
    );
    let first_is_null = must_ok(
        module.add_function(0, 0, 1, &[0x41, 0x00, 0x25, 0x00, 0xD1, 0x0B]),
        "decode table null check",
    );
    let size = must_ok(
        module.add_function(0, 0, 1, &[0xFC, 0x10, 0x00, 0x0B]),
        "decode table.size",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate metered table");
    assert!(instance.invoke(fill, &[]).is_err());
    assert_eq!(
        must_ok(instance.invoke(first_is_null, &[]), "fill atomic"),
        vec![1]
    );
    assert!(instance.invoke(grow, &[]).is_err());
    assert_eq!(must_ok(instance.invoke(size, &[]), "grow atomic"), vec![64]);
}

#[test]
fn standard_multiple_funcref_tables_execute_and_share_one_host_budget() {
    use agenterm_tinyvm::Limits;

    let mut programmatic = WasmModule::new_with_limits(Limits {
        max_table_elems: 3,
        ..Limits::default()
    });
    assert_eq!(
        must_ok(
            programmatic.add_funcref_table(2, Some(3)),
            "append bounded table",
        ),
        0
    );
    assert!(programmatic.add_funcref_table(2, None).is_err());
    assert!(programmatic.add_funcref_table(2, Some(1)).is_err());

    let bytes = multi_table_module();
    let mut instance = must_ok(
        must_ok(WasmModule::from_bytes(&bytes), "load multi-table module").instantiate(),
        "instantiate multi-table module",
    );
    assert_eq!(instance.table_count(), 2);
    assert_eq!(instance.table_elements_at(0), Some(1));
    assert_eq!(instance.table_elements_at(1), Some(2));
    let result = must_ok(
        instance.invoke_by_name("run", &[]),
        "run multi-table module",
    );
    assert!(matches!(result.as_slice(), [Val::I32(143)]));
    assert_eq!(instance.table_elements_at(0), Some(2));
    assert_eq!(instance.table_elements_at(1), Some(2));
    assert_eq!(instance.table_elements(), 4);

    assert!(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_table_elems: 2,
                ..Limits::default()
            },
        )
        .is_err(),
        "the host table budget applies to the aggregate, not to each table"
    );

    let mut aggregate_capped = must_ok(
        must_ok(
            WasmModule::from_bytes_with(
                &bytes,
                Limits {
                    max_table_elems: 3,
                    ..Limits::default()
                },
            ),
            "load at exact aggregate table budget",
        )
        .instantiate(),
        "instantiate at exact aggregate table budget",
    );
    let result = must_ok(
        aggregate_capped.invoke_by_name("run", &[]),
        "run with aggregate growth capped",
    );
    assert!(matches!(result.as_slice(), [Val::I32(140)]));
    assert_eq!(aggregate_capped.table_elements_at(0), Some(1));
    assert_eq!(aggregate_capped.table_elements_at(1), Some(2));

    let mut invalid = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut invalid, 1, &[0x01, 0x60, 0x00, 0x00]);
    section(&mut invalid, 3, &[0x01, 0x00]);
    section(&mut invalid, 4, &[0x02, 0x70, 0x00, 0x01, 0x70, 0x00, 0x01]);
    section(
        &mut invalid,
        10,
        &[0x01, 0x06, 0x00, 0xFC, 0x10, 0x02, 0x1A, 0x0B],
    );
    assert!(
        WasmModule::from_bytes(&invalid).is_err(),
        "an instruction cannot name table index two in a two-table module"
    );

    let mut invalid_export = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(
        &mut invalid_export,
        4,
        &[0x02, 0x70, 0x00, 0x01, 0x70, 0x00, 0x01],
    );
    section(&mut invalid_export, 7, &[0x01, 0x01, b't', 0x01, 0x02]);
    assert!(
        WasmModule::from_bytes(&invalid_export).is_err(),
        "a table export must name an existing table"
    );

    let mut duplicate_export = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut duplicate_export, 1, &[0x01, 0x60, 0x00, 0x00]);
    section(&mut duplicate_export, 3, &[0x01, 0x00]);
    section(&mut duplicate_export, 4, &[0x01, 0x70, 0x00, 0x01]);
    section(
        &mut duplicate_export,
        7,
        &[0x02, 0x01, b'x', 0x00, 0x00, 0x01, b'x', 0x01, 0x00],
    );
    section(&mut duplicate_export, 10, &[0x01, 0x02, 0x00, 0x0B]);
    assert!(
        WasmModule::from_bytes(&duplicate_export).is_err(),
        "export names are unique across function and table kinds"
    );
}

#[test]
fn standard_typed_host_imports_preserve_all_value_kinds() {
    let bytes = typed_host_module();
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load typed host module");
    assert!(
        (0..3)
            .map(|position| module.import_parameter_type(0, position))
            .eq([
                Some(ValueType::I64),
                Some(ValueType::F32),
                Some(ValueType::F64),
            ])
    );
    assert!(
        (0..3)
            .map(|position| module.import_result_type(0, position))
            .eq([
                Some(ValueType::F64),
                Some(ValueType::I64),
                Some(ValueType::F32),
            ])
    );
    assert!(
        module
            .bind_import("host", "mix", |_, _| Ok(vec![0, 0, 0]))
            .is_err(),
        "the legacy i32 door must reject a mixed standard signature at bind time"
    );
    must_ok(
        module.bind_import_typed_in_place("host", "mix", |args, results, memory| {
            assert!(
                memory.is_empty(),
                "a module without memory exposes no host slice"
            );
            assert!(matches!(args, [Val::I64(40), Val::F32(1.5), Val::F64(2.5)]));
            assert_eq!(results.len(), 3);
            results[0] = Val::F64(4.5);
            results[1] = Val::I64(42);
            results[2] = Val::F32(3.5);
            Ok(())
        }),
        "bind typed host module in place",
    );
    let expected = [Val::F64(4.5), Val::I64(42), Val::F32(3.5)];
    assert!(must_ok(module.invoke_by_name("run", &[]), "nested typed host call") == expected);
    assert!(
        must_ok(
            module.invoke_val(0, &[Val::I64(40), Val::F32(1.5), Val::F64(2.5)]),
            "top-level typed host call",
        ) == expected
    );

    let mut returning = must_ok(WasmModule::from_bytes(&bytes), "reload typed host module");
    must_ok(
        returning.bind_import_typed("host", "mix", |args, _memory| {
            assert_eq!(args.len(), 3);
            Ok(vec![Val::F64(4.5), Val::I64(42), Val::F32(3.5)])
        }),
        "bind arbitrary-arity typed compatibility callback",
    );
    assert!(
        must_ok(
            returning.invoke_by_name("run", &[]),
            "typed compatibility call"
        ) == expected
    );

    let mut wrong = must_ok(
        WasmModule::from_bytes(&bytes),
        "reload typed mismatch module",
    );
    must_ok(
        wrong.bind_import_typed_in_place("host", "mix", |_, results, _| {
            results[0] = Val::I32(4);
            Ok(())
        }),
        "bind typed mismatch callback",
    );
    assert!(matches!(
        wrong.invoke_by_name("run", &[]),
        Err(WasmError::Trap("host result type"))
    ));
}

#[test]
fn typed_host_can_borrow_selected_defined_memories_by_standard_index() {
    let bytes = wat::parse_str(
        r#"(module
          (import "host" "touch" (func $touch (param i32) (result i32)))
          (memory 1)
          (memory 1)
          (func (export "run") (result i32)
            i32.const 42
            call $touch))"#,
    )
    .expect("compile selected-memory host fixture");
    let mut module = must_ok(
        WasmModule::from_bytes(&bytes),
        "load selected-memory module",
    );
    must_ok(
        module.bind_import_typed_in_place_with_memories(
            "host",
            "touch",
            |args, results, memories| {
                assert_eq!(memories.len(), 2);
                assert!(!memories.is_empty());
                assert!(memories.memory(2)?.is_none());
                {
                    let first = memories.memory(0)?.expect("defined memory zero");
                    assert_eq!(first[7], 0);
                }
                let mut second = memories.memory_mut(1)?.expect("defined memory one");
                second[7] = 0xA5;
                results[0] = args[0];
                Ok(())
            },
        ),
        "bind selected-memory host callback",
    );
    assert!(
        must_ok(
            module.invoke_by_name("run", &[]),
            "invoke selected-memory host",
        ) == [Val::I32(42)]
    );

    let mut wrong = must_ok(
        WasmModule::from_bytes(&bytes),
        "reload selected-memory result mismatch",
    );
    must_ok(
        wrong.bind_import_typed_in_place_with_memories(
            "host",
            "touch",
            |_args, results, _memories| {
                results[0] = Val::I64(42);
                Ok(())
            },
        ),
        "bind selected-memory result mismatch",
    );
    assert!(matches!(
        wrong.invoke_by_name("run", &[]),
        Err(WasmError::Trap("host result type"))
    ));
}

#[test]
fn selected_memory_context_preserves_aliasing_for_imported_memories() {
    let bytes = wat::parse_str(
        r#"(module
          (import "host" "left" (memory 1 2))
          (import "host" "right" (memory 1 2))
          (import "host" "touch" (func $touch (result i32)))
          (func (export "run") (result i32) call $touch))"#,
    )
    .expect("compile imported selected-memory fixture");
    let shared = must_ok(WasmMemory::new(1, Some(2)), "allocate shared memory");
    let mut module = must_ok(
        WasmModule::from_bytes(&bytes),
        "load imported selected-memory module",
    );
    must_ok(
        module.bind_memory_import("host", "left", &shared),
        "bind left memory alias",
    );
    must_ok(
        module.bind_memory_import("host", "right", &shared),
        "bind right memory alias",
    );
    must_ok(
        module.bind_import_typed_in_place_with_memories(
            "host",
            "touch",
            |_args, results, memories| {
                {
                    let mut left = memories.memory_mut(0)?.expect("left imported memory");
                    left[9] = 77;
                }
                let right = memories.memory(1)?.expect("right imported memory");
                results[0] = Val::I32(i32::from(right[9]));
                Ok(())
            },
        ),
        "bind aliased selected-memory callback",
    );
    assert!(must_ok(module.invoke_by_name("run", &[]), "invoke aliased host") == [Val::I32(77)]);
    assert_eq!(must_ok(shared.view(), "inspect shared memory")[9], 77);
}

#[test]
fn standard_typed_host_funcref_results_are_instance_bounded() {
    let bytes = funcref_host_module();
    let mut null = must_ok(WasmModule::from_bytes(&bytes), "load funcref host module");
    must_ok(
        null.bind_import_typed_in_place("host", "ref", |_, results, _| {
            results[0] = Val::FuncRef(None);
            Ok(())
        }),
        "bind null funcref host result",
    );
    assert!(matches!(
        must_ok(null.invoke_by_name("run", &[]), "return null funcref").as_slice(),
        [Val::FuncRef(None)]
    ));

    let mut foreign = must_ok(WasmModule::from_bytes(&bytes), "reload funcref host module");
    must_ok(
        foreign.bind_import_typed("host", "ref", |_, _| Ok(vec![Val::FuncRef(Some(99))])),
        "bind invalid funcref host result",
    );
    assert!(matches!(
        foreign.invoke_by_name("run", &[]),
        Err(WasmError::Trap("host result type"))
    ));
}

#[test]
fn standard_externref_function_and_global_values_preserve_host_identity() {
    assert!(core::mem::size_of::<Val>() <= 16);
    let bytes = externref_host_module();
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load externref host module");
    assert!(module.import_parameter_type(0, 0) == Some(ValueType::ExternRef));
    assert!(module.import_result_type(0, 0) == Some(ValueType::ExternRef));
    must_ok(
        module.bind_import_typed_in_place("host", "identity", |args, results, _| {
            results[0] = args[0];
            Ok(())
        }),
        "bind externref identity",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate externref host module");
    let reference = must_ok(WasmExternReference::new(), "allocate host externref");
    assert!(matches!(
        must_ok(
            instance.invoke_by_name("pass", &[Val::ExternRef(Some(reference))]),
            "pass externref through host"
        )
        .as_slice(),
        [Val::ExternRef(Some(value))] if *value == reference
    ));
    assert!(matches!(
        must_ok(instance.invoke_by_name("null", &[]), "check null externref").as_slice(),
        [Val::I32(1)]
    ));
    let saved = instance
        .exported_global_handle("saved")
        .expect("externref global export");
    assert!(matches!(saved.value(), Val::ExternRef(None)));
    must_ok(
        saved.set(Val::ExternRef(Some(reference))),
        "set exported externref global",
    );
    assert!(saved.value() == Val::ExternRef(Some(reference)));

    let mut wrong = must_ok(
        WasmModule::from_bytes(&bytes),
        "reload externref host module",
    );
    must_ok(
        wrong.bind_import_typed_in_place("host", "identity", |_, results, _| {
            results[0] = Val::FuncRef(None);
            Ok(())
        }),
        "bind wrong externref result",
    );
    assert!(matches!(
        wrong.invoke_by_name("pass", &[Val::ExternRef(None)]),
        Err(WasmError::Trap("host result type"))
    ));

    let mut externref_table = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut externref_table, 4, &[0x01, 0x6F, 0x00, 0x01]);
    let table_module = must_ok(
        WasmModule::from_bytes(&externref_table),
        "load defined externref table",
    );
    let table_instance = must_ok(table_module.instantiate(), "instantiate externref table");
    assert!(table_instance.table_count() == 1);
    assert!(table_instance.table_elements_at(0) == Some(1));

    let host_table = must_ok(
        WasmTable::new_externref(1, Some(2)),
        "create externref table",
    );
    assert!(host_table.element_type() == ValueType::ExternRef);
    assert!(host_table.is_null(0) == Ok(Some(true)));
    must_ok(
        host_table.set(0, Val::ExternRef(Some(reference))),
        "set host externref table",
    );
    assert!(host_table.get(0) == Ok(Some(Val::ExternRef(Some(reference)))));
    assert!(matches!(
        host_table.set(0, Val::FuncRef(None)),
        Err(WasmError::Trap("table element type"))
    ));

    let mut mixed_copy = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut mixed_copy, 1, &[0x01, 0x60, 0x00, 0x00]);
    section(&mut mixed_copy, 3, &[0x01, 0x00]);
    section(
        &mut mixed_copy,
        4,
        &[0x02, 0x70, 0x00, 0x01, 0x6F, 0x00, 0x01],
    );
    section(
        &mut mixed_copy,
        10,
        &[
            0x01, 0x0C, 0x00, 0x41, 0x00, 0x41, 0x00, 0x41, 0x00, 0xFC, 0x0E, 0x00, 0x01, 0x0B,
        ],
    );
    assert!(matches!(
        WasmModule::from_bytes(&mixed_copy),
        Err(WasmError::Decode("validation: type mismatch"))
    ));
}

#[test]
fn standard_tail_calls_trampoline_across_direct_indirect_and_host_targets() {
    let mut deep = must_ok(
        must_ok(
            WasmModule::from_bytes(&tail_call_module()),
            "load tail-call module",
        )
        .instantiate(),
        "instantiate tail-call module",
    );
    let result = must_ok(
        deep.invoke_by_name("run", &[]),
        "run deep direct and indirect tail calls",
    );
    assert!(matches!(result.as_slice(), [Val::I32(143)]));

    let mut host_module = must_ok(
        WasmModule::from_bytes(&host_tail_call_module()),
        "load host tail-call module",
    );
    must_ok(
        host_module.bind_import("host", "plus_one", |args, _memory| Ok(vec![args[0] + 1])),
        "bind host tail target",
    );
    let result = must_ok(
        host_module.invoke_by_name("run", &[]),
        "tail-call host import",
    );
    assert!(matches!(result.as_slice(), [Val::I32(42)]));

    assert!(
        WasmModule::from_bytes(&mismatched_tail_result_module(false, 0)).is_err(),
        "return_call requires the callee and current function results to match exactly"
    );
    assert!(
        WasmModule::from_bytes(&mismatched_tail_result_module(true, 0)).is_err(),
        "return_call_indirect requires the selected type and current results to match exactly"
    );

    let mut bad_table = mismatched_tail_result_module(true, 1);
    // Make both functions return i64 so the table immediate is the only invalid
    // part of the tail instruction.
    let caller_result = bad_table
        .windows(5)
        .position(|window| window == [0x60, 0x00, 0x01, 0x7F, 0x03])
        .expect("caller type bytes");
    bad_table[caller_result + 3] = 0x7E;
    assert!(
        WasmModule::from_bytes(&bad_table).is_err(),
        "return_call_indirect rejects an unknown table index at load"
    );
}

#[test]
fn standard_bulk_memory_copy_fill_execute_with_wasm_semantics() {
    assert_copy_fill_semantics();
}

#[test]
fn standard_bulk_memory_proposal_executes_with_instance_semantics() {
    assert_copy_fill_semantics();
    let data_bytes = passive_data_module();
    let mut data_a = must_ok(
        must_ok(WasmModule::from_bytes(&data_bytes), "load passive data").instantiate(),
        "instantiate passive data A",
    );
    let mut data_b = must_ok(
        must_ok(WasmModule::from_bytes(&data_bytes), "reload passive data").instantiate(),
        "instantiate passive data B",
    );
    must_ok(data_a.invoke(0, &[8, 1, 3]), "init and drop data A");
    assert_eq!(&must_ok(data_a.memory(), "memory A")[8..11], b"ell");
    assert!(data_a.invoke(0, &[0, 0, 1]).is_err());
    must_ok(data_b.invoke(0, &[0, 0, 5]), "independent data B");
    assert_eq!(&must_ok(data_b.memory(), "memory B")[0..5], b"hello");

    let elem_bytes = passive_elem_module();
    let mut elem = must_ok(
        must_ok(WasmModule::from_bytes(&elem_bytes), "load passive elem").instantiate(),
        "instantiate passive elem",
    );
    must_ok(elem.invoke(0, &[1, 0, 2]), "table.init and elem.drop");
    assert_eq!(
        must_ok(elem.invoke(3, &[1]), "call first funcref"),
        vec![42]
    );
    assert_eq!(
        must_ok(elem.invoke(3, &[2]), "call second funcref"),
        vec![7]
    );
    must_ok(elem.invoke(4, &[0, 1, 2]), "overlap-safe table.copy");
    assert_eq!(
        must_ok(elem.invoke(3, &[0]), "call copied funcref"),
        vec![42]
    );
    assert!(elem.invoke(0, &[0, 0, 1]).is_err());
}
