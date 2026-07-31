use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci.yml").replace("\r\n", "\n"));

fn windows_job() -> &'static str {
    let start = WORKFLOW
        .find("  windows:\n")
        .expect("missing Windows CI job");
    let end = WORKFLOW
        .find("\n  linux-x86_64:\n")
        .expect("missing next CI job");
    &WORKFLOW[start..end]
}

#[test]
fn windows_ci_covers_public_native_ipc_main_and_dev_without_candidate_scope() {
    let windows = windows_job();
    let native_step = windows
        .split("      - name:")
        .find(|step| step.contains("Prove native Windows IPC main/dev authorities"))
        .expect("missing dedicated native IPC main/dev evidence step");

    assert!(native_step.contains("id: native-ipc-main-dev"));
    assert!(native_step.contains("AGENTERM_NO_ACTIVATE: \"1\""));
    assert!(native_step.contains("task run native-ipc-smoke"));
    assert!(native_step.contains("--manifest agenterm.tasks.json -- --ci-main-dev"));
    assert!(
        windows
            .find("Run representative public-interface slice")
            .unwrap()
            < windows
                .find("Prove native Windows IPC main/dev authorities")
                .unwrap()
    );
    assert!(windows.contains("windows_native_ipc_main_dev_ms:"));
    assert!(!windows.contains("check.cmd --release"));
    assert!(!windows.contains("--include-stress"));
}
