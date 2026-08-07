//! agenterm-rhai thin-forward of dev commands to adjacent agenterm-rh.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ensure_adjacent_rh_cli(worker_dir: &Path) {
    let source = repo_root().join("target/debug/agenterm-rh");
    if fs::metadata(&source).is_ok_and(|meta| meta.len() == 0) {
        let status = Command::new("cargo")
            .args(["clean", "-p", "agenterm-rh"])
            .status()
            .expect("clean stale agenterm-rh artifacts");
        assert!(status.success(), "failed to clean agenterm-rh");
    }
    let status = Command::new("cargo")
        .args(["build", "-p", "agenterm-rh", "--bin", "agenterm-rh"])
        .status()
        .expect("build agenterm-rh");
    assert!(
        status.success(),
        "failed to build agenterm-rh for forward test"
    );
    let metadata = fs::metadata(&source).expect("agenterm-rh metadata");
    assert!(metadata.len() > 0, "agenterm-rh binary is empty");
    let adjacent = worker_dir.join("agenterm-rh");
    assert!(
        source == adjacent,
        "expected agenterm-rh adjacent to worker at {}",
        adjacent.display()
    );
}

fn run_rhai(arguments: &[&str]) -> std::process::Output {
    let repo = repo_root();
    ensure_adjacent_rh_cli(&repo.join("target/debug"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
        .current_dir(&repo)
        .args(arguments)
        .output()
        .expect("run agenterm-rhai forward")
}

fn run_rh(arguments: &[&str]) -> std::process::Output {
    let repo = repo_root();
    ensure_adjacent_rh_cli(&repo.join("target/debug"));
    let status = Command::new("cargo")
        .args(["build", "--bin", "agenterm-rhai"])
        .status()
        .expect("build agenterm-rhai compatibility CLI");
    assert!(status.success(), "failed to build compatibility CLI");
    Command::new(repo.join("target/debug/agenterm-rh"))
        .current_dir(&repo)
        .args(arguments)
        .output()
        .expect("run agenterm-rh")
}

#[test]
fn rh_is_the_task_front_door_via_explicit_compat_bridge() {
    let repo = repo_root();
    let manifest = repo.join("agenterm.tasks.json");
    let output = run_rh(&[
        "task",
        "list",
        "--manifest",
        &manifest.display().to_string(),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("task list JSON from rh front door");
    assert!(value.is_array(), "unexpected task list: {value}");
}

#[test]
fn rhai_forwards_check_many_with_lint_style_flags() {
    let repo = repo_root();
    let manifest_path = repo.join("fixtures/rh/check-many-rhai-kind.json");
    let output = run_rhai(&[
        "check-many",
        "--manifest",
        &manifest_path.display().to_string(),
        "--profile",
        "local",
        "--project-root",
        &repo.display().to_string(),
        "--timeout-ms",
        "10000",
        "--max-operations",
        "1000000",
    ]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rhai_forwards_check_for_rh_fixture() {
    let repo = repo_root();
    let fixture = repo.join("fixtures/rh/entry.rh");
    let output = run_rhai(&["check", fixture.display().to_string().as_str()]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rhai_forwards_eval_for_rh_fixture() {
    let repo = repo_root();
    let fixture = repo.join("fixtures/rh/entry.rh");
    let output = run_rhai(&["eval", fixture.display().to_string().as_str()]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "expected eval entry value in stdout: {stdout}"
    );
}

#[test]
fn rhai_forwards_run_as_eval_for_rh_fixture() {
    let repo = repo_root();
    let fixture = repo.join("fixtures/rh/entry.rh");
    let output = run_rhai(&["run", fixture.display().to_string().as_str()]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "expected run->eval entry value in stdout: {stdout}"
    );
}

#[test]
fn rhai_forwards_version_subcommand_to_adjacent_rh() {
    let output = run_rhai(&["version"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agenterm-rh"),
        "expected rh version banner: {stdout}"
    );
}

#[test]
fn rhai_forwards_version_flags_to_adjacent_rh() {
    let output = run_rhai(&["--version"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agenterm-rh"),
        "expected rh version banner: {stdout}"
    );
}

#[test]
fn rhai_forwards_dash_capital_version_to_adjacent_rh() {
    let output = run_rhai(&["-V"]);
    assert!(output.status.success());
}
