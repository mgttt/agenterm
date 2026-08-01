use std::sync::Arc;

use rhai::{Array, Dynamic, Engine, EvalAltResult};
use serde_json::{Value, json};

use crate::operations::{OPERATION_CATALOG, operation_by_id};

pub type FleetBrokerCall =
    Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync + 'static>;

#[derive(Clone)]
struct FleetContext {
    call: FleetBrokerCall,
}

impl FleetContext {
    fn invoke(&self, operation_id: &str, parameters: Value) -> Result<Value, Box<EvalAltResult>> {
        let operation = operation_by_id(operation_id)
            .ok_or_else(|| Box::<EvalAltResult>::from("fleet_operation_unknown"))?;
        if !operation.available {
            return Err(format!("fleet_operation_degraded: {operation_id}").into());
        }
        (self.call)(
            "fleet.call",
            json!({
                "operation_id": operation_id,
                "parameters": parameters,
            }),
        )
        .map_err(Into::into)
    }

    fn query_dynamic(
        &self,
        operation_id: &str,
        parameters: Value,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        let value = self.invoke(operation_id, parameters)?;
        rhai::serde::to_dynamic(value).map_err(|_| "fleet_result_decode".into())
    }

    fn mutate(
        &self,
        operation_id: &str,
        parameters: Value,
    ) -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
        let value = self.invoke(operation_id, parameters)?;
        ScriptFleetReceipt::from_value(value)
    }
}

#[derive(Clone)]
pub struct ScriptFleet(FleetContext);

#[derive(Clone)]
struct FleetProtocol(FleetContext);

#[derive(Clone)]
struct FleetWorkspace(FleetContext);

#[derive(Clone)]
struct FleetTabs(FleetContext);

#[derive(Clone)]
struct FleetUi(FleetContext);

#[derive(Clone)]
struct FleetUiTabs(FleetContext);

#[derive(Clone)]
struct FleetUiWindow(FleetContext);

#[derive(Clone)]
struct FleetTerminalService(FleetContext);

#[derive(Clone)]
struct FleetEvents(FleetContext);

#[derive(Clone)]
struct FleetServer(FleetContext);

#[derive(Clone)]
struct FleetTerminal {
    context: FleetContext,
    tab_id: String,
}

#[derive(Clone)]
pub struct ScriptFleetReceipt {
    receipt: Value,
    events: Vec<ScriptFleetEvent>,
    post_state: ScriptFleetPostState,
}

#[derive(Clone)]
pub struct ScriptFleetEvent(Value);

#[derive(Clone)]
pub struct ScriptFleetPostState {
    verified: bool,
    reason: String,
    value: Value,
}

pub fn bind(call: FleetBrokerCall) -> ScriptFleet {
    ScriptFleet(FleetContext { call })
}

pub fn register(engine: &mut Engine) {
    engine.register_type_with_name::<ScriptFleet>("Fleet");
    engine.register_type_with_name::<FleetProtocol>("FleetProtocol");
    engine.register_type_with_name::<FleetWorkspace>("FleetWorkspace");
    engine.register_type_with_name::<FleetTabs>("FleetTabs");
    engine.register_type_with_name::<FleetUi>("FleetUi");
    engine.register_type_with_name::<FleetUiTabs>("FleetUiTabs");
    engine.register_type_with_name::<FleetUiWindow>("FleetUiWindow");
    engine.register_type_with_name::<FleetTerminalService>("FleetTerminalService");
    engine.register_type_with_name::<FleetEvents>("FleetEvents");
    engine.register_type_with_name::<FleetServer>("FleetServer");
    engine.register_type_with_name::<FleetTerminal>("FleetTerminal");
    engine.register_type_with_name::<ScriptFleetReceipt>("Receipt");
    engine.register_type_with_name::<ScriptFleetEvent>("Event");
    engine.register_type_with_name::<ScriptFleetPostState>("PostState");

    engine.register_get("protocol", |fleet: &mut ScriptFleet| {
        FleetProtocol(fleet.0.clone())
    });
    engine.register_get("workspace", |fleet: &mut ScriptFleet| {
        FleetWorkspace(fleet.0.clone())
    });
    engine.register_get("tabs", |fleet: &mut ScriptFleet| FleetTabs(fleet.0.clone()));
    engine.register_get("ui", |fleet: &mut ScriptFleet| FleetUi(fleet.0.clone()));
    engine.register_get("terminal", |fleet: &mut ScriptFleet| {
        FleetTerminalService(fleet.0.clone())
    });
    engine.register_get("events", |fleet: &mut ScriptFleet| {
        FleetEvents(fleet.0.clone())
    });
    engine.register_get("server", |fleet: &mut ScriptFleet| {
        FleetServer(fleet.0.clone())
    });
    engine.register_fn(
        "terminal",
        |fleet: &mut ScriptFleet, tab_id: &str| -> Result<FleetTerminal, Box<EvalAltResult>> {
            validate_tab_id(tab_id)?;
            Ok(FleetTerminal {
                context: fleet.0.clone(),
                tab_id: tab_id.to_owned(),
            })
        },
    );
    engine.register_fn("operations", fleet_operations);

    engine.register_fn(
        "info",
        |service: &mut FleetProtocol| -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic("protocol.info", json!({}))
        },
    );
    engine.register_fn(
        "info",
        |service: &mut FleetWorkspace| -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic("workspace.info", json!({}))
        },
    );
    engine.register_fn(
        "shutdown",
        |service: &mut FleetWorkspace| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("workspace.shutdown", json!({}))
        },
    );
    engine.register_fn(
        "list",
        |service: &mut FleetTabs| -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic("tabs.list", json!({}))
        },
    );
    engine.register_fn(
        "active",
        |service: &mut FleetTabs| -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic("tabs.active", json!({}))
        },
    );
    engine.register_fn(
        "set_note",
        |service: &mut FleetTabs,
         tab_id: &str,
         note: &str|
         -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            validate_tab_id(tab_id)?;
            service.0.mutate(
                "tabs.set-note",
                json!({
                    "tab": tab_id,
                    "note": note,
                }),
            )
        },
    );
    engine.register_fn(
        "snapshot",
        |service: &mut FleetUi| -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic("ui.snapshot", json!({}))
        },
    );
    engine.register_get("tabs", |service: &mut FleetUi| {
        FleetUiTabs(service.0.clone())
    });
    engine.register_get("window", |service: &mut FleetUi| {
        FleetUiWindow(service.0.clone())
    });
    engine.register_fn(
        "activate",
        |service: &mut FleetUiWindow| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("ui.window.activate", json!({}))
        },
    );
    engine.register_fn(
        "show",
        |service: &mut FleetUiTabs| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("ui.tabs.show", json!({}))
        },
    );
    engine.register_fn(
        "hide",
        |service: &mut FleetUiTabs| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("ui.tabs.hide", json!({}))
        },
    );
    engine.register_fn(
        "toggle",
        |service: &mut FleetUiTabs| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("ui.tabs.toggle", json!({}))
        },
    );
    engine.register_fn(
        "set_width",
        |service: &mut FleetUiTabs,
         width: rhai::INT|
         -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate(
                "ui.tabs.set-width",
                json!({
                    "width": width,
                }),
            )
        },
    );
    engine.register_fn(
        "capture",
        |terminal: &mut FleetTerminal,
         max_bytes: rhai::INT|
         -> Result<Dynamic, Box<EvalAltResult>> {
            terminal.context.query_dynamic(
                "pane.capture",
                json!({
                    "tab": terminal.tab_id,
                    "max_bytes": max_bytes,
                }),
            )
        },
    );
    engine.register_fn(
        "paste",
        |service: &mut FleetTerminalService| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("terminal.paste", json!({}))
        },
    );
    engine.register_fn(
        "read",
        |service: &mut FleetEvents,
         epoch: &str,
         after: rhai::INT|
         -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic(
                "events.read",
                json!({
                    "epoch": epoch,
                    "after": after,
                    "limit": 64,
                }),
            )
        },
    );
    engine.register_fn(
        "read",
        |service: &mut FleetEvents,
         epoch: &str,
         after: rhai::INT,
         limit: rhai::INT|
         -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic(
                "events.read",
                json!({
                    "epoch": epoch,
                    "after": after,
                    "limit": limit,
                }),
            )
        },
    );
    engine.register_fn(
        "wait",
        |service: &mut FleetEvents,
         epoch: &str,
         after: rhai::INT,
         kind: &str,
         timeout_ms: rhai::INT|
         -> Result<Dynamic, Box<EvalAltResult>> {
            service.0.query_dynamic(
                "events.wait",
                json!({
                    "epoch": epoch,
                    "after": after,
                    "kind": kind,
                    "timeout_ms": timeout_ms,
                }),
            )
        },
    );
    engine.register_fn(
        "wait",
        |service: &mut FleetEvents,
         epoch: &str,
         after: rhai::INT,
         kind: &str,
         tab: &str,
         timeout_ms: rhai::INT|
         -> Result<Dynamic, Box<EvalAltResult>> {
            validate_tab_id(tab)?;
            service.0.query_dynamic(
                "events.wait",
                json!({
                    "epoch": epoch,
                    "after": after,
                    "kind": kind,
                    "tab": tab,
                    "timeout_ms": timeout_ms,
                }),
            )
        },
    );
    engine.register_fn(
        "kill",
        |service: &mut FleetServer| -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("server.kill", json!({}))
        },
    );
    engine.register_fn(
        "kill",
        |service: &mut FleetServer,
         target: &str|
         -> Result<ScriptFleetReceipt, Box<EvalAltResult>> {
            service.0.mutate("server.kill", json!({"target": target}))
        },
    );

    register_receipt(engine);
}

fn register_receipt(engine: &mut Engine) {
    engine.register_get("schema_version", |receipt: &mut ScriptFleetReceipt| {
        json_i64(&receipt.receipt, "schema_version")
    });
    engine.register_get("request_id", |receipt: &mut ScriptFleetReceipt| {
        json_string(&receipt.receipt, "request_id")
    });
    engine.register_get("operation_id", |receipt: &mut ScriptFleetReceipt| {
        json_string(&receipt.receipt, "operation_id")
    });
    engine.register_get("outcome", |receipt: &mut ScriptFleetReceipt| {
        json_string(&receipt.receipt, "outcome")
    });
    for field in [
        "resolved",
        "before_position",
        "after_position",
        "result",
        "error",
        "wait",
    ] {
        engine.register_get(field, move |receipt: &mut ScriptFleetReceipt| {
            json_field_dynamic(&receipt.receipt, field)
        });
    }
    engine.register_get("events", |receipt: &mut ScriptFleetReceipt| {
        receipt
            .events
            .iter()
            .cloned()
            .map(Dynamic::from)
            .collect::<Array>()
    });
    engine.register_get("post_state", |receipt: &mut ScriptFleetReceipt| {
        receipt.post_state.clone()
    });

    engine.register_get("schema_version", |event: &mut ScriptFleetEvent| {
        json_i64(&event.0, "schema_version")
    });
    engine.register_get("epoch", |event: &mut ScriptFleetEvent| {
        json_string(&event.0, "epoch")
    });
    engine.register_get("sequence", |event: &mut ScriptFleetEvent| {
        json_i64(&event.0, "sequence")
    });
    engine.register_get("kind", |event: &mut ScriptFleetEvent| {
        json_string(&event.0, "kind")
    });
    engine.register_get("tab_id", |event: &mut ScriptFleetEvent| {
        json_field_dynamic(&event.0, "tab_id")
    });
    engine.register_get("request_id", |event: &mut ScriptFleetEvent| {
        json_string(&event.0, "request_id")
    });
    engine.register_get("operation_id", |event: &mut ScriptFleetEvent| {
        json_string(&event.0, "operation_id")
    });
    engine.register_get("payload", |event: &mut ScriptFleetEvent| {
        json_field_dynamic(&event.0, "payload")
    });

    engine.register_get("verified", |state: &mut ScriptFleetPostState| {
        state.verified
    });
    engine.register_get("reason", |state: &mut ScriptFleetPostState| {
        state.reason.clone()
    });
    engine.register_get("value", |state: &mut ScriptFleetPostState| {
        rhai::serde::to_dynamic(state.value.clone()).unwrap_or(Dynamic::UNIT)
    });
}

impl ScriptFleetReceipt {
    fn from_value(mut value: Value) -> Result<Self, Box<EvalAltResult>> {
        let receipt = value
            .get_mut("receipt")
            .map(Value::take)
            .filter(Value::is_object)
            .ok_or_else(|| Box::<EvalAltResult>::from("fleet_receipt_invalid"))?;
        let events = value
            .get_mut("events")
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
            .unwrap_or_default()
            .into_iter()
            .map(ScriptFleetEvent)
            .collect();
        let post_state = value
            .get_mut("post_state")
            .map(Value::take)
            .unwrap_or(Value::Null);
        let verified = post_state
            .get("verified")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let reason = post_state
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("post_state_missing")
            .to_owned();
        let post_value = post_state.get("value").cloned().unwrap_or(Value::Null);
        Ok(Self {
            receipt,
            events,
            post_state: ScriptFleetPostState {
                verified,
                reason,
                value: post_value,
            },
        })
    }
}

fn validate_tab_id(tab_id: &str) -> Result<(), Box<EvalAltResult>> {
    let valid = tab_id
        .strip_prefix('@')
        .is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()));
    if !valid || tab_id.len() > 32 {
        return Err("fleet_tab_id_invalid: expected stable tab @ID".into());
    }
    Ok(())
}

fn fleet_operations(_fleet: &mut ScriptFleet) -> Result<Array, Box<EvalAltResult>> {
    OPERATION_CATALOG
        .iter()
        .map(|operation| {
            rhai::serde::to_dynamic(json!({
                "id": operation.id,
                "surface_path": operation.script_surface,
                "class": operation.class,
                "parameters": operation.parameters,
                "result_type": operation.result_type,
                "errors": operation.errors,
                "events": operation.events,
                "destructive": operation.destructive,
                "available": operation.available,
                "allowed": true,
                "since": operation.since,
            }))
            .map_err(|_| Box::<EvalAltResult>::from("fleet_catalog_encode"))
        })
        .collect()
}

fn json_string(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn json_i64(value: &Value, field: &str) -> rhai::INT {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| rhai::INT::try_from(value).ok())
        .unwrap_or_default()
}

fn json_field_dynamic(value: &Value, field: &str) -> Dynamic {
    value
        .get(field)
        .cloned()
        .and_then(|value| rhai::serde::to_dynamic(value).ok())
        .unwrap_or(Dynamic::UNIT)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn fake_fleet(calls: Arc<Mutex<Vec<(String, Value)>>>) -> ScriptFleet {
        bind(Arc::new(move |operation, arguments| {
            calls
                .lock()
                .unwrap()
                .push((operation.to_owned(), arguments.clone()));
            let operation_id = arguments["operation_id"].as_str().unwrap_or_default();
            if operation_id == "ui.tabs.set-width" {
                Ok(json!({
                    "receipt": {
                        "schema_version": 1,
                        "request_id": "script-test",
                        "operation_id": operation_id,
                        "outcome": "committed"
                    },
                    "events": [{
                        "schema_version": 1,
                        "epoch": "epoch",
                        "sequence": 2,
                        "kind": "layout.tabs.width",
                        "operation_id": operation_id,
                        "payload": {}
                    }],
                    "post_state": {
                        "verified": true,
                        "reason": "snapshot_and_event",
                        "value": {"tabs_width": 240}
                    }
                }))
            } else if matches!(operation_id, "ui.window.activate" | "terminal.paste") {
                Ok(json!({
                    "receipt": {
                        "schema_version": 1,
                        "request_id": "script-test",
                        "operation_id": operation_id,
                        "outcome": "committed"
                    },
                    "events": [],
                    "post_state": {
                        "verified": true,
                        "reason": "snapshot",
                        "value": {}
                    }
                }))
            } else {
                Ok(json!({"operation_id": operation_id}))
            }
        }))
    }

    #[test]
    fn typed_services_route_through_stable_operation_identity() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut engine = Engine::new();
        register(&mut engine);
        let mut scope = rhai::Scope::new();
        scope.push("fleet", fake_fleet(Arc::clone(&calls)));
        let result = engine
            .eval_with_scope::<rhai::Map>(
                &mut scope,
                r#"
                    let info = fleet.protocol.info();
                    let receipt = fleet.ui.tabs.set_width(240);
                    #{
                        info: info.operation_id,
                        request: receipt.request_id,
                        event: receipt.events[0].kind,
                        verified: receipt.post_state.verified
                    }
                "#,
            )
            .unwrap();
        assert_eq!(
            result["info"].clone().into_string().unwrap(),
            "protocol.info"
        );
        assert_eq!(
            result["request"].clone().into_string().unwrap(),
            "script-test"
        );
        assert_eq!(
            result["event"].clone().into_string().unwrap(),
            "layout.tabs.width"
        );
        assert!(result["verified"].as_bool().unwrap());
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].0, "fleet.call");
        assert_eq!(calls[0].1["operation_id"], "protocol.info");
        assert_eq!(calls[1].1["parameters"]["width"], 240);
    }

    #[test]
    fn every_binding_dispatches_mutation_through_the_typed_broker() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut engine = Engine::new();
        register(&mut engine);
        let mut scope = rhai::Scope::new();
        scope.push("fleet", fake_fleet(Arc::clone(&calls)));
        let _ = engine
            .eval_with_scope::<Dynamic>(&mut scope, "fleet.ui.tabs.set_width(240)")
            .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1["operation_id"], "ui.tabs.set-width");
    }

    #[test]
    fn callable_frontend_operations_route_and_coexist_with_tab_terminal_capture() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut engine = Engine::new();
        register(&mut engine);
        let mut scope = rhai::Scope::new();
        scope.push("fleet", fake_fleet(Arc::clone(&calls)));
        let result = engine
            .eval_with_scope::<rhai::Map>(
                &mut scope,
                r#"
                    let activate = fleet.ui.window.activate();
                    let paste = fleet.terminal.paste();
                    let capture = fleet.terminal("@7").capture(4096);
                    #{
                        activate: activate.operation_id,
                        paste: paste.operation_id,
                        capture: capture.operation_id
                    }
                "#,
            )
            .unwrap();
        assert_eq!(
            result["activate"].clone().into_string().unwrap(),
            "ui.window.activate"
        );
        assert_eq!(
            result["paste"].clone().into_string().unwrap(),
            "terminal.paste"
        );
        assert_eq!(
            result["capture"].clone().into_string().unwrap(),
            "pane.capture"
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].1["operation_id"], "ui.window.activate");
        assert_eq!(calls[0].1["parameters"], json!({}));
        assert_eq!(calls[1].1["operation_id"], "terminal.paste");
        assert_eq!(calls[1].1["parameters"], json!({}));
        assert_eq!(calls[2].1["operation_id"], "pane.capture");
        assert_eq!(calls[2].1["parameters"]["tab"], "@7");
        assert_eq!(calls[2].1["parameters"]["max_bytes"], 4096);
    }

    #[test]
    fn operation_catalog_has_one_script_surface_per_operation() {
        let mut surfaces = OPERATION_CATALOG
            .iter()
            .map(|operation| operation.script_surface)
            .collect::<Vec<_>>();
        assert!(surfaces.iter().all(|surface| !surface.is_empty()));
        surfaces.sort_unstable();
        surfaces.dedup();
        assert_eq!(surfaces.len(), OPERATION_CATALOG.len());
    }
}
