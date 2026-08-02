//! Black-box tests for the public `agenterm-rhai` CLI on Unix hosts.
//! Uses `Command::output()` so exit codes are read from `ExitStatus`, not shell pipes.

#[cfg(unix)]
mod unix {
    use std::path::Path;
    use std::process::Command;
    use std::sync::Mutex;

    static SCRIPT_CLI_LOCK: Mutex<()> = Mutex::new(());

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn script_bin() -> &'static str {
        env!("CARGO_BIN_EXE_agenterm-rhai")
    }

    fn run(args: &[&str]) -> std::process::Output {
        let _guard = SCRIPT_CLI_LOCK.lock().expect("script cli lock");
        Command::new(script_bin())
            .current_dir(repo_root())
            .args(args)
            .output()
            .expect("spawn agenterm-rhai")
    }

    fn format_output(output: &std::process::Output) -> String {
        format!(
            "status={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    #[test]
    fn public_eval_returns_success_envelope() {
        let output = run(&["eval", "40 + 2", "--json"]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let envelope: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("decode eval envelope");
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["value"], 42);
    }

    #[test]
    fn public_run_executes_repository_script() {
        let output = run(&[
            "run",
            "scripts/rhai/internal-version-policy.rhai",
            "--project-root",
            ".",
            "--",
            ".",
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("PASS:"), "unexpected stdout: {stdout}");
    }

    #[test]
    fn public_task_check_accepts_repository_manifest() {
        let manifest = repo_root().join("agenterm.tasks.json");
        let output = run(&[
            "task",
            "check",
            "--manifest",
            manifest.to_str().expect("manifest path"),
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), "OK");
    }

    #[test]
    fn public_task_run_migration_audit_reports_no_drift() {
        let manifest = repo_root().join("agenterm.tasks.json");
        let output = run(&[
            "task",
            "run",
            "migration-audit",
            "--manifest",
            manifest.to_str().expect("manifest path"),
            "--timeout-ms",
            "60000",
            "--max-operations",
            "10000000",
            "--",
            ".",
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("decode migration report");
        assert_eq!(report["drift"], false);
        assert_eq!(report["remaining_count"], 0);
    }

    #[test]
    fn public_task_run_lint_static_passes() {
        let manifest = repo_root().join("agenterm.tasks.json");
        let worker = script_bin();
        let output = run(&[
            "task",
            "run",
            "lint",
            "--manifest",
            manifest.to_str().expect("manifest path"),
            "--timeout-ms",
            "120000",
            "--max-operations",
            "10000000",
            "--",
            ".",
            worker,
            "static",
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("PASS: repository lint"),
            "unexpected stdout: {stdout}"
        );
    }

    #[test]
    fn agenterm_cli_script_hosting_remains_stubbed() {
        let output = Command::new(env!("CARGO_BIN_EXE_agenterm-cli"))
            .args(["script", "eval", "1"])
            .output()
            .expect("spawn agenterm-cli");
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("invoke agenterm-rhai directly"));
    }
}
