use agenterm_tinyvm::{Val, WasmError, WasmModule};

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
