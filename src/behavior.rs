use libp2p::{ping, swarm::NetworkBehaviour};

// Defines the top-level libp2p behaviour for SwapBytes.
// For now it only contains the `ping` behaviour, but more protocols
// (e.g. mDNS, gossipsub) can be added incrementally without touching
// the rest of the application.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "SwapBytesBehaviourEvent")]
pub struct SwapBytesBehaviour {
    pub ping: ping::Behaviour,
}

// Event enum that wraps events from the contained behaviours.
#[derive(Debug)]
pub enum SwapBytesBehaviourEvent {
    Ping(ping::Event),
    // Add other behaviour events here later (e.g., Gossipsub, Mdns)
}

// Implement From conversions for each behaviour's event type.
// This allows the derive macro to automatically wrap the events.
impl From<ping::Event> for SwapBytesBehaviourEvent {
    fn from(event: ping::Event) -> Self {
        SwapBytesBehaviourEvent::Ping(event)
    }
}

impl Default for SwapBytesBehaviour {
    fn default() -> Self {
        Self {
            ping: ping::Behaviour::default(),
        }
    }
} 