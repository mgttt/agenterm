//! Black-box tests for the public Script Runtime CLI on Unix hosts.
//! Primary surface: `agenterm-rh` and `.rh`. The `agenterm-rhai` compatibility shim
//! is exercised only where the legacy entry path is still required.
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

    fn rh_bin() -> &'static str {
        env!("CARGO_BIN_EXE_agenterm-rh")
    }

    fn compatibility_shim_bin() -> &'static str {
        env!("CARGO_BIN_EXE_agenterm-rhai")
    }

    fn run_rh(args: &[&str]) -> std::process::Output {
        let _guard = SCRIPT_CLI_LOCK.lock().expect("script cli lock");
        Command::new(rh_bin())
            .current_dir(repo_root())
            .args(args)
            .output()
            .expect("spawn agenterm-rh")
    }

    fn run_compatibility_shim(args: &[&str]) -> std::process::Output {
        let _guard = SCRIPT_CLI_LOCK.lock().expect("script cli lock");
        Command::new(compatibility_shim_bin())
            .current_dir(repo_root())
            .args(args)
            .output()
            .expect("spawn agenterm-rhai compatibility shim")
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
    fn compatibility_shim_eval_returns_success_envelope() {
        let output = run_compatibility_shim(&["eval", "40 + 2", "--json"]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let envelope: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("decode eval envelope");
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["value"], 42);
    }

    #[test]
    fn public_rh_runs_native_internal_version_policy_task() {
        let output = run_rh(&[
            "task",
            "run",
            "internal-version-policy",
            "--manifest",
            "agenterm.tasks.json",
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("PASS:"), "unexpected stdout: {stdout}");
    }

    #[test]
    fn public_task_check_accepts_ready_task() {
        let manifest = repo_root().join("agenterm.tasks.json");
        let output = run_rh(&[
            "task",
            "check",
            "internal-version-policy",
            "--manifest",
            manifest.to_str().expect("manifest path"),
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), "OK");
    }

    #[test]
    fn public_check_many_accepts_repository_fixture_manifest() {
        let manifest = repo_root().join("fixtures/rh/check-many.json");
        let output = run_rh(&[
            "check-many",
            "--manifest",
            manifest.to_str().expect("manifest path"),
            "--project-root",
            repo_root().to_str().expect("repo root"),
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.starts_with("OK ("),
            "unexpected stdout: {stdout}"
        );
    }

    #[test]
    fn public_check_validates_repository_rh_fixture() {
        let output = run_rh(&[
            "check",
            "fixtures/rh/entry.rh",
            "--project-root",
            repo_root().to_str().expect("repo root"),
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("rh check ok: fixtures/rh/entry.rh"),
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
        // Prefer agenterm-rh wording; accept legacy shim copy until product strings update.
        assert!(
            stderr.contains("invoke agenterm-rh directly")
                || stderr.contains("invoke agenterm-rhai directly")
        );
    }
}
