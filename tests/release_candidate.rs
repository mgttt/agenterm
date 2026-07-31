use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const VERSION: &str = "1.2.3";
const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

struct Fixture {
    root: PathBuf,
    assets: PathBuf,
    metadata: PathBuf,
    manifest: PathBuf,
    created_ms: u64,
    expires_ms: u64,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).expect("read hash input");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(value).expect("serialize JSON")
        ),
    )
    .expect("write JSON");
}

fn unique_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "agenterm-release-candidate-{name}-{}-{nonce}",
        std::process::id()
    ))
}

fn archive_specs() -> [(&'static str, &'static str, &'static str, bool); 6] {
    [
        ("windows", "x86_64", "zip", false),
        ("windows", "aarch64", "zip", false),
        ("linux", "x86_64", "tar.gz", false),
        ("linux", "aarch64", "tar.gz", false),
        ("macos", "aarch64", "zip", true),
        ("macos", "x86_64", "zip", true),
    ]
}

fn fixture(name: &str) -> Fixture {
    let root = unique_root(name);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis() as u64;
    let created_ms = now_ms - 1_000;
    let expires_ms = now_ms + 86_400_000;
    let assets = root.join("assets");
    fs::create_dir_all(root.join(".github/workflows")).expect("create workflow directory");
    fs::create_dir_all(root.join("scripts")).expect("create scripts directory");
    fs::create_dir_all(&assets).expect("create assets directory");

    fs::write(
        root.join(".github/workflows/candidate.yml"),
        "name: Candidate\n",
    )
    .expect("write workflow");
    let workflow_hash = sha256(&root.join(".github/workflows/candidate.yml"));
    fs::write(root.join("Cargo.lock"), "# fixture lock\n").expect("write Cargo.lock");
    write_json(
        &root.join("scripts/artifacts.json"),
        &json!({"schema_version": 2, "executables": [{"name": "agenterm.exe"}]}),
    );
    write_json(
        &root.join("scripts/qualification-gates.json"),
        &json!({
            "schema_version": 1,
            "receipt_schema_version": 1,
            "required_gates": [{"id": "release-gate", "evidence": []}]
        }),
    );

    let cargo_hash = sha256(&root.join("Cargo.lock"));
    let artifact_hash = sha256(&root.join("scripts/artifacts.json"));
    let gate_hash = sha256(&root.join("scripts/qualification-gates.json"));
    let sbom_name = format!("agenterm-{VERSION}-sbom.spdx.json");
    fs::write(assets.join(&sbom_name), "{\"spdxVersion\":\"SPDX-2.3\"}\n").expect("write SBOM");
    let sbom_hash = sha256(&assets.join(&sbom_name));

    write_json(
        &assets.join("qualification-receipt.json"),
        &json!({
            "schema_version": 1,
            "product": "AgenTerm",
            "release": true,
            "stress_included": true,
            "gate_manifest": {"sha256": gate_hash},
            "provenance": {
                "git_head": SOURCE_SHA,
                "source_dirty": false,
                "cargo_lock_sha256": cargo_hash,
                "artifact_manifest_sha256": artifact_hash,
                "sbom_sha256": sbom_hash
            },
            "gates": [{"id": "release-gate", "status": "passed", "evidence": []}]
        }),
    );

    for (os, arch, extension, unsigned_preview) in archive_specs() {
        let suffix = if unsigned_preview {
            "-unsigned-preview"
        } else {
            ""
        };
        let archive_name = format!("agenterm-{VERSION}-{os}-{arch}{suffix}.{extension}");
        let archive = assets.join(&archive_name);
        fs::write(&archive, format!("fixture archive {os} {arch}\n")).expect("write archive");
        let archive_hash = sha256(&archive);
        fs::write(
            assets.join(format!("{archive_name}.sha256")),
            format!("{archive_hash}  {archive_name}\n"),
        )
        .expect("write checksum");
        write_json(
            &assets.join(format!("{archive_name}.provenance.json")),
            &json!({
                "schema_version": 1,
                "product": "AgenTerm",
                "version": VERSION,
                "os": os,
                "arch": arch,
                "channel": if unsigned_preview {
                    "macos-unsigned-preview"
                } else {
                    "release"
                },
                "signed": false,
                "notarized": false,
                "artifact": archive_name,
                "sha256": archive_hash,
                "source_commit": SOURCE_SHA,
                "source_tag": format!("v{VERSION}"),
                "artifact_manifest_sha256": artifact_hash,
                "cargo_lock_sha256": cargo_hash,
                "sbom_sha256": if os == "macos" {
                    sbom_hash.clone()
                } else {
                    String::new()
                }
            }),
        );
    }
    fs::write(
        assets.join("MACOS_UNSIGNED_PREVIEW_README.md"),
        "Unsigned macOS preview.\n",
    )
    .expect("write preview README");
    fs::write(
        assets.join("MACOS_UNSIGNED_PREVIEW_README.zh-Hant.md"),
        "macOS 未簽署預覽。\n",
    )
    .expect("write translated preview README");

    let metadata = root.join("metadata.json");
    write_json(
        &metadata,
        &json!({
            "schema_version": 1,
            "source_sha": SOURCE_SHA,
            "version": VERSION,
            "expected_tag": format!("v{VERSION}"),
            "workflow_path": ".github/workflows/candidate.yml",
            "workflow_sha256": workflow_hash,
            "run_id": 12345,
            "run_attempt": 1,
            "run_event": "workflow_dispatch",
            "created_at_unix_ms": created_ms,
            "expires_at_unix_ms": expires_ms
        }),
    );
    let manifest = root.join("candidate-manifest.json");
    Fixture {
        root,
        assets,
        metadata,
        manifest,
        created_ms,
        expires_ms,
    }
}

fn run_script(script: &str, arguments: &[&Path]) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm-script"));
    command
        .current_dir(repo)
        .arg("run")
        .arg(repo.join("scripts/rhai").join(script))
        .arg("--project-root")
        .arg(repo)
        .arg("--");
    command.args(arguments);
    command.output().expect("run Candidate Rhai script")
}

fn aggregate(fixture: &Fixture) -> Output {
    run_script(
        "candidate-aggregate.rhai",
        &[
            &fixture.root,
            &fixture.metadata,
            &fixture.assets,
            &fixture.manifest,
        ],
    )
}

fn verify(fixture: &Fixture, now: u64) -> Output {
    let now = now.to_string();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .arg("run")
        .arg(repo.join("scripts/rhai/candidate-verify.rhai"))
        .arg("--project-root")
        .arg(repo)
        .arg("--")
        .arg(&fixture.root)
        .arg(&fixture.manifest)
        .arg(&fixture.assets)
        .arg(now)
        .output()
        .expect("verify Candidate")
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn exact_sha_candidate_happy_path_is_self_describing_and_verifiable() {
    let fixture = fixture("happy");
    let created = aggregate(&fixture);
    assert!(created.status.success(), "{}", output_text(&created));
    assert!(output_text(&created).contains("CANDIDATE"));

    let verified = verify(&fixture, fixture.created_ms + 1);
    assert!(verified.status.success(), "{}", output_text(&verified));
    assert!(output_text(&verified).contains("VALID CANDIDATE"));

    let help = run_script("candidate-verify.rhai", &[Path::new("--help")]);
    assert!(help.status.success(), "{}", output_text(&help));
    assert!(output_text(&help).contains("usage: candidate-verify"));
    let help = run_script("candidate-aggregate.rhai", &[Path::new("--help")]);
    assert!(help.status.success(), "{}", output_text(&help));
    assert!(output_text(&help).contains("usage: candidate-aggregate"));
}

#[test]
fn candidate_rejects_archive_tampering_and_extra_payload_files() {
    let fixture = fixture("payload-tamper");
    let created = aggregate(&fixture);
    assert!(created.status.success(), "{}", output_text(&created));

    let archive = fixture
        .assets
        .join(format!("agenterm-{VERSION}-linux-x86_64.tar.gz"));
    fs::write(&archive, "tampered\n").expect("tamper archive");
    let tampered = verify(&fixture, fixture.created_ms + 1);
    assert!(!tampered.status.success());
    assert!(output_text(&tampered).contains("candidate_archive_identity"));

    fs::write(&archive, "fixture archive linux x86_64\n").expect("restore archive");
    fs::write(fixture.assets.join("unexpected.txt"), "unexpected\n")
        .expect("write unexpected payload");
    let extra = verify(&fixture, fixture.created_ms + 1);
    assert!(!extra.status.success());
    assert!(output_text(&extra).contains("candidate_payload_file_set"));
}

#[test]
fn candidate_rejects_expiry_and_workflow_identity_drift() {
    let fixture = fixture("identity-expiry");
    let created = aggregate(&fixture);
    assert!(created.status.success(), "{}", output_text(&created));

    let expired = verify(&fixture, fixture.expires_ms + 1);
    assert!(!expired.status.success());
    assert!(output_text(&expired).contains("candidate_expired"));

    fs::write(
        fixture.root.join(".github/workflows/candidate.yml"),
        "name: changed\n",
    )
    .expect("change workflow");
    let drifted = verify(&fixture, fixture.created_ms + 1);
    assert!(!drifted.status.success());
    assert!(output_text(&drifted).contains("candidate_workflow_hash"));
}

#[test]
fn candidate_rejects_non_stress_or_failed_qualification_receipts() {
    let fixture = fixture("receipt");
    let receipt_path = fixture.assets.join("qualification-receipt.json");
    let mut receipt: Value =
        serde_json::from_slice(&fs::read(&receipt_path).expect("read receipt"))
            .expect("parse receipt");
    receipt["stress_included"] = json!(false);
    receipt["gates"][0]["status"] = json!("failed");
    write_json(&receipt_path, &receipt);

    let rejected = aggregate(&fixture);
    assert!(!rejected.status.success());
    assert!(
        output_text(&rejected).contains("candidate_receipt_profile")
            || output_text(&rejected).contains("candidate_receipt_gate_status")
    );
}

#[test]
fn candidate_rejects_missing_companions_and_duplicate_platforms() {
    {
        let fixture = fixture("set");
        let checksum = fixture
            .assets
            .join(format!("agenterm-{VERSION}-windows-aarch64.zip.sha256"));
        fs::remove_file(&checksum).expect("remove checksum");
        let missing = aggregate(&fixture);
        assert!(!missing.status.success());
        assert!(
            output_text(&missing).contains("candidate_checksum_missing"),
            "{}",
            output_text(&missing)
        );
    }

    let fixture = fixture("duplicate");
    let created = aggregate(&fixture);
    assert!(created.status.success(), "{}", output_text(&created));
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&fixture.manifest).expect("read manifest"))
            .expect("parse manifest");
    let duplicate = manifest["assets"][0].clone();
    manifest["assets"]
        .as_array_mut()
        .expect("asset array")
        .push(duplicate);
    write_json(&fixture.manifest, &manifest);
    let rejected = verify(&fixture, fixture.created_ms + 1);
    assert!(!rejected.status.success());
    assert!(
        output_text(&rejected).contains("candidate_asset_count")
            || output_text(&rejected).contains("candidate_platform_duplicate")
    );
}

#[test]
fn candidate_rejects_provenance_lies_and_missing_unsigned_preview_notice() {
    {
        let fixture = fixture("provenance");
        let provenance_path = fixture.assets.join(format!(
            "agenterm-{VERSION}-macos-aarch64-unsigned-preview.zip.provenance.json"
        ));
        let mut provenance: Value =
            serde_json::from_slice(&fs::read(&provenance_path).expect("read provenance"))
                .expect("parse provenance");
        provenance["source_commit"] = json!("ffffffffffffffffffffffffffffffffffffffff");
        write_json(&provenance_path, &provenance);
        let rejected = aggregate(&fixture);
        assert!(!rejected.status.success());
        assert!(output_text(&rejected).contains("candidate_provenance_source_commit"));
    }

    {
        let fixture = fixture("provenance-cargo-lock");
        let provenance_path = fixture.assets.join(format!(
            "agenterm-{VERSION}-windows-x86_64.zip.provenance.json"
        ));
        let mut provenance: Value =
            serde_json::from_slice(&fs::read(&provenance_path).expect("read provenance"))
                .expect("parse provenance");
        provenance["cargo_lock_sha256"] = json!("f".repeat(64));
        write_json(&provenance_path, &provenance);
        let rejected = aggregate(&fixture);
        assert!(!rejected.status.success());
        assert!(output_text(&rejected).contains("candidate_provenance_cargo_lock"));
    }

    {
        let fixture = fixture("provenance-macos-sbom");
        let provenance_path = fixture.assets.join(format!(
            "agenterm-{VERSION}-macos-x86_64-unsigned-preview.zip.provenance.json"
        ));
        let mut provenance: Value =
            serde_json::from_slice(&fs::read(&provenance_path).expect("read provenance"))
                .expect("parse provenance");
        provenance["sbom_sha256"] = json!("f".repeat(64));
        write_json(&provenance_path, &provenance);
        let rejected = aggregate(&fixture);
        assert!(!rejected.status.success());
        assert!(output_text(&rejected).contains("candidate_provenance_sbom"));
    }

    let fixture = fixture("preview-notice");
    fs::remove_file(fixture.assets.join("MACOS_UNSIGNED_PREVIEW_README.md"))
        .expect("remove preview notice");
    let rejected = aggregate(&fixture);
    assert!(!rejected.status.success());
    assert!(output_text(&rejected).contains("candidate_preview_readme_missing"));
}
