//! Error-path tests: parse failures, unknown symbols/forms, bad FFI types, language errors.

use std::ffi::c_void;

use agenterm_dyn::{Dyn, DynError, REPEAT_MAX};

fn eval_native(env: &mut Dyn, source: &str) -> Result<agenterm_dyn::Value, DynError> {
    // SAFETY: error fixtures intentionally exercise native validation only.
    unsafe { env.eval_native(source) }
}

#[test]
fn parse_unclosed_list() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "(do (set x 1").unwrap_err();
    assert!(matches!(err, DynError::Parse(_)));
}

#[test]
fn parse_trailing_tokens() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "1 2").unwrap_err();
    assert!(matches!(err, DynError::Parse(_)));
}

#[test]
fn parse_rejects_excessively_nested_legal_sexpression_at_public_eval_boundary() {
    // This is deliberately much deeper than a practical bounded parser budget.
    // Keep the assertion at Dyn::eval so callers are protected before recursive
    // evaluation can consume an unbounded call stack.
    const EXCESSIVE_NESTING: usize = 4_096;

    let source = format!(
        "{}0{}",
        "(do ".repeat(EXCESSIVE_NESTING),
        ")".repeat(EXCESSIVE_NESTING)
    );
    let mut env = Dyn::new();
    let err = eval_native(&mut env, &source).unwrap_err();
    assert!(matches!(err, DynError::Parse(_)));
}

#[test]
fn parse_resource_rejection_at_public_eval_boundary_has_no_side_effects() {
    // One outer list plus `(set touched 1)` and `do` is six nodes; the trailing scalar
    // expressions make the source exceed the parser's fixed AST budget before
    // eval can run the preceding set form.
    const EXCESSIVE_AST_NODES: usize = 4_097;
    const PREFIX_NODES: usize = 6;
    let source = format!(
        "(do (set touched 1) {} )",
        "1 ".repeat(EXCESSIVE_AST_NODES - PREFIX_NODES)
    );

    let mut env = Dyn::new();
    assert_eq!(
        eval_native(&mut env, &source),
        Err(DynError::Parse(
            "maximum AST node count (4096) exceeded".into()
        ))
    );
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn parse_bare_string_not_a_value() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, r#""hello""#).unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn unknown_variable() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "missing").unwrap_err();
    assert_eq!(err, DynError::UnknownVar("missing".into()));
}

#[test]
fn unknown_special_form() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "(lambda x x)").unwrap_err();
    assert_eq!(err, DynError::UnknownForm("lambda".into()));
}

#[test]
fn arity_set() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "(set x)").unwrap_err();
    assert!(matches!(err, DynError::Arity { form: "set", .. }));
}

#[test]
fn arity_if() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "(if 1 2)").unwrap_err();
    assert!(matches!(err, DynError::Arity { form: "if", .. }));
}

#[test]
fn arity_dlcall() {
    for (source, got) in [
        ("(dlcall)", 0),
        (r#"(dlcall "lib")"#, 1),
        (r#"(dlcall "lib" "symbol")"#, 2),
    ] {
        let mut env = Dyn::new();
        assert_eq!(
            eval_native(&mut env, source).unwrap_err(),
            DynError::Arity {
                form: "dlcall",
                expected: 3,
                got,
            }
        );
    }
}

#[test]
fn bad_ffi_return_type() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, r#"(dlcall "libc.so.6" "getpid" "notatype")"#).unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
    assert!(err.to_string().contains("notatype"));
}

#[test]
fn bad_ffi_argument_type() {
    let mut env = Dyn::new();
    let err =
        eval_native(&mut env, r#"(dlcall "libc.so.6" "getpid" "i32" "float" 0)"#).unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn dlcall_rejects_float_struct_and_varargs_types() {
    for unsupported in ["f32", "f64", "float", "double", "struct", "..."] {
        for script in [
            format!(r#"(dlcall "missing-library-for-type-validation" "unused" "{unsupported}")"#),
            format!(
                r#"(dlcall "missing-library-for-type-validation" "unused" "i32" "{unsupported}" 0)"#
            ),
        ] {
            let mut env = Dyn::new();
            let err = eval_native(&mut env, &script).unwrap_err();
            assert!(matches!(err, DynError::Type(_)), "{unsupported}: {err}");
            assert!(
                err.to_string().contains(unsupported),
                "{unsupported}: {err}"
            );
        }
    }
}

#[test]
fn dlcall_rejects_unknown_signature_types_before_arguments_or_library_load() {
    for unsupported in ["f32", "struct"] {
        let expected = DynError::Type(format!(
            "unsupported dlcall type `{unsupported}`; only void/integer/pointer types are supported"
        ));

        let mut return_env = Dyn::new();
        let return_script = format!(
            r#"(dlcall "missing-library-for-unknown-return" "unused" "{unsupported}"
                "i32" (set touched 1))"#
        );
        assert_eq!(eval_native(&mut return_env, &return_script), Err(expected));
        assert_eq!(
            eval_native(&mut return_env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );

        let mut argument_env = Dyn::new();
        let argument_script = format!(
            r#"(dlcall "missing-library-for-unknown-argument" "unused" "i32"
                "i32" (set touched 1) "{unsupported}" 0)"#
        );
        assert_eq!(
            eval_native(&mut argument_env, &argument_script),
            Err(DynError::Type(format!(
                "unsupported dlcall type `{unsupported}`; only void/integer/pointer types are supported"
            )))
        );
        assert_eq!(
            eval_native(&mut argument_env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_unknown_argument_types_before_arguments_or_library_load() {
    for unsupported in ["f64", "u128"] {
        let mut env = Dyn::new();
        let script = format!(
            r#"(dlcall "missing-library-for-unknown-argument-type" "unused" "i32"
                "i32" (set touched 1) "{unsupported}" 0)"#
        );
        assert_eq!(
            eval_native(&mut env, &script),
            Err(DynError::Type(format!(
                "unsupported dlcall type `{unsupported}`; only void/integer/pointer types are supported"
            )))
        );
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_usize_type_before_arguments_or_library_load() {
    let expected = || {
        DynError::Type(
            "unsupported dlcall type `usize`; only void/integer/pointer types are supported".into(),
        )
    };
    for script in [
        r#"(dlcall "missing-library-for-usize-return" "unused" "usize"
            "i32" (set touched 1))"#,
        r#"(dlcall "missing-library-for-usize-argument" "unused" "i32"
            "i32" (set touched 1) "usize" 0)"#,
    ] {
        let mut env = Dyn::new();
        assert_eq!(eval_native(&mut env, script), Err(expected()));
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_isize_type_before_arguments_or_library_load() {
    let expected = || {
        DynError::Type(
            "unsupported dlcall type `isize`; only void/integer/pointer types are supported".into(),
        )
    };
    for script in [
        r#"(dlcall "missing-library-for-isize-return" "unused" "isize"
            "i32" (set touched 1))"#,
        r#"(dlcall "missing-library-for-isize-argument" "unused" "i32"
            "i32" (set touched 1) "isize" 0)"#,
    ] {
        let mut env = Dyn::new();
        assert_eq!(eval_native(&mut env, script), Err(expected()));
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_c_abi_aliases_before_arguments_or_library_load() {
    // These familiar C spellings have target-dependent widths or signedness.  dlcall
    // deliberately exposes only fixed-width integer and pointer ABI classes.
    for unsupported in [
        "long",
        "unsigned long",
        "size_t",
        "ssize_t",
        "intptr_t",
        "uintptr_t",
        "off_t",
        "mode_t",
        "pid_t",
        "uid_t",
        "gid_t",
        "time_t",
        "socklen_t",
        "nfds_t",
        "char",
        "short",
        "int",
        "ptrdiff_t",
        "rlim_t",
        "dev_t",
        "ino_t",
        "clockid_t",
        "sigset_t",
        "blkcnt_t",
        "nlink_t",
        "suseconds_t",
        "useconds_t",
        "fsblkcnt_t",
        "pthread_t",
    ] {
        let expected = || {
            DynError::Type(format!(
                "unsupported dlcall type `{unsupported}`; only void/integer/pointer types are supported"
            ))
        };
        for script in [
            format!(
                r#"(dlcall "missing-library-for-c-alias-return" "unused" "{unsupported}"
                    "i32" (set touched 1))"#
            ),
            format!(
                r#"(dlcall "missing-library-for-c-alias-argument" "unused" "i32"
                    "i32" (set touched 1) "{unsupported}" 0)"#
            ),
        ] {
            let mut env = Dyn::new();
            assert_eq!(
                eval_native(&mut env, &script),
                Err(expected()),
                "{unsupported}"
            );
            assert_eq!(
                eval_native(&mut env, "touched").unwrap_err(),
                DynError::UnknownVar("touched".into()),
                "{unsupported}"
            );
        }
    }
}

#[test]
fn dlcall_rejects_bool_type_before_arguments_or_library_load() {
    let expected = || {
        DynError::Type(
            "unsupported dlcall type `bool`; only void/integer/pointer types are supported".into(),
        )
    };
    for script in [
        r#"(dlcall "missing-library-for-bool-return" "unused" "bool"
            "i32" (set touched 1))"#,
        r#"(dlcall "missing-library-for-bool-argument" "unused" "i32"
            "i32" (set touched 1) "bool" 0)"#,
    ] {
        let mut env = Dyn::new();
        assert_eq!(eval_native(&mut env, script), Err(expected()));
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_validates_entire_signature_before_evaluating_arguments() {
    let mut env = Dyn::new();
    let script = r#"(dlcall "missing-library-for-signature-validation" "unused" "i32"
        "i32" (set touched 1) "struct" 0)"#;
    let err = eval_native(&mut env, script).unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn dlcall_rejects_void_argument_before_arguments_or_library_load() {
    let mut env = Dyn::new();
    let script = r#"(dlcall "missing-library-for-void-argument" "unused" "void"
        "i32" (set touched 1) "void" 0)"#;
    assert_eq!(
        eval_native(&mut env, script),
        Err(DynError::Type("void cannot be an argument type".into()))
    );
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn dlcall_rejects_argument_signature_arity_mismatch_before_evaluation_or_load() {
    let expected = "dlcall expects argtype/arg pairs after return type";
    for script in [
        r#"(dlcall "missing-library-for-short-arguments" "unused" "i32"
            "i32" (set touched 1) "i32")"#,
        r#"(dlcall "missing-library-for-extra-argument" "unused" "i32"
            "i32" (set touched 1) 0)"#,
    ] {
        let mut env = Dyn::new();
        assert_eq!(
            eval_native(&mut env, script),
            Err(DynError::Type(expected.into()))
        );
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_more_than_six_arguments() {
    let mut env = Dyn::new();
    let err = eval_native(
        &mut env,
        r#"(dlcall "libc.so.6" "getpid" "i32"
                "i32" 0 "i32" 0 "i32" 0 "i32" 0
                "i32" 0 "i32" 0 "i32" 0)"#,
    )
    .unwrap_err();
    assert!(matches!(err, DynError::DlCall(_)));
    assert!(err.to_string().contains("fixed limit of 6"));
}

#[test]
fn dlcall_rejects_empty_library_or_symbol_before_arguments_or_load() {
    for (script, expected) in [
        (
            r#"(dlcall "" "unused" "i32" "i32" (set touched 1))"#,
            DynError::Library("library name must not be empty".into()),
        ),
        (
            r#"(dlcall "missing-library-for-empty-symbol" "" "i32"
                "i32" (set touched 1))"#,
            DynError::DlCall("symbol name must not be empty".into()),
        ),
    ] {
        let mut env = Dyn::new();
        assert_eq!(eval_native(&mut env, script), Err(expected));
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_blank_library_or_symbol_before_arguments_or_load() {
    for (script, expected) in [
        (
            "(dlcall \" \t \" \"unused\" \"i32\" \"i32\" (set touched 1))",
            DynError::Library("library name must not be blank".into()),
        ),
        (
            "(dlcall \"missing-library-for-blank-symbol\" \" \t \" \"i32\" \"i32\" (set touched 1))",
            DynError::DlCall("symbol name must not be blank".into()),
        ),
    ] {
        let mut env = Dyn::new();
        assert_eq!(eval_native(&mut env, script), Err(expected));
        assert_eq!(
            eval_native(&mut env, "touched").unwrap_err(),
            DynError::UnknownVar("touched".into())
        );
    }
}

#[test]
fn dlcall_rejects_overlong_library_before_arguments_or_load() {
    let library = "x".repeat(256);
    let script = format!(r#"(dlcall "{library}" "unused" "i32" "i32" (set touched 1))"#);
    let mut env = Dyn::new();
    assert_eq!(
        eval_native(&mut env, &script),
        Err(DynError::Library(
            "library name exceeds 255-byte limit".into()
        ))
    );
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn dlcall_rejects_overlong_symbol_before_arguments_or_load() {
    let symbol = "x".repeat(256);
    let script = format!(
        r#"(dlcall "missing-library-for-overlong-symbol" "{symbol}" "i32"
            "i32" (set touched 1))"#
    );
    let mut env = Dyn::new();
    assert_eq!(
        eval_native(&mut env, &script),
        Err(DynError::DlCall(
            "symbol name exceeds 255-byte limit".into()
        ))
    );
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn dlcall_accepts_255_byte_library_and_symbol_names_until_native_processing() {
    let library = "x".repeat(255);
    let mut library_env = Dyn::new();
    let library_script = format!(r#"(dlcall "{library}" "unused" "i32" "i32" (set touched 1))"#);
    let library_err = eval_native(&mut library_env, &library_script).unwrap_err();
    assert!(matches!(library_err, DynError::Library(message) if message.starts_with(&library)));
    assert_eq!(
        eval_native(&mut library_env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );

    let symbol = "x".repeat(255);
    let native_library = if cfg!(target_os = "macos") {
        "libSystem.B.dylib"
    } else if cfg!(target_os = "windows") {
        "kernel32.dll"
    } else {
        "libc.so.6"
    };
    let mut symbol_env = Dyn::new();
    let symbol_script =
        format!(r#"(dlcall "{native_library}" "{symbol}" "i32" "i32" (set touched 1))"#);
    let symbol_err = eval_native(&mut symbol_env, &symbol_script).unwrap_err();
    // Reaching the native resolver is the contract here.  The loader's diagnostic text is
    // platform-specific: macOS does not prefix its `dlsym` error with the symbol name.
    assert!(matches!(symbol_err, DynError::DlCall(_)));
    assert_ne!(
        symbol_err,
        DynError::DlCall("symbol name exceeds 255-byte limit".into())
    );
    assert_eq!(
        eval_native(&mut symbol_env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn dlcall_rejects_nul_library_before_arguments_or_library_load() {
    let mut env = Dyn::new();
    let script = "(dlcall \"bad\0library\" \"unused\" \"i32\" \"i32\" (set touched 1))";
    assert_eq!(
        eval_native(&mut env, script),
        Err(DynError::Library(
            "library name contains interior NUL".into()
        ))
    );
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn dlcall_rejects_nul_symbol_before_arguments_or_library_load() {
    let mut env = Dyn::new();
    let script = "(dlcall \"missing-library-for-nul-symbol\" \"bad\0symbol\" \"i32\" \"i32\" (set touched 1))";
    assert_eq!(
        eval_native(&mut env, script),
        Err(DynError::DlCall("symbol name contains interior NUL".into()))
    );
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn missing_library() {
    let mut env = Dyn::new();
    let err = eval_native(
        &mut env,
        r#"(dlcall "libtotally_missing_agenterm_dyn_test.so" "foo" "i32"
            "i32" (set touched 1))"#,
    )
    .unwrap_err();
    assert!(matches!(err, DynError::Library(_)));
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
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
    let script = format!(
        r#"(dlcall "{lib}" "agenterm_dyn_no_such_symbol_xyz" "i32" "i32" (set touched 1))"#
    );
    let err = eval_native(&mut env, &script).unwrap_err();
    assert!(matches!(err, DynError::DlCall(_)));
    assert_eq!(
        eval_native(&mut env, "touched").unwrap_err(),
        DynError::UnknownVar("touched".into())
    );
}

#[test]
fn comparison_requires_two_int_operands() {
    let mut env = Dyn::new();
    env.bind("p", std::ptr::dangling_mut::<c_void>())
        .expect("bind ptr");
    let err = eval_native(&mut env, "(= 1 p)").unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn comparison_arity_mismatch() {
    let mut env = Dyn::new();
    let err = eval_native(&mut env, "(= 1)").unwrap_err();
    assert!(matches!(
        err,
        DynError::Arity {
            form: "=",
            expected: 2,
            got: 1
        }
    ));
}

#[test]
fn and_and_or_require_at_least_one_operand() {
    let mut env = Dyn::new();
    assert!(matches!(
        eval_native(&mut env, "(and)").unwrap_err(),
        DynError::Arity {
            form: "and",
            expected: 1,
            got: 0
        }
    ));
    assert!(matches!(
        eval_native(&mut env, "(or)").unwrap_err(),
        DynError::Arity {
            form: "or",
            expected: 1,
            got: 0
        }
    ));
}

#[test]
fn not_requires_exactly_one_operand() {
    let mut env = Dyn::new();
    assert!(matches!(
        eval_native(&mut env, "(not)").unwrap_err(),
        DynError::Arity {
            form: "not",
            expected: 1,
            got: 0
        }
    ));
    assert!(matches!(
        eval_native(&mut env, "(not 0 1)").unwrap_err(),
        DynError::Arity {
            form: "not",
            expected: 1,
            got: 2
        }
    ));
}

#[test]
fn arithmetic_type_and_overflow_errors() {
    let mut env = Dyn::new();
    env.bind("z", std::ptr::null_mut::<c_void>()).expect("bind");
    assert!(matches!(
        eval_native(&mut env, "(+ 1 z)").unwrap_err(),
        DynError::Type(_)
    ));
    let huge = format!("(+ {} 1)", i64::MAX);
    let err = eval_native(&mut env, &huge).unwrap_err();
    assert!(matches!(err, DynError::Type(_)));
}

#[test]
fn repeat_rejects_negative_and_over_cap() {
    let mut env = Dyn::new();
    let neg = eval_native(&mut env, "(repeat -1 1)").unwrap_err();
    assert!(matches!(neg, DynError::Type(_)));

    let over = eval_native(&mut env, &format!("(repeat {} 1)", REPEAT_MAX + 1)).unwrap_err();
    assert!(matches!(over, DynError::Type(_)));
}

#[test]
fn repeat_arity_mismatch() {
    let mut env = Dyn::new();
    assert!(matches!(
        eval_native(&mut env, "(repeat 1)").unwrap_err(),
        DynError::Arity {
            form: "repeat",
            expected: 2,
            got: 1
        }
    ));
}
