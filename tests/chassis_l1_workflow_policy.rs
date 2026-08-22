use std::{
    path::Path,
    process::{Command, Output},
    sync::LazyLock,
};

use serde_json::Value;

static WORKFLOW: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/ci-chassis.yml.disabled").replace("\r\n", "\n")
});

fn run_python(root: &Path, arguments: &[&str]) -> Output {
    for interpreter in ["python3", "python"] {
        match Command::new(interpreter)
            .current_dir(root)
            .args(arguments)
            .output()
        {
            Ok(output) => return output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("failed to invoke {interpreter}: {error}"),
        }
    }
    panic!("python3 or python is required for the chassis L1 workflow policy test")
}

fn classify(paths: &[&str]) -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut arguments = vec!["scripts/chassis-l1-change-gate.py", "--paths"];
    arguments.extend_from_slice(paths);
    let output = run_python(root, &arguments);
    assert!(
        output.status.success(),
        "classifier failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("classifier JSON output")
}

#[test]
fn classifier_uses_event_specific_diff_bases_and_explicit_outputs() {
    let classify = WORKFLOW
        .split("\n  l1:\n")
        .next()
        .expect("classify job section");
    assert!(classify.contains("classify:"));
    assert!(classify.contains("fetch-depth: 0"));
    assert!(
        classify.contains("requires_l1_candidate: ${{ steps.gate.outputs.requires_l1_candidate }}")
    );
    assert!(classify.contains("l1_reasons: ${{ steps.gate.outputs.l1_reasons }}"));
    assert!(classify.contains("PR_BASE_SHA: ${{ github.event.pull_request.base.sha }}"));
    assert!(classify.contains("PUSH_BASE_SHA: ${{ github.event.before }}"));
    assert!(classify.contains("HEAD_SHA: ${{ github.sha }}"));
    assert!(classify.contains("if [[ \"$EVENT_NAME\" == \"pull_request\" ]]"));
    assert!(classify.contains("base=\"$PR_BASE_SHA\""));
    assert!(classify.contains("base=\"$PUSH_BASE_SHA\""));
    assert!(classify.contains("--base \"$base\""));
    assert!(classify.contains("--head \"$HEAD_SHA\""));
    assert!(classify.contains("--github-output \"$GITHUB_OUTPUT\""));
    let lock = "run: python3 scripts/chassis-l1-change-gate-test.py";
    assert_eq!(classify.matches(lock).count(), 1);
    assert!(
        classify.find(lock).expect("reason lock step")
            < classify
                .find("name: Classify frozen L1 surface")
                .expect("classifier step")
    );
}

#[test]
fn dispatch_and_missing_push_base_force_the_l1_classifier_true() {
    let classify = WORKFLOW
        .split("\n  l1:\n")
        .next()
        .expect("classify job section");
    assert!(classify.contains("if [[ \"$EVENT_NAME\" == \"workflow_dispatch\" ]]"));
    assert_eq!(classify.matches("--paths src/bin/agenterm.rs").count(), 2);
    assert!(classify.contains("[[ -z \"$base\" || \"$base\" =~ ^0+$ ]]"));
    assert!(!classify.contains("requires_l1_candidate=true"));
}

#[test]
fn six_cell_l1_is_conditional_while_l2_pack_always_runs() {
    let l1 = WORKFLOW
        .split("\n  l1:\n")
        .nth(1)
        .expect("L1 job")
        .split("\n  l2-pack:\n")
        .next()
        .expect("L1 section");
    assert!(l1.contains("needs: classify"));
    assert!(l1.contains("if: needs.classify.outputs.requires_l1_candidate == 'true'"));
    assert_eq!(l1.matches("target:").count(), 6);
    assert_eq!(l1.matches("--features loader").count(), 2);

    let l2 = WORKFLOW
        .split("\n  l2-pack:\n")
        .nth(1)
        .expect("L2 pack job");
    assert!(!l2.contains("needs: classify"));
    assert!(!l2.contains("requires_l1_candidate"));
    assert!(l2.contains("python3 scripts/chassis-ci-pack.py"));
    assert!(l2.contains("python3 scripts/chassis-compose-product-test.py"));
}

#[test]
fn classifier_reason_outputs_are_locked_to_loader_window_pty_and_ipc() {
    let report = classify(&[
        "src/bin/agenterm.rs",
        "crates/agenterm-platform/src/window_host.rs",
        "src/pty/mod.rs",
        "src/protocol.rs",
    ]);
    assert_eq!(report["requires_l1_candidate"], true);
    assert_eq!(
        report["l1_reasons"]["loader"],
        serde_json::json!(["src/bin/agenterm.rs"])
    );
    assert_eq!(
        report["l1_reasons"]["window"],
        serde_json::json!(["crates/agenterm-platform/src/window_host.rs"])
    );
    assert_eq!(
        report["l1_reasons"]["pty"],
        serde_json::json!(["src/pty/mod.rs"])
    );
    assert_eq!(
        report["l1_reasons"]["ipc"],
        serde_json::json!(["src/protocol.rs"])
    );
    assert_eq!(
        report["l1_reasons"]
            .as_object()
            .expect("reason object")
            .len(),
        4
    );
}

#[test]
fn non_l1_surfaces_cannot_become_a_six_cell_candidate_reason() {
    let paths = [
        "crates/agenterm-chassis/l2/host-abi.json",
        "crates/agenterm-chassis/l3/example-app.json",
        "crates/agenterm-cu/src/lib.rs",
        "src/frontend/window.rs",
        "scripts/chassis-compose-product.py",
        "docs/agenterm-rust-cheatsheet.md",
        ".github/workflows/ci-chassis.yml",
    ];
    let report = classify(&paths);
    assert_eq!(report["requires_l1_candidate"], false);
    assert_eq!(report["l1_reasons"], serde_json::json!({}));
    let classified = report["explicitly_not_l1"]
        .as_array()
        .expect("explicit exclusions");
    assert_eq!(classified.len(), paths.len());
    assert!(
        report["unmatched_not_l1"]
            .as_array()
            .expect("unmatched paths")
            .is_empty()
    );
}
