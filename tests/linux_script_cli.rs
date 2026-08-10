//! Black-box tests for the public Script Runtime CLI on Unix hosts.
//! Primary surface: the rh engine and `.rh`, run through the main `agenterm`
//! PE's `__agenterm-internal-engine rh` dispatch (the standalone
//! `agenterm-rh` binary is retired; on Unix this call is in-process, no
//! re-exec).
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

    fn run_rh(args: &[&str]) -> std::process::Output {
        let _guard = SCRIPT_CLI_LOCK.lock().expect("script cli lock");
        Command::new(env!("CARGO_BIN_EXE_agenterm"))
            .args(["__agenterm-internal-engine", "rh"])
            .current_dir(repo_root())
            .args(args)
            .output()
            .expect("spawn agenterm rh")
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
        assert!(stdout.starts_with("OK ("), "unexpected stdout: {stdout}");
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

    /// `agenterm cli script` hosting is LIVE on Unix as of 644e4cf9
    /// ("fix(platform): enable hosted Script workers on Unix", which added
    /// Linux/macOS to `hosted_script_worker_available`). This replaces an
    /// earlier `..._remains_stubbed` test that asserted the pre-644e4cf9
    /// "not yet available on this platform; invoke agenterm rh directly"
    /// message — that expectation is what the platform change deliberately
    /// invalidated, so the test now pins the new contract instead: a real
    /// script really evaluates, and the stub message is gone.
    ///
    /// (The `cli script` spelling itself is a deprecated alias slated for
    /// removal in v0.1.17 — see a1a020ac. Until then it must work, not
    /// half-work.)
    #[test]
    fn agenterm_cli_script_hosting_is_live() {
        let _guard = SCRIPT_CLI_LOCK.lock().expect("script cli lock");
        let output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
            .args(["cli", "script", "eval", "fn entry() { 42 }"])
            .current_dir(repo_root())
            .output()
            .expect("spawn agenterm cli");
        assert_eq!(output.status.code(), Some(0), "{}", format_output(&output));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("42"),
            "hosted eval did not return the entry value: {}",
            format_output(&output)
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("not yet available"),
            "hosting still reports itself unavailable: {}",
            format_output(&output)
        );
    }

    /// The same hosted path still fails closed, with rh's typed diagnostic,
    /// for a source that is not a valid rh entry point — proving the live
    /// path is really rh, not a permissive shim.
    #[test]
    fn agenterm_cli_script_hosting_fails_closed_without_an_entry_point() {
        let _guard = SCRIPT_CLI_LOCK.lock().expect("script cli lock");
        let output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
            .args(["cli", "script", "eval", "1"])
            .current_dir(repo_root())
            .output()
            .expect("spawn agenterm cli");
        assert_eq!(output.status.code(), Some(2), "{}", format_output(&output));
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            combined.contains("cdylib pack requires fn entry()"),
            "unexpected diagnostic: {}",
            format_output(&output)
        );
    }
}
