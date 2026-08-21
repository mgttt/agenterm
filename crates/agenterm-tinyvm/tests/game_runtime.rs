//! Black-box owner for the standard-WASM game ABI v1 boundary.

use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;

use agenterm_tinyvm::GuestResourceHandle;
use agenterm_tinyvm::{
    CartridgeDescriptor, CartridgeManifest, CompletionError, CompletionPoll, GameFrame, GameInput,
    GameLifecycle, GameLimits, GameRuntime, HostProfileV1, HostResourceTable, Limits,
    MAX_CARTRIDGE_BYTES, MAX_NATIVE_CALLS_PER_LIFECYCLE, NativeModuleRegistry, RenderFrame,
    WasmError,
};
#[cfg(feature = "replay")]
use agenterm_tinyvm::{ReplayRecorderV1, ReplayTraceV1};

const CORE: &str = "tinyarcade:core/v1";

#[test]
fn whole_cartridge_size_is_bounded_before_wasm_parsing() {
    let oversized = vec![0; MAX_CARTRIDGE_BYTES + 1];
    assert!(matches!(
        GameRuntime::from_private_bytes(&oversized, Limits::default(), GameLimits::default(), 1,),
        Err(WasmError::Decode("game cartridge size limit"))
    ));
}

#[test]
fn game_profile_rejects_multiple_memories_without_limiting_the_vm() {
    let mut wasm = game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]);
    let memory = wasm
        .windows(5)
        .position(|bytes| bytes == [0x05, 0x03, 0x01, 0x00, 0x01])
        .expect("single-memory section");
    wasm.splice(
        memory..memory + 5,
        [0x05, 0x05, 0x02, 0x00, 0x01, 0x00, 0x01],
    );
    assert!(matches!(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Decode(
            "game cartridge requires exactly one memory"
        ))
    ));
}

#[test]
fn game_profile_rejects_global_imports_without_limiting_the_vm() {
    let mut wasm = game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]);
    let function_section = wasm
        .windows(8)
        .position(|bytes| bytes == [0x03, 0x06, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00])
        .expect("function section");
    let mut import_payload = vec![0x01];
    name(&mut import_payload, "host");
    name(&mut import_payload, "base");
    import_payload.extend_from_slice(&[0x03, 0x7f, 0x00]);
    let mut import_section = Vec::new();
    section(&mut import_section, 2, &import_payload);
    wasm.splice(function_section..function_section, import_section);

    assert!(matches!(
        CartridgeDescriptor::inspect(&wasm, Limits::default()),
        Err(WasmError::Decode(
            "game cartridge does not support global imports"
        ))
    ));
    assert!(matches!(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Decode(
            "game cartridge does not support global imports"
        ))
    ));
}

#[test]
fn game_profile_rejects_memory_imports_without_limiting_the_vm() {
    let mut wasm = game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]);
    let memory_section = wasm
        .windows(5)
        .position(|bytes| bytes == [0x05, 0x03, 0x01, 0x00, 0x01])
        .expect("defined memory section");
    wasm.drain(memory_section..memory_section + 5);
    let function_section = wasm
        .windows(8)
        .position(|bytes| bytes == [0x03, 0x06, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00])
        .expect("function section");
    let mut import_payload = vec![0x01];
    name(&mut import_payload, "host");
    name(&mut import_payload, "ram");
    import_payload.extend_from_slice(&[0x02, 0x00, 0x01]);
    let mut import_section = Vec::new();
    section(&mut import_section, 2, &import_payload);
    wasm.splice(function_section..function_section, import_section);

    assert!(matches!(
        CartridgeDescriptor::inspect(&wasm, Limits::default()),
        Err(WasmError::Decode(
            "game cartridge does not support memory imports"
        ))
    ));
    assert!(matches!(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Decode(
            "game cartridge does not support memory imports"
        ))
    ));
}

#[test]
fn game_profile_rejects_table_imports_without_limiting_the_vm() {
    let mut wasm = game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]);
    let function_section = wasm
        .windows(8)
        .position(|bytes| bytes == [0x03, 0x06, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00])
        .expect("function section");
    let mut import_payload = vec![0x01];
    name(&mut import_payload, "host");
    name(&mut import_payload, "dispatch");
    import_payload.extend_from_slice(&[0x01, 0x70, 0x01, 0x01, 0x03]);
    let mut import_section = Vec::new();
    section(&mut import_section, 2, &import_payload);
    wasm.splice(function_section..function_section, import_section);

    assert!(matches!(
        CartridgeDescriptor::inspect(&wasm, Limits::default()),
        Err(WasmError::Decode(
            "game cartridge does not support table imports"
        ))
    ));
    assert!(matches!(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Decode(
            "game cartridge does not support table imports"
        ))
    ));
}

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

fn without_leading_manifest(module: &[u8]) -> Vec<u8> {
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(module[8], 0, "test module starts with its manifest");
    let mut cursor = 9;
    let mut size = 0usize;
    for shift in (0..35).step_by(7) {
        let byte = module[cursor];
        cursor += 1;
        size |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            let end = cursor + size;
            let mut result = module[..8].to_vec();
            result.extend_from_slice(&module[end..]);
            return result;
        }
    }
    panic!("test manifest section has an invalid size");
}

fn body(code: &[u8]) -> Vec<u8> {
    let mut body = vec![0x00];
    body.extend_from_slice(code);
    body
}

fn manifest_section_for(
    abi_version: u32,
    state_version: u32,
    game_id: &str,
    capabilities: &[&str],
) -> Vec<u8> {
    let mut custom = Vec::new();
    name(&mut custom, "tinyarcade.manifest.v1");
    custom.extend_from_slice(b"TAM1");
    custom.extend_from_slice(&abi_version.to_le_bytes());
    custom.extend_from_slice(&state_version.to_le_bytes());
    for value in [game_id, "1.0.0"] {
        custom.extend_from_slice(&(value.len() as u16).to_le_bytes());
        custom.extend_from_slice(value.as_bytes());
    }
    custom.extend_from_slice(&(capabilities.len() as u16).to_le_bytes());
    for capability in capabilities {
        custom.extend_from_slice(&(capability.len() as u16).to_le_bytes());
        custom.extend_from_slice(capability.as_bytes());
    }
    custom
}

fn manifest_section(abi_version: u32, capabilities: &[&str]) -> Vec<u8> {
    manifest_section_for(abi_version, 1, "test.game", capabilities)
}

fn game_module(imports: &[(&str, &str, usize)], version: i8, tick: &[u8], data: &[u8]) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    let mut capabilities = Vec::new();
    for &(namespace, _, _) in imports {
        if namespace != CORE && !capabilities.contains(&namespace) {
            capabilities.push(namespace);
        }
    }
    section(
        &mut module,
        0,
        &manifest_section(version as u32, &capabilities),
    );
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

    section(&mut module, 3, &[0x05, 0x00, 0x00, 0x00, 0x00, 0x00]);
    section(&mut module, 5, &[0x01, 0x00, 0x01]);

    let first_defined = imports.len();
    let mut exports = Vec::new();
    exports.push(0x05);
    for (field, index) in [
        ("game_abi_version", first_defined),
        ("game_init", first_defined + 1),
        ("game_tick", first_defined + 2),
        ("game_suspend", first_defined + 3),
        ("game_resume", first_defined + 4),
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
        body(&[0x41, 0x00, 0x0b]),
        body(&[0x41, 0x00, 0x0b]),
    ];
    let mut code = vec![0x05];
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

#[cfg(feature = "replay")]
fn native_replay_module() -> Vec<u8> {
    let capability = "fan:physics/v1";
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    section(
        &mut module,
        0,
        &manifest_section_for(1, 1, "test.native-replay", &[capability]),
    );
    section(
        &mut module,
        1,
        &[
            0x02, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
        ],
    );
    let mut imports = vec![0x05];
    for (namespace, field, type_index) in [
        (capability, "step", 0u8),
        (CORE, "submit_render", 1),
        (CORE, "save_state", 1),
        (CORE, "load_state", 1),
        (CORE, "grid3d_version", 0),
    ] {
        name(&mut imports, namespace);
        name(&mut imports, field);
        imports.extend_from_slice(&[0x00, type_index]);
    }
    section(&mut module, 2, &imports);
    section(&mut module, 3, &[0x05, 0, 0, 0, 0, 0]);
    section(&mut module, 5, &[0x01, 0x00, 0x01]);
    let mut exports = vec![0x05];
    for (field, index) in [
        ("game_abi_version", 5usize),
        ("game_init", 6),
        ("game_tick", 7),
        ("game_suspend", 8),
        ("game_resume", 9),
    ] {
        name(&mut exports, field);
        exports.push(0);
        leb(&mut exports, index);
    }
    section(&mut module, 7, &exports);
    let functions = [
        body(&[0x41, 1, 0x0b]),
        body(&[0x41, 0, 0x0b]),
        body(&[
            0x10, 0, 0x1a, 0x41, 0, 0x41, 32, 0x10, 1, 0x1a, 0x41, 0, 0x0b,
        ]),
        body(&[0x41, 32, 0x41, 1, 0x10, 2, 0x1a, 0x41, 0, 0x0b]),
        body(&[0x41, 32, 0x41, 1, 0x10, 3, 0x1a, 0x41, 0, 0x0b]),
    ];
    let mut code = vec![0x05];
    for function in &functions {
        leb(&mut code, function.len());
        code.extend_from_slice(function);
    }
    section(&mut module, 10, &code);
    let mut frame = vec![0; 33];
    frame[0..4].copy_from_slice(b"TAG3");
    frame[4..6].copy_from_slice(&1u16.to_le_bytes());
    frame[6..8].copy_from_slice(&32u16.to_le_bytes());
    for offset in [8, 10, 12] {
        frame[offset..offset + 2].copy_from_slice(&1u16.to_le_bytes());
    }
    frame[24..28].copy_from_slice(&1u32.to_le_bytes());
    frame[32] = 7;
    let mut data = vec![0x01, 0x00, 0x41, 0x00, 0x0b];
    leb(&mut data, frame.len());
    data.extend_from_slice(&frame);
    section(&mut module, 11, &data);
    module
}

fn stateful_game_module(state_version: u32, game_id: &str) -> Vec<u8> {
    let imports = [
        ("input_bits", 0usize),
        ("clock_ms", 0),
        ("random_u32", 0),
        ("submit_render", 1),
        ("submit_audio", 1),
        ("save_state", 1),
        ("load_state", 1),
    ];
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    section(
        &mut module,
        0,
        &manifest_section_for(1, state_version, game_id, &[]),
    );
    section(
        &mut module,
        1,
        &[
            0x02, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
        ],
    );
    let mut import_payload = vec![imports.len() as u8];
    for (field, type_index) in imports {
        name(&mut import_payload, CORE);
        name(&mut import_payload, field);
        import_payload.push(0x00);
        leb(&mut import_payload, type_index);
    }
    section(&mut module, 2, &import_payload);
    section(&mut module, 3, &[0x05, 0x00, 0x00, 0x00, 0x00, 0x00]);
    section(&mut module, 5, &[0x01, 0x00, 0x01]);
    section(&mut module, 6, &[0x01, 0x7f, 0x01, 0x41, 0x00, 0x0b]);

    let first_defined = 7usize;
    let mut exports = vec![0x05];
    for (field, index) in [
        ("game_abi_version", first_defined),
        ("game_init", first_defined + 1),
        ("game_tick", first_defined + 2),
        ("game_suspend", first_defined + 3),
        ("game_resume", first_defined + 4),
    ] {
        name(&mut exports, field);
        exports.push(0x00);
        leb(&mut exports, index);
    }
    section(&mut module, 7, &exports);

    let tick = [
        0x41, 0x00, 0x23, 0x00, 0x36, 0x02, 0x00, // memory[0] = state
        0x41, 0x04, 0x10, 0x02, 0x36, 0x02, 0x00, // memory[4] = rng
        0x41, 0x00, 0x41, 0x08, 0x10, 0x03, 0x1a, // render(memory[0..8])
        0x10, 0x00, 0x24, 0x00, // state = input_bits()
        0x41, 0x00, 0x0b,
    ];
    let suspend = [
        0x41, 0x00, 0x23, 0x00, 0x36, 0x02, 0x00, 0x41, 0x00, 0x41, 0x04, 0x10, 0x05, 0x1a, 0x41,
        0x00, 0x0b,
    ];
    let resume = [
        0x41, 0x00, 0x41, 0x04, 0x10, 0x06, 0x1a, 0x41, 0x00, 0x28, 0x02, 0x00, 0x24, 0x00, 0x41,
        0x00, 0x0b,
    ];
    let functions = [
        body(&[0x41, 0x01, 0x0b]),
        body(&[0x41, 0x00, 0x0b]),
        body(&tick),
        body(&suspend),
        body(&resume),
    ];
    let mut code = vec![0x05];
    for function in &functions {
        leb(&mut code, function.len());
        code.extend_from_slice(function);
    }
    section(&mut module, 10, &code);
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

fn indexed2d_frame() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TAI2");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0xff00_00ffu32.to_le_bytes());
    bytes.extend_from_slice(&0xff00_ff00u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes
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
    let stats = runtime.last_execution_stats();
    assert_eq!(stats.lifecycle, GameLifecycle::Tick);
    assert!(stats.wasm_steps > 0 && stats.wasm_steps < Limits::default().max_steps);
    assert!(stats.peak_call_depth > 0);
    assert!(stats.peak_call_depth <= Limits::default().max_call_depth);
    assert!(stats.peak_activation_slots > 0);
    assert!(stats.peak_activation_slots <= Limits::default().max_activation_slots);
    assert_eq!(stats.memory_pages, 1);
    assert_eq!(stats.table_elements, 0);
    assert_eq!(stats.native_calls, 0);
    assert_eq!((stats.render_bytes, stats.audio_bytes), (3, 2));
    assert_eq!(stats.state_bytes, 0);
}

#[test]
fn tick_into_recycles_bounded_frame_storage() {
    let wasm = game_module(&all_imports(), 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let mut runtime = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 0x1234_5678),
        "load reusable-frame cartridge",
    );
    let mut frame = GameFrame::default();
    must_ok(
        runtime.tick_into(
            GameInput {
                buttons: 0,
                clock_ms: 16,
            },
            &mut frame,
        ),
        "first reusable tick",
    );
    assert_eq!(frame.render, [1, 2, 3]);
    assert_eq!(frame.audio, [4, 5]);
    let render = (frame.render.as_ptr(), frame.render.capacity());
    let audio = (frame.audio.as_ptr(), frame.audio.capacity());

    must_ok(
        runtime.tick_into(
            GameInput {
                buttons: 0,
                clock_ms: 32,
            },
            &mut frame,
        ),
        "second reusable tick",
    );
    assert_eq!((frame.render.as_ptr(), frame.render.capacity()), render);
    assert_eq!((frame.audio.as_ptr(), frame.audio.capacity()), audio);

    assert!(matches!(
        runtime.tick_into(
            GameInput {
                buttons: 1 << 31,
                clock_ms: 48,
            },
            &mut frame,
        ),
        Err(WasmError::Trap("invalid game input"))
    ));
    assert!(frame.render.is_empty() && frame.audio.is_empty());
    assert_eq!((frame.render.as_ptr(), frame.render.capacity()), render);
    assert_eq!((frame.audio.as_ptr(), frame.audio.capacity()), audio);
    assert!(!runtime.is_failed());
}

#[test]
fn execution_stats_are_deterministic_and_cover_guest_host_resources() {
    let wasm = game_module(&all_imports(), 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let open = || {
        must_ok(
            GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 0x1234_5678),
            "open measured game",
        )
    };
    let mut first = open();
    let mut second = open();
    assert_eq!(first.last_execution_stats(), second.last_execution_stats());
    assert_eq!(first.last_execution_stats().lifecycle, GameLifecycle::Init);

    let input = GameInput {
        buttons: 0b101,
        clock_ms: 16,
    };
    let first_frame = must_ok(first.tick(input), "first measured tick");
    let second_frame = must_ok(second.tick(input), "second measured tick");
    assert_eq!(first_frame.render, second_frame.render);
    assert_eq!(first_frame.audio, second_frame.audio);
    let stats = first.last_execution_stats();
    assert_eq!(stats, second.last_execution_stats());
    assert_eq!(stats.lifecycle, GameLifecycle::Tick);
    assert!(stats.wasm_steps > 0 && stats.wasm_steps < Limits::default().max_steps);
    assert_eq!((stats.memory_pages, stats.table_elements), (1, 0));
    assert_eq!(stats.native_calls, 0);
    assert_eq!((stats.render_bytes, stats.audio_bytes), (3, 2));
    assert_eq!(stats.state_bytes, 0);
}

#[test]
fn manifest_authoring_preserves_standard_wasm_and_rejects_rewriting() {
    let mut imports = all_imports().to_vec();
    imports.push(("fan:physics/v1", "step_world", 1));
    imports.push(("fan:audio/v2", "mix_bus", 1));
    let manifested = game_module(&imports, 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let raw_wasm = without_leading_manifest(&manifested);
    let manifest = CartridgeManifest {
        game_id: "fan.converter-proof".into(),
        game_version: "2.1.0".into(),
        abi_version: 1,
        state_version: 7,
        capabilities: vec!["fan:audio/v2".into(), "fan:physics/v1".into()],
    };

    let cartridge = must_ok(
        manifest.append_to_wasm(&raw_wasm),
        "append canonical manifest",
    );
    assert!(cartridge.starts_with(&raw_wasm));
    assert_eq!(
        must_ok(
            manifest.append_to_wasm(&raw_wasm),
            "repeat canonical manifest authoring"
        ),
        cartridge,
        "the same producer bytes and manifest must be reproducible"
    );
    let descriptor = must_ok(
        CartridgeDescriptor::inspect(&cartridge, Limits::default()),
        "inspect authored cartridge",
    );
    assert!(descriptor.manifest == manifest);
    assert_eq!(descriptor.imports.len(), imports.len());
    assert!(matches!(
        manifest.append_to_wasm(&cartridge),
        Err(WasmError::Decode("game manifest already exists"))
    ));

    let noncanonical = CartridgeManifest {
        capabilities: vec!["fan:physics/v2".into(), "fan:physics/v1".into()],
        ..manifest
    };
    assert!(matches!(
        noncanonical.append_to_wasm(&raw_wasm),
        Err(WasmError::Decode("invalid game capability"))
    ));
}

#[test]
fn converter_cli_derives_native_capabilities_and_publishes_once() {
    let mut imports = all_imports().to_vec();
    imports.push(("fan:physics/v1", "step_world", 1));
    imports.push(("fan:audio/v2", "mix_bus", 1));
    let manifested = game_module(&imports, 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let raw_wasm = without_leading_manifest(&manifested);
    let directory = tempfile::tempdir().expect("temporary converter directory");
    let input = directory.path().join("producer.wasm");
    let output = directory.path().join("fan-game-3.0.0.wasm");
    std::fs::write(&input, &raw_wasm).expect("write producer WASM");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_tinyvm"))
            .args([
                "cartridge",
                "attach-manifest",
                input.to_str().expect("input path"),
                output.to_str().expect("output path"),
                "fan.cli-proof",
                "3.0.0",
                "1",
                "9",
            ])
            .output()
            .expect("run converter CLI")
    };
    let first = run();
    assert!(
        first.status.success(),
        "converter failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        String::from_utf8_lossy(&first.stdout)
            .contains("native_capabilities=fan:audio/v2,fan:physics/v1")
    );
    let cartridge = std::fs::read(&output).expect("read manifested output");
    assert!(cartridge.starts_with(&raw_wasm));
    let descriptor = must_ok(
        CartridgeDescriptor::inspect(&cartridge, Limits::default()),
        "inspect CLI cartridge",
    );
    assert_eq!(descriptor.manifest.game_id, "fan.cli-proof");
    assert_eq!(descriptor.manifest.state_version, 9);
    assert_eq!(
        descriptor.manifest.capabilities,
        ["fan:audio/v2", "fan:physics/v1"]
    );

    let second = run();
    assert!(
        !second.status.success(),
        "converter must not overwrite output"
    );
    assert_eq!(
        std::fs::read(&output).expect("reread output"),
        cartridge,
        "failed overwrite must preserve the first artifact"
    );

    let existing_input = directory.path().join("already-manifested.wasm");
    let refused_output = directory.path().join("must-not-exist.wasm");
    std::fs::write(&existing_input, &cartridge).expect("write existing cartridge");
    let refused = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "cartridge",
            "attach-manifest",
            existing_input.to_str().expect("existing input path"),
            refused_output.to_str().expect("refused output path"),
            "fan.cli-proof",
            "3.0.1",
            "1",
            "9",
        ])
        .output()
        .expect("run duplicate-manifest refusal");
    assert!(!refused.status.success());
    assert!(!refused_output.exists());

    let nongame_input = directory.path().join("ordinary-module.wasm");
    let nongame_output = directory.path().join("ordinary-module-cartridge.wasm");
    std::fs::write(&nongame_input, b"\0asm\x01\0\0\0").expect("write ordinary WASM");
    let nongame = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "cartridge",
            "attach-manifest",
            nongame_input.to_str().expect("ordinary input path"),
            nongame_output.to_str().expect("ordinary output path"),
            "fan.not-a-game",
            "1.0.0",
            "1",
            "1",
        ])
        .output()
        .expect("run non-game refusal");
    assert!(!nongame.status.success());
    assert!(!nongame_output.exists());
}

#[test]
fn host_profile_cli_publishes_inspects_and_checks_without_execution() {
    let directory = tempfile::tempdir().expect("temporary host profile directory");
    let profile = directory.path().join("ios-build.tahost");
    let cartridge = directory.path().join("core-only.wasm");
    std::fs::write(&cartridge, game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]))
        .expect("write core-only cartridge");

    let created = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "host-profile",
            "default",
            profile.to_str().expect("profile output path"),
        ])
        .output()
        .expect("create default host profile");
    assert!(
        created.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(String::from_utf8_lossy(&created.stdout).contains("schema=tinyarcade-host-profile-v1"));
    let original = std::fs::read(&profile).expect("read host profile");
    assert_eq!(&original[..4], b"TAH1");

    let overwrite = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "host-profile",
            "default",
            profile.to_str().expect("profile output path"),
        ])
        .output()
        .expect("refuse profile overwrite");
    assert!(!overwrite.status.success());
    assert_eq!(std::fs::read(&profile).expect("reread profile"), original);

    let inspected = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "host-profile",
            "inspect",
            profile.to_str().expect("profile path"),
        ])
        .output()
        .expect("inspect host profile");
    assert!(inspected.status.success());
    assert!(String::from_utf8_lossy(&inspected.stdout).contains("native_functions=0"));

    let checked = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "cartridge",
            "check-profile",
            cartridge.to_str().expect("cartridge path"),
            profile.to_str().expect("profile path"),
        ])
        .output()
        .expect("check cartridge host profile");
    assert!(
        checked.status.success(),
        "profile check failed: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(
        String::from_utf8_lossy(&checked.stdout)
            .contains("OK: cartridge is statically compatible with exact host profile")
    );

    let native_cartridge = directory.path().join("native.wasm");
    std::fs::write(
        &native_cartridge,
        game_module(
            &[("fan:physics/v1", "step_world", 1)],
            1,
            &[0x41, 0x00, 0x0b],
            &[],
        ),
    )
    .expect("write native cartridge");
    let rejected = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args([
            "cartridge",
            "check-profile",
            native_cartridge.to_str().expect("native cartridge path"),
            profile.to_str().expect("profile path"),
        ])
        .output()
        .expect("reject unavailable native import");
    assert!(!rejected.status.success());
    let rejected_stdout = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        rejected_stdout.contains("compatibility_issues=1")
            && rejected_stdout.contains(
                "issue=fan:physics/v1.step_world reason=missing required_params=2 required_results=1"
            )
            && rejected_stdout.contains("compatible=false")
    );
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("host profile has incompatible native imports")
    );
}

#[test]
fn ordinary_ticks_reject_unknown_buttons_and_backwards_time_without_latching() {
    let wasm = game_module(&all_imports(), 1, &tick_with_outputs(3), &[1, 2, 3, 4, 5]);
    let mut runtime = must_ok(
        GameRuntime::from_private_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        "load input validation cartridge",
    );
    must_ok(
        runtime.tick(GameInput {
            buttons: 1 << 4,
            clock_ms: 100,
        }),
        "first valid input",
    );
    for invalid in [
        GameInput {
            buttons: 1 << 31,
            clock_ms: 101,
        },
        GameInput {
            buttons: 0,
            clock_ms: 99,
        },
    ] {
        assert!(matches!(
            runtime.tick(invalid),
            Err(WasmError::Trap("invalid game input"))
        ));
        assert!(!runtime.is_failed());
    }
    must_ok(
        runtime.tick(GameInput {
            buttons: 0,
            clock_ms: 100,
        }),
        "same clock remains valid after rejected host input",
    );
}

#[test]
fn successful_resume_starts_a_new_host_clock_validation_epoch() {
    let wasm = stateful_game_module(1, "test.clock-epoch");
    let mut runtime = must_ok(
        GameRuntime::from_private_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        "load clock epoch cartridge",
    );
    must_ok(
        runtime.tick(GameInput {
            buttons: 0,
            clock_ms: 10_000,
        }),
        "tick before suspend",
    );
    let snapshot = must_ok(runtime.suspend(), "suspend clock epoch cartridge");
    must_ok(runtime.resume(&snapshot), "resume clock epoch cartridge");
    must_ok(
        runtime.tick(GameInput {
            buttons: 0,
            clock_ms: 32,
        }),
        "tick at restored app clock",
    );
}

#[test]
fn descriptor_validates_without_executing_or_granting_native_imports() {
    let wasm = game_module(
        &[("fan:physics/v1", "step_world", 1), (CORE, "input_bits", 0)],
        1,
        &[0x41, 0x00, 0x0b],
        &[],
    );
    let descriptor = must_ok(
        CartridgeDescriptor::inspect(&wasm, Limits::default()),
        "inspect native cartridge",
    );
    assert_eq!(descriptor.manifest.game_id, "test.game");
    assert_eq!(descriptor.manifest.game_version, "1.0.0");
    assert_eq!(descriptor.manifest.capabilities, ["fan:physics/v1"]);
    assert_eq!(descriptor.imports.len(), 2);
    assert_eq!(descriptor.imports[0].module, "fan:physics/v1");
    assert_eq!(descriptor.imports[0].field, "step_world");
    assert_eq!(
        (
            descriptor.imports[0].n_params,
            descriptor.imports[0].n_results
        ),
        (2, 1)
    );

    assert!(matches!(
        GameRuntime::from_private_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Trap("game import is not allowed"))
    ));
}

#[test]
fn standard_core_only_cartridge_drives_an_indexed2d_frame() {
    let pixels = indexed2d_frame();
    let undeclared = game_module(
        &[(CORE, "submit_render", 1)],
        1,
        &[0x41, 0x00, 0x41, 0x1a, 0x10, 0x00, 0x1a, 0x41, 0x00, 0x0b],
        &pixels,
    );
    let mut undeclared_runtime = must_ok(
        GameRuntime::from_private_bytes(&undeclared, Limits::default(), GameLimits::default(), 1),
        "load undeclared indexed2d cartridge",
    );
    assert!(matches!(
        undeclared_runtime.tick(GameInput::default()),
        Err(WasmError::Trap("indexed2d capability not declared"))
    ));
    assert!(undeclared_runtime.is_failed());

    let wasm = game_module(
        &[(CORE, "indexed2d_version", 0), (CORE, "submit_render", 1)],
        1,
        &[
            0x10, 0x00, 0x1a, 0x41, 0x00, 0x41, 0x1a, 0x10, 0x01, 0x1a, 0x41, 0x00, 0x0b,
        ],
        &pixels,
    );
    let mut runtime = must_ok(
        GameRuntime::from_private_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        "load indexed2d cartridge",
    );
    let output = must_ok(
        runtime.tick(GameInput::default()),
        "tick indexed2d cartridge",
    );
    match must_ok(
        RenderFrame::decode(&output.render),
        "decode indexed2d output",
    ) {
        RenderFrame::Indexed2d(frame) => {
            assert_eq!((frame.width, frame.height), (2, 1));
            assert_eq!(frame.pixels(), &[0, 1]);
        }
        RenderFrame::Grid3d(_) => panic!("indexed2d cartridge decoded as grid3d"),
    }
}

#[test]
fn core_v1_media_versions_are_explicit_and_format_matched() {
    for (magic, submit, version, undeclared_trap, render) in [
        (
            b"TAG3".as_slice(),
            "submit_render",
            "grid3d_version",
            "grid3d capability not declared",
            true,
        ),
        (
            b"TAT1".as_slice(),
            "submit_audio",
            "tones_version",
            "tones capability not declared",
            false,
        ),
    ] {
        let tick = [0x41, 0x00, 0x41, 0x04, 0x10, 0x00, 0x1a, 0x41, 0x00, 0x0b];
        let undeclared = game_module(&[(CORE, submit, 1)], 1, &tick, magic);
        let mut runtime = must_ok(
            GameRuntime::from_private_bytes(
                &undeclared,
                Limits::default(),
                GameLimits::default(),
                1,
            ),
            "load undeclared media cartridge",
        );
        assert!(matches!(
            runtime.tick(GameInput::default()),
            Err(WasmError::Trap(message)) if message == undeclared_trap
        ));

        let tick = [
            0x10, 0x00, 0x1a, 0x41, 0x00, 0x41, 0x04, 0x10, 0x01, 0x1a, 0x41, 0x00, 0x0b,
        ];
        let declared = game_module(&[(CORE, version, 0), (CORE, submit, 1)], 1, &tick, magic);
        let mut runtime = must_ok(
            GameRuntime::from_private_bytes(&declared, Limits::default(), GameLimits::default(), 1),
            "load declared media cartridge",
        );
        let frame = must_ok(runtime.tick(GameInput::default()), "submit declared media");
        assert_eq!(if render { &frame.render } else { &frame.audio }, magic);
    }
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
            buttons: 0x0000_0105,
            clock_ms: 1234,
        }),
        "deterministic tick",
    );
    assert_eq!(&frame.render[0..4], &0x0000_0105u32.to_le_bytes());
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
            registry,
        ),
        "load game with native module",
    );
    must_ok(runtime.tick(GameInput::default()), "tick native module");
    assert_eq!(calls.get(), 1);
}

#[test]
fn in_place_native_module_receives_exact_bounded_result_slice() {
    // game_tick returns zero only when the native result is exactly 42.
    let wasm = game_module(
        &[("fan:physics/v1", "step", 1)],
        1,
        &[0x41, 0x14, 0x41, 0x16, 0x10, 0x00, 0x41, 0x2a, 0x47, 0x0b],
        &[],
    );
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let mut registry = NativeModuleRegistry::new();
    must_ok(
        registry.register_in_place("fan:physics/v1", "step", 2, 1, move |args, results, _| {
            assert_eq!(args, [20, 22]);
            assert_eq!(results.len(), 1);
            observed.set(observed.get() + 1);
            results[0] = args[0] + args[1];
            Ok(())
        }),
        "register in-place native module",
    );
    let mut runtime = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            registry,
        ),
        "load game with in-place native module",
    );
    must_ok(
        runtime.tick(GameInput::default()),
        "tick in-place native module",
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn host_profile_is_canonical_and_checks_exact_standard_imports() {
    let wasm = game_module(
        &[("fan:physics/v1", "step_world", 1)],
        1,
        &[0x41, 0x00, 0x0b],
        &[],
    );
    let limits = Limits {
        max_table_elems: 32,
        max_memory_pages: 4,
        max_steps: 250_000,
        ..Limits::default()
    };
    let game_limits = GameLimits {
        max_render_bytes: 20 * 1024,
        max_audio_bytes: 4 * 1024,
        max_state_bytes: 32 * 1024,
    };
    let mut profile = must_ok(HostProfileV1::new(limits, game_limits), "new host profile");
    must_ok(
        profile.add_native_function("fan:physics/v1", "step_world", 2, 1, 8),
        "add native profile function",
    );
    let encoded = must_ok(profile.encode(), "encode host profile");
    assert_eq!(u16::from_le_bytes([encoded[4], encoded[5]]), 2);
    assert_eq!(u16::from_le_bytes([encoded[6], encoded[7]]), 64);
    let decoded = must_ok(HostProfileV1::decode(&encoded), "decode host profile");
    assert_eq!(must_ok(decoded.encode(), "re-encode host profile"), encoded);
    assert_eq!(decoded.native_functions().len(), 1);
    assert_eq!(decoded.native_functions()[0].max_calls_per_lifecycle, 8);
    assert_eq!(decoded.vm_limits().max_call_depth, limits.max_call_depth);
    assert_eq!(
        decoded.vm_limits().max_activation_slots,
        limits.max_activation_slots
    );
    must_ok(
        decoded.inspect_cartridge(&wasm),
        "profile accepts exact standard import",
    );

    let missing = must_ok(HostProfileV1::new(limits, game_limits), "empty profile");
    let report = must_ok(
        missing.compatibility_report(&wasm),
        "report missing profile function",
    );
    assert!(!report.is_compatible());
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].module, "fan:physics/v1");
    assert_eq!(report.issues[0].field, "step_world");
    assert_eq!(report.issues[0].required_params, 2);
    assert_eq!(report.issues[0].required_results, 1);
    assert_eq!(report.issues[0].available_params, None);
    assert_eq!(report.issues[0].available_results, None);
    assert!(matches!(
        missing.inspect_cartridge(&wasm),
        Err(WasmError::Trap("host profile native import unavailable"))
    ));
    let mut wrong = must_ok(HostProfileV1::new(limits, game_limits), "wrong profile");
    must_ok(
        wrong.add_native_function("fan:physics/v1", "step_world", 1, 1, 8),
        "add wrong signature",
    );
    let report = must_ok(
        wrong.compatibility_report(&wasm),
        "report native signature mismatch",
    );
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].required_params, 2);
    assert_eq!(report.issues[0].available_params, Some(1));
    assert_eq!(report.issues[0].available_results, Some(1));
    assert!(matches!(
        wrong.inspect_cartridge(&wasm),
        Err(WasmError::Trap("host profile native import unavailable"))
    ));

    let mut two_page_wasm = wasm.clone();
    let memory = two_page_wasm
        .windows(5)
        .position(|bytes| bytes == [5, 3, 1, 0, 1])
        .expect("test module memory section");
    two_page_wasm[memory + 4] = 2;
    let tight_limits = Limits {
        max_memory_pages: 1,
        ..limits
    };
    let mut tight = must_ok(
        HostProfileV1::new(tight_limits, game_limits),
        "tight memory profile",
    );
    must_ok(
        tight.add_native_function("fan:physics/v1", "step_world", 2, 1, 8),
        "tight profile native function",
    );
    assert!(matches!(
        tight.inspect_cartridge(&two_page_wasm),
        Err(WasmError::Trap("memory size"))
    ));

    let mut duplicate = encoded.clone();
    duplicate[50..52].copy_from_slice(&2u16.to_le_bytes());
    duplicate.extend_from_slice(&encoded[64..]);
    assert!(matches!(
        HostProfileV1::decode(&duplicate),
        Err(WasmError::Decode("host profile is not canonical"))
    ));

    let mut legacy = encoded[..52].to_vec();
    legacy[4..6].copy_from_slice(&1u16.to_le_bytes());
    legacy[6..8].copy_from_slice(&56u16.to_le_bytes());
    legacy.extend_from_slice(&0u32.to_le_bytes());
    legacy.extend_from_slice(&encoded[64..]);
    let legacy = must_ok(HostProfileV1::decode(&legacy), "decode schema-1 profile");
    assert_eq!(legacy.vm_limits().max_call_depth, 512);
    assert_eq!(legacy.vm_limits().max_activation_slots, 1 << 20);

    let mut trailing = encoded;
    trailing.push(0);
    assert!(matches!(
        HostProfileV1::decode(&trailing),
        Err(WasmError::Decode("trailing host profile bytes"))
    ));
}

#[cfg(feature = "replay")]
#[test]
fn replay_preserves_versioned_native_import_registration_boundary() {
    let wasm = native_replay_module();
    let calls = Rc::new(Cell::new(0));
    let open = || {
        let observed = calls.clone();
        let mut registry = NativeModuleRegistry::new();
        must_ok(
            registry.register("fan:physics/v1", "step", 0, 1, move |_, _| {
                observed.set(observed.get() + 1);
                Ok(vec![0])
            }),
            "register replay native module",
        );
        must_ok(
            GameRuntime::from_bytes_with_registry(
                &wasm,
                Limits::default(),
                GameLimits::default(),
                7,
                registry,
            ),
            "open native replay runtime",
        )
    };
    let mut recorded = open();
    let mut recorder = must_ok(
        ReplayRecorderV1::start_runtime(&mut recorded),
        "start native replay",
    );
    for clock_ms in [0, 16, 32, 48] {
        must_ok(
            recorder.record_tick(
                &mut recorded,
                GameInput {
                    buttons: 0,
                    clock_ms,
                },
            ),
            "record native replay tick",
        );
    }
    let encoded = must_ok(recorder.finish(), "finish native replay");
    let trace = must_ok(ReplayTraceV1::decode(&encoded), "decode native replay");
    let mut replayed = open();
    must_ok(
        trace.replay_loaded(&mut replayed, |_, _| Ok(())),
        "verify native replay",
    );
    assert_eq!(calls.get(), 8);

    let missing_registry =
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 7);
    assert!(matches!(
        missing_registry,
        Err(WasmError::Trap("game import is not allowed"))
    ));
}

fn register_test_resource_callbacks(
    registry: &mut NativeModuleRegistry,
    resources: &Rc<RefCell<HostResourceTable<i32>>>,
) {
    let create_resources = resources.clone();
    let read_resources = resources.clone();
    let close_resources = resources.clone();
    must_ok(
        registry.register("fan:texture/v1", "create", 0, 1, move |_, _| {
            let handle = create_resources
                .borrow_mut()
                .insert(41)
                .map_err(|_| WasmError::Trap("native resource table full"))?;
            Ok(vec![handle.as_i32()])
        }),
        "register resource create",
    );
    must_ok(
        registry.register("fan:texture/v1", "read", 1, 1, move |args, _| {
            let handle = GuestResourceHandle::from_i32(args[0])
                .ok_or(WasmError::Trap("native resource handle"))?;
            let value = *read_resources
                .borrow()
                .get(handle)
                .map_err(|_| WasmError::Trap("stale native resource handle"))?;
            Ok(vec![value])
        }),
        "register resource read",
    );
    must_ok(
        registry.register("fan:texture/v1", "close", 1, 1, move |args, _| {
            let handle = GuestResourceHandle::from_i32(args[0])
                .ok_or(WasmError::Trap("native resource handle"))?;
            close_resources
                .borrow_mut()
                .remove(handle)
                .map_err(|_| WasmError::Trap("stale native resource handle"))?;
            Ok(vec![0])
        }),
        "register resource close",
    );
}

#[test]
fn native_module_can_own_a_resource_behind_a_generation_checked_guest_handle() {
    let bare = wat::parse_str(
        r#"(module
            (import "fan:texture/v1" "create" (func $create (result i32)))
            (import "fan:texture/v1" "read" (func $read (param i32) (result i32)))
            (import "fan:texture/v1" "close" (func $close (param i32) (result i32)))
            (import "tinyarcade:core/v1" "save_state"
              (func $save_state (param i32 i32) (result i32)))
            (memory 1)
            (global $handle (mut i32) (i32.const 0))
            (func (export "game_abi_version") (result i32) (i32.const 1))
            (func (export "game_init") (result i32)
              call $create
              global.set $handle
              i32.const 0)
            (func (export "game_tick") (result i32)
              global.get $handle
              call $read
              i32.const 41
              i32.ne
              if (result i32)
                i32.const 9
              else
                global.get $handle
                call $close
                drop
                i32.const 0
              end)
            (func (export "game_suspend") (result i32)
              i32.const 0
              i32.const 0
              call $save_state
              drop
              i32.const 0)
            (func (export "game_resume") (result i32) (i32.const 0)))"#,
    )
    .expect("compile resource-handle cartridge");
    let wasm = must_ok(
        CartridgeManifest {
            game_id: "test.resource-handle".to_owned(),
            game_version: "1.0.0".to_owned(),
            abi_version: 1,
            state_version: 1,
            capabilities: vec!["fan:texture/v1".to_owned()],
        }
        .append_to_wasm(&bare),
        "attach resource-handle manifest",
    );

    let mut allocator = agenterm_tinyvm::ResourceDomainAllocator::new();
    let mut registry = NativeModuleRegistry::new();
    let remaining = allocator.remaining();
    assert!(matches!(
        registry.resource_table::<i32>(
            "fan:invalid/v1",
            agenterm_tinyvm::MAX_RESOURCE_SLOTS + 1,
            &mut allocator
        ),
        Err(WasmError::Trap("invalid native resource table limit"))
    ));
    assert_eq!(allocator.remaining(), remaining);
    let texture_table = must_ok(
        registry.resource_table("fan:texture/v1", 1, &mut allocator),
        "create texture resource table",
    );
    let resource_domain = texture_table.domain();
    let remaining = allocator.remaining();
    assert!(matches!(
        registry.resource_table::<i32>("fan:texture/v1", 1, &mut allocator),
        Err(WasmError::Trap("native resource table already assigned"))
    ));
    assert_eq!(allocator.remaining(), remaining);
    let audio_table = must_ok(
        registry.resource_table::<i32>("fan:audio/v1", 1, &mut allocator),
        "create audio resource table",
    );
    let audio_domain = audio_table.domain();
    assert_ne!(audio_domain, resource_domain);
    assert_eq!(
        registry.assigned_resource_table_domain("fan:texture/v1"),
        Some(resource_domain)
    );
    assert_eq!(
        registry.assigned_resource_table_domain("fan:missing/v1"),
        None
    );
    assert!(
        must_ok(
            registry.host_profile(Limits::default(), GameLimits::default()),
            "profile domain-only registry"
        )
        .native_functions()
        .is_empty(),
        "a resource domain is not an advertised function"
    );

    let mut automatic = NativeModuleRegistry::new();
    let remaining = allocator.remaining();
    must_ok(
        automatic.register("fan:auto/v1", "ping", 0, 1, |_, _| Ok(vec![0])),
        "register automatically assigned module",
    );
    assert_eq!(
        automatic.assigned_resource_table_domain("fan:auto/v1"),
        None
    );
    assert_eq!(allocator.remaining(), remaining);

    let mut replacement_registry = NativeModuleRegistry::new();
    let mut replacement_table = must_ok(
        replacement_registry.resource_table("fan:texture/v1", 1, &mut allocator),
        "create replacement runtime table",
    );
    assert_ne!(replacement_table.domain(), resource_domain);

    let resources = Rc::new(RefCell::new(texture_table));
    let stale = resources
        .borrow_mut()
        .insert(99)
        .expect("create old runtime token");
    let replacement = replacement_table
        .insert(100)
        .expect("create replacement runtime token");
    assert_ne!(stale, replacement);
    assert!(replacement_table.get(stale).is_err());
    replacement_table
        .remove(replacement)
        .expect("remove replacement setup resource");
    resources
        .borrow_mut()
        .remove(stale)
        .expect("remove setup resource");
    register_test_resource_callbacks(&mut registry, &resources);

    let mut runtime = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            registry,
        ),
        "open resource-handle cartridge",
    );
    assert_eq!(resources.borrow().len(), 1);
    must_ok(runtime.tick(GameInput::default()), "use and close resource");
    assert!(resources.borrow().is_empty());
    must_ok(runtime.suspend(), "snapshot after resource quiescence");

    let replacement_resources = Rc::new(RefCell::new(replacement_table));
    register_test_resource_callbacks(&mut replacement_registry, &replacement_resources);
    let mut nonquiescent = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            replacement_registry,
        ),
        "open nonquiescent resource cartridge",
    );
    assert_eq!(replacement_resources.borrow().len(), 1);
    assert!(matches!(
        nonquiescent.suspend(),
        Err(WasmError::Trap("native resources not quiescent"))
    ));
    assert!(nonquiescent.tick(GameInput::default()).is_err());
    replacement_resources.borrow_mut().clear();
    assert!(replacement_resources.borrow().is_empty());
}

#[test]
fn native_completion_queue_bounds_identity_items_and_reserved_bytes() {
    let mut allocator = agenterm_tinyvm::ResourceDomainAllocator::new();
    let mut registry = NativeModuleRegistry::new();
    let mut queue = must_ok(
        registry.completion_queue("fan:network/v1", 2, 8, &mut allocator),
        "create completion queue",
    );

    let first = queue.begin(5).expect("reserve first completion");
    assert!(matches!(
        queue.begin(4),
        Err(CompletionError::ByteBudgetExceeded)
    ));
    let second = queue.begin(3).expect("reserve second completion");
    assert!(matches!(queue.begin(0), Err(CompletionError::Full)));
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.reserved_bytes(), 8);
    assert!(matches!(
        queue.poll(first).expect("poll pending completion"),
        CompletionPoll::Pending
    ));
    assert!(matches!(queue.take(first), Err(CompletionError::NotReady)));

    let rejected = queue
        .try_complete(first, 7, vec![1, 2, 3, 4, 5, 6])
        .expect_err("oversized completion must retain ownership");
    assert_eq!(rejected.error, CompletionError::PayloadTooLarge);
    assert_eq!(rejected.payload, [1, 2, 3, 4, 5, 6]);
    queue
        .try_complete(first, 7, vec![1, 2, 3, 4])
        .expect("complete reserved request");
    match queue.poll(first).expect("poll ready completion") {
        CompletionPoll::Ready { status, payload } => {
            assert_eq!(status, 7);
            assert_eq!(payload, [1, 2, 3, 4]);
        }
        CompletionPoll::Pending => panic!("completion remained pending"),
    }
    let duplicate = queue
        .try_complete(first, 8, vec![9])
        .expect_err("duplicate completion must fail");
    assert_eq!(duplicate.error, CompletionError::AlreadyCompleted);
    assert_eq!(duplicate.payload, [9]);

    let completed = queue.take(first).expect("take ready completion");
    assert_eq!(completed.status, 7);
    assert_eq!(completed.payload, [1, 2, 3, 4]);
    assert_eq!(queue.reserved_bytes(), 3);
    assert!(matches!(
        queue.poll(first),
        Err(CompletionError::StaleHandle)
    ));
    queue.cancel(second).expect("cancel pending completion");
    assert!(queue.is_empty());
    assert_eq!(queue.reserved_bytes(), 0);

    let mut replacement = NativeModuleRegistry::new();
    let mut replacement_queue = must_ok(
        replacement.completion_queue("fan:network/v1", 1, 8, &mut allocator),
        "create replacement completion queue",
    );
    let replacement_handle = replacement_queue
        .begin(1)
        .expect("reserve replacement request");
    assert_ne!(replacement_handle.domain(), first.domain());
    let stale = replacement_queue
        .try_complete(first, 0, vec![1])
        .expect_err("old runtime completion id must fail");
    assert_eq!(stale.error, CompletionError::StaleHandle);
    assert_eq!(stale.payload, [1]);
    replacement_queue
        .cancel(replacement_handle)
        .expect("cancel replacement request");
}

#[test]
fn pending_native_completion_prevents_portable_snapshot() {
    let wasm = game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]);
    let mut allocator = agenterm_tinyvm::ResourceDomainAllocator::new();
    let mut registry = NativeModuleRegistry::new();
    let queue = Rc::new(RefCell::new(must_ok(
        registry.completion_queue("fan:network/v1", 1, 16, &mut allocator),
        "create tracked completion queue",
    )));
    let request = queue
        .borrow_mut()
        .begin(16)
        .expect("start native async request");
    let mut runtime = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            registry,
        ),
        "open runtime with completion queue",
    );
    assert!(matches!(
        runtime.suspend(),
        Err(WasmError::Trap("native resources not quiescent"))
    ));
    assert!(runtime.is_failed());
    queue
        .borrow_mut()
        .cancel(request)
        .expect("cancel failed request");
    assert!(queue.borrow().is_empty());
}

#[test]
fn native_dispatch_quota_is_charged_before_callback_and_resets_per_lifecycle() {
    let wasm = game_module(
        &[("fan:physics/v1", "step", 0)],
        1,
        &[0x10, 0, 0x1a, 0x10, 0, 0x1a, 0x41, 0, 0x0b],
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
        "register one-call native module",
    );
    let mut runtime = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            registry,
        ),
        "load one-call native module",
    );
    assert!(matches!(
        runtime.tick(GameInput::default()),
        Err(WasmError::Trap("native capability call budget"))
    ));
    assert_eq!(
        calls.get(),
        1,
        "over-budget dispatch must not call app code"
    );
    assert!(runtime.is_failed());
    let trapped = runtime.last_execution_stats();
    assert_eq!(trapped.lifecycle, GameLifecycle::Tick);
    assert!(trapped.wasm_steps > 0);
    assert_eq!(trapped.native_calls, 1);

    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let mut registry = NativeModuleRegistry::new();
    must_ok(
        registry.register_with_call_limit("fan:physics/v1", "step", 0, 1, 2, move |_, _| {
            observed.set(observed.get() + 1);
            Ok(vec![0])
        }),
        "register two-call native module",
    );
    let mut runtime = must_ok(
        GameRuntime::from_bytes_with_registry(
            &wasm,
            Limits::default(),
            GameLimits::default(),
            1,
            registry,
        ),
        "load two-call native module",
    );
    must_ok(runtime.tick(GameInput::default()), "first bounded tick");
    must_ok(runtime.tick(GameInput::default()), "second bounded tick");
    assert_eq!(calls.get(), 4, "quota must reset before each game tick");

    for invalid in [0, MAX_NATIVE_CALLS_PER_LIFECYCLE + 1] {
        assert!(matches!(
            NativeModuleRegistry::new().register_with_call_limit(
                "fan:physics/v1",
                "step",
                0,
                1,
                invalid,
                |_, _| Ok(vec![0]),
            ),
            Err(WasmError::Trap("invalid native module registration"))
        ));
    }
}

#[test]
fn native_module_names_are_canonical_and_major_versioned() {
    let mut registry = NativeModuleRegistry::new();
    must_ok(
        registry.register("com.example:physics/v1", "step_world", 2, 1, |_, _| {
            Ok(vec![0])
        }),
        "register reverse-DNS native module",
    );
    for (module, field) in [
        ("Example:physics/v1", "step_world"),
        ("com.example:physics/v01", "step_world"),
        ("com.example:physics/v0", "step_world"),
        ("com.example:physics/v1/extra", "step_world"),
        ("com.example:physics/v1", "StepWorld"),
        ("com.example:physics/v1", "step-world"),
    ] {
        assert!(matches!(
            registry.register(module, field, 0, 0, |_, _| Ok(Vec::new())),
            Err(WasmError::Trap("invalid native module registration"))
        ));
    }
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
                ..GameLimits::default()
            },
            1,
        ),
        "load game",
    );
    assert!(matches!(
        runtime.tick(GameInput::default()),
        Err(WasmError::Trap("game output budget"))
    ));
    assert!(runtime.is_failed());
    assert!(matches!(
        runtime.tick(GameInput::default()),
        Err(WasmError::Trap("game instance failed"))
    ));
}

#[test]
fn suspend_resume_restores_guest_state_and_host_rng() {
    let wasm = stateful_game_module(1, "test.stateful");
    let seed = 0x1234_5678u32;
    let mut rng_one = seed;
    rng_one ^= rng_one << 13;
    rng_one ^= rng_one >> 17;
    rng_one ^= rng_one << 5;
    let mut rng_two = rng_one;
    rng_two ^= rng_two << 13;
    rng_two ^= rng_two >> 17;
    rng_two ^= rng_two << 5;

    let mut first = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), seed),
        "load first stateful game",
    );
    let first_frame = must_ok(
        first.tick(GameInput {
            buttons: 42,
            clock_ms: 1,
        }),
        "first stateful tick",
    );
    assert_eq!(&first_frame.render[0..4], &0u32.to_le_bytes());
    assert_eq!(&first_frame.render[4..8], &rng_one.to_le_bytes());
    let snapshot = must_ok(first.suspend(), "suspend stateful game");
    let suspended = first.last_execution_stats();
    assert_eq!(suspended.lifecycle, GameLifecycle::Suspend);
    assert!(suspended.wasm_steps > 0);
    assert!(suspended.state_bytes > 0);
    assert_eq!((suspended.render_bytes, suspended.audio_bytes), (0, 0));

    let mut restored = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 999),
        "load restored game",
    );
    must_ok(restored.resume(&snapshot), "resume stateful game");
    let resumed = restored.last_execution_stats();
    assert_eq!(resumed.lifecycle, GameLifecycle::Resume);
    assert!(resumed.wasm_steps > 0);
    assert_eq!(resumed.state_bytes, suspended.state_bytes);
    let restored_frame = must_ok(
        restored.tick(GameInput {
            buttons: 7,
            clock_ms: 2,
        }),
        "restored tick",
    );
    assert_eq!(&restored_frame.render[0..4], &42u32.to_le_bytes());
    assert_eq!(&restored_frame.render[4..8], &rng_two.to_le_bytes());
    assert_eq!(restored.manifest().game_id, "test.stateful");
    assert_eq!(restored.manifest().state_version, 1);
}

#[test]
fn snapshot_identity_schema_and_bounds_fail_closed() {
    let wasm = stateful_game_module(1, "test.stateful");
    let mut source = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        "load snapshot source",
    );
    must_ok(
        source.tick(GameInput {
            buttons: 9,
            clock_ms: 0,
        }),
        "prime snapshot source",
    );
    let snapshot = must_ok(source.suspend(), "make snapshot");

    for incompatible in [
        stateful_game_module(2, "test.stateful"),
        stateful_game_module(1, "test.other"),
    ] {
        let mut runtime = must_ok(
            GameRuntime::from_bytes(&incompatible, Limits::default(), GameLimits::default(), 1),
            "load incompatible target",
        );
        assert!(matches!(
            runtime.resume(&snapshot),
            Err(WasmError::Trap("incompatible game snapshot"))
        ));
        assert!(!runtime.is_failed());
    }

    let mut target = must_ok(
        GameRuntime::from_bytes(&wasm, Limits::default(), GameLimits::default(), 1),
        "load truncated target",
    );
    assert!(matches!(
        target.resume(&snapshot[..snapshot.len() - 1]),
        Err(WasmError::Trap("truncated game snapshot"))
    ));

    let mut tight = must_ok(
        GameRuntime::from_bytes(
            &wasm,
            Limits::default(),
            GameLimits {
                max_state_bytes: 3,
                ..GameLimits::default()
            },
            1,
        ),
        "load tight state budget",
    );
    assert!(matches!(
        tight.suspend(),
        Err(WasmError::Trap("game state budget"))
    ));
    assert!(tight.is_failed());
}

#[test]
fn manifest_is_required_before_a_cartridge_can_run() {
    let wasm_without_manifest = b"\0asm\x01\0\0\0";
    assert!(matches!(
        GameRuntime::from_bytes(
            wasm_without_manifest,
            Limits::default(),
            GameLimits::default(),
            1
        ),
        Err(WasmError::Decode("missing game manifest"))
    ));
}

#[test]
fn manifest_capabilities_and_lifecycle_signatures_are_exact() {
    let mut native = game_module(
        &[("fan:physics/v1", "step", 0)],
        1,
        &[0x10, 0x00, 0x1a, 0x41, 0x00, 0x0b],
        &[],
    );
    let manifest_name = b"fan:physics/v1";
    let first = native
        .windows(manifest_name.len())
        .position(|window| window == manifest_name)
        .expect("manifest capability");
    native[first] = b'x';
    let mut registry = NativeModuleRegistry::new();
    must_ok(
        registry.register("fan:physics/v1", "step", 0, 1, |_, _| Ok(vec![0])),
        "register exact native capability",
    );
    assert!(matches!(
        GameRuntime::from_bytes_with_registry(
            &native,
            Limits::default(),
            GameLimits::default(),
            1,
            registry
        ),
        Err(WasmError::Trap("manifest capability mismatch"))
    ));

    let mut wrong_tick = game_module(&[], 1, &[0x41, 0x00, 0x0b], &[]);
    let function_section = [0x03, 0x06, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00];
    let section_start = wrong_tick
        .windows(function_section.len())
        .position(|window| window == function_section)
        .expect("function section");
    wrong_tick[section_start + 5] = 0x02;
    assert!(matches!(
        GameRuntime::from_bytes(&wrong_tick, Limits::default(), GameLimits::default(), 1),
        Err(WasmError::Trap("invalid game lifecycle export"))
    ));
}
