//! PR-A2: the tree-walk interpreter over Language 1, pure values only.
//!
//! No `std::fs`, no `compile_native`, no process fixtures — those are PR-A3.
//! Every test here must pass with a host that implements **nothing**, which is
//! what proves the core-type method surface is an interpreter builtin rather
//! than a `Host::call`.

use std::path::PathBuf;

use agenterm_rh::{Engine, Error, Value};

fn eval(source: &str) -> Value {
    // `sandboxed()` installs a host that implements *nothing*. Every test in
    // this file runs on it, which is what proves the core-type method surface
    // is an interpreter builtin rather than a `Host::call`.
    Engine::sandboxed()
        .eval(source)
        .unwrap_or_else(|error| panic!("eval failed for {source:?}: {error}"))
}

fn eval_err(source: &str) -> Error {
    Engine::sandboxed()
        .eval(source)
        .expect_err("expected this to fail")
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rh")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ---------------------------------------------------------------- fixtures

/// The A2 acceptance fixtures: real files from `fixtures/rh`, no FS, no host.
#[test]
fn for_range_fixture() {
    // 1 + 2 + 3 + 4
    assert_eq!(eval(&fixture("for-range.rh")), Value::Int(10));
}

#[test]
fn for_dyn_range_fixture() {
    // 1 + 2 + 3 with the bound read from a variable
    assert_eq!(eval(&fixture("for-dyn-range.rh")), Value::Int(6));
}

#[test]
fn break_continue_fixture() {
    // 1..10, skip 3, stop at 8 => 1+2+4+5+6+7
    assert_eq!(eval(&fixture("break-continue.rh")), Value::Int(25));
}

/// Pure map/array fixture: `#{}` literal, `.keys()`, `.len`, `for` over the
/// key array, `else if`. Touches no host surface at all.
#[test]
fn json_keys_fixture_runs_without_a_host() {
    assert_eq!(eval(&fixture("json-keys-probe.rh")), Value::Int(3));
}

// ------------------------------------------------------------- control flow

#[test]
fn if_else_chain() {
    assert_eq!(
        eval("fn entry() { let x = 2; if x == 1 { 10 } else if x == 2 { 20 } else { 30 } }"),
        Value::Int(20)
    );
}

#[test]
fn while_loop_with_compound_assign() {
    assert_eq!(
        eval("fn entry() { let x = 3; let n = 0; while x != 0 { x -= 1; n += 2; } n }"),
        Value::Int(6)
    );
}

#[test]
fn early_return_from_a_loop() {
    assert_eq!(
        eval("fn entry() { for i in 1..10 { if i == 4 { return i * 100; } } 0 }"),
        Value::Int(400)
    );
}

#[test]
fn inclusive_range_includes_its_end() {
    assert_eq!(
        eval("fn entry() { let n = 0; for i in 1..=3 { n += i; } n }"),
        Value::Int(6)
    );
}

#[test]
fn local_functions_are_callable_and_recursive() {
    assert_eq!(
        eval("fn add(a, b) { a + b } fn entry() { add(2, add(3, 4)) }"),
        Value::Int(9)
    );
    assert_eq!(
        eval("fn fact(n) { if n <= 1 { 1 } else { n * fact(n - 1) } } fn entry() { fact(5) }"),
        Value::Int(120)
    );
}

#[test]
fn try_catch_catches_a_throw() {
    assert_eq!(
        eval("fn entry() { try { throw 7; } catch (e) { 99 } }"),
        Value::Int(99)
    );
}

#[test]
fn short_circuit_does_not_evaluate_the_rhs() {
    // `1 / 0` would be a runtime error if it were evaluated.
    assert_eq!(
        eval("fn entry() { if false && (1 / 0) == 0 { 1 } else { 2 } }"),
        Value::Int(2)
    );
}

// ------------------------------------------------------------- value model

#[test]
fn arrays_and_maps_are_first_class() {
    assert_eq!(
        eval("fn entry() { let a = [1, 2, 3]; a[1] }"),
        Value::Int(2)
    );
    assert_eq!(
        eval(r#"fn entry() { let m = #{ a: 1, b: 2 }; m.b }"#),
        Value::Int(2)
    );
    assert_eq!(
        eval(r#"fn entry() { let m = #{ a: 1 }; m["a"] }"#),
        Value::Int(1)
    );
}

#[test]
fn maps_keep_insertion_order() {
    let value = eval(r#"fn entry() { let m = #{ z: 1, a: 2, m: 3 }; m.keys() }"#);
    assert_eq!(
        value,
        Value::Array(vec![
            Value::String("z".to_owned()),
            Value::String("a".to_owned()),
            Value::String("m".to_owned()),
        ])
    );
}

#[test]
fn index_and_field_assignment_write_through() {
    assert_eq!(
        eval("fn entry() { let a = [1, 2]; a[0] = 9; a[0] }"),
        Value::Int(9)
    );
    assert_eq!(
        eval(r#"fn entry() { let m = #{ n: 1 }; m.n += 4; m.n }"#),
        Value::Int(5)
    );
    // A key that is not present yet is created.
    assert_eq!(
        eval(r#"fn entry() { let m = #{}; m["k"] = true; m["k"] }"#),
        Value::Bool(true)
    );
}

// ------------------------------------- core-type methods are NOT Host::call

/// The load-bearing A2 claim: `Engine::new()` installs a host that implements
/// nothing, and these still work.
#[test]
fn string_methods_run_on_a_host_that_implements_nothing() {
    assert_eq!(
        eval(r#"fn entry() { "  hi  ".trim() }"#),
        Value::String("hi".to_owned())
    );
    assert_eq!(
        eval(r#"fn entry() { "AbC".to_lower() }"#),
        Value::String("abc".to_owned())
    );
    assert_eq!(
        eval(r#"fn entry() { "a,b,c".split(",").len }"#),
        Value::Int(3)
    );
    assert_eq!(
        eval(r#"fn entry() { "hello".contains("ell") }"#),
        Value::Bool(true)
    );
    assert_eq!(
        eval(r#"fn entry() { "hello".replace("l", "L") }"#),
        Value::String("heLLo".to_owned())
    );
    assert_eq!(eval(r#"fn entry() { "hello".len }"#), Value::Int(5));
}

#[test]
fn array_and_map_methods_run_without_a_host() {
    assert_eq!(
        eval("fn entry() { let a = []; a.push(1); a.push(2); a.len }"),
        Value::Int(2)
    );
    assert_eq!(
        eval(r#"fn entry() { let m = #{ a: 1 }; m.contains("a") }"#),
        Value::Bool(true)
    );
    assert_eq!(
        eval("fn entry() { let a = [1, 2, 3]; a.contains(2) }"),
        Value::Bool(true)
    );
}

#[test]
fn string_concat_stringifies_the_other_side() {
    assert_eq!(
        eval(r#"fn entry() { "n=" + 42 }"#),
        Value::String("n=42".to_owned())
    );
}

#[test]
fn type_of_reports_language_1_names() {
    assert_eq!(
        eval("fn entry() { type_of(1) }"),
        Value::String("int".to_owned())
    );
    assert_eq!(
        eval("fn entry() { type_of([]) }"),
        Value::String("array".to_owned())
    );
}

// ------------------------------------------------------------ fail-closed

/// Host surfaces still need a host. `Engine::new()` supplies `StdHost` and
/// these succeed; `Engine::sandboxed()` supplies nothing and they fail closed.
/// That difference is the sandbox story (D13).
#[test]
fn host_surfaces_fail_closed_without_a_host() {
    let error = eval_err(r#"fn entry() { std::fs::exists("/tmp") }"#);
    assert!(
        matches!(&error, Error::Unsupported { feature } if feature == "std::fs::exists"),
        "{error}"
    );
    // `print` is a Host method too, so a bare engine cannot print.
    assert!(matches!(
        eval_err(r#"fn entry() { print("x") }"#),
        Error::Unsupported { .. }
    ));
}

#[test]
fn unknown_names_are_unsupported_not_silently_unit() {
    assert!(matches!(
        eval_err("fn entry() { no_such_function(1) }"),
        Error::Unsupported { .. }
    ));
    assert!(matches!(
        eval_err(r#"fn entry() { "s".no_such_method() }"#),
        Error::Unsupported { .. }
    ));
}

/// Closures / `do` / `switch` are rejected at check time, not at runtime.
#[test]
fn non_language_1_syntax_fails_check() {
    for source in [
        "fn entry(n) { switch n { 1 => 0, _ => 1 } }",
        "fn entry() { let n = 0; do { n += 1; } while n < 3; n }",
    ] {
        let error = Engine::sandboxed().check(source).expect_err(source);
        assert!(
            matches!(&error, Error::Subset { .. }),
            "{source} => {error}"
        );
    }
}

#[test]
fn fuel_is_enforced() {
    let mut engine = Engine::sandboxed();
    engine.set_fuel(Some(50));
    let error = engine
        .eval("fn entry() { let n = 0; while n < 1000 { n += 1; } n }")
        .expect_err("should run out of fuel");
    assert!(matches!(error, Error::OutOfFuel), "{error}");
}

#[test]
fn cancellation_stops_a_long_loop() {
    let mut engine = Engine::sandboxed();
    engine.set_fuel(None);
    let handle = engine.cancel_handle();
    handle.cancel();
    let error = engine
        .eval("fn entry() { let n = 0; while n < 1000000 { n += 1; } n }")
        .expect_err("cancelled");
    assert!(matches!(error, Error::Cancelled), "{error}");
}

#[test]
fn division_by_zero_is_a_runtime_error_not_a_panic() {
    assert!(matches!(
        eval_err("fn entry() { 1 / 0 }"),
        Error::Runtime(_)
    ));
}

/// `rh::fail` is frozen as a builtin, so it works with no host. Note the
/// deliberate divergence from AOT, where it is `RH_HOST_UTILITY_FAIL` and
/// evaluates to the sentinel int `-5`; the interpreter raises instead.
#[test]
fn rh_fail_raises_without_a_host() {
    let error = eval_err(r#"fn entry() { rh::fail("boom") }"#);
    assert!(
        matches!(&error, Error::Host(message) if message == "boom"),
        "{error}"
    );
}

/// The failure arm of a real fixture: reachable, and it reports its reason
/// rather than dying as an unknown name.
#[test]
fn fixture_failure_arms_report_their_reason() {
    let source = fixture("json-keys-probe.rh").replace("if obj.keys().len != 2", "if true");
    let error = Engine::sandboxed()
        .eval(&source)
        .expect_err("forced failure arm");
    assert!(
        matches!(&error, Error::Host(message) if message == "keys_len"),
        "{error}"
    );
}
