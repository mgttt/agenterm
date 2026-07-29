use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::process::{Child, Stdio};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

static SCRIPT_TASK_LOCK: Mutex<()> = Mutex::new(());

#[cfg(windows)]
struct RunningFixture(Option<Child>);

#[cfg(windows)]
impl RunningFixture {
    fn terminate(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(windows)]
impl Drop for RunningFixture {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn fixture_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "agenterm-rhai-migration-{name}-{}-{nonce}",
        std::process::id()
    ))
}

fn run_clean_locked(directory: &Path, extra: &[&str]) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    let artifacts = repo.join("scripts").join("artifacts.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm-script"));
    command
        .current_dir(repo)
        .args(["task", "run", "clean-locked-artifacts", "--manifest"])
        .arg(manifest)
        .arg("--")
        .arg(directory)
        .arg(artifacts);
    command.args(extra).output().expect("run Rhai cleanup task")
}

fn run_prepare_target(repo_under_test: &Path, target: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "prepare-target-clean", "--manifest"])
        .arg(manifest)
        .arg("--")
        .arg(repo_under_test)
        .arg(target)
        .output()
        .expect("run Rhai target preparation task")
}

fn run_stage_artifact(source: &Path, destination: &Path, name: &str) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "stage-artifact", "--manifest"])
        .arg(manifest)
        .arg("--")
        .arg(source)
        .arg(destination)
        .arg(name)
        .output()
        .expect("run Rhai artifact staging task")
}

fn run_validate_artifact_manifest(path: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "validate-artifact-manifest", "--manifest"])
        .arg(manifest)
        .arg("--")
        .arg(path)
        .output()
        .expect("run Rhai artifact-manifest validation task")
}

fn run_build_identity(repo_under_test: &Path, profile: &str, output_path: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "build-identity", "--manifest"])
        .arg(manifest)
        .arg("--")
        .arg(repo_under_test)
        .arg(profile)
        .arg(output_path)
        .output()
        .expect("run Rhai build-identity task")
}

fn run_write_build_metadata(
    repo_under_test: &Path,
    output_path: &Path,
    artifact_manifest: &Path,
    staged_directory: &Path,
    profile: &str,
    environment: &[(String, String)],
) -> Output {
    const BUILD_IDENTITY_ENVIRONMENT: [&str; 6] = [
        "AGENTERM_BUILD_IDENTITY_VERSION",
        "AGENTERM_BUILD_GIT_COMMIT",
        "AGENTERM_BUILD_GIT_DIRTY",
        "AGENTERM_BUILD_CARGO_LOCK_SHA256",
        "AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256",
        "AGENTERM_BUILD_PROFILE",
    ];

    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm-script"));
    command
        .current_dir(repo)
        .args(["task", "run", "write-build-metadata", "--manifest"])
        .arg(manifest)
        .args(["--timeout-ms", "10000", "--max-operations", "10000000"])
        .arg("--")
        .arg(repo_under_test)
        .arg(output_path)
        .arg(artifact_manifest)
        .arg(staged_directory)
        .arg(profile);
    for name in BUILD_IDENTITY_ENVIRONMENT {
        command.env_remove(name);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().expect("run Rhai build-metadata task")
}

fn run_stage_build(
    repo_under_test: &Path,
    source_directory: &Path,
    destination_directory: &Path,
    profile: &str,
) -> Output {
    const BUILD_IDENTITY_ENVIRONMENT: [&str; 6] = [
        "AGENTERM_BUILD_IDENTITY_VERSION",
        "AGENTERM_BUILD_GIT_COMMIT",
        "AGENTERM_BUILD_GIT_DIRTY",
        "AGENTERM_BUILD_CARGO_LOCK_SHA256",
        "AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256",
        "AGENTERM_BUILD_PROFILE",
    ];

    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm-script"));
    command
        .current_dir(repo)
        .args(["task", "run", "stage-build", "--manifest"])
        .arg(manifest)
        .args(["--timeout-ms", "10000", "--max-operations", "10000000"])
        .arg("--")
        .arg(repo_under_test)
        .arg(source_directory)
        .arg(destination_directory)
        .arg(profile);
    for name in BUILD_IDENTITY_ENVIRONMENT {
        command.env_remove(name);
    }
    command.output().expect("run Rhai stage-build task")
}

fn run_migration_audit(repo_under_test: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "migration-audit", "--manifest"])
        .arg(manifest)
        .args(["--timeout-ms", "10000", "--max-operations", "10000000"])
        .arg("--")
        .arg(repo_under_test)
        .output()
        .expect("run Rhai migration-audit task")
}

fn run_prd_alignment(repo_under_test: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "prd-alignment", "--manifest"])
        .arg(manifest)
        .args(["--timeout-ms", "20000", "--max-operations", "10000000"])
        .arg("--")
        .arg(repo_under_test)
        .arg(env!("CARGO_BIN_EXE_agenterm-cli"))
        .arg(env!("CARGO_BIN_EXE_agenterm-mux"))
        .output()
        .expect("run Rhai PRD-alignment task")
}

#[cfg(windows)]
fn run_harness_cleanup_selftest() -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "harness-cleanup-selftest", "--manifest"])
        .arg(manifest)
        .args(["--timeout-ms", "10000", "--max-operations", "10000000"])
        .output()
        .expect("run Rhai harness cleanup self-test")
}

#[cfg(windows)]
fn run_working_context_smoke() -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["run", "scripts/rhai/working-context-smoke.rhai"])
        .args(["--profile", "local", "--project-root"])
        .arg(repo)
        .args([
            "--timeout-ms",
            "60000",
            "--max-operations",
            "10000000",
            "--",
        ])
        .arg(repo)
        .arg(env!("CARGO_BIN_EXE_agenterm"))
        .arg(env!("CARGO_BIN_EXE_agenterm-cli"))
        .env("AGENTERM_NO_ACTIVATE", "1")
        .output()
        .expect("run Rhai working-context smoke")
}

#[cfg(windows)]
fn run_server_smoke() -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["run", "scripts/rhai/server-smoke.rhai"])
        .args(["--profile", "local", "--project-root"])
        .arg(repo)
        .args([
            "--timeout-ms",
            "60000",
            "--max-operations",
            "10000000",
            "--",
        ])
        .arg(repo)
        .arg(env!("CARGO_BIN_EXE_agenterm-server"))
        .arg(env!("CARGO_BIN_EXE_agenterm-cli"))
        .env("AGENTERM_NO_ACTIVATE", "1")
        .output()
        .expect("run Rhai server smoke")
}

#[cfg(windows)]
fn run_wake_smoke() -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["run", "scripts/rhai/wake-smoke.rhai"])
        .args(["--profile", "local", "--project-root"])
        .arg(repo)
        .args([
            "--timeout-ms",
            "60000",
            "--max-operations",
            "10000000",
            "--",
        ])
        .arg(repo)
        .arg(env!("CARGO_BIN_EXE_agenterm-server"))
        .arg(env!("CARGO_BIN_EXE_agenterm-cli"))
        .env("AGENTERM_NO_ACTIVATE", "1")
        .output()
        .expect("run Rhai wake smoke")
}

fn run_preflight(repo_under_test: &Path, output_path: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "preflight", "--manifest"])
        .arg(manifest)
        .args(["--timeout-ms", "10000", "--max-operations", "10000000"])
        .arg("--")
        .arg(repo_under_test)
        .arg(output_path)
        .arg("--quiet")
        .output()
        .expect("run Rhai preflight task")
}

fn run_preflight_benchmark(repo_under_test: &Path, output_path: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    let worker = Path::new(env!("CARGO_BIN_EXE_agenterm-script"));
    Command::new(worker)
        .current_dir(repo)
        .args(["task", "run", "preflight-benchmark", "--manifest"])
        .arg(&manifest)
        .args(["--timeout-ms", "10000", "--max-operations", "10000000"])
        .arg("--")
        .arg(worker)
        .arg(&manifest)
        .arg(repo_under_test)
        .arg(output_path)
        .arg("5")
        .output()
        .expect("run Rhai preflight-benchmark task")
}

fn run_supply_chain(output_path: &Path) -> Output {
    let _guard = SCRIPT_TASK_LOCK.lock().expect("script task lock");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = repo.join("agenterm.tasks.json");
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "supply-chain", "--manifest"])
        .arg(manifest)
        .args([
            "--timeout-ms",
            "60000",
            "--max-operations",
            "10000000",
            "--max-collection-items",
            "100000",
            "--max-string-bytes",
            "8388608",
            "--max-output-bytes",
            "1048576",
        ])
        .arg("--")
        .arg(repo)
        .arg(output_path)
        .output()
        .expect("run Rhai supply-chain task")
}

fn parse_batch_environment(path: &Path) -> Vec<(String, String)> {
    fs::read_to_string(path)
        .expect("read batch environment")
        .lines()
        .map(|line| {
            let assignment = line
                .strip_prefix("set \"")
                .and_then(|value| value.strip_suffix('"'))
                .expect("batch assignment");
            let (name, value) = assignment.split_once('=').expect("batch name and value");
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

fn sha256(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn init_git_fixture(name: &str) -> PathBuf {
    let root = fixture_root(name);
    fs::create_dir_all(&root).expect("create Git fixture");
    let output = Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg(&root)
        .output()
        .expect("initialize Git fixture");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    root
}

fn commit_git_fixture(root: &Path) {
    for arguments in [
        vec!["config", "user.email", "agenterm-test@example.invalid"],
        vec!["config", "user.name", "AgenTerm Test"],
        vec!["add", "."],
        vec!["commit", "--quiet", "-m", "fixture"],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .output()
            .expect("prepare Git fixture commit");
        assert!(
            output.status.success(),
            "Git fixture command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn copy_fixture_file(repo: &Path, fixture: &Path, relative: &str) {
    let destination = fixture.join(relative);
    fs::create_dir_all(destination.parent().expect("fixture file parent"))
        .expect("create fixture file parent");
    fs::copy(repo.join(relative), destination).expect("copy fixture file");
}

#[test]
fn legacy_profile_spellings_share_the_unrestricted_runtime() {
    for legacy_profile in ["local", "pure", "observe"] {
        let output = Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
            .args([
                "eval",
                "std::process::id()",
                "--profile",
                legacy_profile,
                "--json",
            ])
            .output()
            .expect("evaluate public process ID");
        assert!(
            output.status.success(),
            "{legacy_profile} compatibility label removed a runtime API: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("decode process ID envelope");
        assert_eq!(envelope["ok"], true);
        assert!(
            envelope["value"].as_u64().is_some_and(|pid| pid > 0),
            "process ID was not a positive integer: {envelope}"
        );
    }
}

#[test]
fn child_id_remains_public_after_process_completion() {
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .args([
            "eval",
            "let c=std::process::command(\"rustc\");\
             c.arg(\"--version\");let child=c.start();let before=child.id;\
             child.wait_with_output();let facts=child.platform_facts;\
             #{before:before,after:child.id,state:child.state,\
               window_supported:facts.top_level_window_supported,\
               window_present:facts.top_level_window_present,\
               window_id:facts.top_level_window_id}",
            "--profile",
            "local",
            "--json",
        ])
        .output()
        .expect("evaluate completed child ID");
    assert!(
        output.status.success(),
        "completed child ID evaluation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("decode completed child ID envelope");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["value"]["before"], envelope["value"]["after"]);
    assert_eq!(envelope["value"]["state"], "exited");
    assert_eq!(
        envelope["value"]["window_supported"],
        serde_json::Value::Bool(cfg!(windows))
    );
    assert_eq!(envelope["value"]["window_present"], false);
    assert_eq!(envelope["value"]["window_id"], 0);
}

#[cfg(windows)]
fn windows_short_path(path: &Path) -> PathBuf {
    let mut input: Vec<u16> = path.as_os_str().encode_wide().collect();
    input.push(0);
    let mut output = vec![0_u16; 32_768];
    // SAFETY: Both buffers are valid for the duration of the call, the input is
    // NUL-terminated, and the output capacity is supplied exactly.
    let length = unsafe {
        GetShortPathNameW(
            input.as_ptr(),
            output.as_mut_ptr(),
            output.len().try_into().expect("short-path buffer fits u32"),
        )
    };
    assert!(
        length > 0 && (length as usize) < output.len(),
        "resolve Windows short path for {}",
        path.display()
    );
    PathBuf::from(String::from_utf16(&output[..length as usize]).expect("short path is UTF-16"))
}

#[cfg(windows)]
fn bash_executable() -> PathBuf {
    let output = Command::new("where")
        .arg("git.exe")
        .output()
        .expect("locate Git for Windows");
    assert!(
        output.status.success(),
        "Git for Windows is required for packaging tests"
    );
    let git = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| line.to_ascii_lowercase().ends_with("\\cmd\\git.exe"))
        .map(PathBuf::from)
        .expect("locate Git cmd executable");
    let root = git
        .parent()
        .and_then(Path::parent)
        .expect("derive Git installation root");
    for candidate in [
        root.join("bin").join("bash.exe"),
        root.join("usr").join("bin").join("bash.exe"),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("Git for Windows Bash executable is missing");
}

#[cfg(not(windows))]
fn bash_executable() -> PathBuf {
    PathBuf::from("bash")
}

#[cfg(windows)]
fn python_executable() -> String {
    let output = Command::new("where")
        .arg("python.exe")
        .output()
        .expect("locate Python");
    assert!(
        output.status.success(),
        "Python is required for packaging tests"
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .expect("locate Python executable")
        .replace('\\', "/")
}

#[cfg(not(windows))]
fn python_executable() -> String {
    "python3".to_owned()
}

#[test]
fn macos_package_script_reads_both_platform_rows_and_writes_preview_zip() {
    let root = fixture_root("macos-package");
    let binaries = root.join("bin");
    let output_directory = root.join("dist");
    fs::create_dir_all(&binaries).expect("create binary fixture");
    for name in [
        "agenterm",
        "agenterm-cli",
        "agenterm-mux",
        "agenterm-script",
        "agenterm-mcp",
    ] {
        fs::write(binaries.join(name), format!("fixture-{name}")).expect("write fake executable");
    }

    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = repo.join("scripts").join("package-client-release.sh");
    for architecture in ["aarch64", "x86_64"] {
        let output = Command::new(bash_executable())
            .current_dir(repo)
            .arg(&script)
            .args(["test", "macos", architecture])
            .arg(&binaries)
            .env("AGENTERM_MACOS_UNSIGNED_PREVIEW", "1")
            .env("AGENTERM_PACKAGE_DIST", &output_directory)
            .env("PYTHON", python_executable())
            .output()
            .expect("run macOS package script");
        assert!(
            output.status.success(),
            "macOS {architecture} package failed:\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let archive = output_directory.join(format!("agenterm-test-macos-{architecture}.zip"));
        assert!(
            fs::metadata(&archive)
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false),
            "macOS {architecture} archive is absent or empty"
        );
    }

    fs::remove_dir_all(&root).expect("remove fixture");
}

#[test]
fn clean_locked_artifacts_task_removes_only_owned_candidates() {
    let root = fixture_root("clean-locked");
    fs::create_dir_all(&root).expect("create fixture");
    let stale_gui = root.join("agenterm.locked-123.exe");
    let stale_cli = root.join("agenterm-cli.locked-456.exe");
    let unrelated = root.join("other.locked-789.exe");
    let obsolete = root.join("agentermctl.exe");
    for path in [&stale_gui, &stale_cli, &unrelated, &obsolete] {
        fs::write(path, b"fixture").expect("write fixture");
    }

    let output = run_clean_locked(&root, &["agentermctl.exe"]);
    assert!(
        output.status.success(),
        "cleanup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removed 2 stale locked artifact(s)"));
    assert!(stdout.contains("Removed obsolete artifact agentermctl.exe"));
    assert!(!stale_gui.exists());
    assert!(!stale_cli.exists());
    assert!(!obsolete.exists());
    assert!(unrelated.exists(), "task removed an unowned executable");

    fs::remove_dir_all(&root).expect("remove fixture");
}

#[test]
fn clean_locked_artifacts_task_rejects_obsolete_path_escape() {
    let root = fixture_root("clean-locked-escape");
    fs::create_dir_all(&root).expect("create fixture");

    let output = run_clean_locked(&root, &["..\\outside.exe"]);
    assert!(
        !output.status.success(),
        "path escape unexpectedly succeeded"
    );
    let error = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        error.contains("obsolete_artifact_name_invalid"),
        "missing typed rejection: {error}"
    );

    fs::remove_dir_all(&root).expect("remove fixture");
}

#[test]
fn prepare_target_clean_task_writes_and_preserves_valid_cache_tag() {
    let repo = init_git_fixture("prepare-target");
    let target = repo.join("target-release");
    fs::create_dir_all(&target).expect("create target fixture");

    #[cfg(windows)]
    let repo_argument = windows_short_path(&repo);
    #[cfg(not(windows))]
    let repo_argument = repo.clone();
    let target_argument = repo_argument.join("target-release");

    let first = run_prepare_target(&repo_argument, &target_argument);
    assert!(
        first.status.success(),
        "target preparation failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let tag = target.join("CACHEDIR.TAG");
    let expected = concat!(
        "Signature: 8a477f597d28d172789f06886806bc55\n",
        "# This file is a cache directory tag created by cargo.\n",
        "# For information about cache directory tags see ",
        "https://bford.info/cachedir/\n"
    );
    assert_eq!(fs::read_to_string(&tag).expect("read cache tag"), expected);

    let second = run_prepare_target(&repo_argument, &target_argument);
    assert!(
        second.status.success(),
        "idempotent target preparation failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(fs::read_to_string(&tag).expect("read cache tag"), expected);

    fs::remove_dir_all(&repo).expect("remove fixture");
}

#[test]
fn prepare_target_clean_task_rejects_unexpected_target_and_invalid_tag() {
    let repo = init_git_fixture("prepare-target-reject");
    let unexpected = repo.join("cargo-output");
    fs::create_dir_all(&unexpected).expect("create unexpected target");
    let rejected_path = run_prepare_target(&repo, &unexpected);
    assert!(!rejected_path.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected_path.stdout),
            String::from_utf8_lossy(&rejected_path.stderr)
        )
        .contains("target_clean_path_not_allowed")
    );

    let target = repo.join("target");
    fs::create_dir_all(&target).expect("create target fixture");
    fs::write(target.join("CACHEDIR.TAG"), "not a Cargo cache tag\n")
        .expect("write invalid cache tag");
    let rejected_tag = run_prepare_target(&repo, &target);
    assert!(!rejected_tag.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected_tag.stdout),
            String::from_utf8_lossy(&rejected_tag.stderr)
        )
        .contains("target_clean_cache_tag_invalid")
    );

    fs::remove_dir_all(&repo).expect("remove fixture");
}

#[test]
fn stage_artifact_task_replaces_only_a_valid_named_executable() {
    let root = fixture_root("stage-artifact");
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).expect("create source fixture");
    fs::create_dir_all(&destination).expect("create destination fixture");
    fs::write(source.join("agenterm-cli.exe"), b"new-cli").expect("write source artifact");
    fs::write(destination.join("agenterm-cli.exe"), b"old-cli")
        .expect("write destination artifact");

    let staged = run_stage_artifact(&source, &destination, "agenterm-cli.exe");
    assert!(
        staged.status.success(),
        "artifact staging failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&staged.stdout),
        String::from_utf8_lossy(&staged.stderr)
    );
    assert_eq!(
        fs::read(destination.join("agenterm-cli.exe")).expect("read staged artifact"),
        b"new-cli"
    );
    assert!(
        fs::read_dir(&destination)
            .expect("read destination")
            .all(|entry| !entry
                .expect("read destination entry")
                .file_name()
                .to_string_lossy()
                .contains(".locked-")),
        "unlocked replacement unexpectedly parked the old artifact"
    );

    let rejected = run_stage_artifact(&source, &destination, "..\\outside.exe");
    assert!(!rejected.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        )
        .contains("stage_artifact_name_invalid")
    );

    fs::remove_dir_all(&root).expect("remove fixture");
}

#[test]
fn artifact_manifest_task_accepts_canonical_contract_and_rejects_invalid_fields() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let canonical = run_validate_artifact_manifest(&repo.join("scripts").join("artifacts.json"));
    assert!(
        canonical.status.success(),
        "canonical artifact manifest failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&canonical.stdout),
        String::from_utf8_lossy(&canonical.stderr)
    );
    assert!(
        String::from_utf8_lossy(&canonical.stdout)
            .contains("defines 6 validated Windows executables")
    );

    let root = fixture_root("artifact-manifest");
    fs::create_dir_all(&root).expect("create manifest fixture");
    let valid = serde_json::json!({
        "schema_version": 2,
        "executables": [{
            "name": "agenterm-cli.exe",
            "role": "cli",
            "pe_subsystem": 3,
            "documentation_role": "native control client",
            "offline_probe": ["--version"],
            "release_budget_bytes": 1
        }]
    });
    let mut invalid = Vec::new();

    let mut value = valid.clone();
    value["schema_version"] = serde_json::json!(1);
    invalid.push(("schema", value, "artifact_manifest_schema_unsupported"));

    let mut value = valid.clone();
    let duplicate = value["executables"][0].clone();
    value["executables"]
        .as_array_mut()
        .expect("executables array")
        .push(duplicate);
    invalid.push(("duplicate", value, "artifact_manifest_name_duplicate"));

    let mut value = valid.clone();
    value["executables"][0]["name"] = serde_json::json!("outside.exe");
    invalid.push(("name", value, "artifact_manifest_name_invalid"));

    let mut value = valid.clone();
    value["executables"][0]["pe_subsystem"] = serde_json::json!(1);
    invalid.push(("subsystem", value, "artifact_manifest_pe_subsystem_invalid"));

    let mut value = valid.clone();
    value["executables"][0]["offline_probe"] = serde_json::json!([]);
    invalid.push((
        "console-probe",
        value,
        "artifact_manifest_console_probe_required",
    ));

    let mut value = valid.clone();
    value["executables"][0]["role"] = serde_json::json!(" ");
    invalid.push(("role", value, "artifact_manifest_role_empty"));

    let mut value = valid.clone();
    value["executables"][0]["documentation_role"] = serde_json::json!("");
    invalid.push((
        "documentation-role",
        value,
        "artifact_manifest_documentation_role_empty",
    ));

    let mut value = valid;
    value["executables"][0]["release_budget_bytes"] = serde_json::json!(0);
    invalid.push(("budget", value, "artifact_manifest_release_budget_missing"));

    for (name, value, expected) in invalid {
        let path = root.join(format!("{name}.json"));
        fs::write(
            &path,
            serde_json::to_vec_pretty(&value).expect("encode invalid fixture"),
        )
        .expect("write invalid fixture");
        let rejected = run_validate_artifact_manifest(&path);
        assert!(
            !rejected.status.success(),
            "invalid {name} manifest unexpectedly passed"
        );
        let error = format!(
            "{}{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(
            error.contains(expected),
            "invalid {name} manifest returned wrong error: {error}"
        );
    }

    fs::remove_dir_all(&root).expect("remove manifest fixture");
}

#[test]
fn build_identity_task_freezes_clean_and_dirty_source_inputs() {
    let repo = init_git_fixture("build-identity");
    fs::create_dir_all(repo.join("scripts")).expect("create scripts fixture");
    fs::write(repo.join("Cargo.lock"), b"abc").expect("write Cargo.lock fixture");
    fs::write(repo.join("scripts").join("artifacts.json"), b"{}")
        .expect("write artifact manifest fixture");
    commit_git_fixture(&repo);

    let output_root = fixture_root("build-identity-output");
    fs::create_dir_all(&output_root).expect("create build identity output fixture");
    let clean_path = output_root.join("clean.cmd");
    let clean = run_build_identity(&repo, "release-fast", &clean_path);
    assert!(
        clean.status.success(),
        "clean build identity failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    let clean_environment = fs::read_to_string(&clean_path).expect("read clean build identity");
    let commit = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read fixture commit");
    let commit = String::from_utf8(commit.stdout)
        .expect("fixture commit is UTF-8")
        .trim()
        .to_owned();
    assert!(clean_environment.contains(&format!("set \"AGENTERM_BUILD_GIT_COMMIT={commit}\"")));
    assert!(clean_environment.contains("set \"AGENTERM_BUILD_GIT_DIRTY=false\""));
    assert!(clean_environment.contains(concat!(
        "set \"AGENTERM_BUILD_CARGO_LOCK_SHA256=",
        "ba7816bf8f01cfea414140de5dae2223",
        "b00361a396177a9cb410ff61f20015ad\""
    )));
    assert!(clean_environment.contains(concat!(
        "set \"AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256=",
        "44136fa355b3678a1146ad16f7e8649e",
        "94fb4fc21fe77e8310c060f61caaff8a\""
    )));
    assert!(clean_environment.contains("set \"AGENTERM_BUILD_PROFILE=release-fast\""));

    fs::write(repo.join("Cargo.lock"), b"changed").expect("dirty tracked fixture");
    let dirty_path = output_root.join("dirty.cmd");
    let dirty = run_build_identity(&repo, "dev", &dirty_path);
    assert!(dirty.status.success());
    assert!(
        fs::read_to_string(&dirty_path)
            .expect("read dirty build identity")
            .contains("set \"AGENTERM_BUILD_GIT_DIRTY=true\"")
    );

    let invalid = run_build_identity(&repo, "fastest", &output_root.join("invalid.cmd"));
    assert!(!invalid.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&invalid.stdout),
            String::from_utf8_lossy(&invalid.stderr)
        )
        .contains("build_identity_profile_invalid")
    );

    fs::remove_dir_all(&repo).expect("remove Git fixture");
    fs::remove_dir_all(&output_root).expect("remove output fixture");
}

#[test]
fn build_metadata_task_preserves_frozen_identity_and_detects_drift() {
    let repo = init_git_fixture("build-metadata");
    let scripts = repo.join("scripts");
    let staged = fixture_root("build-metadata-staged");
    let output_root = fixture_root("build-metadata-output");
    fs::create_dir_all(&scripts).expect("create scripts fixture");
    fs::create_dir_all(&staged).expect("create staged fixture");
    fs::create_dir_all(&output_root).expect("create output fixture");

    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"9.8.7\"\n",
    )
    .expect("write Cargo.toml fixture");
    fs::write(repo.join("Cargo.lock"), b"locked-input").expect("write Cargo.lock fixture");
    let artifact_manifest = scripts.join("artifacts.json");
    fs::write(
        &artifact_manifest,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "executables": [{
                "name": "agenterm-cli.exe",
                "role": "cli",
                "pe_subsystem": 3,
                "documentation_role": "fixture client",
                "offline_probe": ["--version"],
                "release_budget_bytes": 1024
            }]
        }))
        .expect("encode artifact manifest"),
    )
    .expect("write artifact manifest");
    let executable = b"fixture-executable";
    fs::write(staged.join("agenterm-cli.exe"), executable).expect("write staged executable");
    commit_git_fixture(&repo);

    let frozen_path = output_root.join("identity.cmd");
    let frozen = run_build_identity(&repo, "release-fast", &frozen_path);
    assert!(
        frozen.status.success(),
        "freeze failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&frozen.stdout),
        String::from_utf8_lossy(&frozen.stderr)
    );
    let frozen_environment = parse_batch_environment(&frozen_path);
    let clean_path = output_root.join("clean.json");
    let clean = run_write_build_metadata(
        &repo,
        &clean_path,
        &artifact_manifest,
        &staged,
        "release-fast",
        &frozen_environment,
    );
    assert!(
        clean.status.success(),
        "clean metadata failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    let clean: serde_json::Value =
        serde_json::from_slice(&fs::read(&clean_path).expect("read clean metadata"))
            .expect("decode clean metadata");
    let frozen_commit = frozen_environment
        .iter()
        .find(|(name, _)| name == "AGENTERM_BUILD_GIT_COMMIT")
        .map(|(_, value)| value)
        .expect("frozen commit");
    assert_eq!(clean["schema_version"], 2);
    assert_eq!(clean["version"], "9.8.7");
    assert_eq!(clean["profile"], "release-fast");
    assert_eq!(clean["git_commit"], frozen_commit.as_str());
    assert_eq!(clean["git_dirty"], false);
    assert_eq!(clean["executables"][0]["name"], "agenterm-cli.exe");
    assert_eq!(clean["executables"][0]["role"], "cli");
    assert_eq!(clean["executables"][0]["size"], executable.len());
    assert_eq!(clean["executables"][0]["sha256"], sha256(executable));
    assert!(
        clean["build_time_utc"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z') && !value.contains('.'))
    );

    fs::write(repo.join("Cargo.lock"), b"changed-locked-input").expect("change frozen input");
    let tracked_change = run_write_build_metadata(
        &repo,
        &output_root.join("tracked-change.json"),
        &artifact_manifest,
        &staged,
        "release-fast",
        &frozen_environment,
    );
    assert!(!tracked_change.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&tracked_change.stdout),
            String::from_utf8_lossy(&tracked_change.stderr)
        )
        .contains("build_metadata_clean_identity_changed")
    );

    let mut frozen_dirty = frozen_environment.clone();
    frozen_dirty
        .iter_mut()
        .find(|(name, _)| name == "AGENTERM_BUILD_GIT_DIRTY")
        .expect("frozen dirty field")
        .1 = "true".to_owned();
    let changed_hash = run_write_build_metadata(
        &repo,
        &output_root.join("changed-hash.json"),
        &artifact_manifest,
        &staged,
        "release-fast",
        &frozen_dirty,
    );
    assert!(!changed_hash.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&changed_hash.stdout),
            String::from_utf8_lossy(&changed_hash.stderr)
        )
        .contains("build_metadata_frozen_input_hash_changed")
    );

    let fallback_path = output_root.join("fallback.json");
    let fallback = run_write_build_metadata(
        &repo,
        &fallback_path,
        &artifact_manifest,
        &staged,
        "dev",
        &[],
    );
    assert!(
        fallback.status.success(),
        "fallback metadata failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&fallback.stdout),
        String::from_utf8_lossy(&fallback.stderr)
    );
    let fallback: serde_json::Value =
        serde_json::from_slice(&fs::read(&fallback_path).expect("read fallback metadata"))
            .expect("decode fallback metadata");
    assert_eq!(fallback["profile"], "dev");
    assert_eq!(fallback["git_dirty"], true);
    assert_eq!(
        fallback["cargo_lock_sha256"],
        sha256(b"changed-locked-input")
    );

    fs::remove_dir_all(&repo).expect("remove Git fixture");
    fs::remove_dir_all(&staged).expect("remove staged fixture");
    fs::remove_dir_all(&output_root).expect("remove output fixture");
}

#[test]
fn stage_build_task_composes_cleanup_staging_and_metadata() {
    let repo = init_git_fixture("stage-build");
    let scripts = repo.join("scripts");
    let source = fixture_root("stage-build-source");
    let destination = fixture_root("stage-build-destination");
    fs::create_dir_all(&scripts).expect("create scripts fixture");
    fs::create_dir_all(&source).expect("create source fixture");
    fs::create_dir_all(&destination).expect("create destination fixture");

    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"1.2.3\"\n",
    )
    .expect("write Cargo.toml fixture");
    fs::write(repo.join("Cargo.lock"), b"stage-build-lock").expect("write Cargo.lock fixture");
    fs::write(
        scripts.join("artifacts.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "executables": [{
                "name": "agenterm-cli.exe",
                "role": "cli",
                "pe_subsystem": 3,
                "documentation_role": "fixture client",
                "offline_probe": ["--version"],
                "release_budget_bytes": 1024
            }]
        }))
        .expect("encode artifact manifest"),
    )
    .expect("write artifact manifest");
    fs::write(source.join("agenterm-cli.exe"), b"new-cli").expect("write source artifact");
    fs::write(destination.join("agenterm-cli.exe"), b"old-cli")
        .expect("write old destination artifact");
    fs::write(destination.join("agenterm-cli.locked-1.exe"), b"stale")
        .expect("write stale artifact");
    fs::write(destination.join("agentermctl.exe"), b"obsolete").expect("write obsolete artifact");
    fs::write(destination.join("other.locked-1.exe"), b"unrelated")
        .expect("write unrelated artifact");
    commit_git_fixture(&repo);

    let staged = run_stage_build(&repo, &source, &destination, "dev");
    assert!(
        staged.status.success(),
        "stage build failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&staged.stdout),
        String::from_utf8_lossy(&staged.stderr)
    );
    assert_eq!(
        fs::read(destination.join("agenterm-cli.exe")).expect("read staged executable"),
        b"new-cli"
    );
    assert!(!destination.join("agenterm-cli.locked-1.exe").exists());
    assert!(!destination.join("agentermctl.exe").exists());
    assert!(destination.join("other.locked-1.exe").exists());
    let metadata: serde_json::Value = serde_json::from_slice(
        &fs::read(destination.join("agenterm.json")).expect("read staged metadata"),
    )
    .expect("decode staged metadata");
    assert_eq!(metadata["version"], "1.2.3");
    assert_eq!(metadata["profile"], "dev");
    assert_eq!(metadata["git_dirty"], false);
    assert_eq!(metadata["executables"][0]["sha256"], sha256(b"new-cli"));

    let rejected_destination = fixture_root("stage-build-rejected");
    let rejected = run_stage_build(&repo.join("scripts"), &source, &rejected_destination, "dev");
    assert!(!rejected.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        )
        .contains("stage_build_repo_not_exact_git_root")
    );
    assert!(
        !rejected_destination.exists(),
        "invalid repository mutated the destination"
    );

    fs::remove_dir_all(&repo).expect("remove Git fixture");
    fs::remove_dir_all(&source).expect("remove source fixture");
    fs::remove_dir_all(&destination).expect("remove destination fixture");
}

#[test]
fn migration_audit_rejects_operational_references_to_deleted_scripts() {
    let repo = init_git_fixture("migration-audit");
    let source_repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ledger_text = fs::read_to_string(
        source_repo
            .join("scripts")
            .join("powershell-migration.json"),
    )
    .expect("read migration ledger");
    let ledger: serde_json::Value =
        serde_json::from_str(&ledger_text).expect("decode migration ledger");
    fs::create_dir_all(repo.join("scripts")).expect("create fixture scripts");
    fs::write(
        repo.join("scripts").join("powershell-migration.json"),
        ledger_text,
    )
    .expect("write fixture ledger");

    for entry in ledger["entries"].as_array().expect("ledger entries") {
        if entry["status"] == "inventory" {
            let path = repo.join(entry["path"].as_str().expect("ledger path"));
            fs::create_dir_all(path.parent().expect("inventory parent"))
                .expect("create inventory parent");
            fs::write(path, b"# inventory fixture\n").expect("write inventory fixture");
        }
    }
    for name in ["build.bat", "check.ps1", "lint.ps1", "release.ps1"] {
        let path = repo.join(name);
        if !path.exists() {
            fs::write(path, b"# operational fixture\n").expect("write operational fixture");
        }
    }
    let workflow = repo.join(".github").join("workflows").join("release.yml");
    fs::create_dir_all(workflow.parent().expect("workflow parent"))
        .expect("create workflow parent");
    let deleted_script_reference = ["run: ./scripts/artifact-manifest", ".ps1\n"].concat();
    fs::write(&workflow, deleted_script_reference).expect("write stale workflow reference");
    commit_git_fixture(&repo);

    let rejected = run_migration_audit(&repo);
    assert!(!rejected.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        )
        .contains("migration_deleted_ps1_reference:.github/workflows/release.yml")
    );

    fs::write(&workflow, b"run: agenterm-script task run package\n")
        .expect("remove stale workflow reference");
    let accepted = run_migration_audit(&repo);
    assert!(
        accepted.status.success(),
        "clean fixture failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&accepted.stdout).expect("decode migration report");
    let entries = ledger["entries"].as_array().expect("ledger entries");
    let expected_deleted = entries
        .iter()
        .filter(|entry| entry["status"] == "deleted")
        .count();
    let expected_remaining = entries.len() - expected_deleted;
    assert_eq!(report["deleted_count"], expected_deleted);
    assert_eq!(report["remaining_count"], expected_remaining);

    fs::remove_dir_all(&repo).expect("remove migration fixture");
}

#[test]
fn prd_alignment_task_matches_public_catalogs_and_fails_closed() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let accepted = run_prd_alignment(repo);
    assert!(
        accepted.status.success(),
        "PRD alignment failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&accepted.stdout).trim(),
        concat!(
            "PASS: PRD aligns with 61 catalog entries, 83 public names, ",
            "11 protocol features, 41 mux commands, 57 capability IDs, ",
            "and 57 executable evidence IDs"
        )
    );

    let fixture = fixture_root("prd-alignment");
    fs::create_dir_all(fixture.join("prd")).expect("create PRD fixture directory");
    fs::create_dir_all(fixture.join("scripts")).expect("create script fixture directory");
    fs::create_dir_all(fixture.join("tests")).expect("create test fixture directory");
    copy_fixture_file(repo, &fixture, "PRD.md");
    copy_fixture_file(repo, &fixture, "scripts/qualification-gates.json");
    for entry in fs::read_dir(repo.join("prd")).expect("read PRD modules") {
        let entry = entry.expect("read PRD module entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "alignment-contract.json" || (name.starts_with("PRD_") && name.ends_with(".md"))
        {
            copy_fixture_file(repo, &fixture, &format!("prd/{name}"));
        }
    }
    for suite in [
        "cli_smoke.ps1",
        "remote_ui_smoke.ps1",
        "fleet_smoke.ps1",
        "script_smoke.ps1",
        "theme_smoke.ps1",
        "workbench_smoke.ps1",
        "ux_smoke.ps1",
    ] {
        copy_fixture_file(repo, &fixture, &format!("tests/{suite}"));
    }
    copy_fixture_file(repo, &fixture, "scripts/rhai/working-context-smoke.rhai");
    copy_fixture_file(repo, &fixture, "scripts/rhai/server-smoke.rhai");
    copy_fixture_file(repo, &fixture, "scripts/rhai/remote-ui-upgrade-smoke.rhai");
    let contract_path = fixture.join("prd").join("alignment-contract.json");
    let malformed = fs::read_to_string(&contract_path)
        .expect("read fixture alignment contract")
        .replacen("\"schema_version\": 2", "\"schema_version\": 99", 1);
    fs::write(&contract_path, malformed).expect("corrupt fixture contract schema");

    let rejected = run_prd_alignment(&fixture);
    assert!(!rejected.status.success());
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected.stdout),
            String::from_utf8_lossy(&rejected.stderr)
        )
        .contains("prd_alignment_contract_schema")
    );
    fs::remove_dir_all(fixture).expect("remove PRD alignment fixture");
}

#[cfg(windows)]
#[test]
fn rhai_harness_cleanup_owns_only_registered_children() {
    let output = run_harness_cleanup_selftest();
    assert!(
        output.status.success(),
        "Rhai harness cleanup failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "PASS: harness cleanup ownership and orphan proof"
    );
}

#[cfg(windows)]
#[test]
fn rhai_working_context_smoke_is_private_ephemeral_and_orphan_free() {
    let output = run_working_context_smoke();
    assert!(
        output.status.success(),
        "Rhai working-context smoke failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("EVIDENCE ux.working-context-proxy"));
    assert!(stdout.contains(
        "PASS: archived Proxy controls retain redacted launch facts without mutation or persistence"
    ));
    assert!(!stdout.contains("credential-"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("credential-"));
}

#[cfg(windows)]
#[test]
fn rhai_server_smoke_preserves_headless_authority_and_cleanup() {
    let output = run_server_smoke();
    assert!(
        output.status.success(),
        "Rhai server smoke failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("EVIDENCE server.headless-authority"));
    assert!(
        stdout.contains("PASS: headless server owns PTY, parser, workspace, events, and no HWND")
    );
}

#[cfg(windows)]
#[test]
fn rhai_wake_smoke_preserves_concurrent_ipc_pty_and_expired_mutation() {
    let output = run_wake_smoke();
    assert!(
        output.status.success(),
        "Rhai wake smoke failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("EVIDENCE fleet.wake-coalescing"));
    assert!(
        stdout
            .contains("PASS: coalesced wake delivery preserved IPC, PTY, and mutation correctness")
    );
}

#[test]
fn preflight_task_is_fail_closed_and_writes_reports_for_real_git_fixtures() {
    let source_repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cases = [
        ("clean", true, ""),
        ("crlf", true, ""),
        ("dirty", false, "clean-tree"),
        ("wrong-branch", false, "branch-main"),
        ("bad-hash", false, "cargo-lock"),
        ("bad-manifest", false, "artifact-manifest"),
    ];

    for (mode, should_pass, expected_failed_gate) in cases {
        let fixture = fixture_root(&format!("preflight-{mode}"));
        fs::create_dir_all(&fixture).expect("create preflight fixture");
        for relative in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "release.ps1",
            "scripts/artifacts.json",
            "scripts/qualification-gates.json",
            ".github/workflows/release.yml",
        ] {
            copy_fixture_file(source_repo, &fixture, relative);
        }
        fs::write(fixture.join(".gitignore"), "/target/\n").expect("write fixture ignore");
        fs::write(fixture.join("README.md"), "fixture\n").expect("write fixture README");

        if mode == "crlf" {
            for relative in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
                let path = fixture.join(relative);
                let normalized = fs::read_to_string(&path)
                    .expect("read CRLF source")
                    .replace("\r\n", "\n")
                    .replace('\r', "\n");
                fs::write(path, normalized.replace('\n', "\r\n")).expect("write CRLF fixture");
            }
        }
        if mode == "bad-hash" {
            let path = fixture.join("Cargo.lock");
            let mut lock = fs::read_to_string(&path).expect("read Cargo.lock fixture");
            let marker = "checksum = \"";
            let start = lock
                .find(marker)
                .expect("fixture contains registry checksum")
                + marker.len();
            let end = lock[start..].find('"').expect("checksum terminator") + start;
            lock.replace_range(start..end, "not-a-sha256");
            fs::write(path, lock).expect("write bad checksum fixture");
        }
        if mode == "bad-manifest" {
            fs::write(
                fixture.join("scripts").join("artifacts.json"),
                "{ invalid json\n",
            )
            .expect("write invalid artifact manifest fixture");
        }

        let initialized = Command::new("git")
            .args(["init", "--quiet", "-b", "main"])
            .arg(&fixture)
            .output()
            .expect("initialize preflight Git fixture");
        assert!(
            initialized.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        let remote = Command::new("git")
            .arg("-C")
            .arg(&fixture)
            .args([
                "remote",
                "add",
                "origin",
                "https://fixture-secret@example.invalid/agenterm.git",
            ])
            .output()
            .expect("add fixture remote");
        assert!(remote.status.success(), "add fixture remote failed");
        commit_git_fixture(&fixture);

        if mode == "dirty" {
            fs::write(fixture.join("README.md"), "fixture\ndirty\n").expect("dirty fixture");
        }
        if mode == "wrong-branch" {
            let switched = Command::new("git")
                .arg("-C")
                .arg(&fixture)
                .args(["switch", "--quiet", "-c", "feature/preflight"])
                .output()
                .expect("switch fixture branch");
            assert!(switched.status.success(), "switch fixture branch failed");
        }

        let report_path = fixture.join("target").join("nested").join("preflight.json");
        let output = run_preflight(&fixture, &report_path);
        assert_eq!(
            output.status.success(),
            should_pass,
            "unexpected preflight exit for {mode}:\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&report_path).expect("preflight emitted JSON report"))
                .expect("decode preflight report");
        assert_eq!(report["passed"], should_pass, "report result for {mode}");
        assert_eq!(report["kind"], "agenterm-read-only-preflight");
        assert_eq!(
            report["remotes"][0]["url"], "https://<redacted>@example.invalid/agenterm.git",
            "remote credentials must be redacted"
        );
        if !should_pass {
            assert!(
                report["gates"]
                    .as_array()
                    .expect("preflight gates")
                    .iter()
                    .any(|gate| gate["id"] == expected_failed_gate && gate["passed"] == false),
                "expected failed gate {expected_failed_gate} for {mode}: {report}"
            );
        }

        fs::remove_dir_all(&fixture).expect("remove preflight fixture");
    }

    let source = fs::read_to_string(source_repo.join("scripts/rhai/preflight.rhai"))
        .expect("read preflight source")
        .to_ascii_lowercase();
    for forbidden in [
        "cargo build",
        "cargo check",
        "cargo test",
        "cargo run",
        "rustc ",
        "git fetch",
        "git push",
        "git ls-remote",
        "invoke-webrequest",
        "invoke-restmethod",
        "start-bitstransfer",
    ] {
        assert!(
            !source.contains(forbidden),
            "preflight contains forbidden active operation: {forbidden}"
        );
    }
}

#[test]
fn preflight_benchmark_task_measures_clean_public_worker_runs() {
    let source_repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = fixture_root("preflight-benchmark");
    let cloned = Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks"])
        .arg(source_repo)
        .arg(&fixture)
        .output()
        .expect("clone clean preflight benchmark fixture");
    assert!(
        cloned.status.success(),
        "clone benchmark fixture failed: {}",
        String::from_utf8_lossy(&cloned.stderr)
    );

    let report_path = fixture.join("target").join("benchmark.json");
    let output = run_preflight_benchmark(&fixture, &report_path);
    assert!(
        output.status.success(),
        "preflight benchmark failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).expect("read benchmark report"))
            .expect("decode benchmark report");
    assert_eq!(report["kind"], "agenterm-read-only-preflight-benchmark");
    assert_eq!(report["iterations"], 5);
    assert_eq!(report["all_preflights_passed"], true);
    assert_eq!(report["target_met"], true);
    assert!(
        report["runs"]
            .as_array()
            .expect("benchmark runs")
            .iter()
            .all(|run| run["passed"] == true && run["exit_code"] == 0),
        "benchmark masked a failed preflight: {report}"
    );
    assert!(
        fs::read_dir(fixture.join("target"))
            .expect("read benchmark output directory")
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().starts_with("runs-")),
        "benchmark retained a scratch directory"
    );

    fs::remove_dir_all(&fixture).expect("remove benchmark fixture");
}

#[test]
fn supply_chain_task_is_deterministic_and_covers_the_resolved_lock_graph() {
    let root = fixture_root("supply-chain");
    fs::create_dir_all(&root).expect("create supply-chain output fixture");
    let first_path = root.join("first.spdx.json");
    let first = run_supply_chain(&first_path);
    assert!(
        first.status.success(),
        "first supply-chain run failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let first_bytes = fs::read(&first_path).expect("read first SPDX document");
    let document: serde_json::Value =
        serde_json::from_slice(&first_bytes).expect("decode SPDX document");
    assert_eq!(document["spdxVersion"], "SPDX-2.3");
    assert_eq!(document["dataLicense"], "CC0-1.0");
    assert_eq!(
        document["creationInfo"]["creators"][0],
        "Tool: agenterm-script task run supply-chain"
    );
    let packages = document["packages"].as_array().expect("SPDX packages");
    let relationships = document["relationships"]
        .as_array()
        .expect("SPDX relationships");
    assert_eq!(relationships.len(), packages.len());
    assert_eq!(
        document["documentDescribes"]
            .as_array()
            .expect("document describes")
            .len(),
        packages.len()
    );
    assert!(
        packages.iter().all(|package| {
            package["SPDXID"]
                .as_str()
                .is_some_and(|id| id.starts_with("SPDXRef-Package-"))
                && package["licenseDeclared"]
                    .as_str()
                    .is_some_and(|license| !license.is_empty())
                && package["externalRefs"][0]["referenceType"] == "purl"
        }),
        "SPDX package contract is incomplete"
    );
    assert!(
        packages.windows(2).all(|pair| {
            let key = |package: &serde_json::Value| {
                format!(
                    "{}\n{}\n{}\n{}",
                    package["name"].as_str().unwrap_or_default(),
                    package["versionInfo"].as_str().unwrap_or_default(),
                    package["comment"].as_str().unwrap_or_default(),
                    package["SPDXID"].as_str().unwrap_or_default()
                )
            };
            key(&pair[0]) <= key(&pair[1])
        }),
        "SPDX packages are not deterministically ordered"
    );

    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock = fs::read_to_string(repo.join("Cargo.lock")).expect("read Cargo.lock");
    let expected_packages = lock.matches("[[package]]").count() - 1;
    assert_eq!(
        packages.len(),
        expected_packages,
        "SPDX inventory omitted or added resolved packages"
    );
    assert!(
        fs::read_dir(&root)
            .expect("read supply-chain output directory")
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with("cargo-metadata-")),
        "supply-chain task retained metadata scratch files"
    );

    fs::remove_dir_all(&root).expect("remove supply-chain fixture");
}

#[cfg(windows)]
#[test]
fn stage_artifact_task_parks_a_running_windows_image() {
    let root = fixture_root("stage-running-artifact");
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).expect("create source fixture");
    fs::create_dir_all(&destination).expect("create destination fixture");
    let name = "agenterm-script.exe";
    fs::write(source.join(name), b"replacement-image").expect("write replacement fixture");
    fs::copy(
        env!("CARGO_BIN_EXE_agenterm-script"),
        destination.join(name),
    )
    .expect("copy runnable artifact fixture");

    let running = Command::new(destination.join(name))
        .arg("--worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start staged worker fixture");
    let mut running = RunningFixture(Some(running));

    let staged = run_stage_artifact(&source, &destination, name);
    running.terminate();
    assert!(
        staged.status.success(),
        "running-image staging failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&staged.stdout),
        String::from_utf8_lossy(&staged.stderr)
    );
    assert_eq!(
        fs::read(destination.join(name)).expect("read replacement image"),
        b"replacement-image"
    );
    let parked = fs::read_dir(&destination)
        .expect("read destination")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name().is_some_and(|file_name| {
                let file_name = file_name.to_string_lossy();
                file_name.starts_with("agenterm-script.locked-") && file_name.ends_with(".exe")
            })
        })
        .expect("running image was not parked");
    assert!(
        String::from_utf8_lossy(&staged.stdout).contains("running copy remains"),
        "staging did not report the parked image"
    );

    fs::remove_file(parked).expect("remove parked image");
    fs::remove_dir_all(&root).expect("remove fixture");
}

#[cfg(windows)]
#[test]
fn clean_locked_artifacts_task_retains_in_use_file_then_retries() {
    let root = fixture_root("clean-locked-in-use");
    fs::create_dir_all(&root).expect("create fixture");
    let locked = root.join("agenterm-server.locked-789.exe");
    let handle = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(&locked)
        .expect("open fixture without delete sharing");

    let retained = run_clean_locked(&root, &[]);
    assert!(
        retained.status.success(),
        "in-use cleanup failed: {}",
        String::from_utf8_lossy(&retained.stderr)
    );
    assert!(locked.exists(), "in-use executable was removed");
    assert!(
        String::from_utf8_lossy(&retained.stdout)
            .contains("Retained 1 locked artifact(s) still in use")
    );

    drop(handle);
    let retried = run_clean_locked(&root, &[]);
    assert!(
        retried.status.success(),
        "retry cleanup failed: {}",
        String::from_utf8_lossy(&retried.stderr)
    );
    assert!(!locked.exists(), "released executable was not removed");
    assert!(
        String::from_utf8_lossy(&retried.stdout).contains("Removed 1 stale locked artifact(s)")
    );

    fs::remove_dir_all(&root).expect("remove fixture");
}
