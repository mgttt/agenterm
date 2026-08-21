use agenterm_tinyvm::{Val, ValueType, WasmError, WasmModule};

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
    instance.memory_mut()[0..8].copy_from_slice(b"abcdefgh");

    must_ok(instance.invoke(copy, &[2, 0, 6]), "overlap-safe copy");
    assert_eq!(&instance.memory()[0..8], b"ababcdef");
    must_ok(instance.invoke(fill, &[1, 0x1234, 3]), "low-byte fill");
    assert_eq!(&instance.memory()[0..8], b"a444cdef");
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

    let mut externref = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut externref, 1, &[0x01, 0x60, 0x01, 0x6F, 0x00]);
    assert!(WasmModule::from_bytes(&externref).is_err());

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
    assert_eq!(&data_a.memory()[8..11], b"ell");
    assert!(data_a.invoke(0, &[0, 0, 1]).is_err());
    must_ok(data_b.invoke(0, &[0, 0, 5]), "independent data B");
    assert_eq!(&data_b.memory()[0..5], b"hello");

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
