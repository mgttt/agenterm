//! Public contract for manifest-owned `.rh` tasks that must stay off whole-script compatibility.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn native_task_source() -> String {
    std::fs::read_to_string(repo_root().join("scripts/rh/native-task-probe.rh"))
        .expect("native task source")
}

#[test]
fn manifest_native_task_transpiles_without_interpreter_fallback() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../agenterm.tasks.json")).expect("task manifest");
    let task = manifest["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["id"] == "rh-native-task-probe")
        .expect("native task");
    assert_eq!(task["entry"], "scripts/rh/native-task-probe.rh");

    let source = native_task_source();
    agenterm_rh::check(&source).expect("check");
    let output = agenterm_rh::transpile_cdylib_with_mode(&source).expect("transpile");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native
    );
    let rust = output.rust;
    let entry = rust
        .split_once("pub fn entry() -> INT {")
        .and_then(|(_, suffix)| suffix.split_once("fn rh_entry_internal()"))
        .map(|(entry, _)| entry)
        .expect("generated entry body");
    assert!(!rust.contains("compat delegating"), "{rust}");
    assert!(!entry.contains("rh_host_run_script"), "{entry}");
    assert!(!entry.contains("rh_host_eval_int"), "{entry}");
    assert!(entry.contains("rh_args_len()"), "{entry}");
    assert!(entry.contains("rh_arg(0)"), "{entry}");
    assert!(entry.contains("first.chars().count() as INT"), "{entry}");
    assert!(entry.contains("rh_std_fs_exists(&first)"), "{entry}");
    assert!(
        entry.contains("rh_std_fs_read_to_string(&first)"),
        "{entry}"
    );
    assert!(entry.contains("content.contains("), "{entry}");
    assert!(entry.contains("for value in 1..5"), "{entry}");
}

#[test]
fn execution_mode_distinguishes_native_host_eval_and_compatibility() {
    let host_eval = agenterm_rh::transpile_cdylib_with_mode(include_str!(
        "../fixtures/rh/for-span-overflow.rh"
    ))
    .expect("localized host eval");
    assert_eq!(
        host_eval.execution_mode,
        agenterm_rh::CdylibExecutionMode::HostEval
    );

    let compatibility =
        agenterm_rh::transpile_cdylib_with_mode("fn entry() { switch 1 { 1 => 42, _ => 0 } }")
            .expect("whole-script compatibility");
    assert_eq!(
        compatibility.execution_mode,
        agenterm_rh::CdylibExecutionMode::CompatDelegating
    );
}

#[test]
fn public_cli_runs_manifest_native_task() {
    let repo = repo_root();
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-rh"))
        .current_dir(&repo)
        .args(["task", "run", "rh-native-task-probe", "--manifest"])
        .arg(repo.join("agenterm.tasks.json"))
        .arg("--json")
        .args(["--", "Cargo.toml", "beta"])
        .output()
        .expect("run native task");
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("native task JSON");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["value"], 24);
    assert!(
        envelope["stdout"]
            .as_str()
            .is_some_and(|stdout| stdout.contains("rh native named-task probe"))
    );
}

#[test]
fn task_corpus_accepts_native_and_compatibility_entries() {
    let repo = repo_root();
    let entries =
        agenterm_rh::extract_task_entries(&repo.join("agenterm.tasks.json")).expect("task entries");
    assert!(entries.iter().any(|entry| entry.ends_with(".rh")));
    assert!(entries.iter().any(|entry| entry.ends_with(".rhai")));
    assert!(entries.contains(&"scripts/rh/native-task-probe.rh".to_owned()));
}
