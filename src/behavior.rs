/*
Sets up all the different protocols and behaviours for our P2P node.
*/

use libp2p::{ping, swarm::NetworkBehaviour, gossipsub, mdns, identity::Keypair, request_response};
use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, time::Duration, iter};
use tokio::io; // Needed for mapping errors
use crate::protocol::{PrivateCodec, PrivateRequest, PrivateResponse, PrivateProtocol};

// This is the main behaviour handler for our P2P node.
// Think of it as the brain that manages different P2P protocols together.
// It bundles up: 
//  - Gossipsub: For broadcasting messages to everyone (like global chat).
//  - mDNS: For finding other nodes on the same local network.
//  - Ping: For checking if another node is still online.
//  - Request-Response: For direct, one-to-one messages (like private chat).
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "SwapBytesBehaviourEvent")] // Tells libp2p how to process events from this behaviour
pub struct SwapBytesBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub ping: ping::Behaviour,
    pub request_response: request_response::Behaviour<PrivateCodec>,
}

// This enum lists all the possible events that can come from our combined `SwapBytesBehaviour`.
// It helps us figure out which protocol (Gossipsub, mDNS, Ping, etc.) generated an event.
#[derive(Debug)]
pub enum SwapBytesBehaviourEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    Ping(ping::Event),
    RequestResponse(request_response::Event<PrivateRequest, PrivateResponse>),
}

// These `From` implementations are helpers.
// They automatically wrap events from the individual protocols (like `gossipsub::Event`)
// into our main `SwapBytesBehaviourEvent` enum.
// This makes handling events in the main loop much cleaner.

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


impl SwapBytesBehaviour {
    // Sets up a new `SwapBytesBehaviour` instance.
    // This involves creating and configuring all the individual protocol behaviours.
    pub fn new(keypair: &Keypair) -> Result<Self, io::Error> {
        
        // --- Gossipsub Setup --- 
        // We need a way to uniquely identify gossipsub messages to avoid duplicates.
        // This function creates a hash of the message content to use as an ID.
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        // Configure gossipsub settings
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(15)) // Regular pings to keep connections alive
            .validation_mode(gossipsub::ValidationMode::Strict) // Enforce message validation
            .message_id_fn(message_id_fn) // Use our custom message ID function
            .build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?; // Convert error type

        // Create the gossipsub behaviour
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair.clone()), // Sign messages with our keypair for security
            gossipsub_config,
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?; // Convert error type

        // --- mDNS Setup --- 
        // Create the mDNS behaviour for local peer discovery
        let mdns = mdns::tokio::Behaviour::new(
            mdns::Config::default(), // Use default mDNS settings
            keypair.public().to_peer_id() // Use our peer ID for identification
        )?;

        // --- Ping Setup --- 
        // Create the Ping behaviour. We set a very long interval to effectively disable
        // automatic keep-alive pings, as we handle presence differently (via gossipsub heartbeats).
        let ping_config = ping::Config::new()
            .with_interval(Duration::from_secs(3 * 60 * 60)); 
        let ping = ping::Behaviour::new(ping_config);

        // --- Request-Response Setup --- 
        // Define the protocol(s) the request-response behaviour will use.
        // Here, we're using our custom `PrivateProtocol` for direct messages.
        let request_response_protocols = iter::once((PrivateProtocol(), request_response::ProtocolSupport::Full));
        // Create the request-response behaviour
        let request_response = request_response::Behaviour::new(
            request_response_protocols, // Tell it which protocols to speak
            request_response::Config::default(), // Use default request-response settings
        );

        // Bundle all the behaviours together
        Ok(Self {
            gossipsub,
            mdns,
            ping,
            request_response,
        })
    }
}
