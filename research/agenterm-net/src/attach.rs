use libp2p::{
    Multiaddr, PeerId, StreamProtocol, Transport,
    core::{
        multiaddr::Protocol,
        muxing::StreamMuxerBox,
        transport::{Boxed, MemoryTransport},
        upgrade,
    },
    futures::{AsyncRead, AsyncWrite, StreamExt},
    identity::{Keypair, PublicKey},
    noise,
    request_response::{self, ProtocolSupport, cbor},
    swarm::{Config as SwarmConfig, Swarm, SwarmEvent},
    tcp, yamux,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const ATTACH_PROTOCOL: &str = "/agenterm/remote-fleet/read-only/1.0.0";
const PAIRING_SCOPE: &str = "fleet.snapshot.read/v1";
const LOGICAL_NOW_MS: u64 = 1_000_000;
const MAX_REQUEST_BYTES: u64 = 8 * 1024;
const MAX_RESPONSE_BYTES: u64 = 16 * 1024;
const MAX_CONCURRENT_STREAMS: usize = 8;
const MAX_SERVERS: u16 = 8;
const MAX_EVENT_DIGESTS: u16 = 16;

type AttachBehaviour = cbor::Behaviour<AttachRequest, AttachResponse>;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PairingInvite {
    schema: String,
    issuer_peer_id: String,
    target_peer_id: String,
    expires_unix_ms: u64,
    nonce: String,
    scope: String,
    max_snapshot_bytes: u64,
    max_event_digests: u16,
    issuer_signature: Vec<u8>,
}

#[derive(Serialize)]
struct UnsignedInvite<'a> {
    schema: &'a str,
    issuer_peer_id: &'a str,
    target_peer_id: &'a str,
    expires_unix_ms: u64,
    nonce: &'a str,
    scope: &'a str,
    max_snapshot_bytes: u64,
    max_event_digests: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AttachRequest {
    schema: String,
    request_id: String,
    requester_peer_id: String,
    invite: PairingInvite,
    requester_signature: Vec<u8>,
}

#[derive(Serialize)]
struct RequestBinding<'a> {
    schema: &'a str,
    request_id: &'a str,
    requester_peer_id: &'a str,
    invite_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AttachResponse {
    schema: String,
    request_id: String,
    state: String,
    code: Option<String>,
    snapshot: Option<FleetSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FleetSnapshot {
    schema: String,
    generated_unix_ms: u64,
    server_count: u16,
    servers: Vec<RemoteServerSummary>,
    event_digests: Vec<EventDigest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RemoteServerSummary {
    server_id: String,
    state: String,
    window_count: u16,
    tab_count: u16,
    latest_event_sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EventDigest {
    server_id: String,
    event_count: u64,
    latest_sequence: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub struct AttachProof {
    pub schema: &'static str,
    pub status: &'static str,
    pub fixture: &'static str,
    pub elapsed_ms: u128,
    pub protocol: &'static str,
    pub transport: &'static str,
    pub authenticated_peer_identity: bool,
    pub issuer_peer_id: String,
    pub paired_peer_id: String,
    pub wrong_peer_id: String,
    pub accepted: AcceptedProof,
    pub rejections: RejectionProof,
    pub limits: LimitProof,
    pub authority: AuthorityProof,
    pub public_bootstrap: bool,
    pub nat_traversal: bool,
    pub relay_serving_default: bool,
}

#[derive(Debug, Serialize)]
pub struct AcceptedProof {
    pub state: String,
    pub snapshot_bytes: usize,
    pub server_count: u16,
    pub event_digest_count: usize,
    pub event_digest_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct RejectionProof {
    pub replay: String,
    pub wrong_peer: String,
    pub expired: String,
}

#[derive(Debug, Serialize)]
pub struct LimitProof {
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub max_concurrent_streams: usize,
    pub max_servers: u16,
    pub max_event_digests: u16,
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

struct AttachServer {
    key: Keypair,
    paired_public_key: PublicKey,
    paired_peer_id: PeerId,
    used_nonces: HashSet<String>,
}

impl AttachServer {
    fn handle(&mut self, authenticated_peer: PeerId, request: AttachRequest) -> AttachResponse {
        let reject = |code: &str| AttachResponse {
            schema: "agenterm-net/remote-fleet-response/v1".to_string(),
            request_id: request.request_id.clone(),
            state: "rejected".to_string(),
            code: Some(code.to_string()),
            snapshot: None,
        };

        if request.schema != "agenterm-net/remote-fleet-request/v1"
            || request.invite.schema != "agenterm-net/pairing-invite/v1"
        {
            return reject("invalid_schema");
        }
        let authenticated_text = authenticated_peer.to_string();
        if authenticated_peer != self.paired_peer_id
            || request.requester_peer_id != authenticated_text
            || request.invite.target_peer_id != authenticated_text
        {
            return reject("wrong_peer");
        }
        if request.invite.issuer_peer_id != self.key.public().to_peer_id().to_string()
            || request.invite.scope != PAIRING_SCOPE
        {
            return reject("invalid_invite");
        }
        if !verify_invite(&self.key.public(), &request.invite) {
            return reject("invalid_invite_signature");
        }
        if request.invite.expires_unix_ms <= LOGICAL_NOW_MS {
            return reject("expired");
        }
        if self.used_nonces.contains(&request.invite.nonce) {
            return reject("replay");
        }
        if request.invite.max_snapshot_bytes > MAX_RESPONSE_BYTES
            || request.invite.max_event_digests > MAX_EVENT_DIGESTS
        {
            return reject("budget_exceeded");
        }
        if !verify_request(&self.paired_public_key, &request) {
            return reject("invalid_request_signature");
        }

        let snapshot = fixture_snapshot();
        let encoded = serde_json::to_vec(&snapshot).unwrap_or_default();
        if encoded.len() as u64 > request.invite.max_snapshot_bytes
            || snapshot.servers.len() > MAX_SERVERS as usize
            || snapshot.event_digests.len() > request.invite.max_event_digests as usize
        {
            return reject("budget_exceeded");
        }
        self.used_nonces.insert(request.invite.nonce.clone());
        AttachResponse {
            schema: "agenterm-net/remote-fleet-response/v1".to_string(),
            request_id: request.request_id,
            state: "complete".to_string(),
            code: None,
            snapshot: Some(snapshot),
        }
    }
}

#[derive(Serialize)]
struct TcpReady {
    event: &'static str,
    peer_id: String,
    address: String,
    pid: u32,
}

#[derive(Serialize)]
struct TcpServerResult {
    event: &'static str,
    pid: u32,
    handled_requests: usize,
    accepted: bool,
    replay_rejected: bool,
    wrong_peer_rejected: bool,
    expired_rejected: bool,
}

#[derive(Serialize)]
struct TcpClientResult {
    event: &'static str,
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

pub async fn run_tcp_server(deadline: Duration) -> Result<(), String> {
    tokio::time::timeout(deadline, run_tcp_server_inner())
        .await
        .map_err(|_| {
            format!(
                "attach TCP server exceeded {} ms deadline",
                deadline.as_millis()
            )
        })?
}

async fn run_tcp_server_inner() -> Result<(), String> {
    let issuer_key = deterministic_key(51)?;
    let paired_key = deterministic_key(52)?;
    let issuer_peer = issuer_key.public().to_peer_id();
    let paired_peer = paired_key.public().to_peer_id();
    let mut swarm = attach_tcp_swarm(&issuer_key)?;
    swarm
        .listen_on(
            "/ip4/127.0.0.1/tcp/0"
                .parse()
                .map_err(|error: libp2p::multiaddr::Error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let mut state = AttachServer {
        key: issuer_key,
        paired_public_key: paired_key.public(),
        paired_peer_id: paired_peer,
        used_nonces: HashSet::new(),
    };
    let mut announced = false;
    let mut handled = 0_usize;
    let mut flushed = 0_usize;
    let mut final_connection = None;
    let mut accepted = false;
    let mut replay_rejected = false;
    let mut wrong_peer_rejected = false;
    let mut expired_rejected = false;
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } if !announced => {
                print_worker(&TcpReady {
                    event: "attach-ready",
                    peer_id: issuer_peer.to_string(),
                    address: address.to_string(),
                    pid: std::process::id(),
                })?;
                announced = true;
            }
            SwarmEvent::Behaviour(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request, channel, ..
                    },
                ..
            }) => {
                let response = state.handle(peer, request);
                handled += 1;
                match (response.state.as_str(), response.code.as_deref()) {
                    ("complete", None) => accepted = true,
                    ("rejected", Some("replay")) => replay_rejected = true,
                    ("rejected", Some("wrong_peer")) => wrong_peer_rejected = true,
                    ("rejected", Some("expired")) => expired_rejected = true,
                    _ => {}
                }
                swarm
                    .behaviour_mut()
                    .send_response(channel, response)
                    .map_err(|_| "attach TCP response channel closed".to_string())?;
            }
            SwarmEvent::Behaviour(request_response::Event::ResponseSent {
                connection_id, ..
            }) => {
                flushed += 1;
                if flushed == 4 {
                    if handled != 4
                        || !accepted
                        || !replay_rejected
                        || !wrong_peer_rejected
                        || !expired_rejected
                    {
                        return Err(
                            "attach TCP server did not observe all required outcomes".to_string()
                        );
                    }
                    final_connection = Some(connection_id);
                }
            }
            SwarmEvent::ConnectionClosed { connection_id, .. }
                if final_connection == Some(connection_id) =>
            {
                print_worker(&TcpServerResult {
                    event: "attach-complete",
                    pid: std::process::id(),
                    handled_requests: handled,
                    accepted,
                    replay_rejected,
                    wrong_peer_rejected,
                    expired_rejected,
                })?;
                return Ok(());
            }
            SwarmEvent::Behaviour(request_response::Event::InboundFailure { error, .. }) => {
                return Err(format!("attach TCP inbound failed: {error}"));
            }
            SwarmEvent::ListenerError { error, .. } => {
                return Err(format!("attach TCP listener failed: {error}"));
            }
            _ => {}
        }
    }
}

pub async fn run_tcp_client(
    address: &str,
    expected_issuer: &str,
    kind: &str,
    deadline: Duration,
) -> Result<(), String> {
    tokio::time::timeout(
        deadline,
        run_tcp_client_inner(address, expected_issuer, kind),
    )
    .await
    .map_err(|_| {
        format!(
            "attach TCP client exceeded {} ms deadline",
            deadline.as_millis()
        )
    })?
}

async fn run_tcp_client_inner(
    address: &str,
    expected_issuer: &str,
    kind: &str,
) -> Result<(), String> {
    let issuer_key = deterministic_key(51)?;
    let paired_key = deterministic_key(52)?;
    let wrong_key = deterministic_key(53)?;
    let issuer_peer = issuer_key.public().to_peer_id();
    if issuer_peer.to_string() != expected_issuer {
        return Err("attach fixture issuer PeerId mismatch".to_string());
    }
    let paired_peer = paired_key.public().to_peer_id();
    let (client_key, invite, request_id) = match kind {
        "valid" | "replay" => (
            &paired_key,
            signed_invite(&issuer_key, paired_peer, LOGICAL_NOW_MS + 10_000, "valid")?,
            "attach-valid",
        ),
        "wrong-peer" => (
            &wrong_key,
            signed_invite(&issuer_key, paired_peer, LOGICAL_NOW_MS + 10_000, "valid")?,
            "attach-wrong-peer",
        ),
        "expired" => (
            &paired_key,
            signed_invite(&issuer_key, paired_peer, LOGICAL_NOW_MS, "expired")?,
            "attach-expired",
        ),
        _ => return Err(format!("unknown attach TCP fixture request kind: {kind}")),
    };
    let request = signed_request(client_key, invite, request_id)?;
    let client_peer = client_key.public().to_peer_id();
    let address: Multiaddr = address
        .parse()
        .map_err(|error| format!("invalid attach TCP address: {error}"))?;
    let mut swarm = attach_tcp_swarm(client_key)?;
    let outbound =
        swarm
            .behaviour_mut()
            .send_request_with_addresses(&issuer_peer, request, vec![address]);
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Response {
                        request_id,
                        response,
                    },
                ..
            }) if request_id == outbound => {
                if peer != issuer_peer {
                    return Err("attach response came from unexpected peer".to_string());
                }
                let (snapshot_bytes, server_count, event_digest_count) = response
                    .snapshot
                    .as_ref()
                    .map(|snapshot| {
                        (
                            serde_json::to_vec(snapshot).unwrap_or_default().len(),
                            snapshot.server_count,
                            snapshot.event_digests.len(),
                        )
                    })
                    .unwrap_or_default();
                print_worker(&TcpClientResult {
                    event: "attach-response",
                    kind: kind.to_string(),
                    pid: std::process::id(),
                    peer_id: client_peer.to_string(),
                    authenticated_server_peer_id: peer.to_string(),
                    state: response.state,
                    code: response.code,
                    snapshot_bytes,
                    server_count,
                    event_digest_count,
                })?;
                return Ok(());
            }
            SwarmEvent::Behaviour(request_response::Event::OutboundFailure {
                request_id,
                error,
                ..
            }) if request_id == outbound => {
                return Err(format!("attach TCP request failed: {error}"));
            }
            SwarmEvent::OutgoingConnectionError { error, .. } => {
                return Err(format!("attach TCP connection failed: {error}"));
            }
            _ => {}
        }
    }
}

pub async fn prove_read_only_attach(deadline: Duration) -> Result<AttachProof, String> {
    let started = Instant::now();
    tokio::time::timeout(deadline, prove_fixture())
        .await
        .map_err(|_| {
            format!(
                "Remote Fleet attach proof exceeded {} ms deadline",
                deadline.as_millis()
            )
        })?
        .map(|mut proof| {
            proof.elapsed_ms = started.elapsed().as_millis();
            proof
        })
}

async fn prove_fixture() -> Result<AttachProof, String> {
    let issuer_key = deterministic_key(51)?;
    let paired_key = deterministic_key(52)?;
    let wrong_key = deterministic_key(53)?;
    let issuer_peer = issuer_key.public().to_peer_id();
    let paired_peer = paired_key.public().to_peer_id();
    let wrong_peer = wrong_key.public().to_peer_id();
    let address = memory_address(45_001);
    let mut server_swarm = attach_swarm(&issuer_key)?;
    let mut paired_swarm = attach_swarm(&paired_key)?;
    let mut wrong_swarm = attach_swarm(&wrong_key)?;
    server_swarm
        .listen_on(address.clone())
        .map_err(|error| error.to_string())?;
    let mut server = AttachServer {
        key: issuer_key.clone(),
        paired_public_key: paired_key.public(),
        paired_peer_id: paired_peer,
        used_nonces: HashSet::new(),
    };

    let invite = signed_invite(&issuer_key, paired_peer, LOGICAL_NOW_MS + 10_000, "valid")?;
    let valid = signed_request(&paired_key, invite.clone(), "attach-valid")?;
    let accepted = exchange(
        &mut server_swarm,
        &mut paired_swarm,
        &mut server,
        issuer_peer,
        address.clone(),
        valid.clone(),
    )
    .await?;
    let snapshot = accepted
        .snapshot
        .ok_or_else(|| "accepted response omitted snapshot".to_string())?;
    if accepted.state != "complete" {
        return Err(format!("valid attach was not accepted: {}", accepted.state));
    }
    let snapshot_bytes = serde_json::to_vec(&snapshot)
        .map_err(|error| error.to_string())?
        .len();
    let digest = snapshot
        .event_digests
        .first()
        .ok_or_else(|| "snapshot omitted event digest".to_string())?
        .sha256
        .clone();

    let replay = exchange(
        &mut server_swarm,
        &mut paired_swarm,
        &mut server,
        issuer_peer,
        address.clone(),
        valid,
    )
    .await?;
    require_code(&replay, "replay")?;

    let wrong_request = signed_request(&wrong_key, invite, "attach-wrong-peer")?;
    let wrong = exchange(
        &mut server_swarm,
        &mut wrong_swarm,
        &mut server,
        issuer_peer,
        address.clone(),
        wrong_request,
    )
    .await?;
    require_code(&wrong, "wrong_peer")?;

    let expired_invite = signed_invite(&issuer_key, paired_peer, LOGICAL_NOW_MS - 1, "expired")?;
    let expired_request = signed_request(&paired_key, expired_invite, "attach-expired")?;
    let expired = exchange(
        &mut server_swarm,
        &mut paired_swarm,
        &mut server,
        issuer_peer,
        address,
        expired_request,
    )
    .await?;
    require_code(&expired, "expired")?;

    Ok(AttachProof {
        schema: "agenterm-net/remote-fleet-attach-proof/v1",
        status: "prototype-proven",
        fixture: "deterministic in-process private-memory pairing fixture; not a product endpoint",
        elapsed_ms: 0,
        protocol: ATTACH_PROTOCOL,
        transport: "memory+Noise+Yamux+CBOR",
        authenticated_peer_identity: true,
        issuer_peer_id: issuer_peer.to_string(),
        paired_peer_id: paired_peer.to_string(),
        wrong_peer_id: wrong_peer.to_string(),
        accepted: AcceptedProof {
            state: accepted.state,
            snapshot_bytes,
            server_count: snapshot.server_count,
            event_digest_count: snapshot.event_digests.len(),
            event_digest_sha256: digest,
        },
        rejections: RejectionProof {
            replay: "replay".to_string(),
            wrong_peer: "wrong_peer".to_string(),
            expired: "expired".to_string(),
        },
        limits: LimitProof {
            request_bytes: MAX_REQUEST_BYTES,
            response_bytes: MAX_RESPONSE_BYTES,
            max_concurrent_streams: MAX_CONCURRENT_STREAMS,
            max_servers: MAX_SERVERS,
            max_event_digests: MAX_EVENT_DIGESTS,
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

async fn exchange(
    server_swarm: &mut Swarm<AttachBehaviour>,
    client_swarm: &mut Swarm<AttachBehaviour>,
    server: &mut AttachServer,
    server_peer: PeerId,
    address: Multiaddr,
    request: AttachRequest,
) -> Result<AttachResponse, String> {
    let outbound = client_swarm.behaviour_mut().send_request_with_addresses(
        &server_peer,
        request,
        vec![address],
    );
    loop {
        tokio::select! {
            event = server_swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(request_response::Event::Message { peer, message: request_response::Message::Request { request, channel, .. }, .. }) => {
                    let response = server.handle(peer, request);
                    server_swarm.behaviour_mut().send_response(channel, response)
                        .map_err(|_| "attach response channel closed".to_string())?;
                }
                SwarmEvent::Behaviour(request_response::Event::InboundFailure { error, .. }) => return Err(format!("attach inbound failed: {error}")),
                SwarmEvent::OutgoingConnectionError { error, .. } => return Err(format!("attach server connection failed: {error}")),
                SwarmEvent::ListenerError { error, .. } => return Err(format!("attach server listener failed: {error}")),
                _ => {}
            },
            event = client_swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(request_response::Event::Message { message: request_response::Message::Response { request_id, response }, .. }) if request_id == outbound => return Ok(response),
                SwarmEvent::Behaviour(request_response::Event::OutboundFailure { request_id, error, .. }) if request_id == outbound => return Err(format!("attach request failed: {error}")),
                SwarmEvent::OutgoingConnectionError { error, .. } => return Err(format!("attach client connection failed: {error}")),
                SwarmEvent::ListenerError { error, .. } => return Err(format!("attach client listener failed: {error}")),
                _ => {}
            }
        }
    }
}

fn signed_invite(
    key: &Keypair,
    target: PeerId,
    expires_unix_ms: u64,
    label: &str,
) -> Result<PairingInvite, String> {
    let issuer = key.public().to_peer_id().to_string();
    let target = target.to_string();
    let nonce = hex_digest(format!("agenterm-pairing-{label}").as_bytes());
    let unsigned = UnsignedInvite {
        schema: "agenterm-net/pairing-invite/v1",
        issuer_peer_id: &issuer,
        target_peer_id: &target,
        expires_unix_ms,
        nonce: &nonce,
        scope: PAIRING_SCOPE,
        max_snapshot_bytes: MAX_RESPONSE_BYTES,
        max_event_digests: MAX_EVENT_DIGESTS,
    };
    let bytes = serde_json::to_vec(&unsigned).map_err(|error| error.to_string())?;
    let signature = key.sign(&bytes).map_err(|error| error.to_string())?;
    Ok(PairingInvite {
        schema: unsigned.schema.to_string(),
        issuer_peer_id: issuer,
        target_peer_id: target,
        expires_unix_ms,
        nonce,
        scope: PAIRING_SCOPE.to_string(),
        max_snapshot_bytes: MAX_RESPONSE_BYTES,
        max_event_digests: MAX_EVENT_DIGESTS,
        issuer_signature: signature,
    })
}

fn verify_invite(public_key: &PublicKey, invite: &PairingInvite) -> bool {
    let unsigned = UnsignedInvite {
        schema: &invite.schema,
        issuer_peer_id: &invite.issuer_peer_id,
        target_peer_id: &invite.target_peer_id,
        expires_unix_ms: invite.expires_unix_ms,
        nonce: &invite.nonce,
        scope: &invite.scope,
        max_snapshot_bytes: invite.max_snapshot_bytes,
        max_event_digests: invite.max_event_digests,
    };
    serde_json::to_vec(&unsigned)
        .is_ok_and(|bytes| public_key.verify(&bytes, &invite.issuer_signature))
}

fn signed_request(
    key: &Keypair,
    invite: PairingInvite,
    request_id: &str,
) -> Result<AttachRequest, String> {
    let requester = key.public().to_peer_id().to_string();
    let binding = request_binding(request_id, &requester, &invite)?;
    let signature = key.sign(&binding).map_err(|error| error.to_string())?;
    Ok(AttachRequest {
        schema: "agenterm-net/remote-fleet-request/v1".to_string(),
        request_id: request_id.to_string(),
        requester_peer_id: requester,
        invite,
        requester_signature: signature,
    })
}

fn verify_request(public_key: &PublicKey, request: &AttachRequest) -> bool {
    request_binding(
        &request.request_id,
        &request.requester_peer_id,
        &request.invite,
    )
    .is_ok_and(|bytes| public_key.verify(&bytes, &request.requester_signature))
}

fn request_binding(
    request_id: &str,
    requester: &str,
    invite: &PairingInvite,
) -> Result<Vec<u8>, String> {
    let invite_bytes = serde_json::to_vec(invite).map_err(|error| error.to_string())?;
    serde_json::to_vec(&RequestBinding {
        schema: "agenterm-net/remote-fleet-binding/v1",
        request_id,
        requester_peer_id: requester,
        invite_sha256: hex_digest(&invite_bytes),
    })
    .map_err(|error| error.to_string())
}

fn fixture_snapshot() -> FleetSnapshot {
    let event_material = b"server-alpha:7:window-count=2:tab-count=3";
    FleetSnapshot {
        schema: "agenterm-net/bounded-fleet-snapshot/v1".to_string(),
        generated_unix_ms: LOGICAL_NOW_MS,
        server_count: 1,
        servers: vec![RemoteServerSummary {
            server_id: "server-alpha".to_string(),
            state: "available".to_string(),
            window_count: 2,
            tab_count: 3,
            latest_event_sequence: 7,
        }],
        event_digests: vec![EventDigest {
            server_id: "server-alpha".to_string(),
            event_count: 7,
            latest_sequence: 7,
            sha256: hex_digest(event_material),
        }],
    }
}

fn require_code(response: &AttachResponse, expected: &str) -> Result<(), String> {
    if response.state == "rejected"
        && response.code.as_deref() == Some(expected)
        && response.snapshot.is_none()
    {
        Ok(())
    } else {
        Err(format!(
            "expected typed {expected} rejection, got {response:?}"
        ))
    }
}

fn attach_swarm(key: &Keypair) -> Result<Swarm<AttachBehaviour>, String> {
    attach_swarm_with_transport(key, memory_transport(key)?)
}

fn attach_tcp_swarm(key: &Keypair) -> Result<Swarm<AttachBehaviour>, String> {
    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true)).boxed();
    attach_swarm_with_transport(key, secure_transport(transport, key)?)
}

fn attach_swarm_with_transport(
    key: &Keypair,
    transport: Boxed<(PeerId, StreamMuxerBox)>,
) -> Result<Swarm<AttachBehaviour>, String> {
    let peer = key.public().to_peer_id();
    let codec = cbor::codec::Codec::default()
        .set_request_size_maximum(MAX_REQUEST_BYTES)
        .set_response_size_maximum(MAX_RESPONSE_BYTES);
    let behaviour = request_response::Behaviour::with_codec(
        codec,
        [(StreamProtocol::new(ATTACH_PROTOCOL), ProtocolSupport::Full)],
        request_response::Config::default()
            .with_request_timeout(Duration::from_secs(3))
            .with_max_concurrent_streams(MAX_CONCURRENT_STREAMS),
    );
    Ok(Swarm::new(
        transport,
        behaviour,
        peer,
        SwarmConfig::with_tokio_executor().with_idle_connection_timeout(Duration::from_secs(10)),
    ))
}

fn print_worker<T: Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn deterministic_key(seed: u8) -> Result<Keypair, String> {
    let mut bytes = [0_u8; 32];
    bytes[0] = seed;
    Keypair::ed25519_from_bytes(bytes).map_err(|error| error.to_string())
}

fn memory_address(id: u64) -> Multiaddr {
    Multiaddr::empty().with(Protocol::Memory(id))
}

fn memory_transport(key: &Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>, String> {
    secure_transport(MemoryTransport::default().boxed(), key)
}

fn secure_transport<Stream>(
    transport: Boxed<Stream>,
    key: &Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, String>
where
    Stream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let noise = noise::Config::new(key).map_err(|error| error.to_string())?;
    Ok(transport
        .upgrade(upgrade::Version::V1)
        .authenticate(noise)
        .multiplex(yamux::Config::default())
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed())
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_schema_contains_only_bounded_observation() {
        let snapshot = fixture_snapshot();
        assert!(snapshot.servers.len() <= MAX_SERVERS as usize);
        assert!(snapshot.event_digests.len() <= MAX_EVENT_DIGESTS as usize);
        let encoded = serde_json::to_string(&snapshot).unwrap();
        for excluded in [
            "shell",
            "command",
            "pty",
            "terminal_input",
            "server_control",
        ] {
            assert!(!encoded.contains(excluded), "snapshot exposed {excluded}");
        }
    }

    #[test]
    fn invite_and_request_signatures_bind_identity_and_payload() {
        let issuer = deterministic_key(61).unwrap();
        let paired = deterministic_key(62).unwrap();
        let invite = signed_invite(
            &issuer,
            paired.public().to_peer_id(),
            LOGICAL_NOW_MS + 1,
            "unit",
        )
        .unwrap();
        assert!(verify_invite(&issuer.public(), &invite));
        let mut request = signed_request(&paired, invite, "unit-request").unwrap();
        assert!(verify_request(&paired.public(), &request));
        request.request_id.push_str("-tampered");
        assert!(!verify_request(&paired.public(), &request));
    }
}
