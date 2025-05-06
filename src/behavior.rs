/*
Sets up all the different protocols and behaviours for our P2P node.
*/

use libp2p::{ping, swarm::NetworkBehaviour, gossipsub, mdns, identity::Keypair, request_response, rendezvous};
use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, time::Duration, iter};
use tokio::io; // Needed for mapping errors
use crate::protocol::{PrivateCodec, PrivateRequest, PrivateResponse, PrivateProtocol};

// --- Behaviour Struct Definition ---

// `SwapBytesBehaviour` bundles multiple libp2p protocols into a single logical unit.
// This struct manages the combined behavior of the node's P2P interactions.
// It derives `NetworkBehaviour` to integrate with the libp2p swarm.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "SwapBytesBehaviourEvent")] // Maps events from inner behaviours to the `SwapBytesBehaviourEvent` enum
pub struct SwapBytesBehaviour {
    /// Handles message broadcasting and propagation using the Gossipsub protocol.
    pub gossipsub: gossipsub::Behaviour,
    /// Enables peer discovery on the local network using the mDNS protocol.
    pub mdns: mdns::tokio::Behaviour,
    /// Manages PING requests to check peer liveness.
    pub ping: ping::Behaviour,
    /// Facilitates direct request-response interactions between peers using a custom protocol.
    pub request_response: request_response::Behaviour<PrivateCodec>,
    /// Handles registration and discovery with a Rendezvous point.
    pub rendezvous: rendezvous::client::Behaviour,
}

// --- Behaviour Event Enum ---

// `SwapBytesBehaviourEvent` aggregates events emitted by the individual behaviours
// within `SwapBytesBehaviour`. This allows the main event loop to handle events
// from different protocols through a single enum type.
#[derive(Debug)]
pub enum SwapBytesBehaviourEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    Ping(ping::Event),
    RequestResponse(request_response::Event<PrivateRequest, PrivateResponse>),
    Rendezvous(rendezvous::client::Event),
}

// --- Event Conversion Implementations (`From` traits) ---

// These implementations automatically convert events from specific protocol behaviours
// (e.g., `gossipsub::Event`) into the unified `SwapBytesBehaviourEvent` enum.
// This simplifies event handling logic in the main application loop.

impl From<gossipsub::Event> for SwapBytesBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        SwapBytesBehaviourEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for SwapBytesBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        SwapBytesBehaviourEvent::Mdns(event)
    }
}

impl From<ping::Event> for SwapBytesBehaviourEvent {
    fn from(event: ping::Event) -> Self {
        SwapBytesBehaviourEvent::Ping(event)
    }
}

impl From<request_response::Event<PrivateRequest, PrivateResponse>> for SwapBytesBehaviourEvent {
    fn from(event: request_response::Event<PrivateRequest, PrivateResponse>) -> Self {
        SwapBytesBehaviourEvent::RequestResponse(event)
    }
}

impl From<rendezvous::client::Event> for SwapBytesBehaviourEvent {
    fn from(event: rendezvous::client::Event) -> Self {
        SwapBytesBehaviourEvent::Rendezvous(event)
    }
}

// --- Behaviour Implementation ---

impl SwapBytesBehaviour {
    /// Constructs a new `SwapBytesBehaviour` instance.
    /// Initializes and configures all the constituent protocol behaviours.
    ///
    /// # Argument: `keypair` - The node's identity keypair, used for signing messages and identification.
    /// 
    pub fn new(keypair: &Keypair) -> Result<Self, io::Error> {
        let local_peer_id = keypair.public().to_peer_id(); // Get PeerId early

        // --- Gossipsub Setup ---
        // Define a function to generate unique IDs for gossipsub messages based on their content hash.
        // This helps prevent processing duplicate messages.
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        // Configure the Gossipsub protocol settings.
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(15)) // Set the interval for heartbeat messages to maintain connections.
            .validation_mode(gossipsub::ValidationMode::Strict) // Enforce strict validation of incoming messages.
            .message_id_fn(message_id_fn) // Use the custom message ID function defined above.
            .build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?; // Map the configuration error to `io::Error`.

        // Create the Gossipsub behaviour instance.
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair.clone()), // Ensure messages are signed with the node's keypair.
            gossipsub_config,
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?; // Map the creation error to `io::Error`.

        // --- mDNS Setup ---
        // Create the mDNS behaviour for discovering peers on the local network.
        let mdns = mdns::tokio::Behaviour::new(
            mdns::Config::default(), // Use default mDNS configuration.
            local_peer_id, // Use the local PeerId
        )?;

        // --- Ping Setup ---
        // Create the Ping behaviour configuration.
        // The interval is set very high to effectively disable automatic pings,
        // as liveness can be inferred from Gossipsub heartbeats.
        let ping_config = ping::Config::new()
            .with_interval(Duration::from_secs(3 * 60 * 60));
        // Create the Ping behaviour instance.
        let ping = ping::Behaviour::new(ping_config);

        // --- Request-Response Setup ---
        // Define the protocols supported by the request-response behaviour.
        // Here, it uses the custom `PrivateProtocol`.
        let request_response_protocols = iter::once((PrivateProtocol(), request_response::ProtocolSupport::Full));
        // Create the Request-Response behaviour instance.
        let request_response = request_response::Behaviour::new(
            request_response_protocols, // Specify the supported protocols.
            request_response::Config::default(), // Use default request-response configuration.
        );

        // --- Rendezvous Client Setup ---
        let rendezvous = rendezvous::client::Behaviour::new(keypair.clone());

        // --- Combine Behaviours ---
        // Construct the `SwapBytesBehaviour` struct with all initialized behaviours.
        Ok(Self {
            gossipsub,
            mdns,
            ping,
            request_response,
            rendezvous,
        })
    }
}
