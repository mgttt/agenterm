use crate::{
    ProcessResourceSample,
    identity::{self, IdentityMode},
    sample_process_resources,
    store::{PersistentStore, StoreSnapshot},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DESCRIPTOR_FILE: &str = "node.json";
const MAX_CONTROL_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeDescriptor {
    pub schema: String,
    pub pid: u32,
    pub peer_id: String,
    pub identity: IdentityMode,
    pub identity_created: bool,
    pub identity_key_path: Option<String>,
    pub control_address: String,
    pub nonce: String,
    pub started_unix_ms: u128,
    pub state_dir: String,
    pub store_dir: String,
    pub network_scope: String,
    pub public_bootstrap: bool,
    pub nat_traversal: bool,
    pub relay_server: bool,
    pub remote_control: bool,
}

#[derive(Debug, Serialize)]
pub struct NodeStatus {
    pub schema: &'static str,
    pub lifecycle: &'static str,
    pub descriptor: NodeDescriptor,
    pub store: StoreSnapshot,
    pub resources: ProcessResourceSample,
}

#[derive(Deserialize, Serialize)]
struct ControlRequest {
    schema: String,
    request_id: String,
    nonce: String,
    action: String,
}

#[derive(Deserialize, Serialize)]
struct ControlResponse {
    schema: String,
    request_id: String,
    state: String,
    status: Option<NodeStatusWire>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct NodeStatusWire {
    lifecycle: String,
    descriptor: NodeDescriptor,
    store: StoreSnapshotWire,
    resources: ProcessResourceSample,
}

#[derive(Deserialize, Serialize)]
struct StoreSnapshotWire {
    schema: String,
    path: String,
    block_count: usize,
    pinned_count: usize,
    verified_bytes: u64,
    stored_bytes: u64,
    corrupt_blocks: usize,
    max_block_bytes: usize,
    max_store_bytes: u64,
    max_store_blocks: usize,
}

impl From<StoreSnapshot> for StoreSnapshotWire {
    fn from(value: StoreSnapshot) -> Self {
        Self {
            schema: value.schema.to_string(),
            path: value.path,
            block_count: value.block_count,
            pinned_count: value.pinned_count,
            verified_bytes: value.verified_bytes,
            stored_bytes: value.stored_bytes,
            corrupt_blocks: value.corrupt_blocks,
            max_block_bytes: value.max_block_bytes,
            max_store_bytes: value.max_store_bytes,
            max_store_blocks: value.max_store_blocks,
        }
    }
}

impl From<StoreSnapshotWire> for StoreSnapshot {
    fn from(value: StoreSnapshotWire) -> Self {
        Self {
            schema: "agenterm-net/store-snapshot/v1",
            path: value.path,
            block_count: value.block_count,
            pinned_count: value.pinned_count,
            verified_bytes: value.verified_bytes,
            stored_bytes: value.stored_bytes,
            corrupt_blocks: value.corrupt_blocks,
            max_block_bytes: value.max_block_bytes,
            max_store_bytes: value.max_store_bytes,
            max_store_blocks: value.max_store_blocks,
        }
    }
}

pub fn start(
    state_dir: &Path,
    mode: IdentityMode,
    deadline: Duration,
) -> Result<NodeStatus, String> {
    fs::create_dir_all(state_dir).map_err(|error| format!("create node state: {error}"))?;
    let descriptor_path = state_dir.join(DESCRIPTOR_FILE);
    if descriptor_path.exists() {
        return match status(state_dir, deadline) {
            Ok(status) => Err(format!(
                "node is already running as pid {}",
                status.descriptor.pid
            )),
            Err(error) => Err(format!(
                "node descriptor exists but its owner is unavailable ({error}); refusing to erase crash evidence or start a second owner"
            )),
        };
    }
    identity::preflight_start(state_dir, mode)?;

    let ready_listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("bind node readiness endpoint: {error}"))?;
    let ready_address = ready_listener
        .local_addr()
        .map_err(|error| format!("read node readiness endpoint: {error}"))?;
    let ready_token = unique_token("ready");
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(executable);
    command
        .arg("__node-daemon")
        .arg(state_dir)
        .arg(mode.label())
        .arg(ready_address.to_string())
        .arg(&ready_token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP avoids attaching the
        // long-lived sidecar (or a conhost) to a caller's captured stdio.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn node: {error}"))?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = ready_listener
            .accept()
            .map_err(|error| error.to_string())
            .and_then(|(stream, _)| {
                stream
                    .set_read_timeout(Some(deadline))
                    .map_err(|error| error.to_string())?;
                let mut line = String::new();
                BufReader::new(stream.take(MAX_CONTROL_BYTES))
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                Ok(line)
            });
        let _ = sender.send(result);
    });
    let line = match receiver.recv_timeout(deadline) {
        Ok(Ok(line)) if !line.is_empty() => line,
        Ok(Ok(_)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("node ended before ready".to_string());
        }
        Ok(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("read node ready: {error}"));
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("node start deadline exceeded".to_string());
        }
    };
    let ready: ControlResponse = serde_json::from_str(&line)
        .map_err(|error| format!("node emitted invalid ready response: {error}"))?;
    if ready.request_id != ready_token || ready.state != "complete" {
        let _ = child.wait();
        return Err(ready
            .message
            .unwrap_or_else(|| "node start failed".to_string()));
    }
    drop(child);
    status(state_dir, deadline)
}

pub fn status(state_dir: &Path, deadline: Duration) -> Result<NodeStatus, String> {
    request(state_dir, "status", deadline)
}

pub fn stop(state_dir: &Path, deadline: Duration) -> Result<NodeStatus, String> {
    request(state_dir, "stop", deadline)
}

fn request(state_dir: &Path, action: &str, deadline: Duration) -> Result<NodeStatus, String> {
    let descriptor = read_descriptor(state_dir)?;
    let mut stream = TcpStream::connect_timeout(
        &descriptor
            .control_address
            .parse()
            .map_err(|error| format!("invalid control address: {error}"))?,
        deadline,
    )
    .map_err(|error| format!("node unavailable: {error}"))?;
    stream
        .set_read_timeout(Some(deadline))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(deadline))
        .map_err(|error| error.to_string())?;
    let expected_request_id = unique_token("request");
    let request = ControlRequest {
        schema: "agenterm-net/control-request/v1".to_string(),
        request_id: expected_request_id.clone(),
        nonce: descriptor.nonce.clone(),
        action: action.to_string(),
    };
    serde_json::to_writer(&mut stream, &request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(stream.take(MAX_CONTROL_BYTES))
        .read_line(&mut line)
        .map_err(|error| format!("read control response: {error}"))?;
    let response: ControlResponse = serde_json::from_str(&line)
        .map_err(|error| format!("invalid control response: {error}"))?;
    if response.schema != "agenterm-net/control-response/v1"
        || response.request_id != expected_request_id
    {
        return Err("control response schema or request ID mismatch".to_string());
    }
    if response.state != "complete" {
        return Err(format!(
            "{}: {}",
            response.code.unwrap_or_else(|| "node_failed".to_string()),
            response
                .message
                .unwrap_or_else(|| "node request failed".to_string())
        ));
    }
    let status = response
        .status
        .ok_or_else(|| "control response omitted status".to_string())?;
    let lifecycle = if action == "stop" {
        "stopped"
    } else {
        "running"
    };
    Ok(NodeStatus {
        schema: "agenterm-net/node-status/v1",
        lifecycle,
        descriptor: status.descriptor,
        store: status.store.into(),
        resources: status.resources,
    })
}

pub fn daemon(
    state_dir: &Path,
    mode: IdentityMode,
    ready_address: &str,
    ready_token: &str,
) -> Result<(), String> {
    fs::create_dir_all(state_dir).map_err(|error| format!("create node state: {error}"))?;
    let identity = identity::load_or_create(state_dir, mode)?;
    let store_dir = state_dir.join("store");
    let store = PersistentStore::open(&store_dir)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("bind private control endpoint: {error}"))?;
    let descriptor = NodeDescriptor {
        schema: "agenterm-net/node-descriptor/v1".to_string(),
        pid: std::process::id(),
        peer_id: identity.peer_id(),
        identity: identity.mode,
        identity_created: identity.created,
        identity_key_path: identity.key_path.map(|path| path.display().to_string()),
        control_address: listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .to_string(),
        nonce: unique_token("node"),
        started_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        state_dir: state_dir.display().to_string(),
        store_dir: store.root().display().to_string(),
        network_scope: "private-mesh only; no listeners until explicitly configured".to_string(),
        public_bootstrap: false,
        nat_traversal: false,
        relay_server: false,
        remote_control: false,
    };
    write_descriptor(state_dir, &descriptor)?;
    let ready = response(
        ready_token,
        NodeStatusWire {
            lifecycle: "running".to_string(),
            descriptor: descriptor.clone(),
            store: store.snapshot()?.into(),
            resources: sample_process_resources(),
        },
    );
    let mut ready_stream = TcpStream::connect(ready_address)
        .map_err(|error| format!("connect node readiness endpoint: {error}"))?;
    write_response(&mut ready_stream, ready)?;

    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| format!("accept control request: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|error| error.to_string())?;
        let mut line = String::new();
        BufReader::new((&mut stream).take(MAX_CONTROL_BYTES))
            .read_line(&mut line)
            .map_err(|error| format!("read control request: {error}"))?;
        let request: ControlRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    &mut stream,
                    failure("unknown", "invalid_request", error.to_string()),
                )?;
                continue;
            }
        };
        if request.schema != "agenterm-net/control-request/v1" || request.nonce != descriptor.nonce
        {
            write_response(
                &mut stream,
                failure(
                    &request.request_id,
                    "unauthorized",
                    "control nonce mismatch".to_string(),
                ),
            )?;
            continue;
        }
        let lifecycle = if request.action == "stop" {
            "stopped"
        } else {
            "running"
        };
        if request.action != "status" && request.action != "stop" {
            write_response(
                &mut stream,
                failure(&request.request_id, "unsupported_action", request.action),
            )?;
            continue;
        }
        let wire = NodeStatusWire {
            lifecycle: lifecycle.to_string(),
            descriptor: descriptor.clone(),
            store: store.snapshot()?.into(),
            resources: sample_process_resources(),
        };
        if lifecycle == "stopped" {
            remove_descriptor_if_owned(state_dir, &descriptor.nonce)?;
        }
        write_response(&mut stream, response(&request.request_id, wire))?;
        if lifecycle == "stopped" {
            return Ok(());
        }
    }
    remove_descriptor_if_owned(state_dir, &descriptor.nonce)
}

fn response(request_id: &str, status: NodeStatusWire) -> ControlResponse {
    ControlResponse {
        schema: "agenterm-net/control-response/v1".to_string(),
        request_id: request_id.to_string(),
        state: "complete".to_string(),
        status: Some(status),
        code: None,
        message: None,
    }
}

fn failure(request_id: &str, code: &str, message: String) -> ControlResponse {
    ControlResponse {
        schema: "agenterm-net/control-response/v1".to_string(),
        request_id: request_id.to_string(),
        state: "failed".to_string(),
        status: None,
        code: Some(code.to_string()),
        message: Some(message),
    }
}

fn write_response(stream: &mut TcpStream, response: ControlResponse) -> Result<(), String> {
    serde_json::to_writer(&mut *stream, &response).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

fn descriptor_path(state_dir: &Path) -> PathBuf {
    state_dir.join(DESCRIPTOR_FILE)
}

fn read_descriptor(state_dir: &Path) -> Result<NodeDescriptor, String> {
    let bytes = fs::read(descriptor_path(state_dir))
        .map_err(|error| format!("node descriptor unavailable: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid node descriptor: {error}"))
}

fn write_descriptor(state_dir: &Path, descriptor: &NodeDescriptor) -> Result<(), String> {
    let path = descriptor_path(state_dir);
    let temporary = state_dir.join(format!("node-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec(descriptor).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("create node descriptor: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write node descriptor: {error}"))?;
    drop(file);
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("replace node descriptor: {error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("commit node descriptor: {error}"))
}

fn remove_descriptor_if_owned(state_dir: &Path, nonce: &str) -> Result<(), String> {
    let path = descriptor_path(state_dir);
    if let Ok(current) = read_descriptor(state_dir)
        && current.nonce == nonce
    {
        fs::remove_file(path).map_err(|error| format!("remove node descriptor: {error}"))?;
    }
    Ok(())
}

fn unique_token(label: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let material = format!("{label}:{}:{now}", std::process::id());
    let digest = Sha256::digest(material.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
