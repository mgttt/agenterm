//! Minimal commissar demo: `eval_wasm(data, globals, locals)` plus qjs2wasm
//! subset sugar (names / ops / host call). World is only those two bindings.
//!
//! From repository root:
//!
//! ```sh
//! cargo run -p agenterm-tinyvm-qjs --example commissar
//! ```

use agenterm_tinyvm::{HostGlobal, Val, eval_wasm};
use agenterm_tinyvm_qjs::{eval_qjs, qjs2wasm};

fn main() {
    println!("tinyvm.eval_wasm commissar demo");
    println!("  face: eval_wasm(data, globals, locals)");
    println!("  sugar: qjs2wasm names / ops / host call → MVP wasm");
    println!("  world: only globals (import table) and locals (this call)");
    println!();

    let globals = [HostGlobal::new("js", "g", Val::I32(40))];
    let locals = [Val::I32(2)];
    let src = "g()+$0";
    let data = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm {src}: {}", e.message()));
    assert!(
        data.starts_with(b"\0asm"),
        "qjs2wasm must emit standard wasm"
    );

    let via_bytes = eval_wasm(&data, &globals, &locals)
        .unwrap_or_else(|e| panic!("eval_wasm {src}: {}", e.message()));
    let via_sugar = eval_qjs(src, &globals, &locals)
        .unwrap_or_else(|e| panic!("eval_qjs {src}: {}", e.message()));
    expect_i32(&via_bytes, 42, "eval_wasm(data, js.g=40, $0=2)");
    expect_i32(&via_sugar, 42, "eval_qjs(\"g()+$0\")");
    if via_bytes != via_sugar {
        panic!("bytes path and sugar path must agree");
    }

    println!("eval_wasm(data, globals, locals)");
    println!(
        "  data     = \\0asm from qjs2wasm(\"g()+$0\") ({} bytes)",
        data.len()
    );
    println!("  globals  = js.g = 40");
    println!("  locals   = [$0 = 2]");
    println!("  result   = 42");
    println!();

    println!("qjs2wasm subset (names / ops / host call)");
    show("40+2", &[], &[], 42);
    show("g()+$0", &globals, &locals, 42);
    show("(g+$0)*2", &globals, &locals, 84);
    show("g()*$0-2", &globals, &locals, 78);
    show("1+2*3", &[], &[], 7);
    println!();

    println!("rejected full JS (not a converter, not an engine)");
    reject("function(){return 1}");
    reject("eval(1)");
    reject("g($0)");
    println!();
    println!("PASS");
}

fn show(src: &str, globals: &[HostGlobal<'_>], locals: &[Val], want: i32) {
    let wasm = qjs2wasm(src).unwrap_or_else(|e| panic!("qjs2wasm {src}: {}", e.message()));
    expect_i32(
        &eval_wasm(&wasm, globals, locals)
            .unwrap_or_else(|e| panic!("eval_wasm {src}: {}", e.message())),
        want,
        src,
    );
    expect_i32(
        &eval_qjs(src, globals, locals)
            .unwrap_or_else(|e| panic!("eval_qjs {src}: {}", e.message())),
        want,
        src,
    );
    println!("  {src:<12} -> {want}");
}

fn reject(src: &str) {
    match qjs2wasm(src) {
        Err(e) => println!("  {src:<24} Decode ({})", e.message()),
        Ok(_) => panic!("{src}: converter accepted full JS"),
    }
}

fn expect_i32(vals: &[Val], want: i32, what: &str) {
    match vals {
        [Val::I32(got)] if *got == want => {}
        _ => panic!("{what}: unexpected values, want i32 {want}"),
    }
}
