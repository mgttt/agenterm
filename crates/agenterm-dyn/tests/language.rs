//! Language-level tests: intern, bind, core forms, comparisons, arithmetic, logic.

use std::ffi::c_void;

use agenterm_dyn::{Dyn, Value};

#[test]
fn intern_is_stable() {
    let mut env = Dyn::new();
    let a = env.intern("ioctl");
    let b = env.intern("ioctl");
    let c = env.intern("getpid");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.index(), b.index());
}

#[test]
fn set_and_lookup() {
    let mut env = Dyn::new();
    let v = env.eval("(do (set x 42) x)").expect("set/get should work");
    assert_eq!(v, Value::Int(42));
}

#[test]
fn if_form_branches() {
    let mut env = Dyn::new();
    assert_eq!(env.eval("(if 1 7 9)").expect("true branch"), Value::Int(7));
    assert_eq!(env.eval("(if 0 7 9)").expect("false branch"), Value::Int(9));
    assert_eq!(env.eval("(if 99 7 9)").expect("nonzero int"), Value::Int(7));
}

#[test]
fn do_sequence_returns_last() {
    let mut env = Dyn::new();
    let v = env
        .eval("(do (set a 1) (set b 2) (set c 3) c)")
        .expect("do sequence");
    assert_eq!(v, Value::Int(3));
}

#[test]
fn nested_lists_and_shadowing() {
    let mut env = Dyn::new();
    let v = env
        .eval(
            r#"
            (do
              (set x 1)
              (do
                (set x 10)
                x)
              x)
            "#,
        )
        .expect("nested do");
    assert_eq!(v, Value::Int(10));
}

#[test]
fn bind_ptr_roundtrip() {
    let mut slot: i64 = 0xDEAD_BEEF;
    let mut env = Dyn::new();
    env.bind("buf", (&mut slot as *mut i64).cast())
        .expect("bind ptr");
    let v = env.eval("buf").expect("lookup bound ptr");
    assert_eq!(v, Value::Ptr((&raw mut slot) as usize));
}

#[test]
fn bind_rejects_empty_name() {
    let mut env = Dyn::new();
    let err = env
        .bind("", std::ptr::null_mut::<c_void>())
        .expect_err("empty name");
    assert_eq!(err.to_string(), "binding name must not be empty");
}

#[test]
fn nil_is_falsy_ptr_is_truthy() {
    let mut slot: i64 = 1;
    let mut env = Dyn::new();
    env.bind("p", (&mut slot as *mut i64).cast())
        .expect("bind nonzero ptr");
    assert_eq!(env.eval("(if p 1 2)").expect("truthy ptr"), Value::Int(1));
    assert_eq!(env.eval("(if 0 1 2)").expect("falsy zero"), Value::Int(2));
}

#[test]
fn comparisons_on_ints() {
    let mut env = Dyn::new();
    assert_eq!(env.eval("(= 3 3)").unwrap(), Value::Int(1));
    assert_eq!(env.eval("(= 3 4)").unwrap(), Value::Int(0));
    assert_eq!(env.eval("(< 2 5)").unwrap(), Value::Int(1));
    assert_eq!(env.eval("(< 5 2)").unwrap(), Value::Int(0));
    assert_eq!(env.eval("(> 9 1)").unwrap(), Value::Int(1));
    assert_eq!(env.eval("(> 1 9)").unwrap(), Value::Int(0));
    assert_eq!(env.eval("(<= 2 2)").unwrap(), Value::Int(1));
    assert_eq!(env.eval("(<= 3 2)").unwrap(), Value::Int(0));
    assert_eq!(env.eval("(>= 2 2)").unwrap(), Value::Int(1));
    assert_eq!(env.eval("(>= 2 3)").unwrap(), Value::Int(0));
}

#[test]
fn not_inverts_truthiness() {
    let mut env = Dyn::new();
    assert_eq!(env.eval("(not 0)").unwrap(), Value::Int(1));
    assert_eq!(env.eval("(not 7)").unwrap(), Value::Int(0));
    assert_eq!(env.eval("(not (not 0))").unwrap(), Value::Int(0));
}

#[test]
fn comparisons_drive_if() {
    let mut env = Dyn::new();
    let t = env.eval("(if (= 2 (+ 1 1)) 10 20)").expect("if true");
    let f = env.eval("(if (> 1 2) 10 20)").expect("if false");
    assert_eq!(t, Value::Int(10));
    assert_eq!(f, Value::Int(20));
}

#[test]
fn and_short_circuits() {
    let mut env = Dyn::new();
    assert_eq!(env.eval("(and 1 2 3)").unwrap(), Value::Int(3));
    assert_eq!(env.eval("(and 1 0 99)").unwrap(), Value::Int(0));
    let v = env
        .eval("(do (set n 0) (and 0 (set n 9)) n)")
        .expect("and short-circuit");
    assert_eq!(v, Value::Int(0));
}

#[test]
fn or_short_circuits() {
    let mut env = Dyn::new();
    assert_eq!(env.eval("(or 0 0 7)").unwrap(), Value::Int(7));
    assert_eq!(env.eval("(or 1 0 7)").unwrap(), Value::Int(1));
    let v = env
        .eval("(do (set n 0) (or 1 (set n 9)) n)")
        .expect("or short-circuit");
    assert_eq!(v, Value::Int(0));
}

#[test]
fn fixnum_add_and_sub() {
    let mut env = Dyn::new();
    assert_eq!(env.eval("(+ 1 2 3)").unwrap(), Value::Int(6));
    assert_eq!(env.eval("(+ 42)").unwrap(), Value::Int(42));
    assert_eq!(env.eval("(- 10 3)").unwrap(), Value::Int(7));
    assert_eq!(env.eval("(- 10 3 2)").unwrap(), Value::Int(5));
    assert_eq!(env.eval("(- 5)").unwrap(), Value::Int(-5));
}

#[test]
fn repeat_runs_body_and_returns_last() {
    let mut env = Dyn::new();
    let v = env
        .eval("(do (set x 0) (repeat 3 (set x (+ x 1))) x)")
        .expect("repeat accum");
    assert_eq!(v, Value::Int(3));
    assert_eq!(env.eval("(repeat 0 99)").unwrap(), Value::Nil);
}

#[test]
fn nested_logic_without_dlcall() {
    let mut env = Dyn::new();
    let script = r#"
        (do
          (set limit 4)
          (set acc 0)
          (repeat limit
            (if (< acc 3)
              (set acc (+ acc 1))
              (set acc acc)))
          acc)
    "#;
    assert_eq!(env.eval(script.trim()).unwrap(), Value::Int(3));
}
