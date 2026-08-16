//! Chassis-L2 Host ABI v3 dispatch/eval boundary tests.

#[allow(dead_code)]
#[path = "../src/bytecode.rs"]
mod bytecode;
#[path = "../src/l2_dispatch.rs"]
mod l2_dispatch;
#[path = "../src/vm.rs"]
mod vm;

use bytecode::{IrOp, L2Source, assemble};
use l2_dispatch::{Dispatcher, HostCallback};
use serde_json::{Value, json};
use vm::{DEFAULT_MAX_STEPS, run};

const HOST_ABI: &str = include_str!("../l2/host-abi.json");

#[derive(Default)]
struct RecordingHost {
    calls: Vec<(String, Value)>,
    result: Value,
    failure: Option<String>,
}

impl HostCallback for RecordingHost {
    fn call(&mut self, capability: &str, parameters: &Value) -> Result<Value, String> {
        self.calls
            .push((capability.to_string(), parameters.clone()));
        if let Some(reason) = &self.failure {
            return Err(reason.clone());
        }
        Ok(self.result.clone())
    }
}

fn dispatcher(declared: &[&str], result: Value) -> Dispatcher<RecordingHost> {
    Dispatcher::from_host_abi_json(
        HOST_ABI,
        &declared
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>(),
        RecordingHost {
            result,
            ..RecordingHost::default()
        },
    )
    .expect("dispatcher")
}

#[test]
fn final_host_abi_v3_resolves_facade_to_one_canonical_callback() {
    let mut dispatch = dispatcher(&["tabs.active"], json!(73));
    assert_eq!(
        dispatch
            .dispatch("fleet.tabs.active", &json!({}))
            .expect("facade dispatch"),
        json!(73)
    );
    assert_eq!(
        dispatch.into_host().calls,
        [("tabs.active".to_string(), json!({}))]
    );
}

#[test]
fn unknown_and_undeclared_calls_fail_before_callback() {
    let mut dispatch = dispatcher(&["tabs.active"], Value::Null);
    assert!(
        dispatch
            .dispatch("not.a.capability", &json!({}))
            .expect_err("unknown")
            .to_string()
            .contains("unknown L2 capability")
    );
    assert!(
        dispatch
            .dispatch("tabs.list", &json!({}))
            .expect_err("undeclared")
            .to_string()
            .contains("did not declare")
    );
    assert!(dispatch.into_host().calls.is_empty());
}

#[test]
fn signature_rejects_missing_and_additional_properties_before_callback() {
    let mut dispatch = dispatcher(&["tabs.set-note"], Value::Null);
    for parameters in [
        json!({"tab":"t-1"}),
        json!({"tab":"t-1","note":"ok","extra":1}),
    ] {
        assert!(dispatch.dispatch("tabs.set-note", &parameters).is_err());
    }
    assert!(dispatch.into_host().calls.is_empty());
}

#[test]
fn named_numeric_bound_is_enforced_before_callback() {
    let mut dispatch = dispatcher(&["ui.tabs.set-width"], json!({"ok":true}));
    for width in [179, 481] {
        let error = dispatch
            .dispatch("ui.tabs.set-width", &json!({"width":width}))
            .expect_err("width bound");
        assert!(error.to_string().contains("180..=480"));
    }
    assert!(dispatch.into_host().calls.is_empty());
}

#[test]
fn bounded_signature_calls_host_exactly_once_when_valid() {
    let mut dispatch = dispatcher(&["tabs.set-note"], json!({"tab":"t-1"}));
    let note = "x".repeat(4096);
    dispatch
        .dispatch("tabs.set-note", &json!({"tab":"t-1","note":note}))
        .expect("maximum valid note");
    assert_eq!(dispatch.into_host().calls.len(), 1);
}

#[test]
fn bounded_string_rejects_oversize_before_callback() {
    let mut dispatch = dispatcher(&["tabs.set-note"], Value::Null);
    let note = "x".repeat(4097);
    assert!(
        dispatch
            .dispatch("tabs.set-note", &json!({"tab":"t-1","note":note}))
            .expect_err("note bound")
            .to_string()
            .contains("UTF-8 size")
    );
    assert!(dispatch.into_host().calls.is_empty());
}

#[test]
fn response_byte_bound_fails_after_exactly_one_callback() {
    let abi = HOST_ABI.replace("8388608", "8");
    let mut dispatch = Dispatcher::from_host_abi_json(
        &abi,
        &["tabs.active".to_string()],
        RecordingHost {
            result: json!("12345678"),
            ..RecordingHost::default()
        },
    )
    .expect("dispatcher");
    assert!(
        dispatch
            .dispatch("tabs.active", &json!({}))
            .expect_err("response bound")
            .to_string()
            .contains("UTF-8 bytes")
    );
    assert_eq!(dispatch.into_host().calls.len(), 1);
}

#[test]
fn host_failure_is_returned_after_exactly_one_callback() {
    let mut dispatch = Dispatcher::from_host_abi_json(
        HOST_ABI,
        &["tabs.active".to_string()],
        RecordingHost {
            failure: Some("offline".to_string()),
            ..RecordingHost::default()
        },
    )
    .expect("dispatcher");
    assert!(
        dispatch
            .dispatch("tabs.active", &json!({}))
            .expect_err("host failure")
            .to_string()
            .contains("offline")
    );
    assert_eq!(dispatch.into_host().calls.len(), 1);
}

#[test]
fn bytecode_eval_calls_empty_signature_once_and_requires_i64() {
    let source = L2Source {
        caps: vec!["tabs.active".to_string()],
        ops: vec![IrOp::Call("tabs.active".to_string()), IrOp::Halt],
    };
    let program = assemble(&source, Some(source.caps.as_slice())).expect("assemble");
    let mut dispatch = dispatcher(&["tabs.active"], json!(91));
    assert_eq!(
        run(&program, &mut dispatch, DEFAULT_MAX_STEPS).expect("eval"),
        91
    );
    assert_eq!(dispatch.into_host().calls.len(), 1);

    let mut dispatch = dispatcher(&["tabs.active"], json!({"not":"scalar"}));
    assert!(run(&program, &mut dispatch, DEFAULT_MAX_STEPS).is_err());
    assert_eq!(dispatch.into_host().calls.len(), 1);
}

#[test]
fn incompatible_or_internally_inconsistent_abi_fails_closed() {
    let old_schema = HOST_ABI.replacen("\"schema\": 2", "\"schema\": 1", 1);
    assert!(
        Dispatcher::from_host_abi_json(&old_schema, &[], RecordingHost::default())
            .err()
            .expect("old schema")
            .to_string()
            .contains("unsupported schema")
    );

    let bad_signature =
        HOST_ABI.replacen("\"signature\": \"empty\"", "\"signature\": \"missing\"", 1);
    assert!(
        Dispatcher::from_host_abi_json(&bad_signature, &[], RecordingHost::default())
            .err()
            .expect("unknown signature")
            .to_string()
            .contains("unknown signature")
    );
}
