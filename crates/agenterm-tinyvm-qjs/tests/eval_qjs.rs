use agenterm_tinyvm::{HostGlobal, Val, WasmError, eval_wasm};
use agenterm_tinyvm_qjs::{eval_qjs, qjs2wasm};

fn must_i32(result: Result<Vec<Val>, WasmError>, want: i32, what: &str) {
    match result {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(got)] if *got == want) => {}
        Ok(_) => panic!("{what}: unexpected values"),
        Err(e) => panic!("{what}: {}", e.message()),
    }
}

fn same_i32(a: Result<Vec<Val>, WasmError>, b: Result<Vec<Val>, WasmError>, want: i32, what: &str) {
    must_i32(a, want, &format!("{what} via eval_wasm(qjs2wasm)"));
    must_i32(b, want, &format!("{what} via eval_qjs"));
}

#[test]
fn qjs2wasm_bytes_feed_eval_wasm_and_match_eval_qjs() {
    let src = "40+2";
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm 40+2: {}", e.message()));
    assert!(wasm.starts_with(b"\0asm"));
    same_i32(
        eval_wasm(&wasm, &[], &[]),
        eval_qjs(src, &[], &[]),
        42,
        "40+2",
    );

    let src = "g+2";
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm g+2: {}", e.message()));
    let globals = [HostGlobal::new("js", "g", Val::I32(40))];
    same_i32(
        eval_wasm(&wasm, &globals, &[]),
        eval_qjs(src, &globals, &[]),
        42,
        "g+2 host name",
    );

    let src = "$0+2";
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm $0+2: {}", e.message()));
    let locals = [Val::I32(40)];
    same_i32(
        eval_wasm(&wasm, &[], &locals),
        eval_qjs(src, &[], &locals),
        42,
        "$0+2 local",
    );
}

#[test]
fn qjs2wasm_rejects_full_js_as_a_converter() {
    assert!(matches!(
        qjs2wasm("function(){return 1}"),
        Err(WasmError::Decode(_))
    ));
    assert!(matches!(qjs2wasm("eval(1)"), Err(WasmError::Decode(_))));
}
