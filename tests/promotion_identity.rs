use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Fixture {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "agenterm-promotion-identity-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create fixture");
    Fixture(root)
}

fn write_manifest(path: &Path, channel: &str, source: &str) {
    let expected_tag = format!("v{CURRENT_VERSION}");
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                // Mirrors what release_candidate.rhai actually seals: the
                // manifest is identified by product, not by a kind field.
                "product": "AgenTerm",
                "version": CURRENT_VERSION,
                "expected_tag": expected_tag,
                "source_sha": source,
                "run": {"id": 30616209992_u64},
                "assets": [
                    {"os": "macos", "channel": channel},
                    {"os": "macos", "channel": channel},
                    {"os": "windows", "channel": "release"}
                ]
            }))
            .expect("serialize manifest")
        ),
    )
    .expect("write manifest");
}

fn generate(root: &Path, manifest: &Path, source: &str) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
        .current_dir(repo)
        .arg("run")
        .arg(repo.join("scripts/rh/promotion-identity.rh"))
        .args(["--project-root"])
        .arg(repo)
        .args(["--"])
        .arg(manifest)
        .arg("30616209992")
        .arg(source)
        .arg(root.join("identity.json"))
        .output()
        .expect("generate promotion identity")
}

#[test]
fn promotion_identity_binds_candidate_and_exact_unsigned_preview_body() {
    let fixture = fixture();
    let manifest = fixture.0.join("candidate.json");
    write_manifest(&manifest, "macos-unsigned-preview", SOURCE_SHA);
    let output = generate(&fixture.0, &manifest, SOURCE_SHA);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let identity: Value =
        serde_json::from_slice(&fs::read(fixture.0.join("identity.json")).expect("read identity"))
            .expect("parse identity");
    assert_eq!(identity["kind"], "agenterm-promotion-identity");
    assert_eq!(identity["candidate_run_id"], "30616209992");
    assert_eq!(identity["source_sha"], SOURCE_SHA);
    assert_eq!(identity["tag"], format!("v{CURRENT_VERSION}"));
    assert_eq!(identity["mac_channel"], "macos-unsigned-preview");
    assert!(
        identity["marker"]
            .as_str()
            .is_some_and(|marker| marker.contains("manifest_sha256="))
    );
    let body = identity["body"].as_str().expect("body");
    assert!(body.contains("macOS unsigned preview"));
    assert!(body.contains("MACOS_UNSIGNED_PREVIEW_README.md"));
    let body_hash = Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(identity["body_sha256"], body_hash);
}

#[test]
fn promotion_identity_rejects_source_and_channel_drift() {
    let fixture = fixture();
    let manifest = fixture.0.join("candidate.json");
    write_manifest(&manifest, "macos-unsigned-preview", SOURCE_SHA);
    let wrong_source = "f".repeat(40);
    let rejected = generate(&fixture.0, &manifest, &wrong_source);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("promotion_identity_manifest"));

    write_manifest(&manifest, "unexpected", SOURCE_SHA);
    let rejected = generate(&fixture.0, &manifest, SOURCE_SHA);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("promotion_identity_macos_channel"));
}
