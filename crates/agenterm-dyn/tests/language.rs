//! Language-level tests: intern, bind, core forms, comparisons, arithmetic, logic.

use std::ffi::c_void;

use agenterm_dyn::{Dyn, DynError, MAX_BINDINGS, MAX_NAME_BYTES, MAX_SYMBOLS, Value};

#[test]
fn intern_is_stable() {
    let mut env = Dyn::new();
    let a = env.intern("ioctl").expect("intern ioctl");
    let b = env.intern("ioctl").expect("reuse ioctl");
    let c = env.intern("getpid").expect("intern getpid");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.index(), b.index());
}

#[test]
fn names_are_bounded_before_binding_or_set_rhs_evaluation() {
    let mut env = Dyn::new();
    let at_limit = "x".repeat(MAX_NAME_BYTES);
    env.bind(&at_limit, std::ptr::null_mut())
        .expect("255-byte binding name");
    let overlong = "x".repeat(MAX_NAME_BYTES + 1);
    assert_eq!(
        env.bind(&overlong, std::ptr::null_mut()),
        Err(DynError::NameTooLong {
            limit: MAX_NAME_BYTES,
        })
    );
    assert_eq!(
        env.eval(&format!("(set {overlong} (set touched 1))")),
        Err(DynError::NameTooLong {
            limit: MAX_NAME_BYTES,
        })
    );
    assert_eq!(
        env.eval("touched"),
        Err(DynError::UnknownVar("touched".into()))
    );
}

#[test]
fn nul_names_are_rejected_before_environment_state_changes() {
    let mut env = Dyn::new();
    assert_eq!(
        env.bind("bound\0name", std::ptr::null_mut()),
        Err(DynError::NameContainsNul)
    );
    env.bind("bound", std::ptr::null_mut())
        .expect("rejection must not add a binding");
    assert_eq!(env.intern("symbol\0name"), Err(DynError::NameContainsNul));
    assert_eq!(
        env.intern("symbol")
            .expect("rejection must not add a symbol")
            .index(),
        0
    );
    assert_eq!(
        env.eval("(set scripted\0name (set touched 1))"),
        Err(DynError::Parse("unexpected character `\0`".into()))
    );
    assert_eq!(
        env.eval("touched"),
        Err(DynError::UnknownVar("touched".into()))
    );
    assert_eq!(env.eval("bound"), Ok(Value::Ptr(0)));
}

#[test]
fn intern_is_bounded_and_existing_names_remain_reusable() {
    let mut env = Dyn::new();
    for index in 0..MAX_SYMBOLS {
        env.intern(&format!("sym_{index}"))
            .expect("symbol below limit");
    }
    assert_eq!(
        env.intern("overflow"),
        Err(DynError::StateLimit {
            resource: "symbols",
            limit: MAX_SYMBOLS,
        })
    );
    assert!(env.intern("sym_0").is_ok());
    assert_eq!(
        env.intern(&"x".repeat(MAX_NAME_BYTES + 1)),
        Err(DynError::NameTooLong {
            limit: MAX_NAME_BYTES,
        })
    );
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
fn safe_eval_rejects_direct_dlcall_before_native_dispatch() {
    let mut env = Dyn::new();
    assert_eq!(
        env.eval(r#"(dlcall "missing-library" "unused" "i32")"#),
        Err(DynError::NativeRequiresUnsafe)
    );
}

#[test]
fn safe_eval_rejects_nested_dlcall_before_any_set_side_effect() {
    let mut env = Dyn::new();
    assert_eq!(
        env.eval(r#"(do (set touched 1) (+ 1 (dlcall "missing-library" "unused" "i32")))"#),
        Err(DynError::NativeRequiresUnsafe)
    );
    assert_eq!(
        env.eval("touched"),
        Err(DynError::UnknownVar("touched".into()))
    );
}

#[test]
fn safe_eval_rejects_dead_branch_dlcall_before_any_set_side_effect() {
    let mut env = Dyn::new();
    assert_eq!(
        env.eval(r#"(do (set touched 1) (if 1 7 (dlcall "missing-library" "unused" "i32")))"#),
        Err(DynError::NativeRequiresUnsafe)
    );
    assert_eq!(
        env.eval("touched"),
        Err(DynError::UnknownVar("touched".into()))
    );
}

#[test]
fn unsafe_native_entry_executes_native_and_preserves_native_errors() {
    let mut env = Dyn::new();
    #[cfg(target_os = "linux")]
    let result = unsafe { env.eval_native(r#"(dlcall "libc.so.6" "getpid" "i32")"#) };
    #[cfg(target_os = "linux")]
    assert_eq!(result, Ok(Value::Int(i64::from(unsafe { libc::getpid() }))));

    #[cfg(target_os = "macos")]
    let result = unsafe { env.eval_native(r#"(dlcall "libSystem.B.dylib" "getpid" "i32")"#) };
    #[cfg(target_os = "macos")]
    assert_eq!(result, Ok(Value::Int(i64::from(unsafe { libc::getpid() }))));

    #[cfg(target_os = "windows")]
    let result =
        unsafe { env.eval_native(r#"(dlcall "kernel32.dll" "GetCurrentProcessId" "u32")"#) };
    #[cfg(target_os = "windows")]
    assert_eq!(
        result,
        Ok(Value::Int(i64::from(unsafe {
            windows_sys::Win32::System::Threading::GetCurrentProcessId()
        })))
    );

    let error = unsafe { env.eval_native(r#"(dlcall "missing-library" "unused" "i32")"#) };
    assert!(matches!(error, Err(DynError::Library(_))));
}

#[test]
fn bindings_are_bounded_and_existing_names_remain_replaceable() {
    let mut env = Dyn::new();
    for index in 0..MAX_BINDINGS {
        env.bind(&format!("slot_{index}"), std::ptr::null_mut())
            .expect("binding below the limit");
    }

    assert_eq!(
        env.bind("overflow", std::ptr::null_mut()),
        Err(DynError::StateLimit {
            resource: "bindings",
            limit: MAX_BINDINGS,
        })
    );
    env.bind("slot_0", std::ptr::null_mut())
        .expect("replacement at capacity");
}

#[test]
fn set_rejects_a_new_binding_before_rhs_side_effects_at_capacity() {
    let mut env = Dyn::new();
    for index in 0..MAX_BINDINGS {
        env.eval(&format!("(set slot_{index} 0)"))
            .expect("binding below the limit");
    }

    assert_eq!(
        env.eval("(set rejected (set touched 1))"),
        Err(DynError::StateLimit {
            resource: "bindings",
            limit: MAX_BINDINGS,
        })
    );
    assert_eq!(
        env.eval("touched"),
        Err(DynError::UnknownVar("touched".into()))
    );
    assert_eq!(env.eval("(set slot_0 1)"), Ok(Value::Int(1)));
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
