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

#[test]
fn eval_and_eval_with_remain_callable_aliases() {
    let wasm = wat::parse_str(r#"(module (func (export "main") (result i32) i32.const 17))"#)
        .unwrap_or_else(|e| panic!("const 17 fixture: {e}"));
    must_i32(eval(&wasm), 17, "eval alias");
    must_i32(eval_with(&wasm, Limits::default()), 17, "eval_with alias");
}

#[test]
fn eval_wasm_rejects_non_wasm_data() {
    assert!(matches!(
        eval_wasm(b"1+1", &[], &[]),
        Err(WasmError::Decode(_))
    ));
}
