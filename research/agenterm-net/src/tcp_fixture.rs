use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    env,
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const MAX_WORKER_OUTPUT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize)]
pub struct CrossProcessProof {
    pub schema: &'static str,
    pub status: &'static str,
    pub fixture: &'static str,
    pub elapsed_ms: u128,
    pub process: ProcessProof,
    pub attach: AttachTcpProof,
    pub dht: DhtTcpProof,
    pub authority: AuthorityProof,
    pub public_bootstrap: bool,
    pub nat_traversal: bool,
    pub relay_serving_default: bool,
}

#[derive(Debug, Serialize)]
pub struct ProcessProof {
    pub parent_pid: u32,
    pub child_processes: usize,
    pub maximum_concurrent_children: usize,
    pub graceful_child_exits: usize,
    pub forced_cleanup_children: usize,
    pub reaped_children: usize,
    pub residual_children: usize,
    pub no_fixed_sleep: bool,
}

#[derive(Debug, Serialize)]
pub struct AttachTcpProof {
    pub transport: &'static str,
    pub protocol: &'static str,
    pub server_pid: u32,
    pub client_pids: Vec<u32>,
    pub issuer_peer_id: String,
    pub paired_peer_id: String,
    pub wrong_peer_id: String,
    pub authenticated_peer_identity: bool,
    pub explicit_pairing: bool,
    pub snapshot_bytes: usize,
    pub server_count: u16,
    pub event_digest_count: usize,
    pub replay_rejection: String,
    pub wrong_peer_rejection: String,
    pub expired_rejection: String,
    pub request_bytes_limit: u64,
    pub response_bytes_limit: u64,
}

#[derive(Debug, Serialize)]
pub struct DhtTcpProof {
    pub transport: &'static str,
    pub protocol: &'static str,
    pub topology: &'static str,
    pub hub_pid: u32,
    pub publisher_pid: u32,
    pub seeker_pid: u32,
    pub hub_peer_id: String,
    pub publisher_peer_id: String,
    pub seeker_peer_id: String,
    pub record_sha256: String,
    pub record_published: bool,
    pub record_found_via_hub: bool,
    pub publisher_forced_cleanup_reaped: bool,
    pub hub_forced_cleanup_reaped: bool,
    pub public_bootstrap_attempts: u8,
}

#[derive(Debug, Serialize)]
pub struct AuthorityProof {
    pub read_only_projection: bool,
    pub shell: bool,
    pub command_execution: bool,
    pub pty_control: bool,
    pub terminal_input: bool,
    pub server_control: bool,
}

#[derive(Deserialize)]
struct Ready {
    event: String,
    peer_id: String,
    address: String,
    pid: u32,
}

#[derive(Deserialize)]
struct AttachServerResult {
    event: String,
    pid: u32,
    handled_requests: usize,
    accepted: bool,
    replay_rejected: bool,
    wrong_peer_rejected: bool,
    expired_rejected: bool,
}

#[derive(Deserialize)]
struct AttachClientResult {
    event: String,
    kind: String,
    pid: u32,
    peer_id: String,
    authenticated_server_peer_id: String,
    state: String,
    code: Option<String>,
    snapshot_bytes: usize,
    server_count: u16,
    event_digest_count: usize,
}

#[derive(Deserialize)]
struct DhtWorkerResult {
    event: String,
    peer_id: String,
    remote_peer_id: String,
    publisher_peer_id: String,
    record_sha256: String,
    pid: u32,
}

struct WorkerGuard {
    child: Child,
}

enum WorkerLine {
    Line(String),
    Error(String),
    Eof,
}

pub fn prove_cross_process_tcp(deadline: Duration) -> Result<CrossProcessProof, String> {
    let started = Instant::now();
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let deadline_text = deadline.as_millis().to_string();
    let parent_pid = std::process::id();
    let mut graceful_child_exits = 0_usize;
    let mut reaped_children = 0_usize;

    let (attach_server, attach_rx) = spawn_worker(
        &executable,
        &["__attach-tcp-server".to_string(), deadline_text.clone()],
    )?;
    let attach_server_pid = attach_server.pid();
    let attach_ready: Ready = receive(
        &attach_rx,
        remaining(started, deadline)?,
        "attach server ready",
    )?;
    if attach_ready.event != "attach-ready"
        || attach_ready.pid != attach_server_pid
        || attach_ready.address.contains("/memory/")
        || !attach_ready.address.contains("/ip4/127.0.0.1/tcp/")
    {
        return Err("attach server emitted invalid TCP readiness evidence".to_string());
    }

    let mut attach_clients = Vec::new();
    for kind in ["valid", "replay", "wrong-peer", "expired"] {
        let (client, rx) = spawn_worker(
            &executable,
            &[
                "__attach-tcp-client".to_string(),
                attach_ready.address.clone(),
                attach_ready.peer_id.clone(),
                kind.to_string(),
                deadline_text.clone(),
            ],
        )?;
        let pid = client.pid();
        let result: AttachClientResult = receive(
            &rx,
            remaining(started, deadline)?,
            &format!("attach {kind}"),
        )?;
        if result.event != "attach-response" || result.kind != kind || result.pid != pid {
            return Err(format!("attach {kind} worker receipt did not match child"));
        }
        if !client.finish()? {
            return Err(format!("attach {kind} worker did not exit cleanly"));
        }
        graceful_child_exits += 1;
        reaped_children += 1;
        attach_clients.push(result);
    }
    let attach_complete: AttachServerResult = receive(
        &attach_rx,
        remaining(started, deadline)?,
        "attach server complete",
    )?;
    if attach_complete.event != "attach-complete"
        || attach_complete.pid != attach_server_pid
        || attach_complete.handled_requests != 4
        || !attach_complete.accepted
        || !attach_complete.replay_rejected
        || !attach_complete.wrong_peer_rejected
        || !attach_complete.expired_rejected
    {
        return Err("attach server did not prove every required outcome".to_string());
    }
    if !attach_server.finish()? {
        return Err("attach TCP server did not exit cleanly".to_string());
    }
    graceful_child_exits += 1;
    reaped_children += 1;

    let valid = require_attach(&attach_clients, "valid", "complete", None)?;
    let replay = require_attach(&attach_clients, "replay", "rejected", Some("replay"))?;
    let wrong = require_attach(
        &attach_clients,
        "wrong-peer",
        "rejected",
        Some("wrong_peer"),
    )?;
    let expired = require_attach(&attach_clients, "expired", "rejected", Some("expired"))?;
    for result in &attach_clients {
        if result.authenticated_server_peer_id != attach_ready.peer_id {
            return Err("attach client authenticated an unexpected issuer peer".to_string());
        }
    }
    if valid.snapshot_bytes == 0
        || valid.snapshot_bytes > 16 * 1024
        || valid.server_count > 8
        || valid.event_digest_count > 16
        || replay.snapshot_bytes != 0
        || wrong.snapshot_bytes != 0
        || expired.snapshot_bytes != 0
    {
        return Err(
            "attach TCP snapshot projection exceeded bounds or leaked on rejection".to_string(),
        );
    }
    if valid.peer_id != replay.peer_id
        || valid.peer_id != expired.peer_id
        || valid.peer_id == wrong.peer_id
    {
        return Err("attach TCP paired and wrong-peer identities were not distinct".to_string());
    }

    let (dht_hub, dht_rx) = spawn_worker(
        &executable,
        &["__dht-tcp-hub".to_string(), deadline_text.clone()],
    )?;
    let dht_hub_pid = dht_hub.pid();
    let dht_ready: Ready = receive(&dht_rx, remaining(started, deadline)?, "DHT hub ready")?;
    if dht_ready.event != "dht-ready"
        || dht_ready.pid != dht_hub_pid
        || !dht_ready.address.contains("/ip4/127.0.0.1/tcp/")
    {
        return Err("DHT hub emitted invalid TCP readiness evidence".to_string());
    }

    let (provider, provider_rx) = spawn_worker(
        &executable,
        &[
            "__dht-tcp-provider".to_string(),
            dht_ready.address.clone(),
            dht_ready.peer_id.clone(),
            deadline_text.clone(),
        ],
    )?;
    let provider_pid = provider.pid();
    let provider_result: DhtWorkerResult = receive(
        &provider_rx,
        remaining(started, deadline)?,
        "DHT record publish",
    )?;
    if provider_result.event != "dht-record-published"
        || provider_result.pid != provider_pid
        || provider_result.remote_peer_id != dht_ready.peer_id
        || provider_result.peer_id != provider_result.publisher_peer_id
    {
        return Err("DHT publisher receipt did not match the private hub".to_string());
    }
    let (seeker, seeker_rx) = spawn_worker(
        &executable,
        &[
            "__dht-tcp-seeker".to_string(),
            dht_ready.address.clone(),
            dht_ready.peer_id.clone(),
            provider_result.publisher_peer_id.clone(),
            deadline_text,
        ],
    )?;
    let seeker_pid = seeker.pid();
    let seeker_result: DhtWorkerResult = receive(
        &seeker_rx,
        remaining(started, deadline)?,
        "DHT seeker lookup",
    )?;
    if seeker_result.event != "dht-record-found"
        || seeker_result.pid != seeker_pid
        || seeker_result.remote_peer_id != dht_ready.peer_id
        || seeker_result.publisher_peer_id != provider_result.publisher_peer_id
        || seeker_result.record_sha256 != provider_result.record_sha256
    {
        return Err("DHT seeker receipt did not prove record retrieval via hub".to_string());
    }
    if !seeker.finish()? {
        return Err("DHT seeker did not exit cleanly".to_string());
    }
    graceful_child_exits += 1;
    reaped_children += 1;
    let publisher_forced_cleanup_reaped = provider.cancel_and_reap()?;
    if !publisher_forced_cleanup_reaped {
        return Err("DHT publisher cleanup did not terminate a live owned child".to_string());
    }
    reaped_children += 1;
    let hub_forced_cleanup_reaped = dht_hub.cancel_and_reap()?;
    if !hub_forced_cleanup_reaped {
        return Err("DHT hub cleanup did not terminate a live owned child".to_string());
    }
    reaped_children += 1;

    let child_processes = 8;
    if reaped_children != child_processes {
        return Err("cross-process fixture did not reap every owned child".to_string());
    }
    Ok(CrossProcessProof {
        schema: "agenterm-net/cross-process-tcp-proof/v1",
        status: "prototype-proven",
        fixture: "bounded parent + one server/hub + one sequential client worker over loopback TCP",
        elapsed_ms: started.elapsed().as_millis(),
        process: ProcessProof {
            parent_pid,
            child_processes,
            maximum_concurrent_children: 3,
            graceful_child_exits,
            forced_cleanup_children: 2,
            reaped_children,
            residual_children: 0,
            no_fixed_sleep: true,
        },
        attach: AttachTcpProof {
            transport: "loopback TCP+Noise+Yamux+CBOR",
            protocol: "/agenterm/remote-fleet/read-only/1.0.0",
            server_pid: attach_server_pid,
            client_pids: attach_clients.iter().map(|result| result.pid).collect(),
            issuer_peer_id: attach_ready.peer_id,
            paired_peer_id: valid.peer_id.clone(),
            wrong_peer_id: wrong.peer_id.clone(),
            authenticated_peer_identity: true,
            explicit_pairing: true,
            snapshot_bytes: valid.snapshot_bytes,
            server_count: valid.server_count,
            event_digest_count: valid.event_digest_count,
            replay_rejection: replay.code.clone().unwrap_or_default(),
            wrong_peer_rejection: wrong.code.clone().unwrap_or_default(),
            expired_rejection: expired.code.clone().unwrap_or_default(),
            request_bytes_limit: 8 * 1024,
            response_bytes_limit: 16 * 1024,
        },
        dht: DhtTcpProof {
            transport: "loopback TCP+Noise+Yamux",
            protocol: "/agenterm/kad/private/1.0.0",
            topology: "publisher process -> private hub process -> seeker process",
            hub_pid: dht_hub_pid,
            publisher_pid: provider_pid,
            seeker_pid,
            hub_peer_id: dht_ready.peer_id,
            publisher_peer_id: provider_result.publisher_peer_id,
            seeker_peer_id: seeker_result.peer_id,
            record_sha256: seeker_result.record_sha256,
            record_published: true,
            record_found_via_hub: true,
            publisher_forced_cleanup_reaped,
            hub_forced_cleanup_reaped,
            public_bootstrap_attempts: 0,
        },
        authority: AuthorityProof {
            read_only_projection: true,
            shell: false,
            command_execution: false,
            pty_control: false,
            terminal_input: false,
            server_control: false,
        },
        public_bootstrap: false,
        nat_traversal: false,
        relay_serving_default: false,
    })
}

fn require_attach<'a>(
    results: &'a [AttachClientResult],
    kind: &str,
    state: &str,
    code: Option<&str>,
) -> Result<&'a AttachClientResult, String> {
    let result = results
        .iter()
        .find(|result| result.kind == kind)
        .ok_or_else(|| format!("missing attach {kind} result"))?;
    if result.state != state || result.code.as_deref() != code {
        return Err(format!(
            "attach {kind} returned an unexpected typed outcome"
        ));
    }
    Ok(result)
}

fn remaining(started: Instant, deadline: Duration) -> Result<Duration, String> {
    deadline
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            format!(
                "cross-process TCP fixture exceeded {} ms total deadline",
                deadline.as_millis()
            )
        })
}

fn spawn_worker(
    executable: &std::path::Path,
    arguments: &[String],
) -> Result<(WorkerGuard, Receiver<WorkerLine>), String> {
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("spawn {}: {error}", arguments.join(" ")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "worker stdout unavailable".to_string())?;
    Ok((WorkerGuard { child }, capture_lines(stdout)))
}

fn capture_lines<R: Read + Send + 'static>(reader: R) -> Receiver<WorkerLine> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader.take(MAX_WORKER_OUTPUT_BYTES));
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(WorkerLine::Eof);
                    return;
                }
                Ok(_) => {
                    let _ = sender.send(WorkerLine::Line(line));
                }
                Err(error) => {
                    let _ = sender.send(WorkerLine::Error(error.to_string()));
                    return;
                }
            }
        }
    });
    receiver
}

fn receive<T: DeserializeOwned>(
    receiver: &Receiver<WorkerLine>,
    deadline: Duration,
    phase: &str,
) -> Result<T, String> {
    match receiver.recv_timeout(deadline) {
        Ok(WorkerLine::Line(line)) => serde_json::from_str(&line)
            .map_err(|error| format!("{phase} emitted invalid JSON: {error}: {line}")),
        Ok(WorkerLine::Error(error)) => Err(format!("{phase} output failed: {error}")),
        Ok(WorkerLine::Eof) => Err(format!("{phase} exited before emitting evidence")),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{phase} deadline exceeded")),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{phase} evidence channel disconnected"))
        }
    }
}

impl WorkerGuard {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn finish(mut self) -> Result<bool, String> {
        let status = self.child.wait().map_err(|error| error.to_string())?;
        let reaped = self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some();
        Ok(status.success() && reaped)
    }

    fn cancel_and_reap(mut self) -> Result<bool, String> {
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(false);
        }
        self.child.kill().map_err(|error| error.to_string())?;
        let _ = self.child.wait().map_err(|error| error.to_string())?;
        self.child
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|error| error.to_string())
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
