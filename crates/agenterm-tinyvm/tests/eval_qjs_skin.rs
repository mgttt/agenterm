//! PRD-mapped acceptance for the language skin. These tests execute the
//! product sentences; they are not a source grep.

use agenterm_tinyvm::{HostGlobal, Val, eval_wasm};
use agenterm_tinyvm_qjs::{eval_qjs, qjs2wasm};

fn must_i32(result: Result<Vec<Val>, agenterm_tinyvm::WasmError>, want: i32, what: &str) {
    match result {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(got)] if *got == want) => {}
        Ok(_) => panic!("{what}: unexpected values"),
        Err(e) => panic!("{what}: {}", e.message()),
    }
}

#[test]
fn language_skin_is_qjs2wasm_over_eval_wasm() {
    let src = "g()+$0";
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("{}", e.message()));
    assert!(wasm.starts_with(b"\0asm"));
    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    let loc = [Val::I32(2)];
    must_i32(eval_wasm(&wasm, &g, &loc), 42, "eval_wasm(qjs2wasm)");
    must_i32(eval_qjs(src, &g, &loc), 42, "eval_qjs");
    match eval_qjs("function(){return 1}", &[], &[]) {
        Err(_) => {}
        Ok(_) => panic!("full JS must not run on the language skin"),
    }
}

#[test]
fn qjs2wasm_names_ops_host_call() {
    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    must_i32(eval_qjs("40+2", &[], &[]), 42, "ops");
    must_i32(eval_qjs("g", &g, &[]), 40, "name");
    must_i32(eval_qjs("g()", &g, &[]), 40, "host call");
    must_i32(
        eval_qjs("g()+$0", &g, &[Val::I32(2)]),
        42,
        "name+op+host+$0",
    );
}

#[test]
fn eval_qjs_is_qjs2wasm_then_eval_wasm() {
    let src = "(g+$0)*2";
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("{}", e.message()));
    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    let loc = [Val::I32(2)];
    match (eval_wasm(&wasm, &g, &loc), eval_qjs(src, &g, &loc)) {
        (Ok(a), Ok(b)) if a == b => must_i32(Ok(a), 84, src),
        _ => panic!("eval_qjs must be eval_wasm(&qjs2wasm(src)?, globals, locals)"),
    }
}

#[test]
fn commissar_demo_eval_wasm_and_sugar() {
    let src = "g()+$0";
    let data = qjs2wasm(src).unwrap_or_else(|e| panic!("{}", e.message()));
    let globals = [HostGlobal::new("js", "g", Val::I32(40))];
    let locals = [Val::I32(2)];
    must_i32(
        eval_wasm(&data, &globals, &locals),
        42,
        "commissar eval_wasm",
    );
    must_i32(eval_qjs(src, &globals, &locals), 42, "commissar eval_qjs");
    match qjs2wasm("g($0)") {
        Err(e) if e.message().contains("two bindings") => {}
        Err(e) => panic!("g($0): {}", e.message()),
        Ok(_) => panic!("commissar demo must reject host args"),
    }
}
