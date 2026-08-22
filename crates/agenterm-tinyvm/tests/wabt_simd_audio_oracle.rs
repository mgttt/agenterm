#![cfg(feature = "simd")]

use std::path::PathBuf;

use agenterm_tinyvm::{Val, ValueType, WasmError, WasmModule};

const LEFT: [i16; 8] = [30_000, -30_000, 100, -100, 32_767, -32_768, 20_000, -20_000];
const RIGHT: [i16; 8] = [10_000, -10_000, 200, -200, 1, -1, -25_000, 25_000];
const EXPECTED: [i16; 8] = [32_767, -32_768, 300, -300, 32_767, -32_768, -5_000, 5_000];
const EXPECTED_SUBTRACT: [i16; 8] = [20_000, -20_000, -100, 100, 32_766, -32_767, 32_767, -32_768];
const LOGIC_LEFT: [u8; 16] = [
    0x00, 0xff, 0x0f, 0xf0, 0xaa, 0x55, 0x81, 0x7e, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
];
const LOGIC_RIGHT: [u8; 16] = [
    0xff, 0x00, 0x33, 0x55, 0x0f, 0xf0, 0x7e, 0x81, 0x87, 0x65, 0x43, 0x21, 0xfe, 0xdc, 0xba, 0x98,
];
const LOGIC_MASK: [u8; 16] = [
    0xff, 0xff, 0x00, 0x00, 0xf0, 0x0f, 0xaa, 0x55, 0xcc, 0x33, 0x5a, 0xa5, 0x80, 0x01, 0x7f, 0xfe,
];

fn must<T>(result: Result<T, WasmError>, context: &str) -> T {
    result.unwrap_or_else(|error| panic!("{context}: {}", error.message()))
}

fn write_samples(memory: &mut [u8], offset: usize, samples: &[i16; 8]) {
    for (lane, sample) in samples.iter().enumerate() {
        let start = offset + lane * 2;
        memory[start..start + 2].copy_from_slice(&sample.to_le_bytes());
    }
}

fn read_samples(memory: &[u8], offset: usize) -> [i16; 8] {
    core::array::from_fn(|lane| {
        let start = offset + lane * 2;
        i16::from_le_bytes([memory[start], memory[start + 1]])
    })
}

fn expected_logic() -> [[u8; 16]; 6] {
    core::array::from_fn(|operation| {
        core::array::from_fn(|index| match operation {
            0 => LOGIC_LEFT[index] & LOGIC_RIGHT[index],
            1 => LOGIC_LEFT[index] | LOGIC_RIGHT[index],
            2 => LOGIC_LEFT[index] ^ LOGIC_RIGHT[index],
            3 => LOGIC_LEFT[index] & !LOGIC_RIGHT[index],
            4 => !LOGIC_LEFT[index],
            5 => {
                (LOGIC_LEFT[index] & LOGIC_MASK[index]) | (LOGIC_RIGHT[index] & !LOGIC_MASK[index])
            }
            _ => unreachable!(),
        })
    })
}

#[test]
#[ignore = "run through smoke-wabt-simd-audio.sh with an independently compiled fixture"]
fn wabt_compiled_simd_audio_and_masks_match_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_SIMD_WASM")
            .expect("TINYVM_WABT_SIMD_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced SIMD wasm");
    let module = must(WasmModule::from_bytes(&bytes), "load SIMD module");
    assert!(module.feature_usage().simd);
    let mut instance = must(module.instantiate(), "instantiate SIMD module");
    {
        let mut memory = must(instance.memory_mut(), "borrow SIMD memory");
        write_samples(&mut memory, 0, &LEFT);
        write_samples(&mut memory, 16, &RIGHT);
        memory[32..48].fill(0x5a);
    }
    let result = must(
        instance.invoke_by_name("mix", &[Val::I32(0), Val::I32(16), Val::I32(32)]),
        "mix SIMD samples",
    );
    assert!(result.is_empty());
    assert_eq!(
        read_samples(&must(instance.memory(), "read SIMD memory"), 32),
        EXPECTED
    );
    must(
        instance.invoke_by_name("subtract", &[Val::I32(0), Val::I32(16), Val::I32(32)]),
        "subtract SIMD samples",
    );
    assert_eq!(
        read_samples(&must(instance.memory(), "read subtracted SIMD memory"), 32),
        EXPECTED_SUBTRACT
    );

    for operation in ["mix", "subtract"] {
        let tail_before = must(instance.memory(), "read tail before trap")[65_520..].to_vec();
        let error = match instance
            .invoke_by_name(operation, &[Val::I32(0), Val::I32(16), Val::I32(65_528)])
        {
            Err(error) => error,
            Ok(_) => panic!("out-of-bounds SIMD {operation} store must trap"),
        };
        assert!(error.message().starts_with("memory access ["));
        assert_eq!(
            &must(instance.memory(), "read tail after trap")[65_520..],
            tail_before
        );
    }

    {
        let mut memory = must(instance.memory_mut(), "borrow SIMD mask memory");
        memory[0..16].copy_from_slice(&LOGIC_LEFT);
        memory[16..32].copy_from_slice(&LOGIC_RIGHT);
        memory[32..48].copy_from_slice(&LOGIC_MASK);
        memory[64..192].fill(0);
    }
    must(
        instance.invoke_by_name(
            "logic",
            &[Val::I32(0), Val::I32(16), Val::I32(32), Val::I32(64)],
        ),
        "run SIMD mask kernel",
    );
    let memory = must(instance.memory(), "read SIMD mask results");
    for (operation, expected) in expected_logic().iter().enumerate() {
        let start = 64 + operation * 16;
        assert_eq!(&memory[start..start + 16], expected);
    }
    drop(memory);
    assert!(matches!(
        must(
            instance.invoke_by_name("any", &[Val::I32(0)]),
            "test nonzero vector"
        )
        .as_slice(),
        [Val::I32(1)]
    ));
    assert!(matches!(
        must(
            instance.invoke_by_name("any", &[Val::I32(176)]),
            "test zero vector"
        )
        .as_slice(),
        [Val::I32(0)]
    ));
}

#[test]
fn unsupported_simd_instruction_fails_during_decode() {
    let bytes = wat::parse_str(
        "(module (func (param v128 v128) (result v128) local.get 0 local.get 1 i16x8.mul))",
    )
    .expect("compile unsupported SIMD instruction");
    let error = match WasmModule::from_bytes(&bytes) {
        Err(error) => error,
        Ok(_) => panic!("unsupported SIMD must fail at load"),
    };
    assert_eq!(error.message(), "unsupported 0xfd opcode");
}

#[test]
fn v128_mask_validation_rejects_scalar_and_missing_operands() {
    for (source, expected) in [
        (
            "(module (func (result v128) i32.const 1 i32.const 2 v128.and))",
            "validation: type mismatch",
        ),
        (
            "(module (func (result v128) v128.const i32x4 0 0 0 0 v128.const i32x4 0 0 0 0 v128.bitselect))",
            "validation: operand stack underflow",
        ),
        (
            "(module (func (result i32) i32.const 1 v128.any_true))",
            "validation: type mismatch",
        ),
    ] {
        let bytes = wat::parse_str(source).expect("encode invalid SIMD type fixture");
        let error = match WasmModule::from_bytes(&bytes) {
            Err(error) => error,
            Ok(_) => panic!("invalid SIMD mask operands must fail at load"),
        };
        assert_eq!(error.message(), expected);
    }
}

#[test]
fn v128_function_local_constant_and_alignment_are_standard_typed() {
    let bytes = wat::parse_str(
        r#"(module
          (memory 1)
          (func (export "pass") (param v128) (result v128) local.get 0)
          (func (export "zero") (result v128) (local v128) local.get 0)
          (func (export "constant") (result v128)
            v128.const i32x4 1 2 3 4)
          (global $constant v128 (v128.const i32x4 1 2 3 4))
          (func (export "global") (result v128) global.get $constant)
          (func (export "load") (param i32) (result v128)
            local.get 0 v128.load))"#,
    )
    .expect("compile v128 type fixture");
    let mut instance = must(
        must(WasmModule::from_bytes(&bytes), "load v128 type fixture").instantiate(),
        "instantiate v128 type fixture",
    );
    let value = [0xA5; 16];
    let passed = must(
        instance.invoke_by_name("pass", &[Val::V128(value)]),
        "pass v128",
    );
    assert!(matches!(passed.as_slice(), [Val::V128(actual)] if *actual == value));
    let zero = must(instance.invoke_by_name("zero", &[]), "zero v128 local");
    assert!(matches!(zero.as_slice(), [Val::V128(actual)] if *actual == [0; 16]));
    let mut expected = [0; 16];
    for (lane, number) in [1_i32, 2, 3, 4].iter().enumerate() {
        expected[lane * 4..lane * 4 + 4].copy_from_slice(&number.to_le_bytes());
    }
    let constant = must(instance.invoke_by_name("constant", &[]), "v128.const");
    assert!(matches!(constant.as_slice(), [Val::V128(actual)] if *actual == expected));
    let global = must(instance.invoke_by_name("global", &[]), "v128 global");
    assert!(matches!(global.as_slice(), [Val::V128(actual)] if *actual == expected));

    let mut over_aligned = wat::parse_str(
        "(module (memory 1) (func (param i32) (result v128) local.get 0 v128.load))",
    )
    .expect("compile SIMD load fixture");
    let memarg = over_aligned
        .windows(4)
        .position(|window| window == [0xFD, 0x00, 0x04, 0x00])
        .expect("locate v128.load memarg");
    over_aligned[memarg + 2] = 0x05;
    let error = match WasmModule::from_bytes(&over_aligned) {
        Err(error) => error,
        Ok(_) => panic!("over-aligned SIMD load must fail at load"),
    };
    assert_eq!(
        error.message(),
        "memory alignment exceeds natural alignment"
    );
}

#[test]
fn v128_round_trips_through_the_typed_host_boundary() {
    let bytes = wat::parse_str(
        r#"(module
          (import "host" "identity" (func $identity (param v128) (result v128)))
          (func (export "run") (result v128)
            v128.const i32x4 1 2 3 4
            call $identity))"#,
    )
    .expect("compile v128 host fixture");
    let mut module = must(WasmModule::from_bytes(&bytes), "load v128 host fixture");
    assert!(module.import_parameter_type(0, 0) == Some(ValueType::V128));
    assert!(module.import_result_type(0, 0) == Some(ValueType::V128));
    must(
        module.bind_import_typed("host", "identity", |arguments, _memory| {
            let [Val::V128(value)] = arguments else {
                return Err(WasmError::Trap("v128 host argument"));
            };
            Ok(vec![Val::V128(*value)])
        }),
        "bind v128 host identity",
    );
    let result = must(
        module.invoke_by_name("run", &[]),
        "invoke v128 host identity",
    );
    let mut expected = [0; 16];
    for (lane, number) in [1_i32, 2, 3, 4].iter().enumerate() {
        expected[lane * 4..lane * 4 + 4].copy_from_slice(&number.to_le_bytes());
    }
    assert!(matches!(result.as_slice(), [Val::V128(actual)] if *actual == expected));
}
