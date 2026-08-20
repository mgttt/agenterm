//! Black-box owner for the standard-WASM game ABI v1 boundary.

use std::cell::Cell;
use std::rc::Rc;

use agenterm_tinyvm::{
    GameInput, GameLimits, GameRuntime, Limits, NativeModuleRegistry, WasmError,
};

const CORE: &str = "tinyarcade:core/v1";

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn leb(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn name(out: &mut Vec<u8>, value: &str) {
    leb(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

fn section(module: &mut Vec<u8>, id: u8, payload: &[u8]) {
    module.push(id);
    leb(module, payload.len());
    module.extend_from_slice(payload);
}

fn body(code: &[u8]) -> Vec<u8> {
    let mut body = vec![0x00];
    body.extend_from_slice(code);
    body
}

fn game_module(imports: &[(&str, &str, usize)], version: i8, tick: &[u8], data: &[u8]) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    section(
        &mut module,
        1,
        &[
            0x03, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, 0x60, 0x01, 0x7e,
            0x01, 0x7f,
        ],
    );

    let mut import_payload = Vec::new();
    leb(&mut import_payload, imports.len());
    for &(namespace, field, type_index) in imports {
        name(&mut import_payload, namespace);
        name(&mut import_payload, field);
        import_payload.push(0x00);
        leb(&mut import_payload, type_index);
    }
    if !imports.is_empty() {
        section(&mut module, 2, &import_payload);
    }

    section(&mut module, 3, &[0x03, 0x00, 0x00, 0x00]);
    section(&mut module, 5, &[0x01, 0x00, 0x01]);

    let first_defined = imports.len();
    let mut exports = Vec::new();
    exports.push(0x03);
    for (field, index) in [
        ("game_abi_version", first_defined),
        ("game_init", first_defined + 1),
        ("game_tick", first_defined + 2),
    ] {
        name(&mut exports, field);
        exports.push(0x00);
        leb(&mut exports, index);
    }
    section(&mut module, 7, &exports);

    let functions = [
        body(&[0x41, version as u8, 0x0b]),
        body(&[0x41, 0x00, 0x0b]),
        body(tick),
    ];
    let mut code = vec![0x03];
    for function in &functions {
        leb(&mut code, function.len());
        code.extend_from_slice(function);
    }
    section(&mut module, 10, &code);

    if !data.is_empty() {
        let mut segment = vec![0x01, 0x00, 0x41, 0x00, 0x0b];
        leb(&mut segment, data.len());
        segment.extend_from_slice(data);
        section(&mut module, 11, &segment);
    }
    module
}

fn all_imports() -> [(&'static str, &'static str, usize); 5] {
    [
        (CORE, "input_bits", 0),
        (CORE, "clock_ms", 0),
        (CORE, "random_u32", 0),
        (CORE, "submit_render", 1),
        (CORE, "submit_audio", 1),
    ]
}

fn tick_with_outputs(render_len: u8) -> Vec<u8> {
    vec![
        0x10, 0x00, 0x1a, 0x10, 0x01, 0x1a, 0x10, 0x02, 0x1a, 0x41, 0x00, 0x41, render_len, 0x10,
        0x03, 0x1a, 0x41, 0x03, 0x41, 0x02, 0x10, 0x04, 0x1a, 0x41, 0x00, 0x0b,
    ]
}

fn tick_with_deterministic_snapshot() -> Vec<u8> {
    vec![
        0x41, 0x00, 0x10, 0x00, 0x36, 0x02, 0x00, // memory[0] = input_bits()
        0x41, 0x04, 0x10, 0x01, 0x36, 0x02, 0x00, // memory[4] = clock_ms()
        0x41, 0x08, 0x10, 0x02, 0x36, 0x02, 0x00, // memory[8] = random_u32()
        0x41, 0x00, 0x41, 0x0c, 0x10, 0x03, 0x1a, // render(0, 12)
        0x41, 0x00, 0x0b,
    ]
}

#[test]
fn standard_wasm_cartridge_drives_one_bounded_frame() {
    let wasm = game_module(&all_imports(), 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let mut runtime = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 0x1234_5678),
        "load standard game cartridge",
    );
    let frame = must_ok(
        runtime.tick(GameInput {
            buttons: 0b101,
            clock_ms: 16,
        }),
        "tick",
    );
    assert_eq!(frame.render, [1, 2, 3]);
    assert_eq!(frame.audio, [4, 5]);
}

#[test]
fn input_clock_and_rng_are_host_owned_and_deterministic() {
    let wasm = game_module(&all_imports(), 1, &tick_with_deterministic_snapshot(), &[]);
    let seed = 0x1234_5678u32;
    let mut expected_rng = seed;
    expected_rng ^= expected_rng << 13;
    expected_rng ^= expected_rng >> 17;
    expected_rng ^= expected_rng << 5;
    let mut runtime = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), seed),
        "load deterministic game",
    );
    let frame = must_ok(
        runtime.tick(GameInput {
            buttons: 0x8000_0005,
            clock_ms: 1234,
        }),
        "deterministic tick",
    );
    assert_eq!(&frame.render[0..4], &0x8000_0005u32.to_le_bytes());
    assert_eq!(&frame.render[4..8], &1234u32.to_le_bytes());
    assert_eq!(&frame.render[8..12], &expected_rng.to_le_bytes());
}

#[test]
fn unknown_native_namespace_fails_closed_until_registered() {
    let wasm = game_module(
        &[("fan:physics/v1", "step", 0)],
        1,
        &[0x41, 0x00, 0x0b],
        &[],
    );
    assert!(matches!(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Trap("game import is not allowed"))
    ));
}

#[test]
fn registered_versioned_native_module_is_bound_by_exact_signature() {
    let wasm = game_module(
        &[("fan:physics/v1", "step", 0)],
        1,
        &[0x10, 0x00, 0x1a, 0x41, 0x00, 0x0b],
        &[],
    );
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let mut registry = NativeModuleRegistry::new();
    must_ok(
        registry.register("fan:physics/v1", "step", 0, 1, move |_, _| {
            observed.set(observed.get() + 1);
            Ok(vec![0])
        }),
        "register native module",
    );
    let mut runtime = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            &registry,
        ),
        "load game with native module",
    );
    must_ok(runtime.tick(GameInput::default()), "tick native module");
    assert_eq!(calls.get(), 1);
}

#[test]
fn core_import_signature_is_checked_before_instantiation() {
    let wasm = game_module(&[(CORE, "input_bits", 2)], 1, &[0x41, 0x00, 0x0b], &[]);
    assert!(matches!(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Trap("game import is not allowed"))
    ));
}

#[test]
fn lifecycle_version_and_frame_budget_are_enforced() {
    let wrong_version = game_module(&[], 2, &[0x41, 0x00, 0x0b], &[]);
    assert!(matches!(
        GameRuntime::from_bytes(&wrong_version, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Trap("unsupported game ABI version"))
    ));

    let wasm = game_module(&all_imports(), 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let mut runtime = must_ok(
        GameRuntime::from_bytes(
            &wasm,
            Limits::default(),
            GameLimits {
                max_render_bytes: 2,
                max_audio_bytes: 2,
            },
            1,
        ),
        "load game",
    );
    assert!(matches!(
        runtime.tick(GameInput::default()),
        Err(WasmError::Trap("game output budget"))
    ));
}
