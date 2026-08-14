//! Error-path tests: parse failures, unknown symbols/forms, bad FFI types, missing libs.

use agenterm_dyn::{Dyn, DynError};

#[test]
fn parse_unclosed_list() {
    let mut env = Dyn::new();
    let err = env.eval("(do (set x 1").unwrap_err();
    assert!(matches!(err, DynError::Parse(_)));
}

#[test]
fn parse_trailing_tokens() {
    let mut env = Dyn::new();
    let err = env.eval("1 2").unwrap_err();
    assert!(matches!(err, DynError::Parse(_)));
}

#[test]
fn parse_bare_string_not_a_value() {
    let mut env = Dyn::new();
    let err = env.eval(r#""hello""#).unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn unknown_variable() {
    let mut env = Dyn::new();
    let err = env.eval("missing").unwrap_err();
    assert_eq!(err, DynError::UnknownVar("missing".into()));
}

#[test]
fn unknown_special_form() {
    let mut env = Dyn::new();
    let err = env.eval("(lambda x x)").unwrap_err();
    assert_eq!(err, DynError::UnknownForm("lambda".into()));
}

#[test]
fn arity_set() {
    let mut env = Dyn::new();
    let err = env.eval("(set x)").unwrap_err();
    assert!(matches!(err, DynError::Arity { form: "set", .. }));
}

#[test]
fn arity_if() {
    let mut env = Dyn::new();
    let err = env.eval("(if 1 2)").unwrap_err();
    assert!(matches!(err, DynError::Arity { form: "if", .. }));
}

#[test]
fn arity_dlcall() {
    let mut env = Dyn::new();
    let err = env.eval("(dlcall)").unwrap_err();
    assert!(matches!(err, DynError::Arity { form: "dlcall", .. }));
}

#[test]
fn bad_ffi_return_type() {
    let mut env = Dyn::new();
    let err = env
        .eval(r#"(dlcall "libc.so.6" "getpid" "notatype")"#)
        .unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
    assert!(err.to_string().contains("notatype"));
}

#[test]
fn bad_ffi_argument_type() {
    let mut env = Dyn::new();
    let err = env
        .eval(r#"(dlcall "libc.so.6" "getpid" "i32" "float" 0)"#)
        .unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn dlcall_odd_arg_pairs() {
    let mut env = Dyn::new();
    let err = env
        .eval(r#"(dlcall "libc.so.6" "getpid" "i32" "i32")"#)
        .unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn missing_library() {
    let mut env = Dyn::new();
    let err = env
        .eval(r#"(dlcall "libtotally_missing_agenterm_dyn_test.so" "foo" "i32")"#)
        .unwrap_err();
    assert!(matches!(err, DynError::Library(_)));
}

#[test]
fn missing_symbol() {
    let mut env = Dyn::new();
    let lib = if cfg!(target_os = "windows") {
        "kernel32.dll"
    } else if cfg!(target_os = "macos") {
        "libSystem.B.dylib"
    } else {
        "libc.so.6"
    };
    let script = format!(r#"(dlcall "{lib}" "agenterm_dyn_no_such_symbol_xyz" "i32")"#);
    let err = env.eval(&script).unwrap_err();
    assert!(matches!(err, DynError::DlCall(_)));
}
