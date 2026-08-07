//! rh front-door ownership and legacy agenterm-rhai forwarding.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use agenterm::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, SCRIPT_FRAME_VERSION, ScriptBudgets, ScriptFrame,
    ScriptFramePayload, ScriptFrameRead, ScriptInvocation, ScriptOperation, ScriptProfile,
    ScriptResult, read_script_frame, write_script_frame,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "agenterm-rh-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

fn ensure_adjacent_rh_cli() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_agenterm-rh"));
    let metadata = fs::metadata(&source).expect("agenterm-rh metadata");
    assert!(metadata.len() > 0, "agenterm-rh binary is empty");
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_agenterm-rhai"));
    let adjacent = worker
        .parent()
        .expect("agenterm-rhai parent")
        .join(binary_name("agenterm-rh"));
    assert!(
        source == adjacent,
        "expected agenterm-rh adjacent to worker at {}",
        adjacent.display()
    );
}

fn run_rhai(arguments: &[&str]) -> std::process::Output {
    let repo = repo_root();
    ensure_adjacent_rh_cli();
    Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
        .current_dir(&repo)
        .args(arguments)
        .output()
        .expect("run agenterm-rhai forward")
}

fn run_rh(arguments: &[&str]) -> std::process::Output {
    let repo = repo_root();
    Command::new(env!("CARGO_BIN_EXE_agenterm-rh"))
        .current_dir(&repo)
        .args(arguments)
        .output()
        .expect("run agenterm-rh")
}

#[test]
fn rh_is_the_task_front_door_without_compat_binary() {
    let repo = repo_root();
    let manifest = repo.join("agenterm.tasks.json");
    let output = run_rh(&[
        "task",
        "list",
        "--manifest",
        &manifest.display().to_string(),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("task list JSON from rh front door");
    assert!(
        value["tasks"]
            .as_array()
            .is_some_and(|tasks| !tasks.is_empty()),
        "unexpected task list: {value}"
    );
}

#[test]
fn isolated_rh_task_list_does_not_require_adjacent_rhai() {
    let repo = repo_root();
    let isolated = TestDirectory::new("missing-compat");
    let isolated_rh = isolated.path().join(binary_name("agenterm-rh"));
    fs::copy(env!("CARGO_BIN_EXE_agenterm-rh"), &isolated_rh).expect("copy isolated rh CLI");
    let manifest = repo.join("agenterm.tasks.json");
    let output = Command::new(isolated_rh)
        .current_dir(isolated.path())
        .args([
            "task",
            "list",
            "--manifest",
            &manifest.display().to_string(),
            "--json",
        ])
        .output()
        .expect("run isolated agenterm-rh");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("task list JSON from isolated rh");
    assert!(
        value["tasks"]
            .as_array()
            .is_some_and(|tasks| !tasks.is_empty()),
        "unexpected task list: {value}"
    );
}

fn worker_invocation(id: &str) -> ScriptInvocation {
    ScriptInvocation {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: id.into(),
        api_version: SCRIPT_API_VERSION,
        operation: ScriptOperation::Run,
        profile: ScriptProfile::Local,
        source_label: format!("{id}.rh"),
        source: "fn entry() { args.len() }".into(),
        project_root: Some(repo_root().display().to_string()),
        invocation_temp_root: None,
        arguments: vec!["alpha".into(), "beta".into()],
        budgets: ScriptBudgets::default(),
        observation: None,
    }
}

#[test]
fn rh_legacy_worker_mode_uses_shared_runtime() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-rh"))
        .arg("--worker")
        .env("AGENTERM_SCRIPT_BACKEND", "rh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn rh legacy worker");
    serde_json::to_writer(
        child.stdin.as_mut().expect("legacy worker stdin"),
        &worker_invocation("rh-legacy-worker"),
    )
    .expect("write legacy invocation");
    child
        .stdin
        .as_mut()
        .expect("legacy worker stdin")
        .write_all(b"\n")
        .expect("finish legacy invocation");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait for legacy worker");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: ScriptResult =
        serde_json::from_slice(&output.stdout).expect("legacy worker result");
    assert!(result.ok, "legacy worker failure: {:?}", result.failure);
    assert_eq!(result.value, Some(serde_json::json!(2)));
}

#[test]
fn rh_framed_worker_mode_uses_shared_runtime() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-rh"))
        .arg("--framed-worker")
        .env("AGENTERM_SCRIPT_BACKEND", "rh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn rh framed worker");
    let frame = ScriptFrame {
        frame_version: SCRIPT_FRAME_VERSION,
        frame_id: "invoke-rh-framed-worker".into(),
        payload: ScriptFramePayload::Invoke(worker_invocation("rh-framed-worker")),
    };
    write_script_frame(child.stdin.as_mut().expect("framed worker stdin"), &frame)
        .expect("write framed invocation");
    drop(child.stdin.take());
    let mut stdout = child.stdout.take().expect("framed worker stdout");
    let result = loop {
        match read_script_frame(&mut stdout).expect("read framed worker result") {
            ScriptFrameRead::Frame(frame) => {
                if let ScriptFramePayload::Result(result) = frame.payload {
                    break result;
                }
            }
            ScriptFrameRead::Eof => panic!("framed worker EOF before result"),
            ScriptFrameRead::Rejected(rejection) => {
                panic!("framed worker rejected frame: {rejection:?}")
            }
        }
    };
    let status = child.wait().expect("wait for framed worker");
    assert!(
        status.success(),
        "framed worker exited with {status}; result={result:?}"
    );
    assert!(result.ok, "framed worker failure: {:?}", result.failure);
    assert_eq!(result.value, Some(serde_json::json!(2)));
}

#[test]
fn rh_task_entry_propagates_shared_cli_exit_code() {
    let output = run_rh(&["task", "not-a-task-subcommand"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rhai_forwards_check_many_with_lint_style_flags() {
    let repo = repo_root();
    let manifest_path = repo.join("fixtures/rh/check-many-rhai-kind.json");
    let output = run_rhai(&[
        "check-many",
        "--manifest",
        &manifest_path.display().to_string(),
        "--profile",
        "local",
        "--project-root",
        &repo.display().to_string(),
        "--timeout-ms",
        "10000",
        "--max-operations",
        "1000000",
    ]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rhai_forwards_check_for_rh_fixture() {
    let repo = repo_root();
    let fixture = repo.join("fixtures/rh/entry.rh");
    let output = run_rhai(&["check", fixture.display().to_string().as_str()]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rhai_forwards_eval_for_rh_fixture() {
    let repo = repo_root();
    let fixture = repo.join("fixtures/rh/entry.rh");
    let output = run_rhai(&["eval", fixture.display().to_string().as_str()]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "expected eval entry value in stdout: {stdout}"
    );
}

#[test]
fn rhai_forwards_run_as_eval_for_rh_fixture() {
    let repo = repo_root();
    let fixture = repo.join("fixtures/rh/entry.rh");
    let output = run_rhai(&["run", fixture.display().to_string().as_str()]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "expected run->eval entry value in stdout: {stdout}"
    );
}

#[test]
fn rhai_inline_eval_stays_on_the_interpreted_compatibility_path() {
    let output = run_rhai(&["eval", "40 + 2", "--json"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("inline eval JSON");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["value"], 42);
}

#[test]
fn rhai_source_run_stays_on_the_interpreted_compatibility_path() {
    let fixture = TestDirectory::new("interpreted-run");
    let script = fixture.path().join("compat.rhai");
    fs::write(&script, "40 + 2\n").expect("write interpreted script");
    let output = run_rhai(&["run", &script.display().to_string(), "--json"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("interpreted run JSON");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["value"], 42);
}

#[test]
fn rhai_forwards_version_subcommand_to_adjacent_rh() {
    let output = run_rhai(&["version"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agenterm-rh"),
        "expected rh version banner: {stdout}"
    );
}

#[test]
fn rhai_forwards_version_flags_to_adjacent_rh() {
    let output = run_rhai(&["--version"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agenterm-rh"),
        "expected rh version banner: {stdout}"
    );
}

#[test]
fn rhai_forwards_dash_capital_version_to_adjacent_rh() {
    let output = run_rhai(&["-V"]);
    assert!(output.status.success());
}
