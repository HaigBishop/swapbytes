use libp2p::{ping, swarm::NetworkBehaviour, gossipsub, mdns, identity::Keypair};
use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, time::Duration};
use tokio::io; // Import io for map_err

// Defines the top-level libp2p behaviour for SwapBytes.
// Combines ping, gossipsub for messaging/presence, and mDNS for local discovery.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "SwapBytesBehaviourEvent")]
pub struct SwapBytesBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub ping: ping::Behaviour,
}

// Event enum that wraps events from the contained behaviours.
#[derive(Debug)]
pub enum SwapBytesBehaviourEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    Ping(ping::Event),
}

// --- From implementations for event wrapping ---

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

// --- Constructor ---

impl SwapBytesBehaviour {
    // Creates a new SwapBytesBehaviour with the given keypair.
    // Handles initializing gossipsub and mDNS.
    pub fn new(keypair: &Keypair) -> Result<Self, io::Error> {
        // Content-address messages by hashing data
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        // Configure gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(15)) // Heartbeat for presence
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(message_id_fn) // Use content addressing
            .build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?; // Map build error

        // Build gossipsub behaviour
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair.clone()), // Sign messages
            gossipsub_config,
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?; // Map String error to io::Error

        // Build mDNS behaviour
        let mdns = mdns::tokio::Behaviour::new(
            mdns::Config::default(),
            keypair.public().to_peer_id() // Identify using our PeerId
        )?;

        // Build Ping behaviour with keep-alive disabled
        let ping_config = ping::Config::new()
            .with_interval(Duration::from_secs(3 * 60 * 60)); // Interval of 3 hours effectively disables keep-alive
        let ping = ping::Behaviour::new(ping_config);

        Ok(Self {
            gossipsub,
            mdns,
            ping, // Use the configured ping behaviour
        })
    }
} 