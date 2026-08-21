//! Public black-box ownership for the persistent game-runtime instance.

use agenterm_tinyvm::wasm::WASM_MAX_DECODE_ITEMS;
use agenterm_tinyvm::wasm::WASM_MAX_DEPTH;
use agenterm_tinyvm::wasm::WASM_PAGE_SIZE;
use agenterm_tinyvm::{Limits, Val, WasmError, WasmModule};

// (global (mut i32) (i32.const 0))
// (func (export "tick") (result i32)
//   global.get 0; i32.const 1; i32.add; global.set 0; global.get 0)
const COUNTER_MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // wasm v1
    0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // type () -> i32
    0x03, 0x02, 0x01, 0x00, // one function, type 0
    0x06, 0x06, 0x01, 0x7f, 0x01, 0x41, 0x00, 0x0b, // mutable global
    0x07, 0x08, 0x01, 0x04, b't', b'i', b'c', b'k', 0x00, 0x00, // export
    0x0a, 0x0d, 0x01, 0x0b, 0x00, 0x23, 0x00, 0x41, 0x01, 0x6a, 0x24, 0x00, 0x23, 0x00, 0x0b,
];

// (memory 1 4)
// (func (export "grow") (param i32) (result i32)
//   local.get 0; memory.grow)
const GROW_MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // wasm v1
    0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f, // (i32) -> i32
    0x03, 0x02, 0x01, 0x00, // one function, type 0
    0x05, 0x04, 0x01, 0x01, 0x01, 0x04, // memory min 1, max 4
    0x07, 0x08, 0x01, 0x04, b'g', b'r', b'o', b'w', 0x00, 0x00, // export
    0x0a, 0x08, 0x01, 0x06, 0x00, 0x20, 0x00, 0x40, 0x00, 0x0b,
];

// (memory 3), with no functions. The host limit test must reject this at load.
const MEMORY_MIN_3: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x05, 0x03, 0x01, 0x00, 0x03,
];

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn only_i32(values: &[Val]) -> i32 {
    match values {
        [Val::I32(value)] => *value,
        _ => panic!("expected one i32 result"),
    }
}

#[test]
fn instance_preserves_globals_but_module_calls_stay_fresh() {
    let module = must_ok(WasmModule::from_bytes(COUNTER_MODULE), "load counter");
    assert_eq!(
        only_i32(&must_ok(module.invoke_by_name("tick", &[]), "fresh tick 1")),
        1
    );
    assert_eq!(
        only_i32(&must_ok(module.invoke_by_name("tick", &[]), "fresh tick 2")),
        1
    );

    let module = must_ok(WasmModule::from_bytes(COUNTER_MODULE), "reload counter");
    let mut instance = must_ok(module.instantiate(), "instantiate counter");
    assert_eq!(
        only_i32(&must_ok(
            instance.invoke_by_name("tick", &[]),
            "live tick 1"
        )),
        1
    );
    assert_eq!(
        only_i32(&must_ok(
            instance.invoke_by_name("tick", &[]),
            "live tick 2"
        )),
        2
    );
}

#[test]
fn instance_runs_start_exactly_once() {
    let mut module = WasmModule::new();
    module.add_global(Val::I32(0), true);
    let start = must_ok(
        module.add_function(0, 0, 0, &[0x23, 0x00, 0x41, 0x01, 0x6a, 0x24, 0x00, 0x0b]),
        "add start",
    );
    let read = must_ok(
        module.add_function(0, 0, 1, &[0x23, 0x00, 0x0b]),
        "add read",
    );
    module.set_start(start);
    module.export("read", read);

    let mut instance = must_ok(module.instantiate(), "instantiate start module");
    assert_eq!(
        only_i32(&must_ok(instance.invoke_by_name("read", &[]), "read 1")),
        1
    );
    assert_eq!(
        only_i32(&must_ok(instance.invoke_by_name("read", &[]), "read 2")),
        1
    );
}

#[test]
fn instruction_budget_is_host_owned_and_resets_per_call() {
    let limits = Limits {
        max_steps: 8,
        ..Limits::default()
    };
    let mut finite = WasmModule::new_with_limits(limits);
    let done = must_ok(finite.add_function(0, 0, 0, &[0x0b]), "add finite");
    let mut finite = must_ok(finite.instantiate(), "instantiate finite");
    must_ok(finite.invoke(done, &[]), "finite call 1");
    must_ok(finite.invoke(done, &[]), "finite call 2");

    let mut runaway = WasmModule::new_with_limits(limits);
    let spin = must_ok(
        runaway.add_function(0, 0, 0, &[0x03, 0x40, 0x0c, 0x00, 0x0b, 0x0b]),
        "add spin",
    );
    let mut runaway = must_ok(runaway.instantiate(), "instantiate spin");
    assert!(matches!(
        runaway.invoke(spin, &[]),
        Err(WasmError::Trap("step budget"))
    ));
}

#[test]
fn memory_budget_rejects_initial_min_and_caps_grow() {
    let limits = Limits {
        max_memory_pages: 2,
        ..Limits::default()
    };
    assert!(matches!(
        WasmModule::from_bytes_with(MEMORY_MIN_3, limits),
        Err(WasmError::Trap("memory size"))
    ));

    let module = must_ok(
        WasmModule::from_bytes_with(GROW_MODULE, limits),
        "load grow module",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate grow module");
    assert_eq!(instance.memory().len(), WASM_PAGE_SIZE);
    assert_eq!(
        only_i32(&must_ok(
            instance.invoke_by_name("grow", &[Val::I32(1)]),
            "grow 1"
        )),
        1
    );
    assert_eq!(instance.memory().len(), 2 * WASM_PAGE_SIZE);
    assert_eq!(
        only_i32(&must_ok(
            instance.invoke_by_name("grow", &[Val::I32(1)]),
            "grow 2"
        )),
        -1
    );
    assert_eq!(instance.memory().len(), 2 * WASM_PAGE_SIZE);
}

#[test]
fn host_can_exchange_bounded_data_through_live_memory() {
    let mut module = WasmModule::new();
    let read = must_ok(
        module.add_function(0, 0, 1, &[0x41, 0x00, 0x28, 0x02, 0x00, 0x0b]),
        "add memory read",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate memory module");
    instance.memory_mut()[0..4].copy_from_slice(&42i32.to_le_bytes());
    assert_eq!(
        only_i32(&must_ok(instance.invoke_val(read, &[]), "read memory")),
        42
    );
}

#[test]
fn guest_call_stack_is_explicit_bounded_and_native_stack_independent() {
    // (func $countdown (param i32) (result i32)
    //   local.get 0 i32.eqz
    //   if (result i32) i32.const 42
    //   else local.get 0 i32.const 1 i32.sub call $countdown end)
    //
    // In debug builds this depth previously overflowed the native stack, which
    // forced a separate 32-frame cap. It now exercises the VM-owned activation
    // vector and has the same deterministic 512-frame boundary in every build.
    let mut module = WasmModule::new();
    let countdown = must_ok(
        module.add_function(
            1,
            0,
            1,
            &[
                0x20, 0x00, 0x45, 0x04, 0x7F, 0x41, 0x2A, 0x05, 0x20, 0x00, 0x41, 0x01, 0x6B, 0x10,
                0x00, 0x0B, 0x0B,
            ],
        ),
        "add recursive countdown",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate recursive countdown");
    assert_eq!(
        only_i32(&must_ok(
            instance.invoke_val(countdown, &[Val::I32(WASM_MAX_DEPTH as i32)]),
            "run at exact call-depth boundary",
        )),
        42
    );
    assert!(matches!(
        instance.invoke_val(countdown, &[Val::I32(WASM_MAX_DEPTH as i32 + 1)]),
        Err(WasmError::Trap("call depth"))
    ));

    let mut indirect_module = WasmModule::new();
    let unary_type = indirect_module.add_type(1, 1);
    let indirect_countdown = must_ok(
        indirect_module.add_function(
            1,
            0,
            1,
            &[
                0x20,
                0x00,
                0x45,
                0x04,
                0x7F,
                0x41,
                0x2A,
                0x05,
                0x20,
                0x00,
                0x41,
                0x01,
                0x6B,
                0x41,
                0x00,
                0x11,
                unary_type as u8,
                0x00,
                0x0B,
                0x0B,
            ],
        ),
        "add indirect recursive countdown",
    );
    indirect_module.add_table(1);
    indirect_module.set_table_entry(0, indirect_countdown);
    let mut indirect_instance = must_ok(
        indirect_module.instantiate(),
        "instantiate indirect recursive countdown",
    );
    assert_eq!(
        only_i32(&must_ok(
            indirect_instance.invoke_val(indirect_countdown, &[Val::I32(WASM_MAX_DEPTH as i32)],),
            "run indirect recursion at exact call-depth boundary",
        )),
        42
    );
}

#[test]
fn guest_call_stack_aggregate_slots_trap_before_next_activation_allocation() {
    fn leb(output: &mut Vec<u8>, mut value: u32) {
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            output.push(byte | if value == 0 { 0 } else { 0x80 });
            if value == 0 {
                return;
            }
        }
    }
    fn section(output: &mut Vec<u8>, id: u8, payload: &[u8]) {
        output.push(id);
        leb(output, payload.len() as u32);
        output.extend_from_slice(payload);
    }

    // One ordinary standard module spends nearly all of its decode-item budget
    // on a legal locals declaration, then recursively calls that function. The
    // aggregate live-slot cap must reject the next activation before cloning
    // that wide locals template again.
    let local_count = WASM_MAX_DECODE_ITEMS as u32 - 64;
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    section(&mut wasm, 1, &[0x01, 0x60, 0x01, 0x7F, 0x01, 0x7F]);
    section(&mut wasm, 3, &[0x01, 0x00]);
    let mut body = vec![0x01];
    leb(&mut body, local_count);
    body.extend_from_slice(&[
        0x7F, 0x20, 0x00, 0x45, 0x04, 0x7F, 0x41, 0x2A, 0x05, 0x20, 0x00, 0x41, 0x01, 0x6B, 0x10,
        0x00, 0x0B, 0x0B,
    ]);
    let mut code = vec![0x01];
    leb(&mut code, body.len() as u32);
    code.extend_from_slice(&body);
    section(&mut wasm, 10, &code);

    let module = must_ok(WasmModule::from_bytes(&wasm), "load wide countdown");
    let mut instance = must_ok(module.instantiate(), "instantiate wide countdown");
    assert!(matches!(
        instance.invoke_val(0, &[Val::I32(5)]),
        Err(WasmError::Trap("call stack"))
    ));
}
