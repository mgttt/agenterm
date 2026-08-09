//! Fast black-box coverage for the rh public CLI, invoked through a
//! hard-linked, isolated copy of the main `agenterm` PE
//! (`__agenterm-internal-engine rh <args>`).
//!
//! Historically this hard-linked the standalone `agenterm-rh` binary; that
//! binary is retired (`agenterm rh`/`__agenterm-internal-engine rh` are now
//! the only entry points), so this hard-links `CARGO_BIN_EXE_agenterm`
//! instead and always prepends the internal-engine marker + `rh` token. The
//! file name/scenario names still say "standalone" — kept this round so a
//! concurrent packaging/CI migration isn't confused by a rename; rename to
//! something like `rh_cli_contract.rs` in a later cleanup pass.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct StandaloneCli {
    root: PathBuf,
    executable: PathBuf,
}

impl StandaloneCli {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agenterm-rh-standalone-{label}-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create standalone CLI directory");

        let executable = root.join(binary_name("agenterm"));
        fs::hard_link(env!("CARGO_BIN_EXE_agenterm"), &executable)
            .expect("hard-link isolated agenterm PE");

        for stale_name in [root.join("agenterm-rhai"), root.join("agenterm-rhai.exe")] {
            assert!(
                !stale_name.exists(),
                "retired compatibility executable unexpectedly exists: {}",
                stale_name.display()
            );
        }

        Self { root, executable }
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(&path, contents).expect("write fixture");
        path
    }

    fn run<I, S>(&self, arguments: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new(&self.executable)
            .args(["__agenterm-internal-engine", "rh"])
            .current_dir(&self.root)
            .args(arguments)
            .output()
            .expect("run isolated agenterm rh")
    }
}

impl Drop for StandaloneCli {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> &str {
    std::str::from_utf8(&output.stderr).expect("stderr is UTF-8")
}

fn assert_exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout={}\nstderr={}",
        stdout(output),
        stderr(output)
    );
}

fn decode_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "decode stdout JSON: {error}\nstdout={}\nstderr={}",
            stdout(output),
            stderr(output)
        )
    })
}

#[test]
fn standalone_basics_preserve_help_version_and_unknown_command_io() {
    let cli = StandaloneCli::new("basics");

    for arguments in [Vec::<&str>::new(), vec!["help"], vec!["--help"]] {
        let output = cli.run(arguments);
        assert_exit(&output, 0);
        assert!(output.stdout.is_empty(), "stdout={}", stdout(&output));
        assert!(
            stderr(&output).contains("commands:"),
            "stderr={}",
            stderr(&output)
        );
        assert!(
            stderr(&output).contains("check-many --manifest FILE"),
            "stderr={}",
            stderr(&output)
        );
    }

    for argument in ["version", "--version", "-V"] {
        let output = cli.run([argument]);
        assert_exit(&output, 0);
        assert!(
            stdout(&output).starts_with("agenterm-rh "),
            "stdout={}",
            stdout(&output)
        );
        assert!(output.stderr.is_empty(), "stderr={}", stderr(&output));
    }

    let output = cli.run(["definitely-not-a-command"]);
    assert_exit(&output, 1);
    assert!(output.stdout.is_empty(), "stdout={}", stdout(&output));
    assert!(
        stderr(&output).contains("unknown command `definitely-not-a-command`"),
        "stderr={}",
        stderr(&output)
    );
}

#[test]
fn standalone_public_check_succeeds_in_plain_and_json_modes() {
    let cli = StandaloneCli::new("check-ok");
    let source = cli.write("good.rh", "fn entry() { 40 + 2 }\n");

    let plain = cli.run(["check".as_ref(), source.as_os_str()]);
    assert_exit(&plain, 0);
    assert_eq!(
        stdout(&plain),
        format!("rh check ok: {}\n", source.display())
    );
    assert!(plain.stderr.is_empty(), "stderr={}", stderr(&plain));

    let json = cli.run(["check".as_ref(), source.as_os_str(), "--json".as_ref()]);
    assert_exit(&json, 0);
    assert!(json.stderr.is_empty(), "stderr={}", stderr(&json));
    let envelope = decode_stdout(&json);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["exit_class"], "success");
    assert!(envelope["failure"].is_null());
}

#[test]
fn standalone_public_check_reports_subset_and_api_failures_as_json() {
    let cli = StandaloneCli::new("check-failures");
    let subset = cli.write("subset.rh", "eval(\"1\");\n");
    let unknown_api = cli.write(
        "unknown-api.rh",
        "fn entry() { std::fs::not_shipped(`x`) }\n",
    );

    let output = cli.run(["check".as_ref(), subset.as_os_str(), "--json".as_ref()]);
    assert_exit(&output, 1);
    assert!(output.stderr.is_empty(), "stderr={}", stderr(&output));
    let envelope = decode_stdout(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["exit_class"], "script");
    assert_eq!(envelope["failure"]["code"], "RH_SUBSET_NO_EVAL");
    assert!(envelope["failure"]["message"].is_string());

    let output = cli.run(["check".as_ref(), unknown_api.as_os_str(), "--json".as_ref()]);
    assert_exit(&output, 1);
    assert!(output.stderr.is_empty(), "stderr={}", stderr(&output));
    let envelope = decode_stdout(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["exit_class"], "script");
    assert_eq!(envelope["failure"]["code"], "script_api_unknown");
    assert!(envelope["failure"]["message"].is_string());
}

#[test]
fn standalone_public_check_rejects_missing_files_and_bad_arguments() {
    let cli = StandaloneCli::new("check-usage");
    let source = cli.write("good.rh", "fn entry() { 42 }\n");

    let missing = cli.run(["check", "missing.rh"]);
    assert_exit(&missing, 2);
    assert!(missing.stdout.is_empty(), "stdout={}", stdout(&missing));
    assert!(
        !missing.stderr.is_empty(),
        "missing file needs a diagnostic"
    );

    let bad_option = cli.run([
        "check".as_ref(),
        source.as_os_str(),
        "--not-an-option".as_ref(),
    ]);
    assert_exit(&bad_option, 2);
    assert!(
        bad_option.stdout.is_empty(),
        "stdout={}",
        stdout(&bad_option)
    );
    assert!(
        stderr(&bad_option).contains("unknown check option `--not-an-option`"),
        "stderr={}",
        stderr(&bad_option)
    );

    let missing_value = cli.run([
        "check".as_ref(),
        source.as_os_str(),
        "--project-root".as_ref(),
    ]);
    assert_exit(&missing_value, 2);
    assert!(
        stderr(&missing_value).contains("missing path after --project-root"),
        "stderr={}",
        stderr(&missing_value)
    );
}

#[test]
fn standalone_check_many_has_success_schema_plain_ok_and_typed_configuration_exit() {
    let cli = StandaloneCli::new("check-many");
    cli.write("one.rh", "fn entry() { 1 }\n");
    cli.write("two.rh", "fn entry() { 2 }\n");
    let manifest = cli.write(
        "check-many.json",
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "kind": "agenterm-rh-check-manifest",
            "files": ["one.rh", "two.rh"],
        }))
        .expect("encode check-many manifest"),
    );

    let json_output = cli.run([
        "check-many".as_ref(),
        "--manifest".as_ref(),
        manifest.as_os_str(),
        "--project-root".as_ref(),
        cli.root.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_exit(&json_output, 0);
    assert!(
        json_output.stderr.is_empty(),
        "stderr={}",
        stderr(&json_output)
    );
    let report = decode_stdout(&json_output);
    assert_eq!(report["schema_version"], 1);
    // Legacy report kind retained for JSON schema compatibility; agenterm-rh is the producer.
    assert_eq!(report["kind"], "agenterm-rhai-check-many");
    assert_eq!(report["ok"], true);
    assert_eq!(report["checked_files"], 2);
    assert!(report["total_source_bytes"].as_u64().is_some());
    assert!(report["duration_ms"].as_u64().is_some());
    assert_eq!(report["failures"], json!([]));

    let plain = cli.run([
        "check-many".as_ref(),
        "--manifest".as_ref(),
        manifest.as_os_str(),
        "--project-root".as_ref(),
        cli.root.as_os_str(),
    ]);
    assert_exit(&plain, 0);
    assert_eq!(stdout(&plain), "OK (2 files)\n");
    assert!(plain.stderr.is_empty(), "stderr={}", stderr(&plain));

    let duplicate_manifest = cli.write(
        "duplicate.json",
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "kind": "agenterm-rh-check-manifest",
            "files": ["one.rh", "./one.rh"],
        }))
        .expect("encode duplicate manifest"),
    );
    let rejected = cli.run([
        "check-many".as_ref(),
        "--manifest".as_ref(),
        duplicate_manifest.as_os_str(),
        "--project-root".as_ref(),
        cli.root.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_exit(&rejected, 2);
    assert!(rejected.stderr.is_empty(), "stderr={}", stderr(&rejected));
    let report = decode_stdout(&rejected);
    assert_eq!(report["ok"], false);
    assert_eq!(report["failures"][0]["path"], "./one.rh");
    assert_eq!(report["failures"][0]["code"], "check_many_duplicate");
    assert_eq!(report["failures"][0]["exit_class"], "configuration");
    assert!(report["failures"][0]["message"].is_string());
    assert!(report["failures"][0]["invocation_id"].is_string());
}

#[test]
fn standalone_task_discovery_and_check_do_not_need_compatibility_binary() {
    let cli = StandaloneCli::new("tasks");
    cli.write("scripts/check.rh", "fn entry() { 40 + 2 }\n");
    let manifest = cli.write(
        "agenterm.tasks.json",
        serde_json::to_vec_pretty(&json!({
            "schema_version": 2,
            "project": {
                "id": "standalone-cli",
                "version": "1.0.0",
                "requires": {
                    "script_api": {"minimum": 2, "maximum": 2},
                    "capabilities": ["runtime.project.named-task"],
                },
            },
            "tasks": [{
                "id": "check",
                "description": "Standalone task discovery fixture",
                "entry": "scripts/check.rh",
                "profile": "local",
            }],
        }))
        .expect("encode task manifest"),
    );

    let list = cli.run([
        "task".as_ref(),
        "list".as_ref(),
        "--manifest".as_ref(),
        manifest.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_exit(&list, 0);
    assert!(list.stderr.is_empty(), "stderr={}", stderr(&list));
    let catalog = decode_stdout(&list);
    assert_eq!(catalog["schema_version"], 2);
    assert_eq!(catalog["project_id"], "standalone-cli");
    assert_eq!(catalog["compatible"], true);
    assert_eq!(catalog["tasks"].as_array().map(Vec::len), Some(1));
    assert_eq!(catalog["tasks"][0]["id"], "check");
    assert_eq!(catalog["tasks"][0]["status"], "ready");

    let show = cli.run([
        "task".as_ref(),
        "show".as_ref(),
        "check".as_ref(),
        "--manifest".as_ref(),
        manifest.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_exit(&show, 0);
    assert!(show.stderr.is_empty(), "stderr={}", stderr(&show));
    let task = decode_stdout(&show);
    assert_eq!(task["project_id"], "standalone-cli");
    assert_eq!(task["tasks"].as_array().map(Vec::len), Some(1));
    assert_eq!(task["tasks"][0]["id"], "check");
    assert_eq!(task["tasks"][0]["status"], "ready");

    let check = cli.run([
        "task".as_ref(),
        "check".as_ref(),
        "check".as_ref(),
        "--manifest".as_ref(),
        manifest.as_os_str(),
    ]);
    assert_exit(&check, 0);
    assert_eq!(stdout(&check), "OK\n");
    assert!(check.stderr.is_empty(), "stderr={}", stderr(&check));
}
