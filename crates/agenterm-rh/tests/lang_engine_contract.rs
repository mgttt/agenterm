//! PR-A1 gate tests for the Language-1 embedder surface.
//!
//! These are the checks the design's PR-A1 calls for: `Engine::eval` of
//! `fn entry() { 42 }` runs through the owned `Send` IR, `compile()` is
//! `Unsupported`, every `Host` method defaults to failing closed, and
//! `Engine` really is `Send`.

use std::thread;

use agenterm_rh::{Engine, Error, Host, NullHost, Scope, Value, exit_from_int};

#[test]
fn eval_entry_returns_its_int() {
    let mut engine = Engine::new();
    let value = engine.eval("fn entry() { 42 }").expect("eval");
    assert_eq!(value, Value::Int(42));
}

#[test]
fn eval_honours_an_explicit_return() {
    let mut engine = Engine::new();
    assert_eq!(
        engine.eval("fn entry() { return 7; }").expect("eval"),
        Value::Int(7)
    );
}

#[test]
fn eval_reads_scope_variables() {
    let mut engine = Engine::new();
    let mut scope = Scope::new();
    scope.set("seed", Value::Int(9));
    let value = engine
        .eval_with_scope("fn entry() { seed }", &mut scope)
        .expect("eval");
    assert_eq!(value, Value::Int(9));
}

/// D17. If this ever fails, rh cannot be embedded in a threaded host, which is
/// the whole reason `check` lowers `rhai::AST` into an owned IR and drops it.
#[test]
fn engine_is_send_and_actually_moves_across_a_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<Engine>();
    assert_send::<Scope>();
    assert_send::<Value>();

    let mut engine = Engine::new();
    let value = thread::spawn(move || engine.eval("fn entry() { 1 }"))
        .join()
        .expect("thread")
        .expect("eval");
    assert_eq!(value, Value::Int(1));
}

/// The reserved AOT/JIT seam must stay closed on default builds (D10), or
/// embedders would start depending on rustc.
#[test]
fn compile_is_unsupported_on_default_builds() {
    let mut engine = Engine::new();
    let error = engine.compile("fn entry() { 0 }").expect_err("reserved");
    assert!(
        matches!(&error, Error::Unsupported { feature } if feature == "compile"),
        "{error}"
    );
}

/// D13: a host that implements nothing fails closed on everything.
#[test]
fn null_host_is_the_fail_closed_default() {
    let mut host = NullHost;
    assert!(matches!(host.print("x"), Err(Error::Unsupported { .. })));
    assert!(matches!(host.args_len(), Err(Error::Unsupported { .. })));
    assert!(matches!(host.arg(0), Err(Error::Unsupported { .. })));
    assert!(matches!(
        host.call("std::fs::exists", &[]),
        Err(Error::Unsupported { .. })
    ));
}

/// `Engine::check` is strict Language 1 — no `compat_validate` bypass. The
/// AgenTerm `check()` entry keeps that bypass and is asserted separately.
#[test]
fn engine_check_is_strict_where_agenterm_check_is_compatible() {
    // `switch` is rejected by the strict subset but waved through by
    // `compat_validate`, which only rejects `eval`.
    let source = "fn entry(n) { switch n { 1 => 0, _ => 1 } }";
    let engine = Engine::new();
    let error = engine.check(source).expect_err("switch is not Language 1");
    assert!(
        matches!(&error, Error::Subset { code, .. } if *code == "RH_SUBSET_NO_LOOP"),
        "{error}"
    );
    // Same source through the AgenTerm-facing checker: the compat bypass means
    // it is accepted there. That asymmetry is deliberate (design §"AgenTerm
    // as embedder"), and PR-A6 does not silently change it.
    agenterm_rh::check(source).expect("AgenTerm compat check still accepts it");
}

/// Language 1 has no fleet grammar: a `fleet.*` name must never surface as
/// `RH_SUBSET_FLEET_SHAPE` from the product checker.
#[test]
fn engine_check_never_reports_fleet_shape() {
    let engine = Engine::new();
    if let Err(error) = engine.check("fn entry() { fleet.protocol.info(); 1 }") {
        assert!(
            !error.to_string().contains("RH_SUBSET_FLEET_SHAPE"),
            "product check must not run fleet-shape validation: {error}"
        );
    }
}

#[test]
fn exit_mapping_matches_script_exit_code() {
    assert_eq!(exit_from_int(0), 0);
    assert_eq!(exit_from_int(3), 3);
    assert_eq!(exit_from_int(300), 1);
}

/// A custom host is how AgenTerm will inject `fleet.*` in PR-D1.
#[test]
fn a_custom_host_can_override_only_what_it_offers() {
    struct Recorder(Vec<String>);
    impl Host for Recorder {
        fn print(&mut self, text: &str) -> Result<(), Error> {
            self.0.push(text.to_owned());
            Ok(())
        }
    }
    let engine = Engine::new_with_host(Recorder(Vec::new()));
    engine.check("fn entry() { 0 }").expect("check");
}
