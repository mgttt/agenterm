use std::{
    io::Write,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

#[test]
fn public_stdio_lifecycle_keeps_stdout_machine_only() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-mcp"))
        .args(["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
        "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
        "\"clientInfo\":{\"name\":\"black-box\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"ping\",\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"resources\",\"method\":\"resources/list\"}\n"
    );
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input.as_bytes())
        .expect("write MCP lifecycle");
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "successful lifecycle wrote diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("stdout line is JSON-RPC"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        responses[0]["result"]["capabilities"],
        json!({"resources": {"subscribe": false, "listChanged": false}})
    );
    assert_eq!(
        responses[1],
        json!({"jsonrpc": "2.0", "id": "ping", "result": {}})
    );
    assert_eq!(
        responses[2]["result"]["resources"]
            .as_array()
            .expect("resource list"),
        &[
            json!({
                "uri": "agenterm://fleet/instances",
                "name": "fleet.instances",
                "title": "AgenTerm Instances",
                "description": "Registered local AgenTerm server metadata",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "agenterm://fleet/workspace",
                "name": "fleet.workspace",
                "title": "AgenTerm Workspace",
                "description": "Selected workspace identity and event baseline",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "agenterm://fleet/tabs",
                "name": "fleet.tabs",
                "title": "AgenTerm Tabs",
                "description": "Metadata-only stable tab inventory",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "agenterm://fleet/snapshot",
                "name": "fleet.snapshot",
                "title": "AgenTerm Fleet Snapshot",
                "description": "One causal metadata-only Fleet snapshot",
                "mimeType": "application/json"
            })
        ]
    );
}
