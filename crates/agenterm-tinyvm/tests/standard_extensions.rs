use agenterm_tinyvm::{WasmError, WasmModule};

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
