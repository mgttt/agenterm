//! agenterm-rhai thin-forward of dev commands to adjacent agenterm-rh.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "agenterm-rh-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

fn ensure_adjacent_rh_cli(worker_dir: &Path) {
    let source = repo_root()
        .join("target/debug")
        .join(binary_name("agenterm-rh"));
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
    let adjacent = worker_dir.join(binary_name("agenterm-rh"));
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
    Command::new(repo.join("target/debug").join(binary_name("agenterm-rh")))
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
    assert!(
        value["tasks"]
            .as_array()
            .is_some_and(|tasks| !tasks.is_empty()),
        "unexpected task list: {value}"
    );
    assert_eq!(
        value["provenance"]["producer"], "agenterm-rhai",
        "compatibility execution must remain explicit until the task engine migrates"
    );
}

#[test]
fn rh_task_front_door_fails_closed_without_compat_binary() {
    let repo = repo_root();
    ensure_adjacent_rh_cli(&repo.join("target/debug"));
    let isolated = TestDirectory::new("missing-compat");
    let isolated_rh = isolated.path().join(binary_name("agenterm-rh"));
    fs::copy(
        repo.join("target/debug").join(binary_name("agenterm-rh")),
        &isolated_rh,
    )
    .expect("copy isolated rh CLI");
    let output = Command::new(isolated_rh)
        .current_dir(isolated.path())
        .args(["task", "list"])
        .env("AGENTERM_PROJECT_ROOT", isolated.path())
        .env(
            "AGENTERM_RHAI_COMPAT_CLI",
            isolated.path().join("missing-compat"),
        )
        .output()
        .expect("run isolated agenterm-rh");
    assert!(!output.status.success(), "missing compat binary must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("requires the temporary agenterm-rhai compatibility binary"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
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
