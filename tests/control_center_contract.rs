use std::process::Command;

#[test]
fn informational_commands_are_machine_readable_without_opening_the_shell() {
    let executable = env!("CARGO_BIN_EXE_agenterm-cc");
    let registry = std::env::temp_dir().join(format!(
        "agenterm-cc-informational-test-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&registry);
    let capabilities = Command::new(executable)
        .args(["capabilities", "--json"])
        .env("AGENTERM_CC_REGISTRY_PATH", &registry)
        .output()
        .expect("run capabilities");
    assert!(capabilities.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&capabilities.stdout).expect("capabilities JSON");
    assert_eq!(document["public_ui_action"], "open-control-center");
    assert_eq!(document["typed_operation"], "control-center.open");
    assert_eq!(document["owns_terminal_authority"], false);

    let snapshot = Command::new(executable)
        .args(["snapshot", "--json"])
        .env("AGENTERM_CC_REGISTRY_PATH", &registry)
        .output()
        .expect("run snapshot");
    assert!(snapshot.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&snapshot.stdout).expect("snapshot JSON");
    assert_eq!(document["connected_server"], serde_json::Value::Null);
    assert!(
        document["views"]
            .as_array()
            .expect("view array")
            .iter()
            .all(|view| view["state"] == "unavailable")
    );
    assert!(
        !registry.exists(),
        "informational commands must not claim the process registry"
    );
}

#[test]
fn cli_control_contract_is_strict_and_does_not_autostart_a_server() {
    let cli = env!("CARGO_BIN_EXE_agenterm-cli");
    let registry = std::env::temp_dir().join(format!(
        "agenterm-cc-cli-contract-test-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&registry);

    let status = Command::new(cli)
        .args(["control-center", "status"])
        .env("AGENTERM_CC_REGISTRY_PATH", &registry)
        .output()
        .expect("run status");
    assert!(status.status.success());
    let document: serde_json::Value = serde_json::from_slice(&status.stdout).expect("status JSON");
    assert_eq!(document["state"], "not_running");
    assert_eq!(document["pid"], serde_json::Value::Null);

    let close = Command::new(cli)
        .args(["control-center", "close"])
        .env("AGENTERM_CC_REGISTRY_PATH", &registry)
        .output()
        .expect("close absent control center");
    assert!(close.status.success());
    let document: serde_json::Value = serde_json::from_slice(&close.stdout).expect("close JSON");
    assert_eq!(document["state"], "not_running");
    assert_eq!(document["pid"], serde_json::Value::Null);

    let snapshot = Command::new(cli)
        .args(["--address", "127.0.0.1:9", "control-center", "snapshot"])
        .env("AGENTERM_CC_REGISTRY_PATH", &registry)
        .output()
        .expect("run disconnected snapshot");
    assert!(snapshot.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&snapshot.stdout).expect("snapshot JSON");
    assert_eq!(document["server_state"], "unavailable");
    assert_eq!(document["server_reason"], "server_unreachable");
    assert!(document["server_detail"].is_string());
    assert_eq!(document["connected_server"], serde_json::Value::Null);
    assert_eq!(document["views"][0]["id"], "cockpit");
    assert_eq!(document["views"][0]["state"], "unavailable");
    assert!(!registry.exists());

    let invalid = Command::new(cli)
        .args(["control-center", "snapshot", "--no-activate"])
        .output()
        .expect("run invalid arguments");
    assert_eq!(invalid.status.code(), Some(2));
}
