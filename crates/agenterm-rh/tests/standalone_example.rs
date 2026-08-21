//! PR-A4 gate, automated: the `standalone_eval` example must **execute** a
//! `.rh` file — print and touch the filesystem — using only this crate.
//!
//! `cargo test` builds examples, so the binary is already next to the test
//! binary; these run it directly rather than nesting a `cargo run`.

use std::path::PathBuf;
use std::process::Command;

/// `target/<profile>/deps/<test>-<hash>` -> `target/<profile>/examples/standalone_eval`
fn example_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop(); // deps/
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("examples");
    path.push(if cfg!(windows) {
        "standalone_eval.exe"
    } else {
        "standalone_eval"
    });
    assert!(
        path.exists(),
        "example binary missing at {} — `cargo test` should have built it",
        path.display()
    );
    path
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rh")
}

/// The gate: a real `print` and a real `std::fs` round trip, exit 0.
#[test]
fn selftest_executes_print_and_fs_through_std_host() {
    let output = Command::new(example_binary())
        .arg("--selftest")
        .output()
        .expect("run example");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "selftest failed: status={:?} stdout={stdout} stderr={stderr}",
        output.status.code()
    );
    // It printed, so `Host::print` ran.
    assert!(stdout.contains("standalone_eval: ok"), "stdout={stdout}");
    // It stat'ed the file it wrote, so `std::fs` ran end to end.
    assert!(
        stdout.contains("wrote 13 bytes"),
        "the fs round trip should report the written length; stdout={stdout}"
    );
}

/// The control arm. Without `StdHost` the very same program must fail — if it
/// passed, the gate above would be proving nothing about extractability.
#[test]
fn selftest_fails_closed_without_std_host() {
    let output = Command::new(example_binary())
        .args(["--selftest", "--sandboxed"])
        .output()
        .expect("run example");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a host that implements nothing must not be able to run the selftest"
    );
    assert!(
        stderr.contains("unsupported"),
        "expected a fail-closed host error, got: {stderr}"
    );
}

/// A shipped language fixture, run for real: its answer must track the actual
/// filesystem, not a constant.
#[test]
fn std_fs_fixture_result_tracks_the_real_filesystem() {
    let fixture = fixtures_dir().join("std-fs-exists-probe.rh");
    let present = tempfile::tempdir().expect("tempdir");
    std::fs::write(present.path().join("Cargo.toml"), "[package]").expect("write");
    let absent = tempfile::tempdir().expect("tempdir");

    // The fixture returns 1 when `Cargo.toml` exists in the cwd, else 0.
    let with_file = Command::new(example_binary())
        .arg(&fixture)
        .current_dir(present.path())
        .output()
        .expect("run example");
    assert_eq!(
        with_file.status.code(),
        Some(1),
        "expected exit 1 where Cargo.toml exists; stderr={}",
        String::from_utf8_lossy(&with_file.stderr)
    );

    let without_file = Command::new(example_binary())
        .arg(&fixture)
        .current_dir(absent.path())
        .output()
        .expect("run example");
    assert_eq!(
        without_file.status.code(),
        Some(0),
        "expected exit 0 where Cargo.toml is absent; stderr={}",
        String::from_utf8_lossy(&without_file.stderr)
    );
}

/// Script arguments reach `args`, so the example is a usable runner and not
/// just a fixed program.
#[test]
fn script_arguments_reach_the_args_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("argc.rh");
    std::fs::write(&script, "fn entry() { args.len }").expect("write");

    let output = Command::new(example_binary())
        .arg(&script)
        .args(["a", "b", "c"])
        .output()
        .expect("run example");
    assert_eq!(output.status.code(), Some(3));
}

/// A non-Language-1 construct is refused before anything runs, with exit 2.
#[test]
fn subset_violations_exit_two_without_executing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("bad.rh");
    std::fs::write(&script, "fn entry(n) { switch n { 1 => 0, _ => 1 } }").expect("write");

    let output = Command::new(example_binary())
        .arg(&script)
        .output()
        .expect("run example");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RH_SUBSET_"), "stderr={stderr}");
}
