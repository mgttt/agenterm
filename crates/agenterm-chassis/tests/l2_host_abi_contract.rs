use std::collections::BTreeSet;

use serde_json::{Value, json};

const ABI: &str = include_str!("../l2/host-abi.json");
const WORKBENCH_OPERATIONS: &str = include_str!("../../../src/operations.rs");
const IPC_TRANSPORT: &str = include_str!("../../../src/platform/ipc_transport_impl.rs");
const CLIPBOARD: &str = include_str!("../../../src/ui_clipboard.rs");
const UI_GEOMETRY: &str = include_str!("../../../src/ui_geometry.rs");

fn abi() -> Value {
    serde_json::from_str(ABI).expect("host ABI JSON")
}

fn accepts(document: &Value, host_version: u64, operation: &str, parameters: &Value) -> bool {
    let compatibility = &document["compatibility"];
    if host_version
        < compatibility["minimum_host_abi"]
            .as_u64()
            .unwrap_or(u64::MAX)
        || host_version > compatibility["maximum_host_abi"].as_u64().unwrap_or(0)
    {
        return false;
    }
    let Some(capability) = document["capabilities"]
        .as_array()
        .and_then(|caps| caps.iter().find(|cap| cap["id"] == operation))
    else {
        return false;
    };
    if capability["host_abi"].as_u64() != Some(host_version) {
        return false;
    }
    let Some(signature_name) = capability["signature"].as_str() else {
        return false;
    };
    let Some(signature) = document["signatures"].get(signature_name) else {
        return false;
    };
    validate_parameters(document, &signature["parameters"], parameters)
}

fn validate_parameters(document: &Value, schema: &Value, parameters: &Value) -> bool {
    let Some(values) = parameters.as_object() else {
        return false;
    };
    let Some(properties) = schema["properties"].as_object() else {
        return false;
    };
    if schema["additional_properties"] != false
        || values.keys().any(|name| !properties.contains_key(name))
    {
        return false;
    }
    let Some(required) = schema["required"].as_array() else {
        return false;
    };
    if required
        .iter()
        .filter_map(Value::as_str)
        .any(|name| !values.contains_key(name))
    {
        return false;
    }
    values.iter().all(|(name, value)| {
        let spec = &properties[name];
        type_matches(&spec["type"], value) && within_bounds(document, spec, value)
    })
}

fn type_matches(expected: &Value, value: &Value) -> bool {
    match expected.as_str() {
        Some("string" | "stable_tab_id") => value.is_string(),
        Some("integer" | "uint32" | "uint64") => {
            value.as_i64().is_some() || value.as_u64().is_some()
        }
        Some("number") => value.is_number(),
        _ => false,
    }
}

fn within_bounds(document: &Value, spec: &Value, value: &Value) -> bool {
    let referenced = spec["bound"]
        .as_str()
        .and_then(|name| document["bounds"].get(name));
    let minimum = referenced
        .and_then(|bound| bound["minimum"].as_i64())
        .or_else(|| spec["minimum"].as_i64());
    let maximum = referenced
        .and_then(|bound| bound["maximum"].as_i64())
        .or_else(|| spec["maximum"].as_i64());
    if let Some(number) = value.as_i64() {
        return minimum.is_none_or(|limit| number >= limit)
            && maximum.is_none_or(|limit| number <= limit);
    }
    if let Some(text) = value.as_str() {
        let bytes = i64::try_from(text.len()).unwrap_or(i64::MAX);
        let string_minimum = spec["minimum_utf8_bytes"].as_i64().or(minimum);
        let string_maximum = spec["maximum_utf8_bytes"].as_i64().or(maximum);
        return string_minimum.is_none_or(|limit| bytes >= limit)
            && string_maximum.is_none_or(|limit| bytes <= limit);
    }
    true
}

#[test]
fn contract_is_discovery_not_authorization_and_has_unique_exact_signatures() {
    let document = abi();
    assert_eq!(document["schema"], 2);
    assert_eq!(document["version"], 3);
    assert_eq!(document["authorization"]["encoded"], false);
    assert_eq!(
        document["capability_semantics"],
        "discovery-and-compatibility-only-not-authorization"
    );
    for policy in [
        "unknown_operation",
        "unknown_signature",
        "unsupported_version",
        "bound_exceeded",
        "unavailable_optional_plugin",
    ] {
        assert_eq!(document["compatibility"][policy], "reject", "{policy}");
    }

    let signatures = document["signatures"].as_object().expect("signatures");
    let capabilities = document["capabilities"].as_array().expect("capabilities");
    let mut ids = BTreeSet::new();
    for capability in capabilities {
        let id = capability["id"].as_str().expect("capability id");
        assert!(ids.insert(id), "duplicate capability {id}");
        assert_eq!(capability["host_abi"], 3, "{id}");
        assert_eq!(capability["availability"], "required", "{id}");
        let signature = capability["signature"].as_str().expect("signature");
        assert!(
            signatures.contains_key(signature),
            "unknown signature {signature}"
        );
    }

    let cu = document["catalog"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["family"] == "computer-use")
        .expect("computer-use catalog entry");
    assert_eq!(cu["availability"], "optional");
    assert_eq!(cu["absence"], "reject");
    assert_eq!(cu["transport"], "native-plugin-cli");
}

#[test]
fn representative_real_workbench_call_shapes_are_accepted() {
    let document = abi();
    for (operation, parameters) in [
        ("protocol.info", json!({})),
        (
            "ui.hello",
            json!({"minimum":1,"maximum":3,"client_id":"chassis"}),
        ),
        ("ui.deltas", json!({"epoch":"e","after":0,"limit":64})),
        ("tabs.set-note", json!({"tab":"@7","note":"ship"})),
        ("pane.capture", json!({"tab":"@7","max_bytes":1048576})),
        ("events.read", json!({"epoch":"e","after":0,"limit":1024})),
        (
            "events.wait",
            json!({"epoch":"e","after":0,"kind":"tab.created","timeout_ms":60000}),
        ),
        ("ui.tabs.set-width", json!({"width":240})),
        (
            "ui.input.pointer",
            json!({"x":12.0,"y":18.0,"action":"move"}),
        ),
        ("ui.input.wheel", json!({"x":12.0,"y":18.0,"delta_y":-1.0})),
        ("terminal.paste", json!({})),
    ] {
        assert!(accepts(&document, 3, operation, &parameters), "{operation}");
    }
}

#[test]
fn unknown_capability_version_signature_and_fields_fail_closed() {
    let document = abi();
    assert!(!accepts(&document, 3, "host.shell", &json!({})));
    assert!(!accepts(&document, 2, "protocol.info", &json!({})));
    assert!(!accepts(&document, 4, "protocol.info", &json!({})));
    assert!(!accepts(
        &document,
        3,
        "terminal.paste",
        &json!({"text":"stale-wrapper-shape"})
    ));

    let mut bad_signature = document.clone();
    let capability = bad_signature["capabilities"].as_array_mut().unwrap()[0]
        .as_object_mut()
        .unwrap();
    capability.insert("signature".into(), json!("missing"));
    assert!(!accepts(&bad_signature, 3, "protocol.info", &json!({})));
}

#[test]
fn all_declared_numeric_and_utf8_bounds_fail_closed() {
    let document = abi();
    for (operation, parameters) in [
        ("ui.deltas", json!({"epoch":"e","after":0,"limit":65})),
        ("events.read", json!({"epoch":"e","after":0,"limit":1025})),
        (
            "events.wait",
            json!({"epoch":"e","after":0,"kind":"k","timeout_ms":60001}),
        ),
        ("pane.capture", json!({"tab":"@7","max_bytes":0})),
        ("pane.capture", json!({"tab":"@7","max_bytes":1048577})),
        ("tabs.set-note", json!({"tab":"@7","note":"x".repeat(4097)})),
        ("ui.tabs.set-width", json!({"width":179})),
        ("ui.tabs.set-width", json!({"width":481})),
    ] {
        assert!(
            !accepts(&document, 3, operation, &parameters),
            "{operation}"
        );
    }
}

#[test]
fn catalog_bounds_track_the_authoritative_workbench_constants() {
    let document = abi();
    assert!(WORKBENCH_OPERATIONS.contains("maximum: Some(60_000)"));
    assert!(WORKBENCH_OPERATIONS.contains("maximum: Some(1024 * 1024)"));
    assert!(WORKBENCH_OPERATIONS.contains("pub const TAB_NOTE_MAX_BYTES: usize = 4096"));
    assert!(CLIPBOARD.contains("TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024"));
    assert!(IPC_TRANSPORT.contains("IPC_RESPONSE_MAX_BYTES: u64 = 8 * 1024 * 1024"));
    assert!(UI_GEOMETRY.contains("TABS_MIN_WIDTH: i32 = 180"));
    assert!(UI_GEOMETRY.contains("TABS_MAX_WIDTH: i32 = 480"));
    assert_eq!(document["bounds"]["event_wait_ms"]["maximum"], 60000);
    assert_eq!(document["bounds"]["capture_bytes"]["maximum"], 1048576);
    assert_eq!(
        document["bounds"]["terminal_paste_utf8_bytes"]["maximum"],
        262144
    );
    assert_eq!(document["bounds"]["tab_note_utf8_bytes"]["maximum"], 4096);
    assert_eq!(document["bounds"]["tabs_width_pixels"]["minimum"], 180);
    assert_eq!(document["bounds"]["tabs_width_pixels"]["maximum"], 480);
    assert_eq!(document["wire"]["response"]["maximum_utf8_bytes"], 8388608);
}
