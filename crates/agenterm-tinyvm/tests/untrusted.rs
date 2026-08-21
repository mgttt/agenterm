//! Independent untrusted-bytes tests for tinyvm.27.
//!
//! These drive the shipped [`eval`] face. They are not the interpreter's
//! in-crate example table.

use std::process::Command;

use agenterm_tinyvm::{WasmError, eval, wasm::WASM_MAX_DECODE_ITEMS};

/// 44-byte module: `(memory 65536)` plus an exported empty `main`.
///
/// `min=65536` is the spec page maximum (4 GiB). Instantiating it with
/// `vec![0; 4GiB]` aborts the process; `eval` must return `Err` instead.
const MEMORY_MIN_65536: [u8; 44] = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // \0asm v1
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type () -> ()
    0x03, 0x02, 0x01, 0x00, // func type 0
    0x05, 0x05, 0x01, 0x00, 0x80, 0x80, 0x04, // memory min=65536
    0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // export "main"
    0x0a, 0x07, 0x01, 0x05, 0x00, 0x01, 0x01, 0x01, 0x0b, // nop×3 end
];

/// Build a module whose `main` is `n` consecutive `i32.const 1` (then `end`).
/// A structured `loop`+`br` would unwind and never grow the operand stack.
/// A stack bomb that is a *valid* module: it pushes `n` constants and folds
/// them back with `n - 1` adds, so the body is balanced (result i32) while its
/// peak operand-stack height is still `n`. Load-time validation rejects the
/// unbalanced variant, so the host cap has to be proved with a legal program.
fn push_n_module(n: usize) -> Vec<u8> {
    let mut expr = Vec::with_capacity(n * 3 + 1);
    for _ in 0..n {
        expr.extend_from_slice(&[0x41, 0x01]);
    }
    // n - 1 × i32.add, folding the pushed constants back to one value.
    expr.resize(expr.len() + n.saturating_sub(1), 0x6a);
    expr.push(0x0b);
    let mut body = vec![0x00];
    body.extend_from_slice(&expr);
    let mut payload = vec![0x01];
    leb_u32(&mut payload, body.len() as u32);
    payload.extend_from_slice(&body);
    let mut wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
        0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, 0x0a,
    ];
    leb_u32(&mut wasm, payload.len() as u32);
    wasm.extend_from_slice(&payload);
    wasm
}

fn leb_u32(out: &mut Vec<u8>, mut n: u32) {
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}

fn err_msg(bytes: &[u8]) -> &'static str {
    match eval(bytes) {
        Err(e) => e.message(),
        Ok(_) => panic!("expected Err from eval, got Ok"),
    }
}

fn module_with_sections(sections: &[&[u8]]) -> Vec<u8> {
    let mut wasm = b"\0asm\x01\0\0\0".to_vec();
    for section in sections {
        wasm.extend_from_slice(section);
    }
    wasm
}

fn br_table_count_bomb() -> Vec<u8> {
    module_with_sections(&[
        &[0x01, 0x04, 0x01, 0x60, 0x00, 0x00],
        &[0x03, 0x02, 0x01, 0x00],
        &[
            0x0a, 0x09, 0x01, 0x07, 0x00, 0x0e, 0xff, 0xff, 0xff, 0xff, 0x0f,
        ],
    ])
}

fn element_count_bomb() -> Vec<u8> {
    module_with_sections(&[&[
        0x09, 0x0a, 0x01, 0x00, 0x41, 0x00, 0x0b, 0xff, 0xff, 0xff, 0xff, 0x0f,
    ]])
}

fn local_count_bomb() -> Vec<u8> {
    let mut body = vec![0x01];
    leb_u32(&mut body, (WASM_MAX_DECODE_ITEMS + 1) as u32);
    body.extend_from_slice(&[0x7f, 0x0b]);
    let mut code = vec![0x01];
    leb_u32(&mut code, body.len() as u32);
    code.extend_from_slice(&body);
    let mut code_section = vec![0x0a];
    leb_u32(&mut code_section, code.len() as u32);
    code_section.extend_from_slice(&code);
    module_with_sections(&[
        &[0x01, 0x04, 0x01, 0x60, 0x00, 0x00],
        &[0x03, 0x02, 0x01, 0x00],
        &code_section,
    ])
}

fn exit_rc(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    -1
}

#[test]
fn huge_memory_min_eval_is_err() {
    assert_eq!(MEMORY_MIN_65536.len(), 44);
    match eval(&MEMORY_MIN_65536) {
        Err(e) => assert!(!e.message().is_empty(), "memory-size Err must be loud"),
        Ok(_) => panic!("min=65536 must not succeed"),
    }
}

#[test]
fn huge_memory_min_child_rc_is_not_134() {
    let exe = std::env::current_exe().expect("test executable");
    let output = Command::new(exe)
        .arg("huge_memory_min_eval_is_err")
        .arg("--exact")
        .output()
        .expect("spawn eval child");
    let rc = exit_rc(&output.status);
    assert_ne!(
        rc,
        134,
        "child aborted (SIGABRT). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "child rc={rc} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tiny_declared_count_bombs_fail_before_allocation() {
    for wasm in [
        br_table_count_bomb(),
        element_count_bomb(),
        local_count_bomb(),
    ] {
        assert!(wasm.len() < 40);
        assert_eq!(err_msg(&wasm), "module decode budget");
    }
}

#[test]
fn declared_count_bomb_child_does_not_abort() {
    let exe = std::env::current_exe().expect("test executable");
    let output = Command::new(exe)
        .arg("tiny_declared_count_bombs_fail_before_allocation")
        .arg("--exact")
        .output()
        .expect("spawn count-bomb child");
    let rc = exit_rc(&output.status);
    assert_ne!(
        rc,
        134,
        "count bomb aborted. stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "child rc={rc} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn standard_sections_are_unique_ordered_and_fully_consumed() {
    let duplicate_type = module_with_sections(&[&[0x01, 0x01, 0x00], &[0x01, 0x01, 0x00]]);
    assert_eq!(
        err_msg(&duplicate_type),
        "duplicate or out-of-order section"
    );

    let out_of_order = module_with_sections(&[&[0x03, 0x01, 0x00], &[0x01, 0x01, 0x00]]);
    assert_eq!(err_msg(&out_of_order), "duplicate or out-of-order section");

    let trailing_type_byte = module_with_sections(&[&[0x01, 0x02, 0x00, 0x00]]);
    assert_eq!(err_msg(&trailing_type_byte), "trailing type section bytes");

    let unknown_standard_section = module_with_sections(&[&[0x0c, 0x00]]);
    assert_eq!(err_msg(&unknown_standard_section), "unsupported section id");

    let custom_around_type = module_with_sections(&[
        &[0x00, 0x01, 0x00],
        &[0x01, 0x01, 0x00],
        &[0x00, 0x01, 0x00],
    ]);
    assert!(eval(&custom_around_type).is_ok());
}

#[test]
fn operand_stack_cap_traps_before_step_budget() {
    // Well above 65_536, far below the 16M step budget, and ~1 MiB of
    // values rather than the hundreds of MB a 16M-step push bomb would take.
    match eval(&push_n_module(70_000)) {
        Err(WasmError::Trap(msg)) => {
            assert_eq!(msg, "operand stack");
            assert_ne!(msg, "step budget");
        }
        _other => panic!("push growth must trap on operand stack, got error-or-ok"),
    }
}

fn trunc_module(const_op: u8, const_bytes: &[u8], trunc_op: u8, result: u8) -> Vec<u8> {
    let mut expr = Vec::new();
    expr.push(const_op);
    expr.extend_from_slice(const_bytes);
    expr.push(trunc_op);
    expr.push(0x0b);
    let mut body = vec![0x00]; // 0 local groups
    body.extend_from_slice(&expr);
    let mut code = vec![0x01, body.len() as u8];
    code.extend_from_slice(&body);
    let mut wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, result,
        0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, 0x0a,
    ];
    wasm.push(code.len() as u8);
    wasm.extend_from_slice(&code);
    wasm
}

#[test]
fn eight_trunc_traps_have_nonempty_messages() {
    let nan_f32 = 0x7fc0_0000u32.to_le_bytes();
    let nan_f64 = 0x7ff8_0000_0000_0000u64.to_le_bytes();
    let cases: [(&str, u8, &[u8], u8, u8); 8] = [
        ("i32.trunc_f32_s", 0x43, &nan_f32, 0xa8, 0x7f),
        ("i32.trunc_f32_u", 0x43, &nan_f32, 0xa9, 0x7f),
        ("i32.trunc_f64_s", 0x44, &nan_f64, 0xaa, 0x7f),
        ("i32.trunc_f64_u", 0x44, &nan_f64, 0xab, 0x7f),
        ("i64.trunc_f32_s", 0x43, &nan_f32, 0xae, 0x7e),
        ("i64.trunc_f32_u", 0x43, &nan_f32, 0xaf, 0x7e),
        ("i64.trunc_f64_s", 0x44, &nan_f64, 0xb0, 0x7e),
        ("i64.trunc_f64_u", 0x44, &nan_f64, 0xb1, 0x7e),
    ];
    let mut lines = Vec::new();
    for (name, cop, cbytes, top, rty) in cases {
        let wasm = trunc_module(cop, cbytes, top, rty);
        let msg = err_msg(&wasm);
        assert!(!msg.is_empty(), "{name} trap message is empty");
        assert!(
            msg.contains("trunc") || msg == name,
            "{name}: unexpected message {msg:?}"
        );
        eprintln!("{name} -> {msg}");
        lines.push(format!("{name} -> {msg}"));
    }
    assert_eq!(lines.len(), 8);
}
