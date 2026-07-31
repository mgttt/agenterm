use cid::Cid;
use libp2p::{
    Multiaddr, PeerId, SwarmBuilder,
    futures::StreamExt,
    identity as libp2p_identity, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};

mod attach;
mod identity;
mod mesh;
mod node;
mod store;
use multihash_codetable::{Code, MultihashDigest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DEFAULT_DEADLINE_MS: u64 = 10_000;
const MAX_BLOCK_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHILD_OUTPUT_BYTES: u64 = 64 * 1024;

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
}

#[derive(Serialize)]
struct Envelope<T: Serialize> {
    schema: &'static str,
    request_id: String,
    state: &'static str,
    deadline_ms: u64,
    result: T,
    receipt: Receipt,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    algorithm: &'static str,
    digest: String,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    schema: &'static str,
    request_id: String,
    state: &'static str,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct Capability {
    name: &'static str,
    state: &'static str,
    note: &'static str,
}

#[derive(Serialize)]
struct Capabilities {
    stability: &'static str,
    network_scope: &'static str,
    identity_persistence: bool,
    store_persistence: bool,
    capabilities: Vec<Capability>,
}

#[derive(Serialize)]
struct PeerIdentity {
    peer_id: String,
    persistence: &'static str,
}

#[derive(Deserialize, Serialize)]
struct Ready {
    event: String,
    peer_id: String,
    address: String,
    pid: u32,
}

#[derive(Clone, Deserialize, Serialize)]
struct WorkerResult {
    event: String,
    peer_id: String,
    remote_peer_id: String,
    rtt_us: u128,
    pid: u32,
    resources: ProcessResourceSample,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ProcessResourceSample {
    peak_rss_bytes: Option<u64>,
    thread_count: Option<u32>,
    source: String,
    scope: String,
    limitation: Option<String>,
}

#[derive(Serialize)]
struct BlockEvidence {
    cid: String,
    codec: &'static str,
    bytes: usize,
    round_trip_verified: bool,
    corruption_rejected: bool,
    store_removed: bool,
}

#[derive(Serialize)]
struct ProcessEvidence {
    listener_pid: u32,
    connector_pid: u32,
    listener_peer_id: String,
    connector_peer_id: String,
    address: String,
    handshake: bool,
    bounded_ping: bool,
    child_exit_clean: bool,
    orphan_cleanup_armed: bool,
    forced_cleanup_pid: u32,
    forced_cleanup_reaped: bool,
}

#[derive(Serialize)]
struct ResourceFacts {
    elapsed_ms: u128,
    executable_bytes: Option<u64>,
    child_processes: usize,
    peak_child_rss_bytes: Option<u64>,
    max_observed_child_threads: Option<u32>,
    measurement_complete: bool,
    process_samples: Vec<ProcessResourceSample>,
    max_block_bytes: usize,
    transport: &'static str,
    security: &'static str,
    multiplexer: &'static str,
}

#[derive(Serialize)]
struct SelfTestResult {
    status: &'static str,
    process: ProcessEvidence,
    block: BlockEvidence,
    resources: ResourceFacts,
    exclusions: Vec<&'static str>,
}

struct ChildGuard {
    child: Child,
}

struct TemporaryStore {
    path: PathBuf,
}

impl TemporaryStore {
    fn create() -> Result<Self, String> {
        let path = temporary_store_path();
        fs::create_dir(&path).map_err(|error| format!("create temporary store: {error}"))?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn finish(mut self) -> Result<bool, String> {
        let status = self.child.wait().map_err(|error| error.to_string())?;
        Ok(status.success())
    }

    fn cancel_and_reap(mut self) -> Result<bool, String> {
        let was_running = self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none();
        if !was_running {
            return Ok(false);
        }
        self.child.kill().map_err(|error| error.to_string())?;
        self.child
            .wait()
            .map(|_| true)
            .map_err(|error| error.to_string())
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

enum ChildMessage {
    Line(String),
    IoError(String),
    Eof,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err((request_id, code, message)) => {
            let error = ErrorEnvelope {
                schema: "agenterm-net/error/v1",
                request_id,
                state: "failed",
                code,
                message,
            };
            println!(
                "{}",
                serde_json::to_string(&error).unwrap_or_else(|_| {
                    r#"{"schema":"agenterm-net/error/v1","state":"failed","code":"serialization"}"#
                        .to_string()
                })
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), (String, &'static str, String)> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let request_id = request_id();
    match arguments.as_slice() {
        [command, flag] if command == "capabilities" && flag == "--json" => {
            let result = Capabilities {
                stability: "experimental-n2-m1-foundation",
                network_scope:
                    "explicit private loopback control and memory fixture; no public bootstrap",
                identity_persistence: true,
                store_persistence: true,
                capabilities: vec![
                    Capability {
                        name: "libp2p.tcp-noise-yamux-ping",
                        state: "prototype",
                        note: "two-process bounded local handshake",
                    },
                    Capability {
                        name: "content.cid-v1-raw-sha2-256",
                        state: "prototype",
                        note: "create, parse, and content verification",
                    },
                    Capability {
                        name: "node.explicit-lifecycle",
                        state: "prototype",
                        note: "deadline-bounded start/status/stop with nonce-bound loopback control",
                    },
                    Capability {
                        name: "identity.ephemeral-or-durable-ed25519",
                        state: "prototype",
                        note: "explicit identity mode; durable key uses local state directory",
                    },
                    Capability {
                        name: "content.persistent-verified-block-store",
                        state: "prototype",
                        note: "bounded put/get, pin/unpin, GC, snapshots, and corruption rejection",
                    },
                    Capability {
                        name: "private-mesh.dht",
                        state: "prototype",
                        note: "explicit deterministic private-memory provide/find-provider proof",
                    },
                    Capability {
                        name: "private-mesh.pubsub",
                        state: "prototype",
                        note: "explicit bounded signed-topic delivery proof",
                    },
                    Capability {
                        name: "private-mesh.relay",
                        state: "prototype",
                        note: "explicit circuit-relay-v2 reservation/circuit proof; relay server remains false by default",
                    },
                    Capability {
                        name: "remote-fleet.read-only-attach",
                        state: "prototype",
                        note: "explicit paired read-only memory fixture; no product attach endpoint or control authority",
                    },
                ],
            };
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [command] if command == "peer-id" => {
            let key = libp2p_identity::Keypair::generate_ed25519();
            let result = PeerIdentity {
                peer_id: key.public().to_peer_id().to_string(),
                persistence: "ephemeral",
            };
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [command, flag] if command == "self-test" && flag == "--json" => {
            let result = run_self_test(DEFAULT_DEADLINE_MS)
                .map_err(|message| (request_id.clone(), "self_test_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [command, flag] if command == "mesh-self-test" && flag == "--json" => {
            let result = mesh::prove_private_mesh(Duration::from_millis(DEFAULT_DEADLINE_MS))
                .await
                .map_err(|message| (request_id.clone(), "private_mesh_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [command, flag] if command == "attach-self-test" && flag == "--json" => {
            let result =
                attach::prove_read_only_attach(Duration::from_millis(DEFAULT_DEADLINE_MS))
                    .await
                    .map_err(|message| {
                        (
                            request_id.clone(),
                            "remote_fleet_attach_failed",
                            message,
                        )
                    })?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [group, command, state_flag, state_dir, identity_flag, mode, json]
            if group == "node"
                && command == "start"
                && state_flag == "--state-dir"
                && identity_flag == "--identity"
                && json == "--json" =>
        {
            let mode = identity::IdentityMode::parse(mode)
                .map_err(|message| (request_id.clone(), "invalid_identity", message))?;
            let result = node::start(
                Path::new(state_dir),
                mode,
                Duration::from_millis(DEFAULT_DEADLINE_MS),
            )
            .map_err(|message| (request_id.clone(), "node_start_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [group, command, state_flag, state_dir, json]
            if group == "node"
                && command == "status"
                && state_flag == "--state-dir"
                && json == "--json" =>
        {
            let result = node::status(
                Path::new(state_dir),
                Duration::from_millis(DEFAULT_DEADLINE_MS),
            )
            .map_err(|message| (request_id.clone(), "node_status_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [group, command, state_flag, state_dir, json]
            if group == "node"
                && command == "stop"
                && state_flag == "--state-dir"
                && json == "--json" =>
        {
            let result = node::stop(
                Path::new(state_dir),
                Duration::from_millis(DEFAULT_DEADLINE_MS),
            )
            .map_err(|message| (request_id.clone(), "node_stop_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [group, command, store_flag, store_dir, input_flag, input, pin, json]
            if group == "store"
                && command == "put"
                && store_flag == "--store"
                && input_flag == "--input"
                && pin == "--pin"
                && json == "--json" =>
        {
            run_store_put(&request_id, store_dir, input, true)
        }
        [group, command, store_flag, store_dir, input_flag, input, json]
            if group == "store"
                && command == "put"
                && store_flag == "--store"
                && input_flag == "--input"
                && json == "--json" =>
        {
            run_store_put(&request_id, store_dir, input, false)
        }
        [group, command, store_flag, store_dir, cid_flag, cid, output_flag, output, json]
            if group == "store"
                && command == "get"
                && store_flag == "--store"
                && cid_flag == "--cid"
                && output_flag == "--output"
                && json == "--json" =>
        {
            let store = store::PersistentStore::open(store_dir)
                .map_err(|message| (request_id.clone(), "store_open_failed", message))?;
            let cid: Cid = cid
                .parse::<Cid>()
                .map_err(|error| (request_id.clone(), "invalid_cid", error.to_string()))?;
            let result = store
                .get_to(&cid, output)
                .map_err(|message| (request_id.clone(), "store_get_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [group, command, store_flag, store_dir, cid_flag, cid, json]
            if group == "store"
                && (command == "pin" || command == "unpin")
                && store_flag == "--store"
                && cid_flag == "--cid"
                && json == "--json" =>
        {
            let store = store::PersistentStore::open(store_dir)
                .map_err(|message| (request_id.clone(), "store_open_failed", message))?;
            let cid: Cid = cid
                .parse::<Cid>()
                .map_err(|error| (request_id.clone(), "invalid_cid", error.to_string()))?;
            let result = store
                .set_pin(&cid, command == "pin")
                .map_err(|message| (request_id.clone(), "store_pin_failed", message))?;
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [group, command, store_flag, store_dir, json]
            if group == "store"
                && (command == "gc" || command == "status")
                && store_flag == "--store"
                && json == "--json" =>
        {
            let store = store::PersistentStore::open(store_dir)
                .map_err(|message| (request_id.clone(), "store_open_failed", message))?;
            if command == "gc" {
                let result = store
                    .gc()
                    .map_err(|message| (request_id.clone(), "store_gc_failed", message))?;
                print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            } else {
                let result = store
                    .snapshot()
                    .map_err(|message| (request_id.clone(), "store_status_failed", message))?;
                print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            }
            Ok(())
        }
        [command, state_dir, mode, ready_address, ready_token] if command == "__node-daemon" => {
            let mode = identity::IdentityMode::parse(mode)
                .map_err(|message| (request_id.clone(), "invalid_identity", message))?;
            node::daemon(Path::new(state_dir), mode, ready_address, ready_token)
                .map_err(|message| (request_id, "node_daemon_failed", message))
        }
        [command, deadline] if command == "__listen" => {
            let deadline = parse_deadline(deadline)
                .map_err(|message| (request_id.clone(), "invalid_deadline", message))?;
            run_listener(deadline)
                .await
                .map_err(|message| (request_id, "listener_failed", message))
        }
        [command, address, peer, deadline] if command == "__connect" => {
            let deadline = parse_deadline(deadline)
                .map_err(|message| (request_id.clone(), "invalid_deadline", message))?;
            run_connector(address, peer, deadline)
                .await
                .map_err(|message| (request_id, "connector_failed", message))
        }
        _ => Err((
            request_id,
            "usage",
            "usage: agenterm-net capabilities --json | peer-id | self-test --json | mesh-self-test --json | attach-self-test --json | node start|status|stop ... --json | store put|get|pin|unpin|gc|status ... --json".to_string(),
        )),
    }
}

fn parse_deadline(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| (100..=60_000).contains(value))
        .ok_or_else(|| "deadline must be between 100 and 60000 milliseconds".to_string())
}

fn run_store_put(
    request_id: &str,
    store_dir: &str,
    input: &str,
    pin: bool,
) -> Result<(), (String, &'static str, String)> {
    let input_bytes = fs::metadata(input)
        .map_err(|message| {
            (
                request_id.to_string(),
                "input_read_failed",
                message.to_string(),
            )
        })?
        .len();
    if input_bytes > store::MAX_BLOCK_BYTES as u64 {
        return Err((
            request_id.to_string(),
            "store_put_failed",
            format!(
                "input exceeds {} byte per-block budget",
                store::MAX_BLOCK_BYTES
            ),
        ));
    }
    let bytes = fs::read(input).map_err(|message| {
        (
            request_id.to_string(),
            "input_read_failed",
            message.to_string(),
        )
    })?;
    let store = store::PersistentStore::open(store_dir)
        .map_err(|message| (request_id.to_string(), "store_open_failed", message))?;
    let result = store
        .put(&bytes, pin)
        .map_err(|message| (request_id.to_string(), "store_put_failed", message))?;
    print_envelope(request_id, DEFAULT_DEADLINE_MS, result);
    Ok(())
}

fn print_envelope<T: Serialize>(request_id: &str, deadline_ms: u64, result: T) {
    let result_json = serde_json::to_vec(&result).unwrap_or_default();
    let receipt = Receipt {
        schema: "agenterm-net/receipt/v1",
        algorithm: "sha2-256",
        digest: hex_digest(&result_json),
    };
    let envelope = Envelope {
        schema: "agenterm-net/result/v1",
        request_id: request_id.to_string(),
        state: "complete",
        deadline_ms,
        result,
        receipt,
    };
    println!(
        "{}",
        serde_json::to_string(&envelope).expect("JSON envelope")
    );
}

fn request_id() -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("net-{}-{time:x}", std::process::id())
}

fn hex_digest(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn make_swarm() -> Result<libp2p::Swarm<Behaviour>, String> {
    let builder = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|error| error.to_string())?
        .with_behaviour(|_| Behaviour {
            ping: ping::Behaviour::new(ping::Config::new()),
        })
        .map_err(|error| error.to_string())?;
    Ok(builder.build())
}

async fn run_listener(deadline_ms: u64) -> Result<(), String> {
    let mut swarm = make_swarm()?;
    let peer_id = *swarm.local_peer_id();
    swarm
        .listen_on(
            "/ip4/127.0.0.1/tcp/0"
                .parse()
                .map_err(|error: libp2p::multiaddr::Error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let deadline = tokio::time::sleep(Duration::from_millis(deadline_ms));
    tokio::pin!(deadline);
    let mut announced = false;
    loop {
        tokio::select! {
            _ = &mut deadline => return Err("listener deadline exceeded".to_string()),
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } if !announced => {
                    let ready = Ready {
                        event: "ready".to_string(),
                        peer_id: peer_id.to_string(),
                        address: address.to_string(),
                        pid: std::process::id(),
                    };
                    println!("{}", serde_json::to_string(&ready).map_err(|e| e.to_string())?);
                    announced = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::Ping(event)) if event.result.is_ok() => {
                    let result = WorkerResult {
                        event: "ping".to_string(),
                        peer_id: peer_id.to_string(),
                        remote_peer_id: event.peer.to_string(),
                        rtt_us: event.result.unwrap_or_default().as_micros(),
                        pid: std::process::id(),
                        resources: sample_process_resources(),
                    };
                    println!("{}", serde_json::to_string(&result).map_err(|e| e.to_string())?);
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

async fn run_connector(address: &str, expected_peer: &str, deadline_ms: u64) -> Result<(), String> {
    let mut swarm = make_swarm()?;
    let peer_id = *swarm.local_peer_id();
    let expected: PeerId = expected_peer
        .parse()
        .map_err(|error| format!("invalid peer id: {error}"))?;
    let mut address: Multiaddr = address
        .parse()
        .map_err(|error| format!("invalid address: {error}"))?;
    address.push(libp2p::multiaddr::Protocol::P2p(expected));
    swarm.dial(address).map_err(|error| error.to_string())?;
    let deadline = tokio::time::sleep(Duration::from_millis(deadline_ms));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Err("connector deadline exceeded".to_string()),
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(BehaviourEvent::Ping(event))
                    if event.peer == expected && event.result.is_ok() => {
                    let result = WorkerResult {
                        event: "ping".to_string(),
                        peer_id: peer_id.to_string(),
                        remote_peer_id: event.peer.to_string(),
                        rtt_us: event.result.unwrap_or_default().as_micros(),
                        pid: std::process::id(),
                        resources: sample_process_resources(),
                    };
                    println!("{}", serde_json::to_string(&result).map_err(|e| e.to_string())?);
                    return Ok(());
                }
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    return Err(format!("peer connection failed: {error}"));
                }
                _ => {}
            }
        }
    }
}

fn run_self_test(deadline_ms: u64) -> Result<SelfTestResult, String> {
    let started = Instant::now();
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let deadline = Duration::from_millis(deadline_ms);
    let listener = Command::new(&executable)
        .arg("__listen")
        .arg(deadline_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("spawn listener: {error}"))?;
    let mut listener = ChildGuard::new(listener);
    let listener_pid = listener.pid();
    let listener_rx = capture_lines(
        listener
            .child
            .stdout
            .take()
            .ok_or_else(|| "listener stdout unavailable".to_string())?,
    );
    let ready: Ready = receive_json(&listener_rx, deadline, "listener ready")?;
    if ready.event != "ready" || ready.pid != listener_pid {
        return Err("listener ready receipt did not match child".to_string());
    }

    let connector = Command::new(&executable)
        .arg("__connect")
        .arg(&ready.address)
        .arg(&ready.peer_id)
        .arg(deadline_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("spawn connector: {error}"))?;
    let mut connector = ChildGuard::new(connector);
    let connector_pid = connector.pid();
    let connector_rx = capture_lines(
        connector
            .child
            .stdout
            .take()
            .ok_or_else(|| "connector stdout unavailable".to_string())?,
    );
    let connector_result: WorkerResult = receive_json(&connector_rx, deadline, "connector ping")?;
    let listener_result: WorkerResult = receive_json(&listener_rx, deadline, "listener ping")?;
    if connector_result.event != "ping"
        || listener_result.event != "ping"
        || connector_result.remote_peer_id != ready.peer_id
        || listener_result.remote_peer_id != connector_result.peer_id
    {
        return Err("cross-process identity or ping receipt mismatch".to_string());
    }
    let process_samples = vec![
        listener_result.resources.clone(),
        connector_result.resources.clone(),
    ];
    let peak_child_rss_bytes = process_samples
        .iter()
        .filter_map(|sample| sample.peak_rss_bytes)
        .max();
    let max_observed_child_threads = process_samples
        .iter()
        .filter_map(|sample| sample.thread_count)
        .max();
    let measurement_complete = process_samples
        .iter()
        .all(|sample| sample.peak_rss_bytes.is_some() && sample.thread_count.is_some());
    let connector_clean = connector.finish()?;
    let listener_clean = listener.finish()?;
    let (forced_cleanup_pid, forced_cleanup_reaped) =
        forced_cleanup_self_test(&executable, deadline_ms, deadline)?;
    let block = block_self_test()?;
    let executable_bytes = fs::metadata(&executable)
        .ok()
        .map(|metadata| metadata.len());
    Ok(SelfTestResult {
        status: "research-pass",
        process: ProcessEvidence {
            listener_pid,
            connector_pid,
            listener_peer_id: ready.peer_id,
            connector_peer_id: connector_result.peer_id,
            address: ready.address,
            handshake: true,
            bounded_ping: true,
            child_exit_clean: listener_clean && connector_clean,
            orphan_cleanup_armed: true,
            forced_cleanup_pid,
            forced_cleanup_reaped,
        },
        block,
        resources: ResourceFacts {
            elapsed_ms: started.elapsed().as_millis(),
            executable_bytes,
            child_processes: 3,
            peak_child_rss_bytes,
            max_observed_child_threads,
            measurement_complete,
            process_samples,
            max_block_bytes: MAX_BLOCK_BYTES,
            transport: "libp2p TCP loopback",
            security: "Noise",
            multiplexer: "Yamux",
        },
        exclusions: vec![
            "not linked into AgenTerm GUI or server",
            "persistent identity and store remain isolated experimental node commands",
            "no cross-process TCP DHT, relay, pubsub, reconnect, or cross-platform load evidence",
            "no public listener, bootstrap, NAT traversal, or gateway",
            "no Remote Fleet attach, terminal input, command, PTY, or server authority",
            "not a stable or release-packaged capability",
        ],
    })
}

fn forced_cleanup_self_test(
    executable: &Path,
    deadline_ms: u64,
    deadline: Duration,
) -> Result<(u32, bool), String> {
    let child = Command::new(executable)
        .arg("__listen")
        .arg(deadline_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("spawn cleanup probe: {error}"))?;
    let mut child = ChildGuard::new(child);
    let pid = child.pid();
    let receiver = capture_lines(
        child
            .child
            .stdout
            .take()
            .ok_or_else(|| "cleanup probe stdout unavailable".to_string())?,
    );
    let ready: Ready = receive_json(&receiver, deadline, "cleanup probe ready")?;
    if ready.event != "ready" || ready.pid != pid {
        return Err("cleanup probe ready receipt did not match child".to_string());
    }
    Ok((pid, child.cancel_and_reap()?))
}

#[cfg(target_os = "linux")]
pub(crate) fn sample_process_resources() -> ProcessResourceSample {
    let status = fs::read_to_string("/proc/self/status");
    let mut peak_rss_bytes = None;
    let mut thread_count = None;
    let mut limitation = None;
    match status {
        Ok(status) => {
            for line in status.lines() {
                if let Some(value) = line.strip_prefix("VmHWM:") {
                    peak_rss_bytes = value
                        .split_whitespace()
                        .next()
                        .and_then(|value| value.parse::<u64>().ok())
                        .and_then(|kib| kib.checked_mul(1024));
                } else if let Some(value) = line.strip_prefix("Threads:") {
                    thread_count = value.trim().parse::<u32>().ok();
                }
            }
            if peak_rss_bytes.is_none() || thread_count.is_none() {
                limitation = Some("/proc/self/status omitted VmHWM or Threads".to_string());
            }
        }
        Err(error) => limitation = Some(format!("read /proc/self/status: {error}")),
    }
    ProcessResourceSample {
        peak_rss_bytes,
        thread_count,
        source: "linux:/proc/self/status".to_string(),
        scope: "per worker at successful ping; RSS is OS high-water mark, threads are observed"
            .to_string(),
        limitation,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn sample_process_resources() -> ProcessResourceSample {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage initializes the provided rusage structure for the
    // current process when it returns zero.
    let peak_rss_bytes = if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
        // macOS reports ru_maxrss in bytes.
        Some(unsafe { usage.assume_init() }.ru_maxrss as u64)
    } else {
        None
    };
    let thread_count = Command::new("ps")
        .args(["-o", "thcount=", "-p", &std::process::id().to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse::<u32>().ok());
    let limitation = (peak_rss_bytes.is_none() || thread_count.is_none())
        .then(|| "getrusage(RUSAGE_SELF) or macOS ps thcount was unavailable".to_string());
    ProcessResourceSample {
        peak_rss_bytes,
        thread_count,
        source: "macos:getrusage+ps-thcount".to_string(),
        scope: "per worker at successful ping; RSS is OS high-water mark, threads are observed"
            .to_string(),
        limitation,
    }
}

#[cfg(windows)]
pub(crate) fn sample_process_resources() -> ProcessResourceSample {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::GetCurrentProcess,
        },
    };

    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: counters is initialized with the documented size and the
    // pseudo-handle is valid for querying the current process.
    let peak_rss_bytes = (unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    } != 0)
        .then_some(counters.PeakWorkingSetSize as u64);

    let pid = std::process::id();
    let mut thread_count = None;
    // SAFETY: the returned snapshot is checked and closed exactly once; the
    // THREADENTRY32 size is initialized as required by Toolhelp.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot != INVALID_HANDLE_VALUE {
        let mut entry: THREADENTRY32 = unsafe { zeroed() };
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut count = 0_u32;
        if unsafe { Thread32First(snapshot, &mut entry) } != 0 {
            loop {
                if entry.th32OwnerProcessID == pid {
                    count = count.saturating_add(1);
                }
                if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
                    break;
                }
            }
            thread_count = Some(count);
        }
        unsafe {
            CloseHandle(snapshot);
        }
    }
    let limitation = (peak_rss_bytes.is_none() || thread_count.is_none())
        .then(|| "GetProcessMemoryInfo or Toolhelp thread snapshot was unavailable".to_string());
    ProcessResourceSample {
        peak_rss_bytes,
        thread_count,
        source: "windows:ProcessMemoryCounters+Toolhelp".to_string(),
        scope: "per worker at successful ping; RSS is OS high-water mark, threads are observed"
            .to_string(),
        limitation,
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn sample_process_resources() -> ProcessResourceSample {
    ProcessResourceSample {
        peak_rss_bytes: None,
        thread_count: None,
        source: "unsupported-platform".to_string(),
        scope: "per worker at successful ping".to_string(),
        limitation: Some("resource sampler is not implemented on this target".to_string()),
    }
}

fn capture_lines<R: io::Read + Send + 'static>(reader: R) -> Receiver<ChildMessage> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader.take(MAX_CHILD_OUTPUT_BYTES));
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(ChildMessage::Eof);
                    break;
                }
                Ok(_) => {
                    let _ = sender.send(ChildMessage::Line(line));
                }
                Err(error) => {
                    let _ = sender.send(ChildMessage::IoError(error.to_string()));
                    break;
                }
            }
        }
    });
    receiver
}

fn receive_json<T: serde::de::DeserializeOwned>(
    receiver: &Receiver<ChildMessage>,
    deadline: Duration,
    phase: &str,
) -> Result<T, String> {
    match receiver.recv_timeout(deadline) {
        Ok(ChildMessage::Line(line)) => serde_json::from_str(&line)
            .map_err(|error| format!("{phase} emitted invalid JSON: {error}: {line}")),
        Ok(ChildMessage::IoError(error)) => Err(format!("{phase} read failed: {error}")),
        Ok(ChildMessage::Eof) => Err(format!("{phase} ended before emitting evidence")),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{phase} deadline exceeded")),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{phase} evidence channel disconnected"))
        }
    }
}

fn block_self_test() -> Result<BlockEvidence, String> {
    let payload = b"AgenTerm content-addressed research block";
    let cid = cid_for(payload)?;
    let parsed: Cid = cid
        .to_string()
        .parse()
        .map_err(|error| format!("parse generated CID: {error}"))?;
    if parsed != cid {
        return Err("CID v1 parse round trip changed identity".to_string());
    }
    let store = TemporaryStore::create()?;
    let path = put_block(&store.path, &cid, payload)?;
    let round_trip_verified = get_block(&store.path, &cid)? == payload;
    fs::write(&path, b"corrupt").map_err(|error| format!("corrupt test block: {error}"))?;
    let corruption_rejected = get_block(&store.path, &cid).is_err();
    let store_path = store.path.clone();
    drop(store);
    Ok(BlockEvidence {
        cid: cid.to_string(),
        codec: "raw",
        bytes: payload.len(),
        round_trip_verified,
        corruption_rejected,
        store_removed: !store_path.exists(),
    })
}

fn cid_for(bytes: &[u8]) -> Result<Cid, String> {
    if bytes.len() > MAX_BLOCK_BYTES {
        return Err(format!("block exceeds {MAX_BLOCK_BYTES} byte bound"));
    }
    Ok(Cid::new_v1(0x55, Code::Sha2_256.digest(bytes)))
}

fn put_block(store: &Path, cid: &Cid, bytes: &[u8]) -> Result<PathBuf, String> {
    if cid_for(bytes)? != *cid {
        return Err("put content does not match requested CID".to_string());
    }
    let path = store.join(cid.to_string());
    fs::write(&path, bytes).map_err(|error| format!("write block: {error}"))?;
    Ok(path)
}

fn get_block(store: &Path, cid: &Cid) -> Result<Vec<u8>, String> {
    let bytes = fs::read(store.join(cid.to_string())).map_err(|error| error.to_string())?;
    if cid_for(&bytes)? != *cid {
        return Err("stored block failed CID verification".to_string());
    }
    Ok(bytes)
}

fn temporary_store_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("agenterm-net-{}-{nonce:x}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_is_v1_raw_and_verifies_content() {
        let payload = b"hello";
        let cid = cid_for(payload).unwrap();
        assert_eq!(cid.version(), cid::Version::V1);
        assert_eq!(cid.codec(), 0x55);
        assert_eq!(cid_for(payload).unwrap(), cid);
        assert_ne!(cid_for(b"world").unwrap(), cid);
    }

    #[test]
    fn block_size_is_bounded() {
        let oversized = vec![0; MAX_BLOCK_BYTES + 1];
        assert!(cid_for(&oversized).unwrap_err().contains("exceeds"));
    }

    #[test]
    fn temporary_store_is_removed_on_scope_exit() {
        let path = {
            let store = TemporaryStore::create().unwrap();
            fs::write(store.path.join("partial"), b"partial").unwrap();
            store.path.clone()
        };
        assert!(!path.exists());
    }

    #[test]
    fn child_guard_reaps_completed_process() {
        let child = if cfg!(windows) {
            Command::new("cmd")
                .args(["/c", "exit", "0"])
                .spawn()
                .unwrap()
        } else {
            Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap()
        };
        assert!(ChildGuard::new(child).finish().unwrap());
    }
}
