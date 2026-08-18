//! tinyvm.hl1: host-owned table budget. Independent of in-crate examples.
//!
//! Drives the shipped `eval` / `from_bytes_with` face. A 33-byte
//! `table min=0x0FFFFFFF` module must return Err without aborting.

use std::process::Command;

use agenterm_tinyvm::{Limits, WasmError, WasmModule, eval};

/// Probe module from limits-review: `(table 0x0FFFFFFF funcref)` plus a
/// 15-byte custom section so the file is exactly 33 bytes.
const HUGE_TABLE_MIN: [u8; 33] = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // \0asm v1
    0x04, 0x08, 0x01, 0x70, 0x00, 0xff, 0xff, 0xff, 0xff, 0x0f, // table min=0x0FFFFFFF
    0x00, 0x0d, 0x05, b'p', b'r', b'o', b'b', b'e', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, // custom "probe"+7
];

/// `(table 1 funcref) (func (export "main"))`
const TABLE_MIN_1: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x03, 0x02,
    0x01, 0x00, 0x04, 0x04, 0x01, 0x70, 0x00, 0x01, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e,
    0x00, 0x00, 0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
];

/// `(table 16 funcref)` — used to prove the reject point follows the host
/// budget, not a crate constant.
const TABLE_MIN_16: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x04, 0x04, 0x01, 0x70, 0x00, 0x10,
];

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
fn huge_table_min_eval_is_err() {
    assert_eq!(HUGE_TABLE_MIN.len(), 33);
    match eval(&HUGE_TABLE_MIN) {
        Err(e) => assert!(!e.message().is_empty(), "table-size Err must be loud"),
        Ok(_) => panic!("table min=0x0FFFFFFF must not succeed"),
    }
}

#[test]
fn huge_table_min_child_rc_is_not_134() {
    let exe = std::env::current_exe().expect("test executable");
    let output = Command::new(exe)
        .arg("huge_table_min_eval_is_err")
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
fn legal_table_min_1_eval_is_ok() {
    match eval(TABLE_MIN_1) {
        Ok(_) => {}
        Err(e) => panic!("table min=1 must succeed, got {}", e.message()),
    }
}

#[test]
fn table_budget_follows_host_not_crate_constant() {
    let tight = Limits { max_table_elems: 8 };
    let wide = Limits {
        max_table_elems: 32,
    };
    match WasmModule::from_bytes_with(TABLE_MIN_16, tight) {
        Err(WasmError::Trap(msg)) => assert_eq!(msg, "table size"),
        Err(e) => panic!("tight budget must Trap(table size), got {}", e.message()),
        Ok(_) => panic!("table min=16 under host=8 must not instantiate"),
    }
    match WasmModule::from_bytes_with(TABLE_MIN_16, wide) {
        Ok(_) => {}
        Err(e) => panic!(
            "table min=16 under host=32 must succeed, got {}",
            e.message()
        ),
    }
}
