//! `eval_wasm(data, globals, locals)` must actually deliver the host door.

use agenterm_tinyvm::{HostGlobal, Limits, Val, WasmError, eval, eval_wasm, eval_with};

fn import_add_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"
        (module
          (import "env" "g" (func $g (result i32)))
          (func (export "main") (param i32) (result i32)
            (i32.add (call $g) (local.get 0)))
        )
        "#,
    )
    .unwrap_or_else(|e| panic!("import add fixture: {e}"))
}

fn imported_global_add_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"
        (module
          (import "env" "g" (global i32))
          (func (export "main") (param i32) (result i32)
            (i32.add (global.get 0) (local.get 0)))
        )
        "#,
    )
    .unwrap_or_else(|e| panic!("imported global fixture: {e}"))
}

fn must_i32(result: Result<Vec<Val>, WasmError>, want: i32, what: &str) -> Vec<Val> {
    match result {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(got)] if *got == want) => vals,
        Ok(_) => panic!("{what}: unexpected values"),
        Err(e) => panic!("{what}: {}", e.message()),
    }
}

#[test]
fn eval_wasm_sends_globals_and_locals_to_the_host_door() {
    let wasm = import_add_wasm();
    must_i32(
        eval_wasm(
            &wasm,
            &[HostGlobal::new("env", "g", Val::I32(40))],
            &[Val::I32(2)],
        ),
        42,
        "function-import host door",
    );

    must_i32(
        eval_wasm(
            &wasm,
            &[HostGlobal::new("env", "g", Val::I32(7))],
            &[Val::I32(3)],
        ),
        10,
        "different globals/locals",
    );

    must_i32(
        eval_wasm(
            &imported_global_add_wasm(),
            &[HostGlobal::new("env", "g", Val::I32(40))],
            &[Val::I32(2)],
        ),
        42,
        "global-import host door",
    );
}

fn const_17_wasm() -> Vec<u8> {
    wat::parse_str(r#"(module (func (export "main") (result i32) i32.const 17))"#)
        .unwrap_or_else(|e| panic!("const 17 fixture: {e}"))
}

fn must_decode(data: &[u8], what: &str) {
    match eval_wasm(data, &[], &[]) {
        Err(WasmError::Decode(_)) => {}
        Err(e) => panic!("{what}: expected Decode, got {}", e.message()),
        Ok(_) => panic!("{what}: eval_wasm must not eat non-wasm"),
    }
}

#[test]
fn eval_wasm_eats_only_wasm() {
    must_decode(b"1+1", "JS-like arithmetic");
    must_decode(
        b"(module (func (export \"main\") (result i32) i32.const 17))",
        "WAT text",
    );
    must_decode(b"function(){return 1}", "JS source");
    must_decode(b"", "empty");
    must_decode(b"\0asm", "truncated magic");
    must_i32(
        eval_wasm(&const_17_wasm(), &[], &[]),
        17,
        "standard wasm bytes",
    );
}

#[test]
fn eval_and_eval_with_remain_callable_aliases() {
    let wasm = const_17_wasm();
    must_i32(eval(&wasm), 17, "eval alias");
    must_i32(eval_with(&wasm, Limits::default()), 17, "eval_with alias");
    match (eval(&wasm), eval_wasm(&wasm, &[], &[])) {
        (Ok(a), Ok(b)) if a == b => {}
        _ => panic!("eval(bytes) must be eval_wasm(bytes, &[], &[])"),
    }
    match (
        eval_with(&wasm, Limits::default()),
        eval_wasm(&wasm, &[], &[]),
    ) {
        (Ok(a), Ok(b)) if a == b => {}
        _ => panic!("eval_with empty-gate must match eval_wasm"),
    }
}

#[test]
fn eval_wasm_rejects_non_wasm_data() {
    must_decode(b"1+1", "1+1");
}

#[test]
fn eval_wasm_unbound_import_traps() {
    let wasm = import_add_wasm();
    match eval_wasm(&wasm, &[], &[Val::I32(2)]) {
        Err(WasmError::Trap(_)) => {}
        Err(e) => panic!("unbound import: expected Trap, got {}", e.message()),
        Ok(_) => panic!("unbound import must not run"),
    }
    match eval_wasm(
        &wasm,
        &[HostGlobal::new("js", "g", Val::I32(40))],
        &[Val::I32(2)],
    ) {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(42)]) => {
            panic!("wrong module is not the host door")
        }
        Ok(_) | Err(_) => {}
    }
}
