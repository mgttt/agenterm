use agenterm_tinyvm::{WasmError, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
fn standard_bulk_memory_copy_fill_execute_with_wasm_semantics() {
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
