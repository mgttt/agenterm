use libp2p::{
    Multiaddr, PeerId, StreamProtocol, Transport,
    core::{
        multiaddr::Protocol,
        muxing::StreamMuxerBox,
        transport::{Boxed, MemoryTransport, OrTransport},
        upgrade,
    },
    futures::{AsyncRead, AsyncWrite, StreamExt},
    gossipsub,
    identity::Keypair,
    kad::{self, RecordKey, store::MemoryStore},
    noise, ping, relay,
    swarm::{Config as SwarmConfig, NetworkBehaviour, Swarm, SwarmEvent},
    yamux,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const DHT_PROTOCOL: &str = "/agenterm/kad/private/1.0.0";
const PUBSUB_TOPIC: &str = "agenterm.private.mesh.v1";
const PUBSUB_MAX_MESSAGE_BYTES: usize = 16 * 1024;

#[derive(Debug, Serialize)]
pub struct PrivateMeshProof {
    pub schema: &'static str,
    pub status: &'static str,
    pub fixture: &'static str,
    pub elapsed_ms: u128,
    pub public_bootstrap: bool,
    pub nat_traversal: bool,
    pub relay_serving_default: bool,
    pub remote_fleet_control: bool,
    pub dht: DhtProof,
    pub pubsub: PubsubProof,
    pub relay: RelayProof,
}

#[derive(Debug, Serialize)]
pub struct DhtProof {
    pub capability: &'static str,
    pub state: &'static str,
    pub protocol: &'static str,
    pub topology: &'static str,
    pub provider_peer_id: String,
    pub seeker_peer_id: String,
    pub provider_record_published: bool,
    pub provider_found_via_hub: bool,
    pub public_bootstrap_attempts: u8,
}

#[derive(Debug, Serialize)]
pub struct PubsubProof {
    pub capability: &'static str,
    pub state: &'static str,
    pub protocol: &'static str,
    pub topic: &'static str,
    pub publisher_peer_id: String,
    pub subscriber_peer_id: String,
    pub signed: bool,
    pub payload_bytes: usize,
    pub payload_verified: bool,
    pub max_message_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct RelayProof {
    pub capability: &'static str,
    pub state: &'static str,
    pub protocol: &'static str,
    pub relay_peer_id: String,
    pub source_peer_id: String,
    pub destination_peer_id: String,
    pub reservation_accepted: bool,
    pub circuit_accepted: bool,
    pub source_connected_to_destination: bool,
    pub destination_connected_to_source: bool,
    pub relay_serving_is_fixture_only: bool,
}

pub async fn prove_private_mesh(deadline: Duration) -> Result<PrivateMeshProof, String> {
    let started = Instant::now();
    let result = tokio::time::timeout(deadline, async {
        tokio::try_join!(prove_dht(), prove_pubsub(), prove_relay())
    })
    .await
    .map_err(|_| {
        format!(
            "private mesh proof exceeded {} ms deadline",
            deadline.as_millis()
        )
    })?;
    let (dht, pubsub, relay) = result?;
    Ok(PrivateMeshProof {
        schema: "agenterm-net/private-mesh-proof/v1",
        status: "proven-in-deterministic-fixture",
        fixture: "three isolated in-process meshes; deterministic Ed25519 identities and memory addresses",
        elapsed_ms: started.elapsed().as_millis(),
        public_bootstrap: false,
        nat_traversal: false,
        relay_serving_default: false,
        remote_fleet_control: false,
        dht,
        pubsub,
        relay,
    })
}

async fn prove_dht() -> Result<DhtProof, String> {
    let mut hub = kad_swarm(11)?;
    let mut provider = kad_swarm(12)?;
    let mut seeker = kad_swarm(13)?;
    let hub_id = *hub.local_peer_id();
    let provider_id = *provider.local_peer_id();
    let seeker_id = *seeker.local_peer_id();
    let hub_addr = memory_address(41_001);
    let provider_addr = memory_address(41_002);

    hub.listen_on(hub_addr.clone())
        .map_err(|error| error.to_string())?;
    provider
        .listen_on(provider_addr.clone())
        .map_err(|error| error.to_string())?;
    hub.behaviour_mut().add_address(&provider_id, provider_addr);
    provider
        .behaviour_mut()
        .add_address(&hub_id, hub_addr.clone());
    seeker.behaviour_mut().add_address(&hub_id, hub_addr);

    let key_material = Sha256::digest(b"agenterm deterministic private provider record v1");
    let key = RecordKey::new(&key_material);
    let publish_query = provider
        .behaviour_mut()
        .start_providing(key.clone())
        .map_err(|error| format!("start DHT provider: {error}"))?;
    let mut published = false;
    while !published {
        tokio::select! {
            event = hub.select_next_some() => reject_swarm_error("dht hub", &event)?,
            event = provider.select_next_some() => {
                reject_swarm_error("dht provider", &event)?;
                if let SwarmEvent::Behaviour(kad::Event::OutboundQueryProgressed { id, result, .. }) = event
                    && id == publish_query
                {
                    match result {
                        kad::QueryResult::StartProviding(Ok(ok)) if ok.key == key => published = true,
                        kad::QueryResult::StartProviding(Err(error)) => return Err(format!("publish provider record: {error}")),
                        _ => {}
                    }
                }
            }
        }
    }

    let find_query = seeker.behaviour_mut().get_providers(key);
    let mut found = false;
    while !found {
        tokio::select! {
            event = hub.select_next_some() => reject_swarm_error("dht hub", &event)?,
            event = provider.select_next_some() => reject_swarm_error("dht provider", &event)?,
            event = seeker.select_next_some() => {
                reject_swarm_error("dht seeker", &event)?;
                if let SwarmEvent::Behaviour(kad::Event::OutboundQueryProgressed { id, result, .. }) = event
                    && id == find_query
                {
                    match result {
                        kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { providers, .. }))
                            if providers.contains(&provider_id) => found = true,
                        kad::QueryResult::GetProviders(Err(error)) => return Err(format!("find provider record: {error}")),
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(DhtProof {
        capability: "private-mesh.dht",
        state: "prototype-proven",
        protocol: DHT_PROTOCOL,
        topology: "provider -> private hub <- seeker; seeker has no direct provider address",
        provider_peer_id: provider_id.to_string(),
        seeker_peer_id: seeker_id.to_string(),
        provider_record_published: published,
        provider_found_via_hub: found,
        public_bootstrap_attempts: 0,
    })
}

async fn prove_pubsub() -> Result<PubsubProof, String> {
    let mut publisher = gossipsub_swarm(21)?;
    let mut subscriber = gossipsub_swarm(22)?;
    let publisher_id = *publisher.local_peer_id();
    let subscriber_id = *subscriber.local_peer_id();
    let publisher_addr = memory_address(42_001);
    let topic = gossipsub::IdentTopic::new(PUBSUB_TOPIC);
    let payload = b"agenterm bounded private pubsub v1".to_vec();

    publisher
        .behaviour_mut()
        .subscribe(&topic)
        .map_err(|error| format!("publisher subscribe: {error}"))?;
    subscriber
        .behaviour_mut()
        .subscribe(&topic)
        .map_err(|error| format!("subscriber subscribe: {error}"))?;
    publisher
        .listen_on(publisher_addr.clone())
        .map_err(|error| error.to_string())?;
    subscriber
        .dial(publisher_addr.with(Protocol::P2p(publisher_id)))
        .map_err(|error| error.to_string())?;

    let mut published = false;
    loop {
        tokio::select! {
            event = publisher.select_next_some() => {
                reject_swarm_error("pubsub publisher", &event)?;
                if let SwarmEvent::Behaviour(gossipsub::Event::Subscribed { peer_id, topic: remote_topic }) = event
                    && peer_id == subscriber_id
                    && remote_topic == topic.hash()
                    && !published
                {
                    publisher.behaviour_mut().publish(topic.clone(), payload.clone())
                        .map_err(|error| format!("publish private message: {error}"))?;
                    published = true;
                }
            },
            event = subscriber.select_next_some() => {
                reject_swarm_error("pubsub subscriber", &event)?;
                if let SwarmEvent::Behaviour(gossipsub::Event::Message { propagation_source, message, .. }) = event
                    && propagation_source == publisher_id
                    && message.topic == topic.hash()
                    && message.data == payload
                {
                    return Ok(PubsubProof {
                        capability: "private-mesh.pubsub",
                        state: "prototype-proven",
                        protocol: "gossipsub/1.1 signed strict validation",
                        topic: PUBSUB_TOPIC,
                        publisher_peer_id: publisher_id.to_string(),
                        subscriber_peer_id: subscriber_id.to_string(),
                        signed: message.source == Some(publisher_id),
                        payload_bytes: payload.len(),
                        payload_verified: true,
                        max_message_bytes: PUBSUB_MAX_MESSAGE_BYTES,
                    });
                }
            }
        }
    }
}

async fn prove_relay() -> Result<RelayProof, String> {
    let mut relay = relay_server_swarm(31)?;
    let mut destination = relay_client_swarm(32)?;
    let relay_id = *relay.local_peer_id();
    let destination_id = *destination.local_peer_id();
    let relay_addr = memory_address(43_001);
    relay
        .listen_on(relay_addr.clone())
        .map_err(|error| error.to_string())?;
    relay.add_external_address(relay_addr.clone());

    let reservation_addr = relay_addr
        .with(Protocol::P2p(relay_id))
        .with(Protocol::P2pCircuit);
    destination
        .listen_on(reservation_addr)
        .map_err(|error| error.to_string())?;
    let mut destination_relay_addr = None;
    let mut reservation_accepted = false;
    while !reservation_accepted || destination_relay_addr.is_none() {
        tokio::select! {
            event = relay.select_next_some() => reject_swarm_error("relay server", &event)?,
            event = destination.select_next_some() => {
                reject_swarm_error("relay destination", &event)?;
                match event {
                    SwarmEvent::Behaviour(RelayClientEvent::Relay(relay::client::Event::ReservationReqAccepted { relay_peer_id, renewal, .. }))
                        if relay_peer_id == relay_id && !renewal => reservation_accepted = true,
                    SwarmEvent::NewListenAddr { address, .. }
                        if address.iter().any(|part| matches!(part, Protocol::P2pCircuit)) => destination_relay_addr = Some(address),
                    _ => {}
                }
            }
        }
    }

    let mut source = relay_client_swarm(33)?;
    let source_id = *source.local_peer_id();
    let destination_relay_addr =
        destination_relay_addr.expect("loop only exits after relay listen address");
    source
        .dial(destination_relay_addr)
        .map_err(|error| format!("dial private relay circuit: {error}"))?;

    let mut circuit_accepted = false;
    let mut source_connected = false;
    let mut destination_connected = false;
    while !circuit_accepted || !source_connected || !destination_connected {
        tokio::select! {
            event = relay.select_next_some() => {
                reject_swarm_error("relay server", &event)?;
                if let SwarmEvent::Behaviour(RelayServerEvent::Relay(relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id })) = event
                    && src_peer_id == source_id
                    && dst_peer_id == destination_id
                {
                    circuit_accepted = true;
                }
            },
            event = destination.select_next_some() => {
                reject_swarm_error("relay destination", &event)?;
                if let SwarmEvent::ConnectionEstablished { peer_id, .. } = event
                    && peer_id == source_id
                {
                    destination_connected = true;
                }
            },
            event = source.select_next_some() => {
                reject_swarm_error("relay source", &event)?;
                if let SwarmEvent::ConnectionEstablished { peer_id, .. } = event
                    && peer_id == destination_id
                {
                    source_connected = true;
                }
            }
        }
    }

    Ok(RelayProof {
        capability: "private-mesh.relay",
        state: "prototype-proven",
        protocol: "circuit-relay-v2",
        relay_peer_id: relay_id.to_string(),
        source_peer_id: source_id.to_string(),
        destination_peer_id: destination_id.to_string(),
        reservation_accepted,
        circuit_accepted,
        source_connected_to_destination: source_connected,
        destination_connected_to_source: destination_connected,
        relay_serving_is_fixture_only: true,
    })
}

fn kad_swarm(seed: u8) -> Result<Swarm<kad::Behaviour<MemoryStore>>, String> {
    let key = deterministic_key(seed)?;
    let peer = key.public().to_peer_id();
    let mut config = kad::Config::new(StreamProtocol::new(DHT_PROTOCOL));
    config
        .set_query_timeout(Duration::from_secs(3))
        .set_periodic_bootstrap_interval(None)
        .set_provider_publication_interval(None);
    let mut behaviour = kad::Behaviour::with_config(peer, MemoryStore::new(peer), config);
    behaviour.set_mode(Some(kad::Mode::Server));
    Ok(Swarm::new(
        memory_transport(&key)?,
        behaviour,
        peer,
        swarm_config(),
    ))
}

fn gossipsub_swarm(seed: u8) -> Result<Swarm<gossipsub::Behaviour>, String> {
    let key = deterministic_key(seed)?;
    let peer = key.public().to_peer_id();
    let config = gossipsub::ConfigBuilder::default()
        .heartbeat_initial_delay(Duration::from_millis(10))
        .heartbeat_interval(Duration::from_millis(50))
        .validation_mode(gossipsub::ValidationMode::Strict)
        .max_transmit_size(PUBSUB_MAX_MESSAGE_BYTES)
        .build()
        .map_err(|error| error.to_string())?;
    let behaviour =
        gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Signed(key.clone()), config)
            .map_err(|error| error.to_string())?;
    Ok(Swarm::new(
        memory_transport(&key)?,
        behaviour,
        peer,
        swarm_config(),
    ))
}

#[derive(NetworkBehaviour)]
struct RelayServer {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
}

#[derive(NetworkBehaviour)]
struct RelayClient {
    relay: relay::client::Behaviour,
    ping: ping::Behaviour,
}

fn relay_server_swarm(seed: u8) -> Result<Swarm<RelayServer>, String> {
    let key = deterministic_key(seed)?;
    let peer = key.public().to_peer_id();
    let behaviour = RelayServer {
        relay: relay::Behaviour::new(peer, relay::Config::default()),
        ping: ping::Behaviour::new(ping::Config::new()),
    };
    Ok(Swarm::new(
        memory_transport(&key)?,
        behaviour,
        peer,
        swarm_config(),
    ))
}

fn relay_client_swarm(seed: u8) -> Result<Swarm<RelayClient>, String> {
    let key = deterministic_key(seed)?;
    let peer = key.public().to_peer_id();
    let (relay_transport, relay_behaviour) = relay::client::new(peer);
    let transport = secure_transport(
        OrTransport::new(relay_transport, MemoryTransport::default()).boxed(),
        &key,
    )?;
    Ok(Swarm::new(
        transport,
        RelayClient {
            relay: relay_behaviour,
            ping: ping::Behaviour::new(ping::Config::new()),
        },
        peer,
        swarm_config(),
    ))
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

fn swarm_config() -> SwarmConfig {
    SwarmConfig::with_tokio_executor().with_idle_connection_timeout(Duration::from_secs(10))
}

fn reject_swarm_error<T>(label: &str, event: &SwarmEvent<T>) -> Result<(), String> {
    match event {
        SwarmEvent::OutgoingConnectionError { error, .. } => {
            Err(format!("{label} outgoing connection failed: {error}"))
        }
        SwarmEvent::ListenerError { error, .. } => Err(format!("{label} listener failed: {error}")),
        _ => Ok(()),
    }
}
