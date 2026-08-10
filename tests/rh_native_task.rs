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
    assert!(entry.contains("root.chars().count() as INT"), "{entry}");
    assert!(
        entry.contains("rh_path_join(&root, \"Cargo.toml\")"),
        "{entry}"
    );
    assert!(entry.contains("rh_std_fs_exists(&path)"), "{entry}");
    assert!(entry.contains("rh_std_fs_read_to_string(&path)"), "{entry}");
    assert!(entry.contains("content.contains("), "{entry}");
    assert!(entry.contains("for value in 1..5"), "{entry}");
}

#[test]
fn execution_mode_is_native_only() {
    let overflow = include_str!("../fixtures/rh/for-span-overflow.rh");
    match agenterm_rh::transpile_cdylib_with_mode(overflow) {
        Err(_) => {}
        Ok(output) => {
            assert_eq!(
                output.execution_mode,
                agenterm_rh::CdylibExecutionMode::Native,
                "{}",
                output.rust
            );
            assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
            assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
            assert!(!output.rust.contains("compat delegating"));
        }
    }

    match agenterm_rh::transpile_cdylib_with_mode("fn entry() { switch 1 { 1 => 42, _ => 0 } }") {
        Err(_) => {}
        Ok(output) => {
            assert_eq!(
                output.execution_mode,
                agenterm_rh::CdylibExecutionMode::Native,
                "{}",
                output.rust
            );
            assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
            assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
            assert!(!output.rust.contains("compat delegating"));
        }
    }
}

#[test]
fn public_cli_runs_manifest_native_task() {
    let repo = repo_root();
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(&repo)
        .args(["task", "run", "rh-native-task-probe", "--manifest"])
        .arg(repo.join("agenterm.tasks.json"))
        .arg("--json")
        .args(["--", ".", "beta"])
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
    assert_eq!(envelope["value"], 15);
    assert!(
        envelope["stdout"]
            .as_str()
            .is_some_and(|stdout| stdout.contains("rh native named-task probe"))
    );
}

#[test]
fn task_corpus_accepts_native_rh_entries() {
    let repo = repo_root();
    let entries =
        agenterm_rh::extract_task_entries(&repo.join("agenterm.tasks.json")).expect("task entries");
    assert!(
        entries
            .iter()
            .all(|entry| entry.ends_with(".rh") || entry.ends_with(".lua"))
    );
    assert!(!entries.iter().any(|entry| entry.ends_with(".rhai")));
    assert!(entries.contains(&"scripts/rh/native-task-probe.rh".to_owned()));
    assert!(entries.contains(&"scripts/rh/verify-docs-site.rh".to_owned()));
}

#[test]
fn verify_docs_site_is_native_and_archives_interpreted_source() {
    let repo = repo_root();
    let source = std::fs::read_to_string(repo.join("scripts/rh/verify-docs-site.rh"))
        .expect("native docs task");
    let output = agenterm_rh::transpile_cdylib_with_mode(&source).expect("transpile docs task");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(output.rust.contains("rh_fail("));
    assert!(output.rust.contains("rh_std_fs_exists_case_exact(&image)"));
    assert!(!output.rust.contains("compat delegating"));

    let manifest = std::fs::read_to_string(repo.join("agenterm.tasks.json")).expect("manifest");
    assert!(manifest.contains("\"entry\": \"scripts/rh/verify-docs-site.rh\""));
    assert!(!manifest.contains("\"entry\": \"scripts/rhai/verify-docs-site.rhai\""));
    // See tests/rhai_migration.rs's matching comment: individually archived
    // .rhai files were compacted into one tarball (b0045922).
    assert!(repo.join("scripts/archive/rhai-old.tgz").is_file());
    assert!(!repo.join("scripts/rhai/verify-docs-site.rhai").exists());
}

#[test]
fn public_cli_runs_native_docs_task_and_preserves_failure() {
    let repo = repo_root();
    let manifest = repo.join("agenterm.tasks.json");
    let success = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(&repo)
        .args(["task", "run", "verify-docs-site", "--manifest"])
        .arg(&manifest)
        .arg("--json")
        .output()
        .expect("run docs task");
    assert!(
        success.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&success.stdout),
        String::from_utf8_lossy(&success.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&success.stdout).expect("success JSON");
    assert_eq!(envelope["ok"], true);
    assert!(
        envelope["stdout"]
            .as_str()
            .is_some_and(|stdout| stdout.contains("PASS: bilingual docs site"))
    );

    let failure = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(&repo)
        .args(["task", "run", "verify-docs-site", "--manifest"])
        .arg(&manifest)
        .arg("--json")
        .arg("--")
        .arg("missing-extra-argument")
        .output()
        .expect("run invalid docs task");
    assert_eq!(failure.status.code(), Some(2));
    let envelope: serde_json::Value =
        serde_json::from_slice(&failure.stdout).expect("failure JSON");
    assert_eq!(envelope["ok"], false);
    assert!(
        envelope["failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("expected: DOCS_ROOT"))
    );
}

#[test]
fn internal_version_policy_is_native_and_archives_interpreted_source() {
    let repo = repo_root();
    let source = std::fs::read_to_string(repo.join("scripts/rh/internal-version-policy.rh"))
        .expect("native internal version policy");
    let output =
        agenterm_rh::transpile_cdylib_with_mode(&source).expect("transpile version policy");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(output.rust.contains("rh_process_status("));
    assert!(!output.rust.contains("compat delegating"));

    let manifest = std::fs::read_to_string(repo.join("agenterm.tasks.json")).expect("manifest");
    assert!(manifest.contains("\"entry\": \"scripts/rh/internal-version-policy.rh\""));
    assert!(!manifest.contains("\"entry\": \"scripts/rhai/internal-version-policy.rhai\""));
    // See tests/rhai_migration.rs's matching comment: individually archived
    // .rhai files were compacted into one tarball (b0045922).
    assert!(repo.join("scripts/archive/rhai-old.tgz").is_file());
    assert!(
        !repo
            .join("scripts/rhai/internal-version-policy.rhai")
            .exists()
    );
}

#[test]
fn public_cli_runs_native_internal_version_policy() {
    let repo = repo_root();
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(&repo)
        .args([
            "task",
            "run",
            "internal-version-policy",
            "--manifest",
            "agenterm.tasks.json",
            "--json",
        ])
        .output()
        .expect("run native internal version policy");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).expect("success JSON");
    assert_eq!(envelope["ok"], true);
    assert!(
        envelope["stdout"]
            .as_str()
            .is_some_and(|stdout| stdout.contains("PASS: internal-only version"))
    );
}

#[test]
fn native_internal_version_policy_rejects_governed_git_tag() {
    let repo = repo_root();
    let fixture = std::env::temp_dir().join(format!(
        "agenterm-rh-internal-version-policy-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&fixture);
    std::fs::create_dir_all(fixture.join("scripts/rh")).expect("scripts");
    std::fs::create_dir_all(fixture.join(".github/workflows")).expect("workflows");
    std::fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"policy-fixture\"\nversion = \"0.1.7\"\n",
    )
    .expect("Cargo.toml");
    std::fs::write(
        fixture.join("scripts/rh/release.rh"),
        "if version != \"0.1.7\" {}\n",
    )
    .expect("release policy");
    std::fs::write(
        fixture.join(".github/workflows/candidate.yml"),
        "if [[ \"$expected\" == \"v0.1.7\" ]]; then exit 1; fi\n",
    )
    .expect("candidate policy");
    let git = |arguments: &[&str]| {
        let output = Command::new("git")
            .current_dir(&fixture)
            .args(arguments)
            .output()
            .expect("git fixture");
        assert!(
            output.status.success(),
            "git {:?}: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=AgenTerm Test",
        "-c",
        "user.email=agenterm@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "fixture",
    ]);
    git(&["tag", "v0.1.7"]);

    let mut manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo.join("agenterm.tasks.json")).expect("task manifest"),
    )
    .expect("task manifest JSON");
    let task = manifest["tasks"]
        .as_array_mut()
        .expect("tasks")
        .iter_mut()
        .find(|task| task["id"] == "internal-version-policy")
        .expect("internal version task");
    task["args"] = serde_json::json!([fixture.to_string_lossy()]);
    let test_manifest = repo.join(format!(
        ".agenterm-internal-version-policy-test-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &test_manifest,
        serde_json::to_vec_pretty(&manifest).expect("encode task manifest"),
    )
    .expect("write task manifest");
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(&repo)
        .args(["task", "run", "internal-version-policy", "--manifest"])
        .arg(&test_manifest)
        .arg("--json")
        .output()
        .expect("run governed version fixture");
    let _ = std::fs::remove_file(&test_manifest);
    let _ = std::fs::remove_dir_all(&fixture);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).expect("failure JSON");
    assert_eq!(envelope["ok"], false);
    assert!(
        envelope["failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("must not have a local Git tag"))
    );
}
