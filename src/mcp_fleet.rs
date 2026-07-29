use std::{env, time::Duration};

use serde_json::{Value, json};

use crate::{
    client::send_ipc_request_to_timeout,
    instances::{discover_instances, instance_process_is_alive},
};

pub(crate) const RESOURCE_INSTANCES: &str = "agenterm://fleet/instances";
pub(crate) const RESOURCE_WORKSPACE: &str = "agenterm://fleet/workspace";
pub(crate) const RESOURCE_TABS: &str = "agenterm://fleet/tabs";
pub(crate) const RESOURCE_SNAPSHOT: &str = "agenterm://fleet/snapshot";

const MCP_BACKEND_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Debug)]
pub(crate) struct McpFleetError {
    pub code: i64,
    pub message: &'static str,
    pub data: Value,
}

pub(crate) fn read_resource(uri: &str, address: Option<&str>) -> Result<Value, McpFleetError> {
    match uri {
        RESOURCE_INSTANCES => instance_inventory(),
        RESOURCE_WORKSPACE | RESOURCE_TABS | RESOURCE_SNAPSHOT => {
            let address = select_address(address)?;
            let snapshot = fetch_snapshot(&address)?;
            let value = match uri {
                RESOURCE_WORKSPACE => workspace_inventory(&address, &snapshot),
                RESOURCE_TABS => tab_inventory(&address, &snapshot),
                RESOURCE_SNAPSHOT => fleet_snapshot(&address, &snapshot),
                _ => unreachable!(),
            };
            Ok(value)
        }
        _ => Err(McpFleetError {
            code: -32002,
            message: "Resource not found",
            data: json!({"uri": uri}),
        }),
    }
}

fn instance_inventory() -> Result<Value, McpFleetError> {
    let instances = discover_instances().map_err(|error| McpFleetError {
        code: -32603,
        message: "Could not discover AgenTerm instances",
        data: json!({"class": "instance_discovery", "detail": bounded(&error.to_string())}),
    })?;
    let instances = instances
        .into_iter()
        .map(|instance| {
            json!({
                "pid": instance.record.pid,
                "address": instance.record.address,
                "version": instance.record.version,
                "session": instance.record.session,
                "started_at_unix_ms": instance.record.started_at_unix_ms,
                "alive": instance_process_is_alive(instance.record.pid)
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema_id": "agenterm.mcp.resource.instances.v1",
        "schema_version": 1,
        "instances": instances
    }))
}

fn select_address(explicit: Option<&str>) -> Result<String, McpFleetError> {
    if let Some(address) = explicit
        .filter(|address| !address.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            env::var("AGENTERM_IPC_ADDRESS")
                .ok()
                .filter(|address| !address.trim().is_empty())
        })
    {
        return Ok(address);
    }
    let instances = discover_instances().map_err(|error| McpFleetError {
        code: -32603,
        message: "Could not discover AgenTerm instances",
        data: json!({"class": "instance_discovery", "detail": bounded(&error.to_string())}),
    })?;
    let candidates = instances
        .into_iter()
        .filter(|instance| instance_process_is_alive(instance.record.pid))
        .map(|instance| {
            json!({
                "pid": instance.record.pid,
                "address": instance.record.address,
                "session": instance.record.session,
                "version": instance.record.version
            })
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [candidate] => Ok(candidate["address"]
            .as_str()
            .expect("candidate address is a string")
            .to_owned()),
        [] => Err(McpFleetError {
            code: -32001,
            message: "No running AgenTerm server is available",
            data: json!({
                "class": "server_not_found",
                "hint": "start AgenTerm or pass --address HOST:PORT"
            }),
        }),
        _ => Err(McpFleetError {
            code: -32001,
            message: "Multiple AgenTerm servers require explicit selection",
            data: json!({"class": "server_ambiguous", "candidates": candidates}),
        }),
    }
}

fn fetch_snapshot(address: &str) -> Result<Value, McpFleetError> {
    let response =
        send_ipc_request_to_timeout(address, vec!["ui-snapshot".to_owned()], MCP_BACKEND_TIMEOUT)
            .map_err(|error| McpFleetError {
            code: -32001,
            message: "AgenTerm server is unreachable",
            data: json!({
                "class": "server_unreachable",
                "address": address,
                "detail": bounded(&error.to_string())
            }),
        })?;
    if !response.ok {
        return Err(McpFleetError {
            code: -32603,
            message: "AgenTerm snapshot request failed",
            data: json!({
                "class": "snapshot_failed",
                "address": address,
                "backend_code": response.error_code,
                "backend_category": response.error_category
            }),
        });
    }
    serde_json::from_str(&response.output).map_err(|error| McpFleetError {
        code: -32603,
        message: "AgenTerm returned an invalid snapshot",
        data: json!({
            "class": "snapshot_invalid",
            "address": address,
            "detail": bounded(&error.to_string())
        }),
    })
}

fn workspace_inventory(address: &str, snapshot: &Value) -> Value {
    json!({
        "schema_id": "agenterm.mcp.resource.workspace.v1",
        "schema_version": 1,
        "server": server_identity(address, snapshot),
        "event_position": snapshot["event_position"],
        "active_tab_id": snapshot["focus"]["window_id"],
        "tab_count": snapshot["tabs"].as_array().map_or(0, Vec::len),
        "window": {
            "visible": snapshot["window"]["visible"],
            "detached": snapshot["window"]["detached"],
            "minimized": snapshot["window"]["minimized"],
            "state": snapshot["window"]["state"]
        }
    })
}

fn tab_inventory(address: &str, snapshot: &Value) -> Value {
    json!({
        "schema_id": "agenterm.mcp.resource.tabs.v1",
        "schema_version": 1,
        "server": server_identity(address, snapshot),
        "event_position": snapshot["event_position"],
        "tabs": safe_tabs(snapshot)
    })
}

fn fleet_snapshot(address: &str, snapshot: &Value) -> Value {
    json!({
        "schema_id": "agenterm.mcp.resource.fleet-snapshot.v1",
        "schema_version": 1,
        "server": server_identity(address, snapshot),
        "event_position": snapshot["event_position"],
        "workspace": {
            "active_tab_id": snapshot["focus"]["window_id"],
            "tab_count": snapshot["tabs"].as_array().map_or(0, Vec::len),
            "window_state": snapshot["window"]["state"],
            "window_visible": snapshot["window"]["visible"],
            "window_detached": snapshot["window"]["detached"]
        },
        "tabs": safe_tabs(snapshot)
    })
}

fn server_identity(address: &str, snapshot: &Value) -> Value {
    json!({
        "address": address,
        "pid": snapshot["server_pid"],
        "protocol_version": snapshot["protocol_version"]
    })
}

fn safe_tabs(snapshot: &Value) -> Vec<Value> {
    snapshot["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|tab| {
            json!({
                "id": tab["id"],
                "parent_id": tab["parent_id"],
                "name": tab["name"],
                "note": tab["note"],
                "state": tab["state"],
                "pid": tab["pid"],
                "exit_code": tab["exit_code"],
                "active": tab["active"],
                "has_children": tab["has_children"],
                "collapsed": tab["collapsed"],
                "depth": tab["depth"]
            })
        })
        .collect()
}

fn bounded(value: &str) -> String {
    value.chars().take(1024).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        json!({
            "server_pid": 42,
            "protocol_version": 1,
            "event_position": {"epoch": "epoch-a", "sequence": 7},
            "focus": {"window_id": "@2"},
            "window": {"visible": true, "detached": false, "minimized": false, "state": "normal"},
            "tabs": [{
                "id": "@2",
                "parent_id": null,
                "name": "worker",
                "note": "safe metadata",
                "state": "running",
                "pid": 99,
                "exit_code": null,
                "active": true,
                "has_children": false,
                "collapsed": false,
                "depth": 0,
                "terminal_title": "secret command",
                "working_context": {
                    "cwd": {"path": "secret path"},
                    "proxy": {"endpoint": "secret proxy"}
                },
                "composer": "secret draft"
            }]
        })
    }

    #[test]
    fn fleet_projection_excludes_content_and_environment_fields() {
        let value = fleet_snapshot("127.0.0.1:48815", &fixture());
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(!encoded.contains("secret command"));
        assert!(!encoded.contains("secret path"));
        assert!(!encoded.contains("secret proxy"));
        assert!(!encoded.contains("secret draft"));
        assert_eq!(value["event_position"]["epoch"], "epoch-a");
        assert_eq!(value["tabs"][0]["id"], "@2");
    }

    #[test]
    fn unknown_resource_is_typed_without_backend_access() {
        let error = read_resource("agenterm://fleet/unknown", None).unwrap_err();
        assert_eq!(error.code, -32002);
        assert_eq!(error.data["uri"], "agenterm://fleet/unknown");
    }
}
