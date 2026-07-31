use cid::Cid;
use libp2p::{
    Multiaddr, PeerId, SwarmBuilder,
    futures::StreamExt,
    identity, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
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
    receipt: String,
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

#[derive(Deserialize, Serialize)]
struct WorkerResult {
    event: String,
    peer_id: String,
    remote_peer_id: String,
    rtt_us: u128,
    pid: u32,
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
}

#[derive(Serialize)]
struct ResourceFacts {
    elapsed_ms: u128,
    executable_bytes: Option<u64>,
    child_processes: usize,
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
                stability: "research-only",
                network_scope: "explicit IPv4 loopback only",
                identity_persistence: false,
                store_persistence: false,
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
                        name: "content.temporary-block-store",
                        state: "prototype",
                        note: "bounded put/get with corruption rejection",
                    },
                ],
            };
            print_envelope(&request_id, DEFAULT_DEADLINE_MS, result);
            Ok(())
        }
        [command] if command == "peer-id" => {
            let key = identity::Keypair::generate_ed25519();
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
            "usage: agenterm-net capabilities --json | peer-id | self-test --json".to_string(),
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

fn print_envelope<T: Serialize>(request_id: &str, deadline_ms: u64, result: T) {
    let result_json = serde_json::to_vec(&result).unwrap_or_default();
    let receipt = hex_digest(&result_json);
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
    let connector_clean = connector.finish()?;
    let listener_clean = listener.finish()?;
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
        },
        block,
        resources: ResourceFacts {
            elapsed_ms: started.elapsed().as_millis(),
            executable_bytes,
            child_processes: 2,
            max_block_bytes: MAX_BLOCK_BYTES,
            transport: "libp2p TCP loopback",
            security: "Noise",
            multiplexer: "Yamux",
        },
        exclusions: vec![
            "not linked into AgenTerm GUI or server",
            "no persistent identity or block store",
            "no public listener, DHT, relay, pubsub, NAT traversal, or gateway",
            "not a stable or release-packaged capability",
        ],
    })
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
