//! Framed-worker subprocess parity for rh backend task arguments.

use std::process::{Command, Stdio};
use std::sync::Mutex;

use agenterm::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, SCRIPT_FRAME_VERSION, ScriptBudgets, ScriptFrame,
    ScriptFramePayload, ScriptFrameRead, ScriptInvocation, ScriptOperation, ScriptProfile,
    read_script_frame, write_script_frame,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_rh_backend<T>(run: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "rh");
    }
    let out = run();
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
    out
}

#[test]
fn framed_worker_run_honors_task_arguments_with_rh_backend() {
    with_rh_backend(|| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn framed worker");

        let invocation = ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "rh-framed-args".into(),
            api_version: SCRIPT_API_VERSION,
            operation: ScriptOperation::Run,
            profile: ScriptProfile::Local,
            source_label: "rh-framed-args".into(),
            source: "fn entry() { args.len() }".into(),
            project_root: Some(env!("CARGO_MANIFEST_DIR").into()),
            invocation_temp_root: None,
            arguments: vec!["alpha".into(), "beta".into()],
            budgets: ScriptBudgets::default(),
            observation: None,
        };

        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "invoke-rh-framed-args".into(),
            payload: ScriptFramePayload::Invoke(invocation),
        };

        {
            let mut stdin = child.stdin.take().expect("stdin");
            write_script_frame(&mut stdin, &frame).expect("write invoke frame");
        }

        let mut stdout = child.stdout.take().expect("stdout");
        let result = loop {
            match read_script_frame(&mut stdout).expect("read frame") {
                ScriptFrameRead::Frame(frame) => {
                    if let ScriptFramePayload::Result(result) = frame.payload {
                        break result;
                    }
                }
                ScriptFrameRead::Eof => panic!("worker EOF before result"),
                ScriptFrameRead::Rejected(rejection) => {
                    panic!("frame rejected: {rejection:?}");
                }
            }
        };
        let _ = child.wait();

        assert!(result.ok, "expected ok result, got {:?}", result.failure);
        assert_eq!(result.value, Some(serde_json::json!(2)));
    });
}

#[test]
fn framed_worker_run_entry_fixture_with_rh_backend() {
    with_rh_backend(|| {
        let source = std::fs::read_to_string("fixtures/rh/entry.rh").expect("read entry.rh");
        let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn framed worker");

        let invocation = ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "rh-framed-entry".into(),
            api_version: SCRIPT_API_VERSION,
            operation: ScriptOperation::Run,
            profile: ScriptProfile::Local,
            source_label: "fixtures/rh/entry.rh".into(),
            source,
            project_root: Some(env!("CARGO_MANIFEST_DIR").into()),
            invocation_temp_root: None,
            arguments: vec![],
            budgets: ScriptBudgets::default(),
            observation: None,
        };

        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "invoke-rh-framed-entry".into(),
            payload: ScriptFramePayload::Invoke(invocation),
        };

        {
            let mut stdin = child.stdin.take().expect("stdin");
            write_script_frame(&mut stdin, &frame).expect("write invoke frame");
        }

        let mut stdout = child.stdout.take().expect("stdout");
        let result = loop {
            match read_script_frame(&mut stdout).expect("read frame") {
                ScriptFrameRead::Frame(frame) => {
                    if let ScriptFramePayload::Result(result) = frame.payload {
                        break result;
                    }
                }
                ScriptFrameRead::Eof => {
                    let stderr = child.stderr.take().map(|mut stderr| {
                        let mut text = String::new();
                        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
                        text
                    });
                    panic!("worker EOF before result: stderr={stderr:?}");
                }
                ScriptFrameRead::Rejected(rejection) => {
                    panic!("frame rejected: {rejection:?}");
                }
            }
        };
        let _ = child.wait();

        assert!(result.ok, "expected ok result, got {:?}", result.failure);
        assert_eq!(result.value, Some(serde_json::json!(42)));
    });
}

#[test]
fn framed_worker_captures_compat_fallback_print_output() {
    with_rh_backend(|| {
        let source =
            std::fs::read_to_string("scripts/rhai/lint.rhai").expect("read lint task source");
        let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn framed worker");

        let mut budgets = ScriptBudgets::default();
        budgets.operations = 100_000_000;
        budgets.wall_time_ms = 120_000;
        budgets.output_bytes = 1_048_576;
        budgets.string_bytes = 1_048_576;
        let invocation = ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "rh-framed-compat-output".into(),
            api_version: SCRIPT_API_VERSION,
            operation: ScriptOperation::Run,
            profile: ScriptProfile::Local,
            source_label: "scripts/rhai/lint.rhai".into(),
            source,
            project_root: Some(env!("CARGO_MANIFEST_DIR").into()),
            invocation_temp_root: None,
            arguments: vec![env!("CARGO_MANIFEST_DIR").into(), "static".into()],
            budgets,
            observation: None,
        };
        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "invoke-rh-framed-compat-output".into(),
            payload: ScriptFramePayload::Invoke(invocation),
        };

        {
            let mut stdin = child.stdin.take().expect("stdin");
            write_script_frame(&mut stdin, &frame).expect("write invoke frame");
        }

        let mut stdout = child.stdout.take().expect("stdout");
        let result = loop {
            match read_script_frame(&mut stdout).expect("read frame") {
                ScriptFrameRead::Frame(frame) => {
                    if let ScriptFramePayload::Result(result) = frame.payload {
                        break result;
                    }
                }
                ScriptFrameRead::Eof => panic!("worker EOF before result"),
                ScriptFrameRead::Rejected(rejection) => {
                    panic!("frame rejected by compat output: {rejection:?}");
                }
            }
        };
        let _ = child.wait();

        assert!(result.ok, "expected ok result, got {:?}", result.failure);
        assert!(
            result.stdout.starts_with("PASS: repository lint ("),
            "unexpected captured stdout: {:?}",
            result.stdout
        );
    });
}
