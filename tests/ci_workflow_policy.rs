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

#[test]
fn windows_ci_uses_fail_fast_non_powershell_steps_with_millisecond_outputs() {
    let windows = windows_job();
    assert!(!windows.contains("shell: pwsh"));
    assert!(!windows.contains("shell: powershell"));

    for (name, command) in [
        ("Install lint components", "rustup component add rustfmt clippy"),
        (
            "Run quality gate",
            "./check.cmd --skip-smoke --timing target/qualification/timing.json",
        ),
        (
            "Run representative public-interface slice",
            "task run startup-smoke --manifest agenterm.tasks.json",
        ),
        (
            "Prove native Windows IPC main/dev authorities",
            "task run native-ipc-smoke \\\n            --manifest agenterm.tasks.json -- --ci-main-dev",
        ),
    ] {
        let step = windows
            .split("      - name:")
            .find(|step| step.contains(name))
            .unwrap_or_else(|| panic!("missing Windows CI step: {name}"));
        assert!(step.contains("shell: bash"), "non-Bash step: {name}");
        assert!(
            step.contains("set -euo pipefail"),
            "step does not propagate command failure: {name}"
        );
        assert!(step.contains(command), "missing owned command: {name}");
        assert!(
            step.contains("start_ms=$(date +%s%3N)")
                && step.contains("duration_ms=$((end_ms - start_ms))")
                && step.contains("echo \"duration_ms=$duration_ms\" >> \"$GITHUB_OUTPUT\""),
            "step does not publish millisecond timing: {name}"
        );
    }

    let public_step = windows
        .split("      - name:")
        .find(|step| step.contains("Run representative public-interface slice"))
        .expect("missing public-interface step");
    assert!(public_step.contains("task run cli-smoke --manifest agenterm.tasks.json"));
    assert!(public_step.contains("AGENTERM_NO_ACTIVATE: \"1\""));
}
