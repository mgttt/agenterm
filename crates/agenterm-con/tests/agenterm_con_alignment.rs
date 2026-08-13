use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if manifest
        .join("crates/agenterm-con/alignment-contract.json")
        .is_file()
    {
        manifest.to_owned()
    } else {
        manifest.join("../..")
    }
}

fn load_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn required_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be a string in {value}"))
}

fn required_array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} must be an array in {value}"))
}

fn assert_exact_keys(value: &Value, expected: &[&str], context: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{context} must be an object"));
    let actual: BTreeSet<_> = object.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    assert_eq!(actual, expected, "unknown or missing fields in {context}");
}

#[test]
fn machine_contract_matches_public_cli_and_registered_journeys() {
    let root = repo_root();
    let package = root.join("crates/agenterm-con");
    let contract = load_json(&package.join("alignment-contract.json"));
    let registry = load_json(&package.join("evidence-registry.json"));

    assert_exact_keys(
        &contract,
        &[
            "schema_version",
            "product",
            "public_commands",
            "capabilities",
        ],
        "contract",
    );
    assert_exact_keys(
        &registry,
        &["schema_version", "product", "evidence"],
        "registry",
    );

    for document in [&contract, &registry] {
        assert_eq!(document["schema_version"], 1, "unsupported schema");
        assert_eq!(document["product"], "agenterm-con", "wrong product");
    }

    let mut registered = BTreeMap::new();
    let manifest = fs::read_to_string(package.join("Cargo.toml")).expect("read con manifest");
    for item in required_array(&registry, "evidence") {
        assert_exact_keys(
            item,
            &[
                "id",
                "kind",
                "emitter",
                "platform",
                "test_target",
                "test_name",
                "source",
            ],
            "evidence entry",
        );
        let id = required_str(item, "id");
        let target = required_str(item, "test_target");
        let test_name = required_str(item, "test_name");
        let source = required_str(item, "source");
        let package_source = source
            .strip_prefix("crates/agenterm-con/")
            .unwrap_or_else(|| panic!("con evidence source must be package-owned: {source}"));
        assert_eq!(required_str(item, "kind"), "public-black-box");
        assert_eq!(required_str(item, "emitter"), "cargo-test-harness");
        assert_eq!(required_str(item, "platform"), "windows-x86_64");
        assert_eq!(id, format!("{target}::{test_name}"));
        assert!(
            registered.insert(id, item).is_none(),
            "duplicate evidence {id}"
        );

        let source_path = root.join(source);
        let source_text = fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", source_path.display()));
        assert!(
            source_text.contains(&format!("fn {test_name}()")),
            "registered test {id} is absent from {source}"
        );
        assert!(
            manifest.contains(&format!("name = \"{target}\""))
                && manifest.contains(&format!("path = \"{package_source}\"")),
            "registered target {target} and source {source} are absent from Cargo.toml"
        );
    }

    let mut capability_ids = BTreeSet::new();
    let mut referenced_evidence = BTreeSet::new();
    let mut contracted_commands = BTreeSet::new();
    let declared_commands: BTreeSet<_> = required_array(&contract, "public_commands")
        .iter()
        .map(|command| command.as_str().expect("public command must be a string"))
        .collect();
    assert_eq!(
        declared_commands.len(),
        required_array(&contract, "public_commands").len(),
        "duplicate public command"
    );
    let control_prd = fs::read_to_string(root.join("prd/PRD_02_26_con_control_cli.md"))
        .expect("read con control PRD");
    for capability in required_array(&contract, "capabilities") {
        assert_exact_keys(
            capability,
            &[
                "id",
                "kind",
                "status",
                "evidence_mode",
                "prd",
                "prd_anchor",
                "evidence",
                "commands",
            ],
            "capability",
        );
        let id = required_str(capability, "id");
        assert!(capability_ids.insert(id), "duplicate capability {id}");
        assert!(
            id.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || b".-".contains(&byte)),
            "invalid capability id {id}"
        );
        assert!(matches!(
            required_str(capability, "kind"),
            "behavior" | "visual" | "architecture"
        ));
        assert_eq!(required_str(capability, "status"), "shipped");
        assert_eq!(required_str(capability, "evidence_mode"), "black-box");

        let prd = required_str(capability, "prd");
        assert!(prd.starts_with("prd/PRD_02_"), "invalid PRD owner {prd}");
        let prd_text = fs::read_to_string(root.join(prd))
            .unwrap_or_else(|error| panic!("read PRD owner {prd}: {error}"));
        let anchor = required_str(capability, "prd_anchor");
        assert!(
            anchor.starts_with("- [x] "),
            "non-shipped PRD anchor for {id}"
        );
        assert!(
            prd_text.contains(anchor),
            "missing exact PRD anchor for {id}"
        );

        let evidence = required_array(capability, "evidence");
        assert!(!evidence.is_empty(), "capability {id} has no evidence");
        for evidence_id in evidence {
            let evidence_id = evidence_id
                .as_str()
                .unwrap_or_else(|| panic!("non-string evidence for {id}"));
            assert!(
                registered.contains_key(evidence_id),
                "capability {id} cites unregistered evidence {evidence_id}"
            );
            referenced_evidence.insert(evidence_id);
        }

        for command in required_array(capability, "commands") {
            let command = command
                .as_str()
                .unwrap_or_else(|| panic!("non-string command for {id}"));
            assert!(
                contracted_commands.insert(command),
                "command {command} has multiple capability owners"
            );
            assert!(
                control_prd.contains(&format!("`{command}`")),
                "public command {command} is absent from the control PRD"
            );
        }
    }
    assert!(!capability_ids.is_empty(), "contract has no capabilities");
    assert_eq!(
        referenced_evidence,
        registered.keys().copied().collect(),
        "evidence registry contains an orphan or the contract missed an entry"
    );
    assert_eq!(
        declared_commands, contracted_commands,
        "public command ownership drifted"
    );

    let contract_text = contract.to_string();
    let registry_text = registry.to_string();
    for forbidden in [
        "prd/alignment-contract.json",
        "agenterm cli",
        "agenterm.tasks.json",
    ] {
        assert!(
            !contract_text.contains(forbidden),
            "contract borrowed {forbidden}"
        );
        assert!(
            !registry_text.contains(forbidden),
            "registry borrowed {forbidden}"
        );
    }

    let Some(executable) = option_env!("CARGO_BIN_EXE_agenterm-con") else {
        return;
    };
    let output = Command::new(executable)
        .args(["cli", "list-commands"])
        .output()
        .expect("launch agenterm-con cli list-commands");
    assert!(
        output.status.success(),
        "list-commands failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("command catalog is UTF-8");
    let public_commands: BTreeSet<_> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        public_commands, contracted_commands,
        "machine contract and running agenterm-con CLI catalog diverged"
    );
}
