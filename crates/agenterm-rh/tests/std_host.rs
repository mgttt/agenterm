//! PR-A3: `StdHost` implements the Language-1 name allowlist with `std` only.
//!
//! No Fleet, no GUI/clipboard/task, no `agenterm-platform`, no rustc. The
//! process fixtures here run real programs from `tempfile` scratch dirs.

use std::path::{Path, PathBuf};

use agenterm_rh::{Engine, Error, StdHost, Value};

fn engine_with_args(args: &[&str]) -> Engine {
    let host = StdHost::new().with_args(args.iter().map(|a| (*a).to_owned()).collect());
    Engine::new_with_host(host)
}

fn eval(source: &str) -> Value {
    Engine::new()
        .eval(source)
        .unwrap_or_else(|error| panic!("eval failed for {source:?}: {error}"))
}

fn eval_in(dir: &Path, source: &str) -> Value {
    let previous = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(dir).expect("chdir");
    let result = Engine::new().eval(source);
    std::env::set_current_dir(previous).expect("restore cwd");
    result.unwrap_or_else(|error| panic!("eval failed: {error}"))
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rh")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A program that exists on every supported cell, with an argument that makes
/// it print something predictable.
fn echo_program() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("/bin/echo", "")
    }
}

// ------------------------------------------------------------------ fs/env

#[test]
fn std_fs_exists_fixture() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("Cargo.toml"), "[package]").expect("write");
    assert_eq!(
        eval_in(dir.path(), &fixture("std-fs-exists-probe.rh")),
        Value::Int(1)
    );

    let empty = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        eval_in(empty.path(), &fixture("std-fs-exists-probe.rh")),
        Value::Int(0)
    );
}

#[test]
fn env_has_get_fixture() {
    // PATH is set in every environment this runs in; the fixture returns its
    // length, so any positive number means has+get both worked.
    let value = eval(&fixture("env-has-get-probe.rh"));
    match value {
        Value::Int(len) => assert!(len > 0, "PATH should be non-empty, got {len}"),
        other => panic!("expected an int, got {other:?}"),
    }
}

#[test]
fn fs_round_trip_write_read_metadata_remove() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("note.txt");
    let path = path.display().to_string().replace('\\', "/");

    assert_eq!(
        eval(&format!(
            r#"fn entry() {{ std::fs::write("{path}", "hello"); std::fs::read_to_string("{path}") }}"#
        )),
        Value::String("hello".to_owned())
    );
    assert_eq!(
        eval(&format!(
            r#"fn entry() {{ let m = std::fs::metadata("{path}"); m.len }}"#
        )),
        Value::Int(5)
    );
    assert_eq!(
        eval(&format!(
            r#"fn entry() {{ let m = std::fs::metadata("{path}"); m.is_file }}"#
        )),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&format!(
            r#"fn entry() {{ std::fs::remove_file("{path}"); std::fs::exists("{path}") }}"#
        )),
        Value::Bool(false)
    );
}

#[test]
fn fs_read_is_capped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("big.bin");
    std::fs::write(&path, vec![0_u8; 4096]).expect("write");
    let path = path.display().to_string().replace('\\', "/");

    let host = StdHost::new().with_fs_read_cap(16);
    let error = Engine::new_with_host(host)
        .eval(&format!(
            r#"fn entry() {{ std::fs::read_to_string("{path}") }}"#
        ))
        .expect_err("over the cap");
    assert!(
        matches!(&error, Error::Host(message) if message.contains("cap")),
        "{error}"
    );
}

// ---------------------------------------------------------------- DirEntry

#[test]
fn direntry_fixtures_walk_a_real_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), "aa").expect("write");
    std::fs::write(dir.path().join("b.txt"), "bbb").expect("write");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    let arg = dir.path().display().to_string();

    // file_name: three entries, all with non-empty names
    let mut engine = engine_with_args(&[&arg]);
    assert_eq!(
        engine
            .eval(&fixture("direntry-file-name-probe.rh"))
            .expect("eval"),
        Value::Int(3)
    );

    // is_file: two of the three
    let mut engine = engine_with_args(&[&arg]);
    assert_eq!(
        engine
            .eval(&fixture("direntry-is-file-probe.rh"))
            .expect("eval"),
        Value::Int(2)
    );

    // metadata: total bytes (2 + 3) plus the oldest mtime in millis
    let mut engine = engine_with_args(&[&arg]);
    match engine
        .eval(&fixture("direntry-metadata-probe.rh"))
        .expect("eval")
    {
        Value::Int(total) => assert!(total > 5, "expected bytes+millis, got {total}"),
        other => panic!("expected an int, got {other:?}"),
    }
}

/// The `args` object is `Host::args_len` / `Host::arg`, not a variable.
#[test]
fn args_object_is_host_backed() {
    let mut engine = engine_with_args(&["one", "two"]);
    assert_eq!(
        engine.eval("fn entry() { args.len }").expect("eval"),
        Value::Int(2)
    );
    let mut engine = engine_with_args(&["one", "two"]);
    assert_eq!(
        engine.eval("fn entry() { args[1] }").expect("eval"),
        Value::String("two".to_owned())
    );
}

#[test]
fn direntry_fixture_fails_without_its_argument() {
    // No args => the fixture's own `rh::fail` arm, per D25 a raised Error::Host.
    let error = Engine::new()
        .eval(&fixture("direntry-file-name-probe.rh"))
        .expect_err("missing DIRECTORY");
    assert!(
        matches!(&error, Error::Host(message) if message.contains("DIRECTORY")),
        "{error}"
    );
}

// ----------------------------------------------------------------- process

#[test]
fn command_arg_fixture_builds_without_spawning() {
    // `command.arg(..)` mutates the host-side Command; nothing is spawned, so
    // the fixture's fake "tool" program never has to exist.
    assert_eq!(eval(&fixture("command-arg-probe.rh")), Value::Int(0));
}

#[test]
fn process_kill_fixture_is_best_effort() {
    // PID 4242 almost certainly is not ours; `kill` reports false rather than
    // failing the script.
    assert_eq!(eval(&fixture("process-kill-probe.rh")), Value::Int(0));
}

#[test]
fn command_output_captures_a_real_program() {
    let (program, flag) = echo_program();
    let source = if flag.is_empty() {
        format!(
            r#"fn entry() {{ let c = std::process::command("{program}"); c.arg("hi"); let out = c.output(); out.stdout_text }}"#
        )
    } else {
        format!(
            r#"fn entry() {{ let c = std::process::command("{program}"); c.arg("{flag}"); c.arg("echo hi"); let out = c.output(); out.stdout_text }}"#
        )
    };
    match eval(&source) {
        Value::String(text) => assert_eq!(text.trim(), "hi"),
        other => panic!("expected stdout text, got {other:?}"),
    }
}

#[test]
fn command_output_reports_success_and_exit_code() {
    let (program, flag) = echo_program();
    let build = if flag.is_empty() {
        format!(r#"let c = std::process::command("{program}"); c.arg("x");"#)
    } else {
        format!(r#"let c = std::process::command("{program}"); c.arg("{flag}"); c.arg("echo x");"#)
    };
    assert_eq!(
        eval(&format!(
            "fn entry() {{ {build} let o = c.output(); o.success }}"
        )),
        Value::Bool(true)
    );
    assert_eq!(
        eval(&format!(
            "fn entry() {{ {build} let o = c.output(); o.exit_code }}"
        )),
        Value::Int(0)
    );
}

#[test]
fn child_stdout_fixture_shape_runs_against_a_real_program() {
    let (program, flag) = echo_program();
    // Same shape as `child-stdout-probe.rh`, with a program that exists.
    let source = if flag.is_empty() {
        format!(
            r#"fn entry() {{ let c = std::process::command("{program}"); c.arg("hi"); let ch = c.start(); ch.stdout; 0 }}"#
        )
    } else {
        format!(
            r#"fn entry() {{ let c = std::process::command("{program}"); c.arg("{flag}"); c.arg("echo hi"); let ch = c.start(); ch.stdout; 0 }}"#
        )
    };
    assert_eq!(eval(&source), Value::Int(0));
}

/// The fixture as shipped names a program that does not exist, so `start()`
/// must fail cleanly rather than panic.
#[test]
fn child_stdout_fixture_fails_cleanly_on_a_missing_program() {
    let error = Engine::new()
        .eval(&fixture("child-stdout-probe.rh"))
        .expect_err("`tool` does not exist");
    assert!(matches!(&error, Error::Host(_)), "{error}");
}

#[test]
fn process_caps_are_enforced() {
    let (program, _) = echo_program();
    let args = (0..300)
        .map(|i| format!(r#"c.arg("{i}");"#))
        .collect::<Vec<_>>()
        .join(" ");
    let error = Engine::new()
        .eval(&format!(
            r#"fn entry() {{ let c = std::process::command("{program}"); {args} let o = c.output(); o.exit_code }}"#
        ))
        .expect_err("over the 256 argument cap");
    assert!(
        matches!(&error, Error::Host(message) if message.contains("process_arguments_too_many")),
        "{error}"
    );
}

// -------------------------------------------------------------- path/time

#[test]
fn path_objects_expose_their_frozen_methods() {
    assert_eq!(
        eval(r#"fn entry() { let p = std::path::PathBuf::from("/a/b/c.txt"); p.file_name }"#),
        Value::String("c.txt".to_owned())
    );
    assert_eq!(
        eval(r#"fn entry() { let p = std::path::PathBuf::from("/a/b/c.txt"); p.extension }"#),
        Value::String("txt".to_owned())
    );
    assert_eq!(
        eval(r#"fn entry() { let p = std::path::PathBuf::from("/a/b"); p.is_absolute }"#),
        Value::Bool(true)
    );
    assert_eq!(
        eval(
            r#"fn entry() { let p = std::path::PathBuf::from("/a"); let q = p.join("b"); q.display }"#
        ),
        Value::String(if cfg!(windows) {
            "/a\\b".to_owned()
        } else {
            "/a/b".to_owned()
        })
    );
}

#[test]
fn system_time_formats_rfc3339_and_millis() {
    match eval("fn entry() { let t = std::time::SystemTime::now(); t.rfc3339 }") {
        Value::String(text) => {
            assert!(text.ends_with('Z'), "{text}");
            assert_eq!(text.len(), 20, "{text}");
            assert!(text.starts_with("20"), "{text}");
        }
        other => panic!("expected a string, got {other:?}"),
    }
    match eval("fn entry() { let t = std::time::SystemTime::now(); t.unix_millis }") {
        Value::Int(millis) => assert!(millis > 1_700_000_000_000, "{millis}"),
        other => panic!("expected an int, got {other:?}"),
    }
}

/// `FileLock` and `Duration` are constructible but have no members.
#[test]
fn filelock_and_duration_have_no_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir
        .path()
        .join("lock")
        .display()
        .to_string()
        .replace('\\', "/");
    // Constructing the lock works.
    eval(&format!(
        r#"fn entry() {{ let l = std::fs::try_lock_exclusive("{path}"); 0 }}"#
    ));
    let error = Engine::new()
        .eval(&format!(
            r#"fn entry() {{ let l = std::fs::try_lock_exclusive("{path}"); l.release() }}"#
        ))
        .expect_err("FileLock has no methods");
    assert!(matches!(&error, Error::Unsupported { .. }), "{error}");

    let error = Engine::new()
        .eval("fn entry() { let d = std::time::Duration::from_secs(1); d.as_millis() }")
        .expect_err("Duration has no methods");
    assert!(matches!(&error, Error::Unsupported { .. }), "{error}");
}

// -------------------------------------------------------------- rh::* std

#[test]
fn rh_json_round_trips() {
    assert_eq!(
        eval(r#"fn entry() { let d = rh::json::parse("{\"n\":7}"); d.n }"#),
        Value::Int(7)
    );
    assert_eq!(
        eval(r#"fn entry() { let d = rh::json::parse("[1,2,3]"); d.len }"#),
        Value::Int(3)
    );
    assert_eq!(
        eval(r#"fn entry() { rh::json::stringify(#{ a: 1 }) }"#),
        Value::String(r#"{"a":1}"#.to_owned())
    );
}

#[test]
fn rh_crypto_and_hash_match_known_vectors() {
    // Well-known SHA-256 of the empty input.
    assert_eq!(
        eval(r#"fn entry() { rh::crypto::sha256("") }"#),
        Value::String(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned()
        )
    );
    // FNV-1a 64 of "a" is 0xaf63dc4c8601ec8c.
    assert_eq!(
        eval(r#"fn entry() { rh::hash::fnv1a64("a") }"#),
        Value::Int(0xaf63_dc4c_8601_ec8c_u64 as i64)
    );
}

#[test]
fn rh_bytes_and_runtime_helpers() {
    assert_eq!(
        eval(r#"fn entry() { let b = rh::bytes::from_text("hi"); b.len }"#),
        Value::Int(2)
    );
    assert_eq!(
        eval(r#"fn entry() { let b = rh::bytes::from_array([104, 105]); b.to_text() }"#),
        Value::String("hi".to_owned())
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir
        .path()
        .join("atomic.txt")
        .display()
        .to_string()
        .replace('\\', "/");
    assert_eq!(
        eval(&format!(
            r#"fn entry() {{ rh::runtime::atomic_write("{path}", "v1"); std::fs::read_to_string("{path}") }}"#
        )),
        Value::String("v1".to_owned())
    );
}

// ------------------------------------------------------- out of Language 1

/// Fleet, GUI, clipboard and task are not in Language 1 — `StdHost` must
/// refuse them by name, not quietly do something.
#[test]
fn std_host_refuses_everything_outside_language_1() {
    for source in [
        r#"fn entry() { fleet.tabs.list() }"#,
        r#"fn entry() { rh::clipboard::get_text() }"#,
        r#"fn entry() { rh::http::request("x") }"#,
        r#"fn entry() { rh::task::sleep(1) }"#,
        r#"fn entry() { std::net::TcpStream::connect("x") }"#,
    ] {
        let error = Engine::new().eval(source).expect_err(source);
        assert!(
            matches!(&error, Error::Unsupported { .. }),
            "{source} => {error}"
        );
    }
}
