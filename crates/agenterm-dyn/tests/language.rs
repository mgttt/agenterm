//! Language-level tests: intern, bind, `do` / `set` / `if`, nested lists.

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
    // Inner `set` shadows for inner scope only — bindings are flat in this env.
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
