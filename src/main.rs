/*
A temporary libp2p test program "ping".
*/


use std::error::Error;
use futures::prelude::*;
use libp2p::{noise, ping, swarm::SwarmEvent, tcp, yamux, Multiaddr};
use tracing_subscriber::EnvFilter;


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // Set up structured logging so we can see what the swarm is doing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Create a libp2p swarm with a new identity
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        // Construct a transport: 
        //  - Default TCP to establish connections
        //  - Noise to encrypt the connection
        //  - Yamux to multiplex the connection
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        // Add a ping behaviour to the swarm
        .with_behaviour(|_| ping::Behaviour::default())?
        .build();

    // Tell the swarm to listen on all interfaces and a random, OS-assigned port.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // If there is a multi-address given as a second command-line argument
    if let Some(addr) = std::env::args().nth(1) {
        // Dial the peer identified by the multi-address
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        println!("Dialed {addr}")
    }

    // Loop until the program is interrupted
    loop {
        // Wait for the next event in the swarm
        match swarm.select_next_some().await {
            // If the swarm is listening on a new address, print it
            SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {address:?}"),
            // If the swarm is doing something, print it
            SwarmEvent::Behaviour(event) => println!("{event:?}"),
            // If the swarm is doing nothing, do nothing
            _ => {}
        }
    }
}
