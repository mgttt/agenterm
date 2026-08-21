//! Development-only boundary benchmark inspired by the QJWasm comparison.
//!
//! This is ignored in ordinary tests because elapsed time is evidence, not a
//! deterministic correctness gate. Run it through `smoke-boundary-benchmark.sh`.

use std::cell::RefCell;
use std::hint::black_box;
use std::rc::Rc;
use std::time::Instant;

use agenterm_tinyvm::{Val, WasmError, WasmModule};

const PAYLOAD_SIZES: [usize; 5] = [0, 64, 1_024, 65_536, 76_800];

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn iterations() -> usize {
    std::env::var("TINYVM_BOUNDARY_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20_000)
        .max(100)
}

fn report(metric: &str, payload_bytes: usize, count: usize, start: Instant) {
    let elapsed = start.elapsed().as_nanos();
    let nanos_per_operation = elapsed as f64 / count as f64;
    println!("tinyvm,{metric},{payload_bytes},{count},{nanos_per_operation:.2}");
}

fn only_i32(values: &[Val]) -> i32 {
    match values {
        [Val::I32(value)] => *value,
        _ => panic!("expected one i32 result"),
    }
}

fn sample(bytes: &[u8], args: &[Val]) -> Result<i32, WasmError> {
    let [Val::I32(pointer), Val::I32(length)] = args else {
        return Err(WasmError::Trap("benchmark host arguments"));
    };
    let pointer = *pointer as u32 as usize;
    let length = *length as u32 as usize;
    let end = pointer
        .checked_add(length)
        .ok_or(WasmError::Trap("benchmark host range"))?;
    let payload = bytes
        .get(pointer..end)
        .ok_or(WasmError::Trap("benchmark host range"))?;
    Ok(if payload.is_empty() {
        0
    } else {
        i32::from(payload[0] ^ payload[payload.len() - 1])
    })
}

#[test]
#[ignore = "development benchmark; run through smoke-boundary-benchmark.sh"]
fn boundary_benchmark_separates_call_view_copy_and_guest_costs() {
    let path = std::env::var("TINYVM_BOUNDARY_BENCH_WASM")
        .expect("TINYVM_BOUNDARY_BENCH_WASM must point to the WABT fixture");
    let bytes = std::fs::read(path).expect("read boundary benchmark fixture");
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load benchmark fixture");
    must_ok(
        module.bind_import_typed_in_place("bench", "memory_zero", |args, results, memory| {
            results[0] = Val::I32(sample(memory, args)?);
            Ok(())
        }),
        "bind memory-zero benchmark import",
    );
    must_ok(
        module.bind_import_typed_in_place_with_memories(
            "bench",
            "selected_memory",
            |args, results, memories| {
                let memory = memories
                    .memory(0)?
                    .ok_or(WasmError::Trap("benchmark selected memory"))?;
                results[0] = Val::I32(sample(&memory, args)?);
                Ok(())
            },
        ),
        "bind selected-memory benchmark import",
    );
    let scratch = Rc::new(RefCell::new(vec![
        0;
        PAYLOAD_SIZES[PAYLOAD_SIZES.len() - 1]
    ]));
    let copy_scratch = scratch.clone();
    must_ok(
        module.bind_import_typed_in_place_with_memories(
            "bench",
            "selected_copy",
            move |args, results, memories| {
                let [Val::I32(pointer), Val::I32(length)] = args else {
                    return Err(WasmError::Trap("benchmark host arguments"));
                };
                let pointer = *pointer as u32 as usize;
                let length = *length as u32 as usize;
                let end = pointer
                    .checked_add(length)
                    .ok_or(WasmError::Trap("benchmark host range"))?;
                let memory = memories
                    .memory(0)?
                    .ok_or(WasmError::Trap("benchmark selected memory"))?;
                let payload = memory
                    .get(pointer..end)
                    .ok_or(WasmError::Trap("benchmark host range"))?;
                let mut scratch = copy_scratch.borrow_mut();
                scratch[..length].copy_from_slice(payload);
                results[0] = Val::I32(if length == 0 {
                    0
                } else {
                    i32::from(scratch[0] ^ scratch[length - 1])
                });
                Ok(())
            },
        ),
        "bind selected-copy benchmark import",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate benchmark fixture");
    let count = iterations();

    must_ok(instance.invoke_val(3, &[]), "warm empty");
    must_ok(
        instance.invoke_val(4, &[Val::I32(7), Val::I64(8), Val::F32(1.5), Val::F64(2.5)]),
        "warm scalars",
    );

    println!("engine,metric,payload_bytes,iterations,nanoseconds_per_operation");

    let start = Instant::now();
    for _ in 0..count {
        black_box(must_ok(instance.invoke_val(3, &[]), "empty call"));
    }
    report("empty_call", 0, count, start);

    let scalar_args = [Val::I32(7), Val::I64(8), Val::F32(1.5), Val::F64(2.5)];
    let start = Instant::now();
    for _ in 0..count {
        let values = must_ok(instance.invoke_val(4, &scalar_args), "scalar call");
        assert_eq!(only_i32(&values), 7);
        black_box(values);
    }
    report("scalar_call", 0, count, start);

    for payload_size in PAYLOAD_SIZES {
        let source: Vec<u8> = (0..payload_size)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        {
            let mut memory = must_ok(instance.memory_mut(), "initial memory view");
            memory[..payload_size].copy_from_slice(&source);
        }
        let start = Instant::now();
        for _ in 0..count {
            let memory = must_ok(instance.memory(), "borrowed memory view");
            let sample = if payload_size == 0 {
                0
            } else {
                memory[0] ^ memory[payload_size - 1]
            };
            black_box(sample);
        }
        report("borrowed_view", payload_size, count, start);

        let copy_count = (64usize * 1_024 * 1_024)
            .checked_div(payload_size)
            .unwrap_or(count)
            .clamp(100, count);
        let start = Instant::now();
        for _ in 0..copy_count {
            let mut memory = must_ok(instance.memory_mut(), "copy memory view");
            memory[..payload_size].copy_from_slice(&source);
            black_box(&memory[..payload_size]);
        }
        report("intentional_copy", payload_size, copy_count, start);

        let expected = if payload_size == 0 {
            0
        } else {
            source[0] ^ source[payload_size - 1]
        };
        let touch_args = [Val::I32(0), Val::I32(payload_size as i32)];
        let start = Instant::now();
        for _ in 0..count {
            let values = must_ok(instance.invoke_val(5, &touch_args), "guest touch call");
            assert_eq!(only_i32(&values), i32::from(expected));
            black_box(values);
        }
        report("guest_touch_call", payload_size, count, start);

        for (metric, function) in [
            ("guest_host_memory0_view", 6),
            ("guest_host_selected0_view", 7),
        ] {
            let start = Instant::now();
            for _ in 0..count {
                let values = must_ok(instance.invoke_val(function, &touch_args), metric);
                assert_eq!(only_i32(&values), i32::from(expected));
                black_box(values);
            }
            report(metric, payload_size, count, start);
        }

        let start = Instant::now();
        for _ in 0..copy_count {
            let values = must_ok(instance.invoke_val(8, &touch_args), "selected copy");
            assert_eq!(only_i32(&values), i32::from(expected));
            black_box(values);
        }
        report("guest_host_selected0_copy", payload_size, copy_count, start);
    }
}
