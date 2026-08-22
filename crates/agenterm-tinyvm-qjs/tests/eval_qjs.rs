//! Acceptance suite for the language skin. One test per product sentence.

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

fn decode_mentions(src: &str, needle: &str) {
    match qjs2wasm(src) {
        Err(e) => assert!(
            e.message().contains(needle),
            "{src}: want message containing {needle:?}, got {}",
            e.message()
        ),
        Ok(_) => panic!("{src}: converter accepted forbidden source"),
    }
    match eval_qjs(src, &[], &[]) {
        Err(e) => assert!(
            e.message().contains(needle),
            "eval_qjs({src}): want {needle:?}, got {}",
            e.message()
        ),
        Ok(_) => panic!("eval_qjs({src}): ran forbidden source"),
    }
}

#[test]
fn qjs2wasm_names_ops_host_call() {
    roundtrip("40+2", &[], &[], 42);
    roundtrip("40-2", &[], &[], 38);
    roundtrip("1+2*3", &[], &[], 7);
    roundtrip("(1+2)*3", &[], &[], 9);
    roundtrip("8/3", &[], &[], 2);
    roundtrip("8%3", &[], &[], 2);
    roundtrip("-2+5", &[], &[], 3);

    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    roundtrip("g+2", &g, &[], 42);
    roundtrip("g()+$0", &g, &[Val::I32(2)], 42);
    roundtrip("(g+$0)*2", &g, &[Val::I32(2)], 84);
    roundtrip("-$0+g()", &g, &[Val::I32(2)], 38);

    let two = [
        HostGlobal::new("js", "g", Val::I32(40)),
        HostGlobal::new("js", "h", Val::I32(2)),
    ];
    roundtrip("g+h", &two, &[], 42);
    roundtrip("$0+$1", &[], &[Val::I32(40), Val::I32(2)], 42);
}

#[test]
fn eval_qjs_is_qjs2wasm_then_eval_wasm() {
    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    let loc = [Val::I32(2)];
    let src = "g()+$0";
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm: {}", e.message()));
    let via_bytes = eval_wasm(&wasm, &g, &loc);
    let via_sugar = eval_qjs(src, &g, &loc);
    match (via_bytes, via_sugar) {
        (Ok(a), Ok(b)) if a == b => must_i32(Ok(a), 42, src),
        _ => panic!("{src}: eval_qjs is not eval_wasm(qjs2wasm)"),
    }

    match (qjs2wasm("eval(1)"), eval_qjs("eval(1)", &[], &[])) {
        (Err(a), Err(b)) if a == b => {}
        _ => panic!("eval_qjs must fail with the same error as qjs2wasm"),
    }
}

#[test]
fn qjs_world_is_only_two_bindings() {
    let g = [HostGlobal::new("js", "g", Val::I32(40))];
    let loc = [Val::I32(99)];

    roundtrip("g", &g, &[], 40);
    roundtrip("$0", &[], &loc, 99);
    roundtrip("g+$0", &g, &[Val::I32(2)], 42);

    match eval_qjs("g", &[], &[]) {
        Err(WasmError::Trap(_)) => {}
        Err(e) => panic!("unbound host name must trap, got {}", e.message()),
        Ok(_) => panic!("unbound host name must not invent a third world"),
    }
    match eval_qjs("g", &[], &loc) {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(99)]) => {
            panic!("locals must not satisfy host names")
        }
        Ok(_) | Err(_) => {}
    }
    match eval_qjs("$0", &g, &[]) {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(40)]) => {
            panic!("globals must not satisfy locals")
        }
        Ok(_) | Err(_) => {}
    }
    match eval_qjs("g", &[HostGlobal::new("env", "g", Val::I32(40))], &[]) {
        Ok(vals) if matches!(vals.as_slice(), [Val::I32(40)]) => {
            panic!("wrong module is not the host door")
        }
        Ok(_) | Err(_) => {}
    }
}

#[test]
fn full_js_is_not_a_converter() {
    for src in [
        "function(){return 1}",
        "eval(1)",
        "const x = 1",
        "let x = 1",
        "class C {}",
        "new F",
        "this",
        "return 1",
        "async x",
    ] {
        decode_mentions(src, "full JS");
    }
}

#[test]
fn host_call_with_args_is_third_world() {
    for src in ["g($0)", "g(1)", "add(1,2)", "g($0,$1)"] {
        decode_mentions(src, "two bindings");
    }
    for src in ["{a:1}", "[1]", "g.x"] {
        decode_mentions(src, "two bindings");
    }
}
