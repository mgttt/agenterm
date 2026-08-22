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

fn roundtrip(src: &str, globals: &[HostGlobal<'_>], locals: &[Val], want: i32) {
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm {src}: {}", e.message()));
    assert!(wasm.starts_with(b"\0asm"), "{src} must emit wasm");
    same_i32(
        eval_wasm(&wasm, globals, locals),
        eval_qjs(src, globals, locals),
        want,
        src,
    );
}

#[test]
fn qjs2wasm_bytes_feed_eval_wasm_and_match_eval_qjs() {
    roundtrip("40+2", &[], &[], 42);

    let globals = [HostGlobal::new("js", "g", Val::I32(40))];
    roundtrip("g+2", &globals, &[], 42);
    roundtrip("g()+$0", &globals, &[Val::I32(2)], 42);

    roundtrip("$0+2", &[], &[Val::I32(40)], 42);
}

#[test]
fn thicker_subset_names_ops_host_call() {
    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    let loc = [Val::I32(2)];
    roundtrip("(g+$0)*2", &g, &loc, 84);
    roundtrip("g()*$0-2", &g, &loc, 78);
    roundtrip("1+2*3", &[], &[], 7);
    roundtrip("(1+2)*3", &[], &[], 9);
    roundtrip("8/3", &[], &[], 2);
    roundtrip("8%3", &[], &[], 2);
    roundtrip("-2+5", &[], &[], 3);
    roundtrip("-$0+g()", &g, &loc, 38);
}

#[test]
fn qjs2wasm_rejects_full_js_as_a_converter() {
    assert!(matches!(
        qjs2wasm("function(){return 1}"),
        Err(WasmError::Decode(_))
    ));
    assert!(matches!(qjs2wasm("eval(1)"), Err(WasmError::Decode(_))));
    assert!(matches!(qjs2wasm("const x = 1"), Err(WasmError::Decode(_))));
    match qjs2wasm("g($0)") {
        Err(e) => assert!(
            e.message().contains("two bindings"),
            "g($0) must name the two-binding rule, got {}",
            e.message()
        ),
        Ok(_) => panic!("g($0) must not grow a third world"),
    }
}
