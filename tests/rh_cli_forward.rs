//! rh front-door ownership and shared worker modes.

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
