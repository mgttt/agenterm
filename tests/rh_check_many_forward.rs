//! agenterm-rhai thin-forward of check-many to adjacent agenterm-rh (lint.rhai shape).

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
    assert!(status.success(), "failed to build agenterm-rh for forward test");
    let metadata = fs::metadata(&source).expect("agenterm-rh metadata");
    assert!(metadata.len() > 0, "agenterm-rh binary is empty");
    let adjacent = worker_dir.join("agenterm-rh");
    assert!(
        source == adjacent,
        "expected agenterm-rh adjacent to worker at {}",
        adjacent.display()
    );
}

#[test]
fn rhai_forwards_check_many_with_lint_style_flags() {
    let repo = repo_root();
    let worker_dir = repo.join("target/debug");
    ensure_adjacent_rh_cli(&worker_dir);

    let manifest_path = repo.join("fixtures/rh/check-many-rhai-kind.json");
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
        .current_dir(&repo)
        .args([
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
        ])
        .output()
        .expect("run forwarded check-many");

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
