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
