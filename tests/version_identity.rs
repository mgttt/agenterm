use std::{fs, path::Path};

fn package_version(path: impl AsRef<Path>) -> String {
    let source = fs::read_to_string(path.as_ref()).expect("read Cargo manifest");
    let mut in_package = false;
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(value) = line
                .strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
            {
                return value.to_owned();
            }
        }
    }
    panic!("missing [package] version in {}", path.as_ref().display());
}

fn locked_package_version(source: &str, package: &str) -> String {
    for block in source.split("[[package]]").skip(1) {
        let mut name = None;
        let mut version = None;
        for line in block.lines() {
            let line = line.trim();
            if let Some(value) = line
                .strip_prefix("name = \"")
                .and_then(|value| value.strip_suffix('"'))
            {
                name = Some(value);
            }
            if let Some(value) = line
                .strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
            {
                version = Some(value);
            }
        }
        if name == Some(package) {
            return version
                .unwrap_or_else(|| panic!("locked package {package} has no version"))
                .to_owned();
        }
    }
    panic!("Cargo.lock has no package named {package}");
}

#[test]
fn workspace_product_versions_have_one_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let product = package_version(root.join("Cargo.toml"));
    let platform = package_version(root.join("crates/agenterm-platform/Cargo.toml"));
    assert_eq!(platform, product, "agenterm-platform version drift");

    let tasks: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("agenterm.tasks.json")).expect("read task manifest"),
    )
    .expect("decode task manifest");
    assert_eq!(
        tasks
            .pointer("/project/version")
            .and_then(|value| value.as_str()),
        Some(product.as_str()),
        "task project version drift"
    );
    let revision = tasks
        .pointer("/project/provenance/revision")
        .and_then(|value| value.as_str())
        .expect("task project provenance revision");
    assert!(
        revision.starts_with(&format!("v{product}-")) && revision.ends_with("-unpublished"),
        "task project provenance {revision:?} does not bind version {product}"
    );

    let lock = fs::read_to_string(root.join("Cargo.lock")).expect("read Cargo.lock");
    assert_eq!(
        locked_package_version(&lock, "agenterm"),
        product,
        "root lockfile version drift"
    );
    assert_eq!(
        locked_package_version(&lock, "agenterm-platform"),
        product,
        "platform lockfile version drift"
    );
}
