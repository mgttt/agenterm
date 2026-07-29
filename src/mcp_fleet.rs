use std::{
    env,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

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
pub(crate) const WAIT_EVENT_KINDS: &[&str] = &[
    "focus.changed",
    "layout.tree.collapse",
    "layout.tabs.visibility",
    "layout.tabs.width",
    "tab.closed",
    "tab.created",
    "tab.note",
    "tab.parent",
    "tab.renamed",
    "tab.selected",
    "tab.state",
    "terminal.resized",
    "terminal.viewport",
    "ui.lease",
    "window.visibility",
    "workspace.saved",
    "workspace.shutdown",
];

#[derive(Clone, Debug)]
pub(crate) struct McpWaitRequest {
    pub epoch: String,
    pub after_sequence: u64,
    pub event_kind: String,
    pub tab_id: Option<String>,
    pub timeout_ms: u64,
}

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

pub(crate) fn wait_event(
    address: Option<&str>,
    request: McpWaitRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<Value, McpFleetError> {
    let address = select_address(address)?;
    let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
    let mut after = request.after_sequence;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(wait_outcome("cancelled", &request, after, None, None));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(wait_outcome("timeout", &request, after, None, None));
        }
        let response = send_ipc_request_to_timeout(
            &address,
            vec![
                "read-events".to_owned(),
                "--epoch".to_owned(),
                request.epoch.clone(),
                "--after".to_owned(),
                after.to_string(),
                "--limit".to_owned(),
                "64".to_owned(),
            ],
            remaining.min(MCP_BACKEND_TIMEOUT),
        )
        .map_err(|error| McpFleetError {
            code: -32001,
            message: "AgenTerm event journal is unreachable",
            data: json!({
                "class": "server_unreachable",
                "address": address,
                "detail": bounded(&error.to_string())
            }),
        })?;
        if !response.ok {
            let class = match response.error_code.as_str() {
                "server_restart" => "server_restart",
                "journal_gap" => "journal_gap",
                "future_sequence" => "future_sequence",
                _ => "event_read_failed",
            };
            return Ok(wait_outcome(
                class,
                &request,
                after,
                None,
                Some(json!({
                    "backend_code": response.error_code,
                    "backend_category": response.error_category
                })),
            ));
        }
        let batch: Value =
            serde_json::from_str(&response.output).map_err(|error| McpFleetError {
                code: -32603,
                message: "AgenTerm returned an invalid event batch",
                data: json!({
                    "class": "event_batch_invalid",
                    "detail": bounded(&error.to_string())
                }),
            })?;
        if let Some(events) = batch["events"].as_array() {
            for event in events {
                if let Some(sequence) = event["sequence"].as_u64() {
                    after = after.max(sequence);
                }
                let tab_matches = request.tab_id.as_deref().is_none_or(|expected| {
                    event["tab_id"]
                        .as_u64()
                        .map(|id| format!("@{id}"))
                        .as_deref()
                        == Some(expected)
                });
                let projected_event = || {
                    json!({
                        "epoch": event["epoch"],
                        "sequence": event["sequence"],
                        "kind": event["kind"],
                        "tab_id": event["tab_id"].as_u64().map(|id| format!("@{id}"))
                    })
                };
                if event["kind"].as_str() == Some(request.event_kind.as_str()) && tab_matches {
                    let post_snapshot = fetch_snapshot(&address)?;
                    let post_state = json!({
                        "schema_id": "agenterm.mcp.wait-post-state.v1",
                        "event_position": post_snapshot["event_position"],
                        "active_tab_id": post_snapshot["focus"]["window_id"],
                        "tab_count": post_snapshot["tabs"].as_array().map_or(0, Vec::len)
                    });
                    return Ok(wait_outcome(
                        "matched",
                        &request,
                        after,
                        Some(projected_event()),
                        Some(post_state),
                    ));
                }
                if request.tab_id.is_some()
                    && event["kind"].as_str() == Some("tab.closed")
                    && tab_matches
                {
                    return Ok(wait_outcome(
                        "target_closed",
                        &request,
                        after,
                        Some(projected_event()),
                        None,
                    ));
                }
            }
        }
        if let Some(sequence) = batch["position"]["sequence"].as_u64() {
            after = after.max(sequence);
        }
        thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn wait_outcome(
    outcome: &str,
    request: &McpWaitRequest,
    after: u64,
    event: Option<Value>,
    detail: Option<Value>,
) -> Value {
    json!({
        "schema_id": "agenterm.mcp.tool.wait-result.v1",
        "schema_version": 1,
        "outcome": outcome,
        "requested": {
            "epoch": request.epoch,
            "after_sequence": request.after_sequence,
            "event_kind": request.event_kind,
            "tab_id": request.tab_id,
            "timeout_ms": request.timeout_ms
        },
        "position": {
            "epoch": request.epoch,
            "sequence": after
        },
        "event": event,
        "detail": detail
    })
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
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
        sync::atomic::{AtomicBool, AtomicUsize},
    };

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

    #[test]
    fn already_cancelled_wait_returns_without_backend_access() {
        let result = wait_event(
            Some("127.0.0.1:1"),
            McpWaitRequest {
                epoch: "epoch-a".to_owned(),
                after_sequence: 7,
                event_kind: "tab.selected".to_owned(),
                tab_id: Some("@2".to_owned()),
                timeout_ms: 100,
            },
            Arc::new(AtomicBool::new(true)),
        )
        .unwrap();
        assert_eq!(result["outcome"], "cancelled");
        assert_eq!(result["position"]["sequence"], 7);
    }

    #[test]
    fn active_wait_observes_cancellation_between_bounded_polls() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let request_count = Arc::new(AtomicUsize::new(0));
        let server = thread::spawn({
            let stop = Arc::clone(&stop);
            let request_count = Arc::clone(&request_count);
            move || {
                while !stop.load(Ordering::Acquire) {
                    let (stream, _) = listener.accept().unwrap();
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    request_count.fetch_add(1, Ordering::AcqRel);
                    reply_with_empty_event_batch(stream);
                }
            }
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let canceller = thread::spawn({
            let cancelled = Arc::clone(&cancelled);
            move || {
                thread::sleep(Duration::from_millis(75));
                cancelled.store(true, Ordering::Release);
            }
        });
        let result = wait_event(
            Some(&address),
            McpWaitRequest {
                epoch: "epoch-a".to_owned(),
                after_sequence: 7,
                event_kind: "tab.selected".to_owned(),
                tab_id: None,
                timeout_ms: 2_000,
            },
            cancelled,
        )
        .unwrap();
        canceller.join().unwrap();
        assert_eq!(result["outcome"], "cancelled");
        assert!(request_count.load(Ordering::Acquire) > 0);
        stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(&address);
        server.join().unwrap();
    }

    #[test]
    fn closing_a_filtered_tab_is_a_distinct_terminal_outcome() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            reply_with_event_batch(
                stream,
                json!({
                    "events": [{
                        "epoch": "epoch-a",
                        "sequence": 8,
                        "kind": "tab.closed",
                        "tab_id": 2,
                        "payload": {"content": "must not escape"}
                    }],
                    "position": {"epoch": "epoch-a", "sequence": 8}
                }),
            );
        });
        let result = wait_event(
            Some(&address),
            McpWaitRequest {
                epoch: "epoch-a".to_owned(),
                after_sequence: 7,
                event_kind: "tab.note".to_owned(),
                tab_id: Some("@2".to_owned()),
                timeout_ms: 1_000,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(result["outcome"], "target_closed");
        assert_eq!(result["event"]["kind"], "tab.closed");
        assert_eq!(result["event"]["tab_id"], "@2");
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("must not escape")
        );
    }

    #[test]
    fn journal_position_failures_remain_distinct_wait_outcomes() {
        for expected in ["server_restart", "journal_gap", "future_sequence"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap().to_string();
            let server = thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                reply_with_failure(stream, expected);
            });
            let result = wait_event(
                Some(&address),
                McpWaitRequest {
                    epoch: "epoch-a".to_owned(),
                    after_sequence: 7,
                    event_kind: "tab.note".to_owned(),
                    tab_id: None,
                    timeout_ms: 1_000,
                },
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            server.join().unwrap();
            assert_eq!(result["outcome"], expected);
            assert_eq!(result["detail"]["backend_code"], expected);
        }
    }

    fn reply_with_empty_event_batch(stream: TcpStream) {
        reply_with_event_batch(
            stream,
            json!({
                "events": [],
                "position": {"epoch": "epoch-a", "sequence": 7}
            }),
        );
    }

    fn reply_with_event_batch(mut stream: TcpStream, batch: Value) {
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        assert!(!request.is_empty());
        let response = json!({
            "ok": true,
            "output": batch.to_string(),
            "error": "",
            "error_code": "",
            "error_category": "",
            "retryable": false
        });
        writeln!(stream, "{response}").unwrap();
    }

    fn reply_with_failure(mut stream: TcpStream, code: &str) {
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        assert!(!request.is_empty());
        let response = json!({
            "ok": false,
            "output": "",
            "error": "fixture failure",
            "error_code": code,
            "error_category": "event_position",
            "retryable": false
        });
        writeln!(stream, "{response}").unwrap();
    }
}
