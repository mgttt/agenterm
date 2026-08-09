use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> Fixture {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "agenterm-rh-check-many-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create fixture");
    Fixture { root }
}

fn write_manifest(root: &Path, files: Vec<String>) -> PathBuf {
    let path = root.join("check-many.json");
    fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "agenterm-rh-check-manifest",
                "files": files,
            }))
            .expect("encode manifest")
        ),
    )
    .expect("write manifest");
    path
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(root)
        .args(arguments)
        .env("AGENTERM_SCRIPT_AUDIT_PATH", root.join("audit.jsonl"))
        .output()
        .expect("run agenterm rh")
}

#[test]
fn check_many_preserves_typed_check_failures_and_plain_diagnostics() {
    let fixture = fixture();
    fs::write(fixture.root.join("good.rh"), "let answer = 40 + 2;\n").expect("write good script");
    fs::write(fixture.root.join("bad.rh"), "let broken = ;\n").expect("write bad script");
    let manifest = write_manifest(
        &fixture.root,
        vec!["good.rh".to_owned(), "bad.rh".to_owned()],
    );
    let manifest_text = manifest.to_str().expect("manifest path");
    let root_text = fixture.root.to_str().expect("fixture path");

    let batch = run(
        &fixture.root,
        &[
            "check-many",
            "--manifest",
            manifest_text,
            "--project-root",
            root_text,
            "--json",
        ],
    );
    assert_eq!(batch.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&batch.stdout).expect("decode batch report");
    assert_eq!(report["schema_version"], 1);
    // Legacy report kind retained for JSON schema compatibility; agenterm-rh is the producer.
    assert_eq!(report["kind"], "agenterm-rhai-check-many");
    assert_eq!(report["ok"], false);
    assert_eq!(report["checked_files"], 2);
    assert_eq!(report["failures"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["failures"][0]["path"], "bad.rh");

    let single = run(
        &fixture.root,
        &["check", "bad.rh", "--project-root", root_text, "--json"],
    );
    assert_eq!(single.status.code(), Some(1));
    let envelope: Value = serde_json::from_slice(&single.stdout).expect("decode check envelope");
    assert_eq!(report["failures"][0]["code"], envelope["failure"]["code"]);
    assert_eq!(
        report["failures"][0]["message"],
        envelope["failure"]["message"]
    );
    assert_eq!(report["failures"][0]["exit_class"], envelope["exit_class"]);

    let plain = run(
        &fixture.root,
        &[
            "check-many",
            "--manifest",
            manifest_text,
            "--project-root",
            root_text,
        ],
    );
    assert_eq!(plain.status.code(), Some(1));
    assert!(plain.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&plain.stderr);
    let payload = diagnostic
        .strip_prefix("bad.rh: ")
        .expect("path-prefixed diagnostic");
    let failure: Value = serde_json::from_str(payload.trim()).expect("typed diagnostic");
    assert_eq!(failure["code"], envelope["failure"]["code"]);
    assert_eq!(failure["exit_class"], envelope["exit_class"]);
}

#[test]
fn check_many_rejects_unbounded_or_ambiguous_manifests() {
    let fixture = fixture();
    fs::write(fixture.root.join("good.rh"), "40 + 2\n").expect("write script");
    let too_many = write_manifest(
        &fixture.root,
        (0..257).map(|index| format!("file-{index}.rh")).collect(),
    );
    let rejected = run(
        &fixture.root,
        &[
            "check-many",
            "--manifest",
            too_many.to_str().expect("manifest path"),
            "--project-root",
            fixture.root.to_str().expect("fixture path"),
        ],
    );
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("check_many_manifest_files"));

    fs::write(&too_many, vec![b' '; 65_537]).expect("write oversized manifest");
    let rejected = run(
        &fixture.root,
        &[
            "check-many",
            "--manifest",
            too_many.to_str().expect("manifest path"),
            "--project-root",
            fixture.root.to_str().expect("fixture path"),
        ],
    );
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("check_many_manifest_size"));

    let duplicate = write_manifest(
        &fixture.root,
        vec!["good.rh".to_owned(), "./good.rh".to_owned()],
    );
    let rejected = run(
        &fixture.root,
        &[
            "check-many",
            "--manifest",
            duplicate.to_str().expect("manifest path"),
            "--project-root",
            fixture.root.to_str().expect("fixture path"),
            "--json",
        ],
    );
    assert_eq!(rejected.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&rejected.stdout).expect("decode duplicate report");
    assert_eq!(report["failures"][0]["code"], "check_many_duplicate");
    assert_eq!(report["failures"][0]["exit_class"], "configuration");
}
