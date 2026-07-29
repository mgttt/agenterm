use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

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

    let first = run_prepare_target(&repo, &target);
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

    let second = run_prepare_target(&repo, &target);
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

#[cfg(windows)]
#[test]
fn clean_locked_artifacts_task_retains_in_use_file_then_retries() {
    let root = fixture_root("clean-locked-in-use");
    fs::create_dir_all(&root).expect("create fixture");
    let locked = root.join("agenterm-server.locked-789.exe");
    let handle = OpenOptions::new()
        .create(true)
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
