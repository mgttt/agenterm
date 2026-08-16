use std::fs;
use std::path::Path;

use agenterm_chassis::bytecode::{L2Source, assemble};
use agenterm_chassis::vm::{CapHost, DEFAULT_MAX_STEPS, run};
use agenterm_chassis::{CELLS, compose};

const ABI: &str = include_str!("../l2/host-abi.json");
const ACTIVE_TAB: &str = include_str!("../l2/programs/active-tab.json");

struct ActiveTabHost {
    calls: Vec<String>,
    active_tab: i64,
}

impl CapHost for ActiveTabHost {
    fn call(&mut self, cap: &str) -> Result<i64, String> {
        self.calls.push(cap.to_string());
        match cap {
            "tabs.active" => Ok(self.active_tab),
            other => Err(format!("unexpected capability `{other}`")),
        }
    }
}

#[test]
fn catalog_classifies_product_surfaces_as_l2_discovery() {
    let abi: serde_json::Value = serde_json::from_str(ABI).expect("host ABI JSON");
    assert_eq!(abi["layer"], "chassis-l2");
    assert_eq!(
        abi["capability_semantics"],
        "discovery-and-compatibility-only-not-authorization"
    );

    let catalog = abi["catalog"].as_array().expect("catalog array");
    for family in [
        "fleet.*",
        "tab",
        "clipboard",
        "control-center",
        "computer-use",
    ] {
        let entry = catalog
            .iter()
            .find(|entry| entry["family"] == family)
            .unwrap_or_else(|| panic!("missing {family} classification"));
        assert_eq!(entry["layer"], "chassis-l2", "{family}");
        assert_eq!(entry["l1"], false, "{family}");
    }

    let cu = catalog
        .iter()
        .find(|entry| entry["family"] == "computer-use")
        .expect("computer-use classification");
    assert_eq!(cu["delivery"], "rare-native-plugin");
    assert_eq!(cu["plugin"], "cu");
    assert_eq!(cu["availability"], "optional");

    let capabilities = abi["capabilities"].as_array().expect("capabilities array");
    assert!(capabilities.iter().all(|cap| cap["layer"] == "chassis-l2"));
    assert!(capabilities.iter().any(|cap| {
        cap["family"] == "fleet.*"
            && cap["facade"]
                .as_str()
                .is_some_and(|name| name.starts_with("fleet."))
    }));
    for family in ["tab", "clipboard", "control-center"] {
        assert!(
            capabilities.iter().any(|cap| cap["family"] == family),
            "missing concrete {family} capability"
        );
    }
}

#[test]
fn active_tab_artifact_is_deterministic_and_calls_the_host_abi() {
    let metadata: serde_json::Value = serde_json::from_str(ACTIVE_TAB).expect("artifact JSON");
    let source: L2Source = serde_json::from_str(ACTIVE_TAB).expect("L2 source");
    let abi: serde_json::Value = serde_json::from_str(ABI).expect("host ABI JSON");
    let allowed = abi["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .map(|cap| cap["id"].as_str().expect("capability id").to_string())
        .collect::<Vec<_>>();

    let program = assemble(&source, Some(allowed.as_slice())).expect("assemble artifact");
    assert_eq!(hex(&program.code), metadata["expected_bytecode_hex"]);

    let mut host = ActiveTabHost {
        calls: Vec::new(),
        active_tab: 73,
    };
    assert_eq!(
        run(&program, &mut host, DEFAULT_MAX_STEPS).expect("run artifact"),
        73
    );
    assert_eq!(host.calls, ["tabs.active"]);
}

#[test]
fn compose_copies_replaceable_l2_artifact_without_compile() {
    let root = std::env::temp_dir().join(format!(
        "chassis-l2-catalog-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    let from = root.join("from");
    let out = root.join("out");
    write_layout(&from);

    let manifest = compose(&from, &out).expect("compose");
    assert!(!manifest.compile);
    assert!(!manifest.invokes_cargo);
    assert_eq!(
        fs::read(from.join("l2/programs/active-tab.json")).expect("source artifact"),
        fs::read(out.join("l2/programs/active-tab.json")).expect("composed artifact")
    );
    let _ = fs::remove_dir_all(&root);
}

fn write_layout(root: &Path) {
    for cell in CELLS {
        let dir = root.join("l1").join(cell);
        fs::create_dir_all(&dir).expect("L1 cell");
        fs::write(dir.join("loader"), format!("frozen-{cell}")).expect("loader");
    }
    fs::create_dir_all(root.join("l2/programs")).expect("L2 programs");
    fs::write(root.join("l2/host-abi.json"), ABI).expect("host ABI");
    fs::write(root.join("l2/programs/active-tab.json"), ACTIVE_TAB).expect("artifact");
    fs::create_dir_all(root.join("l3")).expect("L3");
    fs::write(
        root.join("l3/app.json"),
        r#"{"schema":1,"name":"catalog-test","capabilities":["tabs.active"]}"#,
    )
    .expect("L3 app");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
