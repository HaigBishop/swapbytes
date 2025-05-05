/*
The main file for the SwapBytes CLI file-sharing application.
*/


// Standard library imports
use std::{error::Error, time::Duration, time::Instant, time::SystemTime, time::UNIX_EPOCH};

// Async imports
use futures::prelude::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio::time::interval; // Import interval

// libp2p imports
use libp2p::{noise, ping, swarm::SwarmEvent, tcp, yamux, identity, PeerId, gossipsub, mdns};

// Terminal UI imports
use crossterm::event;
use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout, Position},
    widgets::Block,
};

// Serialization for messages
use serde::{Serialize, Deserialize}; // Import Serialize/Deserialize

// Local modules
mod tui;
mod utils;
mod commands;
mod behavior;
mod protocol;
use tui::{App, AppEvent, InputMode, FocusPane, layout_chunks, PeerInfo, OnlineStatus}; // Add PeerInfo/OnlineStatus
use behavior::{SwapBytesBehaviour, SwapBytesBehaviourEvent};

/// The public topic used for global chat messages via Gossipsub.
const SWAPBYTES_TOPIC: &str = "swapbytes-global-chat";
/// How often we send out a "I'm still here" message (heartbeat).
const HEARTBEAT_INTERVAL_SECS: u64 = 2;
/// How long we wait without hearing from a peer before marking them as potentially offline.
const PEER_TIMEOUT_SECS: u64 = 8;

// --- Define Message Types ---
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")] // Use a 'type' field to distinguish message types
enum Message {
    /// A periodic message to signal presence and share nickname.
    Heartbeat {
        timestamp_ms: u64,        // When the heartbeat was sent.
        nickname: Option<String>, // The sender's nickname, if set.
    },
    /// A message sent to the public chat topic.
    GlobalChatMessage {
        content: String,
        timestamp_ms: u64,
        nickname: Option<String>,
    },
    // Add other message types like Chat, NicknameUpdate later
}

/// Entry point: sets up TUI, libp2p, and event loop.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // --- Initialize App State ---
    let mut app = App::default();

    // --- Terminal UI setup ---
    let mut terminal = ratatui::init();


    // --- Generate Identity ---
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // --- Event channel and cancellation token ---
    // Used to communicate between background tasks and the UI loop.
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>(); // Specify type
    let cancel = CancellationToken::new();

    // Channel for commands from UI loop to Swarm task
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<AppEvent>();


    // --- libp2p Swarm setup ---
    // 1. Create our custom behaviour with the generated key.
    let mut behaviour = SwapBytesBehaviour::new(&local_key)?;

    // 2. Define the gossipsub topic and subscribe.
    let topic = gossipsub::IdentTopic::new(SWAPBYTES_TOPIC);
    behaviour.gossipsub.subscribe(&topic)?;

    // 3. Build the Swarm using the existing identity and the behaviour.
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)? // Pass the constructed behaviour
        .build();

    // Listen on all interfaces, random OS-assigned port.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;


    // --- Background task: forward swarm events to UI ----
    let swarm_tx = tx.clone(); // For Swarm->UI events
    let swarm_cancel = cancel.clone();
    // Capture initial nickname for the swarm task
    let initial_nickname = app.nickname.clone();
    // Capture initial visibility state
    let initial_visibility = app.is_visible;
    tokio::spawn(async move {
        // Swarm task owns swarm and swarm_tx
        let mut swarm = swarm;
        // Store the current nickname locally within the swarm task
        let mut current_nickname = initial_nickname;
        // Store the current visibility state locally within the swarm task
        let mut is_visible = initial_visibility;
        // Heartbeat interval timer
        let mut heartbeat_timer = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        // Define the topic here once
        let topic = gossipsub::IdentTopic::new(SWAPBYTES_TOPIC);

        loop {
            tokio::select! {
                _ = swarm_cancel.cancelled() => break, // Graceful shutdown

                // --- Heartbeat Broadcaster ---
                _ = heartbeat_timer.tick() => {
                    // Only send heartbeat if visible
                    if is_visible {
                        // Log heartbeat sending
                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Sending heartbeat (visible: {})", is_visible)));
                        // Get current timestamp in milliseconds
                        let timestamp_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("Time went backwards")
                            .as_millis() as u64; // Use u64

                        let heartbeat_msg = Message::Heartbeat {
                            timestamp_ms,
                            nickname: current_nickname.clone(), // Use the task's nickname
                        };

                        match serde_json::to_vec(&heartbeat_msg) {
                            Ok(encoded_msg) => {
                                if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), encoded_msg) {
                                    // Ignore these errors: "Failed to publish heartbeat: InsufficientPeers"
                                    if e.to_string() != "InsufficientPeers" {
                                        // Log error, but don't crash the task
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to publish heartbeat: {e}")));
                                    }
                                }
                            }
                            Err(e) => {
                                 // Log serialization error
                                 let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to serialize heartbeat: {e}")));
                            }
                        }
                    } // else: do nothing if not visible
                }

                // Handle commands from the UI (e.g., Dial)
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        AppEvent::Dial(addr) => {
                            let log_msg = match swarm.dial(addr.clone()) {
                                Ok(()) => format!("Dialing {addr}"),
                                Err(e) => format!("Dial error: {e}"),
                            };
                            // Send log message back to UI
                            let _ = swarm_tx.send(AppEvent::LogMessage(log_msg));
                        }
                        // Handle nickname updates from the UI/commands
                        AppEvent::NicknameUpdated(_peer_id, nickname) => {
                            // Update the nickname stored within the swarm task
                            current_nickname = Some(nickname);
                        }
                        // Handle visibility changes from the UI/commands
                        AppEvent::VisibilityChanged(new_visibility) => {
                            is_visible = new_visibility;
                        }
                        // Handle publishing gossipsub messages
                        AppEvent::PublishGossipsub(data) => {
                            if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), data) {
                                // Ignore InsufficientPeers, log other messages though
                                if e.to_string() != "InsufficientPeers" {
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to publish chat message: {e}")));
                                }
                            }
                        }
                        // Handle sending private messages via Request/Response
                        AppEvent::SendPrivateMessage { target_peer, message } => {
                            let request = protocol::PrivateRequest::ChatMessage(message);
                            // Send the request
                            swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                            // Log the attempt (optional)
                            // let _ = swarm_tx.send(AppEvent::LogMessage(format!("Sent private message request to {}", target_peer)));
                        }
                        // Handle sending file offers
                        AppEvent::SendFileOffer { target_peer, file_path } => {
                            match std::fs::metadata(&file_path) {
                                Ok(metadata) => {
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Preparing SendFileOffer event for {} to {}", file_path.display(), target_peer)));
                                    if metadata.is_file() {
                                        let size_bytes = metadata.len();
                                        // Extract filename from path
                                        let filename = file_path.file_name().map_or_else(
                                            || "unknown_file".to_string(), // Fallback filename
                                            |os_name| os_name.to_string_lossy().into_owned()
                                        );

                                        let request = protocol::PrivateRequest::Offer { filename, size_bytes };
                                        // Send the request
                                        swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                                        // Log after attempting send
                                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Attempted send_request (Offer) to {} for {}", target_peer, file_path.display())));
                                    } else {
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("Error: Offer path is not a file: {}", file_path.display())));
                                    }
                                }
                                Err(e) => {
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("Error getting file metadata for offer: {} ({})", file_path.display(), e)));
                                }
                            }
                        }
                        // Handle declining file offers
                        AppEvent::DeclineFileOffer { target_peer, filename } => {
                            let request = protocol::PrivateRequest::DeclineOffer { filename };
                            // Send the request
                            swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                            // Log the attempt (optional)
                            // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Sent DeclineOffer request to {} for file {}", target_peer, filename)));
                        }
                        // Handle accepting file offers
                        AppEvent::SendAcceptOffer { target_peer, filename } => {
                            let request = protocol::PrivateRequest::AcceptOffer { filename };
                            // Send the request
                            swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                            // Log the attempt (optional)
                            // let _ = swarm_tx.send(AppEvent::LogMessage(format!("Sent AcceptOffer request to {} for file {}", target_peer, filename)));
                        }
                        // Ignore other commands if any were sent here by mistake
                        _ => {}
                    }
                }

                // Handle Swarm events
                ev = swarm.next() => {
                    if let Some(event) = ev {
                        match event {
                            SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                                for (peer_id, _multiaddr) in list {
                                    // Log mDNS discovery (optional)
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("mDNS Discovered: {peer_id}")));
                                    // Add the newly discovered peer to Gossipsub's routing table
                                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                    // Send event to UI task
                                    let _ = swarm_tx.send(AppEvent::PeerDiscovered(peer_id));
                                }
                            }
                            SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                                for (peer_id, _multiaddr) in list {
                                    // Log mDNS expiry (optional)
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("mDNS Expired: {peer_id}")));
                                    // Remove the peer from Gossipsub's routing table
                                    swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                    // Send event to UI task
                                    // We now rely on heartbeat timeout, so PeerExpired from mDNS is less critical
                                    // We could still send it, but let's comment it out to rely purely on heartbeat for status
                                    // let _ = swarm_tx.send(AppEvent::PeerExpired(peer_id));
                                }
                            }
                            // --- Handle Gossipsub Messages ---
                            SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                propagation_source: peer_id, // The peer who forwarded us the message
                                message_id: _id,
                                message,
                            })) => {
                                // Attempt to deserialize the message
                                match serde_json::from_slice::<Message>(&message.data) {
                                    Ok(deserialized_msg) => {
                                        // Log raw message reception before processing
                                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Received Gossipsub msg ({} bytes) from {}", message.data.len(), peer_id)));
                                        match deserialized_msg {
                                            Message::Heartbeat { timestamp_ms: _, nickname } => {
                                                // Send NicknameUpdated event if nickname is present
                                                if let Some(nick) = nickname {
                                                    // Use the actual source peer ID from the message if available,
                                                    // otherwise, we assume the forwarder is the source for nickname updates.
                                                    // NOTE: For gossipsub, message.source is usually None unless message signing is enabled.
                                                    let source_peer_id = message.source.unwrap_or(peer_id);
                                                    let _ = swarm_tx.send(AppEvent::NicknameUpdated(source_peer_id, nick));
                                                }
                                                // Also forward the raw event to update the forwarder's last_seen
                                                let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                                    propagation_source: peer_id,
                                                    message_id: _id,
                                                    message: message.clone(), // Clone message needed here
                                                }))));
                                            }
                                            Message::GlobalChatMessage { content, timestamp_ms, nickname } => {
                                                // Send the specific GlobalMessageReceived event
                                                // Use the actual source peer ID if available, otherwise use the forwarder
                                                let source_peer_id = message.source.unwrap_or(peer_id);
                                                let _ = swarm_tx.send(AppEvent::GlobalMessageReceived {
                                                    sender_id: source_peer_id,
                                                    sender_nickname: nickname,
                                                    content,
                                                    timestamp_ms,
                                                });
                                                // Also forward the raw event to update the forwarder's last_seen
                                                 let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                                    propagation_source: peer_id,
                                                    message_id: _id,
                                                    message: message.clone(), // Clone message needed here
                                                }))));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Log deserialization error, but still forward raw event for presence
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Failed to deserialize gossipsub msg from {}: {}", peer_id, e)));
                                        let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                            propagation_source: peer_id,
                                            message_id: _id,
                                            message: message.clone(), // Clone message needed here
                                        }))));
                                    }
                                }
                            }
                            // --- Handle Request/Response Events ---
                            SwarmEvent::Behaviour(SwapBytesBehaviourEvent::RequestResponse(event)) => {
                                use libp2p::request_response::{Event, Message};
                                match event {
                                    Event::Message { peer, message, connection_id: _ } => match message {
                                        Message::Request { request, channel, .. } => {
                                            match request {
                                                protocol::PrivateRequest::ChatMessage(text) => {
                                                    // Send PrivateMessageReceived event to UI task
                                                    if let Err(e) = swarm_tx.send(AppEvent::PrivateMessageReceived { 
                                                        sender_id: peer,
                                                        content: text,
                                                    }) {
                                                        // Log if sending to UI fails
                                                        eprintln!("[Swarm] Error sending PrivateMessageReceived to UI: {}", e);
                                                    }
                                                    
                                                    // Send Ack response
                                                    if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                    }
                                                }
                                                // Handle incoming file offers
                                                protocol::PrivateRequest::Offer { filename, size_bytes } => {
                                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Received Offer request from {}: File '{}' ({} bytes)", peer, filename, size_bytes)));
                                                    // Send FileOfferReceived event to UI task
                                                    if let Err(e) = swarm_tx.send(AppEvent::FileOfferReceived {
                                                        sender_id: peer,
                                                        filename: filename.clone(), // Clone filename
                                                        size_bytes,
                                                    }) {
                                                        eprintln!("[Swarm] Error sending FileOfferReceived to UI: {}", e);
                                                    }

                                                    // Send Ack response to confirm receipt of the offer message
                                                    if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                    }
                                                    // TODO: File Transfer - Step 2: Peer accepted our offer. Start listening/preparing to send file chunks.
                                                    //    - Need to know the file path corresponding to `filename`.
                                                    //    - Maybe initiate a separate stream or use Req/Res for chunk transfer?
                                                }
                                                // Handle incoming decline messages
                                                protocol::PrivateRequest::DeclineOffer { filename } => {
                                                    // Notify the UI that the offer was declined by the peer
                                                    if let Err(e) = swarm_tx.send(AppEvent::FileOfferDeclined { peer_id: peer, filename }) {
                                                        eprintln!("[Swarm] Error sending FileOfferDeclined to UI: {}", e);
                                                    }
                                                    // Acknowledge receipt of the decline message
                                                    if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                    }
                                                }
                                                // Handle incoming accept offer messages
                                                protocol::PrivateRequest::AcceptOffer { filename } => {
                                                    // Notify the UI that the offer was accepted by the peer
                                                    if let Err(e) = swarm_tx.send(AppEvent::FileOfferAccepted { peer_id: peer, filename }) {
                                                        eprintln!("[Swarm] Error sending FileOfferAccepted to UI: {}", e);
                                                    }
                                                    // Acknowledge receipt of the accept message
                                                    if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                    }
                                                    // TODO: File Transfer - Step 2: Peer accepted our offer. Start listening/preparing to send file chunks.
                                                    //    - Need to know the file path corresponding to `filename`.
                                                    //    - Maybe initiate a separate stream or use Req/Res for chunk transfer?
                                                }
                                                // Add other PrivateRequest variants later (e.g., Offer)
                                            }
                                        }
                                        Message::Response { request_id, response } => {
                                            match response {
                                                protocol::PrivateResponse::Ack => {
                                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Received Ack for request {:?} from {}", request_id, peer)));
                                                }
                                                protocol::PrivateResponse::AcceptOffer => {
                                                     // Received confirmation that our offer was accepted.
                                                     // The actual notification happens when the *requester* receives AcceptOffer request.
                                                     // This Ack just confirms *our* AcceptOffer *request* was received.
                                                     // Placeholder: Log that we received acceptance ack
                                                     // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Received Ack for AcceptOffer request {:?}", request_id)));
                                                }
                                                protocol::PrivateResponse::DeclineOffer => {
                                                     // Received confirmation that our offer was declined.
                                                     // The actual notification happens when the *requester* receives DeclineOffer request.
                                                     // This Ack just confirms *our* DeclineOffer *request* was received.
                                                     // Placeholder: Log that we received decline ack
                                                     // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Received Ack for DeclineOffer request {:?}", request_id)));
                                                }
                                                // Add other PrivateResponse variants later (e.g., Accept/Decline)
                                            }
                                            // Avoid request_id unused warning
                                            let _ = request_id;
                                        }
                                    }
                                    Event::OutboundFailure { peer, request_id, error, connection_id: _ } => {
                                        // Explicitly log outbound failures for RequestResponse
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Outbound RequestResponse Failure to {}: ReqID {:?}, Error: {}", peer, request_id, error)));
                                    }
                                    Event::InboundFailure { peer, request_id, error, connection_id: _ } => {
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Inbound RequestResponse failure from {}: Request {:?}, Error: {}", peer, request_id, error)));
                                    }
                                    Event::ResponseSent { peer, request_id, connection_id: _ } => {
                                        // Optional: Log when response is successfully sent
                                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Sent response for request {:?} to {}", request_id, peer)));

                                        let _ = peer;
                                        let _ = request_id;
                                    }
                                }
                            }
                            // Forward other behaviour events (like Ping) generically
                            SwarmEvent::Behaviour(other_behaviour_event) => {
                                // Check if it's NOT a Gossipsub or RequestResponse event before forwarding generically
                                if !matches!(other_behaviour_event, SwapBytesBehaviourEvent::Gossipsub(_) | SwapBytesBehaviourEvent::RequestResponse(_)) {
                                     let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(other_behaviour_event)));
                                } else {
                                    // Log ignored Gossipsub/ReqRes events
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Ignored specific Gossipsub/ReqRes event: {:?}", other_behaviour_event)));
                                }
                            }
                            // Forward non-behaviour swarm events (like NewListenAddr, ConnectionEstablished, etc.)
                            other_swarm_event => {
                                
                                // Log specific connection events
                                // match &other_swarm_event {
                                //     SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                                //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] ConnectionEstablished: {} ({:?})", peer_id, endpoint.get_remote_address())));
                                //     },
                                //     SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                                //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] ConnectionClosed: {} (Cause: {:?})", peer_id, cause)));
                                //     },
                                //     SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                                //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] OutgoingConnectionError: {:?} ({})", peer_id, error)));
                                //     },
                                //     SwarmEvent::IncomingConnectionError { error, local_addr, send_back_addr, .. } => {
                                //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] IncomingConnectionError: {} (Local: {:?}, Remote: {:?})", error, local_addr, send_back_addr)));
                                //     },
                                //     _ => {} // Ignore others for now
                                // }

                                let _ = swarm_tx.send(AppEvent::Swarm(other_swarm_event));
                            }
                        }
                    } else {
                        break; // Swarm stream ended
                    }
                }
            }
        }
    });


    // --- Background task: handle keyboard input ---
    let kb_tx = tx.clone(); // Clone tx for the keyboard task -> UI
    let kb_cancel = cancel.clone();
    tokio::spawn(async move {
        loop {
            if kb_cancel.is_cancelled() { break; }
            // Poll for key events every 150ms (non-blocking)
            if event::poll(Duration::from_millis(150)).unwrap() {
                if let event::Event::Key(key) = event::read().unwrap() {
                    // Use kb_tx here
                    if kb_tx.send(AppEvent::Input(key)).is_err() {
                        break; // UI gone
                    }
                }
            }
        }
    });


    // --- Application state and main event loop ---
    app.local_peer_id = Some(local_peer_id);
    app.push("Welcome to SwapBytes!".to_string());
    app.push("Run /help to get started.".to_string());
    let mut redraw = true; // Force initial draw

    // Timer for checking peer staleness in the UI task
    let mut check_peers_interval = interval(Duration::from_secs(5)); // Check every 5s

    loop {
        // --- Check Ping Timeout ---
        // Must be done *before* drawing or selecting
        if app.pinging {
            if let Some(start_time) = app.ping_start_time {
                // Use the constant from the tui module
                if start_time.elapsed() > tui::PINGING_DURATION {
                    app.pinging = false;
                    app.ping_start_time = None;
                    // Don't log timeout here, let ping result handle success/failure message
                    redraw = true; // Need redraw to update input title
                }
            } else {
                // Should not happen if pinging is true, reset defensively
                app.pinging = false;
                redraw = true;
            }
        }

        // Redraw UI only if something changed
        if redraw {
            terminal.draw(|f| {
                // --- Draw main application widget ---
                // We draw the app first using its Widget impl
                // This draws everything EXCEPT the stateful scrollbar
                f.render_widget(&app, f.area());

                // Calculate layout areas using layout_chunks helper
                let (chat_area, console_area, _users_area) = layout_chunks(f.area());

                // --- Calculate Console Log Area for scrollbar ---
                let console_block = Block::bordered(); // Temporary block for inner area calc
                let console_inner_area = console_block.inner(console_area);
                let console_chunks = Layout::vertical([
                    Constraint::Min(1),      // Log area
                    Constraint::Length(3), // Command input area
                ]).split(console_inner_area);
                let log_area = console_chunks[0];

                // --- Render Console Scrollbar ---
                // Update scrollbar state stored in app (ensure content len & viewport are correct)
                app.console_viewport_height = log_area.height as usize;

                // --- Calculate Chat Message Area and Update Viewport Height ---
                let chat_block = Block::bordered(); // Temporary block for inner area calc
                let chat_inner_area = chat_block.inner(chat_area);
                let chat_chunks_for_height = Layout::vertical([
                    Constraint::Min(1),      // Messages area
                    Constraint::Length(3), // Chat input area
                ]).split(chat_inner_area);
                let messages_area = chat_chunks_for_height[0];
                app.chat_viewport_height = messages_area.height as usize;

                // --- Set cursor position (only in Command mode) ---
                match app.input_mode {
                    InputMode::Normal => {} // Cursor hidden by default
                    InputMode::Command => {
                        // Command Input area is the second chunk of the console layout
                        let command_input_area = console_chunks[1];
                        f.set_cursor_position(Position::new(
                            command_input_area.x + app.cursor_position as u16 + 1, // +1 for border
                            command_input_area.y + 1, // +1 for border
                        ));
                    }
                    InputMode::Chat => {
                        // --- Calculate Chat Input Area for cursor (re-use chat_chunks_for_height) ---
                        // Note: We calculated chunks earlier to get messages_area height.
                        // Re-using the calculation here avoids redundant code.
                        let chat_input_area = chat_chunks_for_height[1];

                        // Set cursor for chat input
                        f.set_cursor_position(Position::new(
                            chat_input_area.x + app.chat_cursor_position as u16 + 1, // +1 for border
                            chat_input_area.y + 1, // +1 for border
                        ));
                    }
                }
            })?;
            redraw = false;
        }

        // --- Event Handling ---
        tokio::select! {
            // Handle events from Swarm or Keyboard tasks
            maybe_ev = rx.recv() => {
                if let Some(ev) = maybe_ev {
                    match ev {
                        AppEvent::Swarm(se) => {
                            // Only handle events forwarded from the swarm task here
                            match se { // No need for `&se` anymore as we own it
                                SwarmEvent::NewListenAddr { address, .. } => {
                                    // Store the address
                                    app.listening_addresses.push(address.clone());
                                    // app.push(format!("Listening on {address}"));
                                }
                                SwarmEvent::Behaviour(
                                    SwapBytesBehaviourEvent::Ping(ping::Event { peer, result, .. })
                                ) => {
                                    match result {
                                        Ok(latency) => {
                                            // Only log if we initiated the ping
                                            if app.pinging {
                                                app.push(format!("Successfully pinged peer: {peer} ({latency:?})"));
                                                // No need to reset pinging here, the timer handles it
                                            }
                                        }
                                        Err(e) => {
                                            // Only log if we initiated the ping
                                            if app.pinging {
                                                app.push(format!("Ping failed for peer: {peer} ({e:?})"));
                                                // No need to reset pinging here, the timer handles it
                                            }
                                        }
                                    }
                                    // Ping activity (even incoming pings, or failed outgoing ones)
                                    // should still update the peer's last seen time.
                                    if let Some(peer_info) = app.peers.get_mut(&peer) {
                                        peer_info.last_seen = Instant::now();
                                        peer_info.status = OnlineStatus::Online; // Mark online on successful ping
                                    }
                                }
                                // Mdns events are handled in the swarm task now
                                // SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Mdns(...)) => { ... }

                                // --- Handle forwarded Gossipsub Messages to update last_seen ---
                                // This ensures peers stay online if they are forwarding messages (e.g., heartbeats)
                                // It also re-adds peers to the list if they were forgotten.
                                SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                    propagation_source: peer_id, // The peer who forwarded the message
                                    .. // Ignore other fields here, we just need the source
                                })) => {
                                    // Use entry API to insert if not present or update if present
                                    let now = Instant::now();
                                    let peer_info = app.peers.entry(peer_id).or_insert_with(|| PeerInfo {
                                        nickname: None, // Nickname might be updated separately via NicknameUpdated event
                                        status: OnlineStatus::Online, // Assume online if we got a message
                                        last_seen: now,
                                    });
                                    // Update existing entry's status and last_seen
                                    peer_info.last_seen = now;
                                    peer_info.status = OnlineStatus::Online; // Mark online on any message received
                                    // If the peer is not in the map yet, PeerDiscovered or NicknameUpdated will handle adding them.
                                }

                                SwarmEvent::OutgoingConnectionError { error, .. } => {
                                    // Log in UI task as well
                                    // app.push(format!("[UI] Outgoing connection error: {error}"));
                                    let _ = error;
                                }
                                // Add logging for ConnectionEstablished and ConnectionClosed here
                                SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                                    // app.push(format!("[UI] Connection Established with: {} ({}) (Total: {})", peer_id, endpoint.get_remote_address(), num_established));
                                    // Mark peer as online immediately on connection
                                     if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                                        peer_info.status = OnlineStatus::Online;
                                        peer_info.last_seen = Instant::now(); // Also update last_seen
                                    }

                                    // Avoid unused variable warning
                                    let _ = endpoint;
                                    let _ = num_established;
                                }
                                SwarmEvent::ConnectionClosed { peer_id, cause, num_established, .. } => {
                                    // app.push(format!("[UI] Connection Closed with: {} (Cause: {:?}) (Remaining: {})", peer_id, cause, num_established));
                                    // Optionally mark as offline immediately on closure, depending on the cause
                                    // if cause.is_some() { // Only mark offline if there was an error? Or always? Let's mark always for now.
                                    //     if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                                    //         peer_info.status = OnlineStatus::Offline;
                                    //     }
                                    // }
                                    // Avoid unused variable warning
                                    let _ = peer_id;
                                    let _ = cause;
                                    let _ = num_established;
                                }
                                // Add other SwarmEvent variants as needed
                                _ => {
                                    // app.push(format!("Unhandled Swarm Event: {:?}", se));
                                }
                            }
                            redraw = true; // Redraw after any swarm event
                        }
                        AppEvent::Input(key) => {
                            // Handle Ctrl+q globally to quit
                            if key.kind == KeyEventKind::Press
                                && key.code == KeyCode::Char('q')
                                && key.modifiers.contains(event::KeyModifiers::CONTROL)
                            {
                                cancel.cancel();
                                app.exit = true;
                                continue; // Skip further processing for this event
                            }

                            // Handle based on input mode
                            match app.input_mode {
                                InputMode::Normal => { // Normal mode: focus switching, command/chat entry
                                    if key.kind == KeyEventKind::Press {
                                        match key.code {
                                            // Focus Switching
                                            // (if user hits Tab)
                                            KeyCode::Tab => {
                                                app.focused_pane = match app.focused_pane {
                                                    FocusPane::Console => FocusPane::UsersList, // Console -> Users
                                                    FocusPane::UsersList => FocusPane::Chat,
                                                    FocusPane::Chat => FocusPane::Console, // Chat -> Console
                                                };
                                                redraw = true;
                                            }
                                            // Enter Command Mode (if user hits /) - Removed focus check
                                            KeyCode::Char('/') => {
                                                app.focused_pane = FocusPane::Console; // Ensure console is focused
                                                app.input_mode = InputMode::Command;
                                                app.input.clear();
                                                app.input.push('/');
                                                app.cursor_position = 1;
                                                redraw = true;
                                            }
                                            // Console Scrolling (Up/Down)
                                            KeyCode::Up if app.focused_pane == FocusPane::Console => {
                                                app.console_scroll = app.console_scroll.saturating_sub(1);
                                                redraw = true;
                                            }
                                            KeyCode::Down if app.focused_pane == FocusPane::Console => {
                                                let max_scroll = app.log.len().saturating_sub(app.console_viewport_height);
                                                app.console_scroll = app.console_scroll.saturating_add(1).min(max_scroll);
                                                redraw = true;
                                            }
                                            // Chat Scrolling (Up/Down)
                                            KeyCode::Up if app.focused_pane == FocusPane::Chat => {
                                                app.chat_scroll = app.chat_scroll.saturating_sub(1);
                                                redraw = true;
                                            }
                                            KeyCode::Down if app.focused_pane == FocusPane::Chat => {
                                                // Calculate max_scroll based on chat history and viewport
                                                let max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height);
                                                app.chat_scroll = app.chat_scroll.saturating_add(1).min(max_scroll);
                                                redraw = true;
                                            }
                                            // Enter Chat Mode (if any char pressed and Chat focused)
                                            // Exclude Tab and potentially other control keys if needed
                                            KeyCode::Char(c) if app.focused_pane == FocusPane::Chat => {
                                                app.input_mode = InputMode::Chat;
                                                app.chat_input.clear();
                                                app.reset_chat_cursor();
                                                app.enter_chat_char(c);
                                                redraw = true;
                                            }
                                            _ => {} // Ignore other keys in normal mode
                                        }
                                    }
                                }
                                InputMode::Command => { // Command mode: handle command input
                                    if key.kind == KeyEventKind::Press {
                                        match key.code {
                                            KeyCode::Enter => {
                                                // submit_command now returns an optional event
                                                if let Some(event) = app.submit_command() {
                                                    match event {
                                                        AppEvent::Quit => {
                                                            // Handle Quit directly here
                                                            cancel.cancel();
                                                            app.exit = true;
                                                        }
                                                        // Send other commands (like Dial) to the swarm task
                                                        AppEvent::Dial(addr) => {
                                                            let _ = cmd_tx.send(AppEvent::Dial(addr));
                                                        }
                                                        // Send NicknameUpdated event to the swarm task
                                                        AppEvent::NicknameUpdated(peer_id, nickname) => {
                                                            let _ = cmd_tx.send(AppEvent::NicknameUpdated(peer_id, nickname));
                                                        }
                                                        // Send VisibilityChanged event to the swarm task
                                                        AppEvent::VisibilityChanged(is_visible) => {
                                                            app.push(format!("Command sent visibility change: {}", is_visible));
                                                            let _ = cmd_tx.send(AppEvent::VisibilityChanged(is_visible));
                                                        }
                                                        // Send SendFileOffer event to the swarm task
                                                        AppEvent::SendFileOffer { target_peer, file_path } => {
                                                            let _ = cmd_tx.send(AppEvent::SendFileOffer { target_peer, file_path });
                                                        }
                                                        // Send DeclineFileOffer event to the swarm task
                                                        AppEvent::DeclineFileOffer { target_peer, filename } => {
                                                            let _ = cmd_tx.send(AppEvent::DeclineFileOffer { target_peer, filename });
                                                        }
                                                        // Handle accepting file offers
                                                        AppEvent::SendAcceptOffer { target_peer, filename } => {
                                                            let _ = cmd_tx.send(AppEvent::SendAcceptOffer { target_peer, filename });
                                                        }
                                                        // Ignore any other event types potentially returned by submit_command
                                                        _ => {}
                                                    }
                                                }
                                                // Redraw only if we didn't just handle Quit
                                                if !app.exit {
                                                    redraw = true;
                                                }
                                            }
                                            KeyCode::Char(to_insert) => {
                                                app.enter_char(to_insert);
                                                redraw = true;
                                            }
                                            KeyCode::Backspace => {
                                                app.delete_char();
                                                redraw = true;
                                            }
                                            KeyCode::Left => {
                                                app.move_cursor_left();
                                                redraw = true;
                                            }
                                            KeyCode::Right => {
                                                app.move_cursor_right();
                                                redraw = true;
                                            }
                                            // Add scrolling for Command mode
                                            KeyCode::Up => {
                                                app.console_scroll = app.console_scroll.saturating_sub(1);
                                                redraw = true;
                                            }
                                            KeyCode::Down => {
                                                let max_scroll = app.log.len().saturating_sub(app.console_viewport_height);
                                                app.console_scroll = app.console_scroll.saturating_add(1).min(max_scroll);
                                                redraw = true;
                                            }
                                            KeyCode::Esc => {
                                                app.input_mode = InputMode::Normal;
                                                app.input.clear();
                                                app.reset_cursor();
                                                redraw = true;
                                            }
                                            KeyCode::Tab => {
                                                // Exit command mode, stay in console focus
                                                app.input_mode = InputMode::Normal;
                                                app.input.clear();
                                                app.reset_cursor();
                                                redraw = true;
                                            }
                                            _ => {} // Ignore other keys in Command mode
                                        }
                                    }
                                }
                                InputMode::Chat => { // Chat mode: handle chat input
                                    if key.kind == KeyEventKind::Press {
                                        match key.code {
                                            KeyCode::Enter => {
                                                // Only process if there's input and we are in global chat
                                                if !app.chat_input.is_empty() && app.current_chat_context == tui::ChatContext::Global {
                                                    let timestamp_ms = SystemTime::now()
                                                        .duration_since(UNIX_EPOCH)
                                                        .expect("Time went backwards")
                                                        .as_millis() as u64;
                                                    let nickname = app.nickname.clone();
                                                    let local_peer_id = app.local_peer_id.expect("Local PeerID must be set");
                                                    let content = app.chat_input.clone();

                                                    // Create the network message
                                                    let message = Message::GlobalChatMessage {
                                                        content: content.clone(),
                                                        timestamp_ms,
                                                        nickname: nickname.clone(),
                                                    };

                                                    // Serialize the message for the network
                                                    match serde_json::to_vec(&message) {
                                                        Ok(data) => {
                                                            // Send event to swarm task to publish
                                                            if let Err(e) = cmd_tx.send(AppEvent::PublishGossipsub(data)) {
                                                                app.push(format!("Error sending publish command: {}", e));
                                                            }

                                                            // Add the message to local history
                                                            let local_chat_msg = tui::ChatMessage {
                                                                sender_id: local_peer_id,
                                                                sender_nickname: nickname,
                                                                content,
                                                                timestamp_ms,
                                                            };
                                                            app.global_chat_history.push(local_chat_msg);

                                                            // Auto-scroll chat view IF already at the bottom
                                                            let current_max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height.max(1)).saturating_sub(1); // Max scroll before adding
                                                            if app.chat_scroll >= current_max_scroll {
                                                                let new_max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height.max(1));
                                                                app.chat_scroll = new_max_scroll;
                                                            }
                                                             // Otherwise, if user has scrolled up, don't force scroll down.

                                                            // Reset input field and mode
                                                            app.chat_input.clear();
                                                            app.reset_chat_cursor();
                                                            app.input_mode = InputMode::Normal;
                                                            redraw = true;

                                                        }
                                                        Err(e) => {
                                                            app.push(format!("Error serializing chat message: {}", e));
                                                            // Optionally, don't clear input on serialization error
                                                        }
                                                    }
                                                } else if !app.chat_input.is_empty() {
                                                    // Handle Private Chat Context
                                                    if let tui::ChatContext::Private { target_peer_id, .. } = app.current_chat_context {
                                                        let message_content = app.chat_input.clone();

                                                        // Send SendPrivateMessage event to swarm task
                                                        if let Err(e) = cmd_tx.send(AppEvent::SendPrivateMessage {
                                                            target_peer: target_peer_id,
                                                            message: message_content.clone(), // Clone needed for potential local history
                                                        }) {
                                                            app.push(format!("Error sending private message command: {}", e));
                                                        } else {
                                                            // app.push(format!("Sending private message to {:?}...", target_peer_id));
                                                            // Successfully sent event to swarm task, now add to local history
                                                            let timestamp_ms = SystemTime::now()
                                                                .duration_since(UNIX_EPOCH)
                                                                .expect("Time went backwards")
                                                                .as_millis() as u64;
                                                            let local_peer_id = app.local_peer_id.expect("Local PeerID must be set");
                                                            // Note: Use app.nickname (our own nickname) for sender_nickname here.
                                                            let chat_msg = tui::ChatMessage {
                                                                sender_id: local_peer_id,
                                                                sender_nickname: app.nickname.clone(),
                                                                content: message_content, // Use the cloned content
                                                                timestamp_ms,
                                                            };
                                                            
                                                            let history = app.private_chat_histories.entry(target_peer_id).or_default();
                                                            let current_len = history.len(); // Length before adding
                                                            history.push(tui::PrivateChatItem::Message(chat_msg));

                                                            // Auto-scroll (since we are viewing this chat)
                                                            let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                                                            if app.chat_scroll >= current_max_scroll {
                                                                let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                                                app.chat_scroll = new_max_scroll;
                                                            }
                                                        }

                                                        // Clear input and return to normal mode (do this regardless of send success)
                                                        app.chat_input.clear();
                                                        app.reset_chat_cursor();
                                                        app.input_mode = InputMode::Normal;
                                                        redraw = true;

                                                    } else {
                                                        // Should not happen if context is not Global, but handle defensively
                                                        app.push("Error: Cannot send message in unknown context.".to_string());
                                                        app.input_mode = InputMode::Normal; // Reset mode
                                                        redraw = true;
                                                    }
                                                } else {
                                                    // Input is empty, just go back to normal mode
                                                    app.input_mode = InputMode::Normal;
                                                    redraw = true;
                                                }
                                            }
                                            KeyCode::Char(to_insert) => {
                                                app.enter_chat_char(to_insert);
                                                redraw = true;
                                            }
                                            KeyCode::Backspace => {
                                                app.delete_chat_char();
                                                redraw = true;
                                            }
                                            KeyCode::Left => {
                                                app.move_chat_cursor_left();
                                                redraw = true;
                                            }
                                            KeyCode::Right => {
                                                app.move_chat_cursor_right();
                                                redraw = true;
                                            }
                                            // Add scrolling for Chat mode
                                            KeyCode::Up => {
                                                app.chat_scroll = app.chat_scroll.saturating_sub(1);
                                                redraw = true;
                                            }
                                            KeyCode::Down => {
                                                let max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height);
                                                app.chat_scroll = app.chat_scroll.saturating_add(1).min(max_scroll);
                                                redraw = true;
                                            }
                                            KeyCode::Esc => {
                                                // Exit chat mode without sending
                                                app.input_mode = InputMode::Normal;
                                                app.chat_input.clear();
                                                app.reset_chat_cursor();
                                                redraw = true;
                                            }
                                            KeyCode::Tab => {
                                                // Exit chat mode, stay in chat focus
                                                app.input_mode = InputMode::Normal;
                                                app.chat_input.clear();
                                                app.reset_chat_cursor();
                                                redraw = true;
                                            }
                                            _ => {} // Ignore other keys in Chat mode
                                        }
                                    }
                                }
                            }
                        }
                        AppEvent::LogMessage(msg) => {
                            app.push(msg);
                            redraw = true;
                        }
                        AppEvent::PeerDiscovered(peer_id) => {
                            if peer_id != app.local_peer_id.expect("Local peer ID should be set") { // Don't add self
                                let now = Instant::now();
                                let peer_info = app.peers.entry(peer_id).or_insert_with(|| PeerInfo {
                                    nickname: None, // Nickname unknown initially
                                    status: OnlineStatus::Online,
                                    last_seen: now, // Set last_seen on discovery
                                });
                                // Modify the entry (whether newly inserted or existing)
                                peer_info.last_seen = now;
                                peer_info.status = OnlineStatus::Online;
                                redraw = true;
                            }
                        }
                        AppEvent::PeerExpired(peer_id) => {
                            // We no longer rely on mDNS expiry for primary status
                            // If we kept this, we would mark offline here, but heartbeat check is better
                            // if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                            //     peer_info.status = OnlineStatus::Offline;
                            // }
                            // app.push(format!("mDNS Expired (ignored for status): {peer_id}"));
                            let _ = peer_id; // Avoid unused variable warning
                            // redraw = true; // Don't redraw if we ignore it
                        }
                        AppEvent::NicknameUpdated(peer_id, new_nickname) => {
                            // Ignore updates for self
                            if Some(peer_id) == app.local_peer_id {
                                // We still need to update our own nickname in the swarm task if changed via command
                                // but the UI state (app.nickname) is already updated by the command handler.
                                // The swarm task gets a dedicated message for this.
                            } else if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                                let old_nickname_opt = peer_info.nickname.clone(); // Clone old nickname option
                                let new_nickname_opt = Some(new_nickname.clone()); // Wrap new nickname in Option

                                // Check if the nickname actually changed and wasn't initially None
                                let should_log = old_nickname_opt != new_nickname_opt && old_nickname_opt.is_some();

                                // Always update the nickname in the peer info *before* logging
                                peer_info.nickname = Some(new_nickname.clone()); // Update with cloned new nickname
                                redraw = true;

                                // Now log if necessary (app is no longer borrowed mutably by peer_info)
                                if should_log {
                                    let old_name = old_nickname_opt.unwrap_or_else(|| "Unknown".to_string()); // Should be Some due to check
                                    let id_str = peer_id.to_base58();
                                    let len = id_str.len();
                                    let id_suffix = format!("(...{})", &id_str[len.saturating_sub(6)..]);
                                    // Use the already updated nickname from the `new_nickname` variable
                                    app.push(format!("Peer changed nickname: {} → {} {}", old_name, new_nickname, id_suffix));
                                }

                                // --- Update Chat Title if viewing private chat with this peer ---
                                if let tui::ChatContext::Private { target_peer_id, target_nickname } = &mut app.current_chat_context {
                                    if *target_peer_id == peer_id {
                                        *target_nickname = Some(new_nickname.clone()); // Update the nickname in the context
                                    }
                                }

                                // --- Update Private Chat History for this peer ---
                                if let Some(history) = app.private_chat_histories.get_mut(&peer_id) {
                                    for item in history.iter_mut() {
                                        if let tui::PrivateChatItem::Message(message) = item {
                                            // Only update messages sent *by* this peer
                                            if message.sender_id == peer_id {
                                                message.sender_nickname = Some(new_nickname.clone());
                                            }
                                        }
                                    }
                                }

                                // --- Update Global Chat History for this peer ---
                                for message in app.global_chat_history.iter_mut() {
                                    // Only update messages sent *by* this peer
                                    if message.sender_id == peer_id {
                                        message.sender_nickname = Some(new_nickname.clone());
                                    }
                                }

                            }
                            // Optionally log if the peer wasn't found, but for now, just ignore it.
                        }
                        AppEvent::Dial(_) => {} // Handled by swarm task
                        AppEvent::Quit => {} // Already handled in Command mode Enter
                        AppEvent::VisibilityChanged(_) => {} // Handled by swarm task
                        AppEvent::EnterChat(_msg) => {
                            // Handle submitting a chat message
                            // For now, just log it to the console
                            // app.push(format!("[CHAT SUBMITTED] {}", msg));
                            redraw = true;
                        }
                        AppEvent::GlobalMessageReceived { sender_id, sender_nickname, content, timestamp_ms } => {
                            // app.log(format!("GlobalMessageReceived: {content} {timestamp_ms} from {sender_id}"));
                            // Create the chat message struct
                            let chat_msg = tui::ChatMessage {
                                sender_id,
                                sender_nickname: sender_nickname.clone(), // Clone nickname
                                content,
                                timestamp_ms,
                            };
                            // Log that we processed this specific event
                            // app.log(format!("[UI] Processed GlobalMessage from {} ({})",
                            //     sender_nickname.unwrap_or_else(|| format!("PeerID:{}", sender_id)),
                            //     content));

                            // Add to history
                            app.global_chat_history.push(chat_msg);

                            // --- Add notification if user is in a private chat ---
                            if let tui::ChatContext::Private { .. } = app.current_chat_context {
                                let sender_display_name = sender_nickname.clone().unwrap_or_else(|| {
                                    let id_str = sender_id.to_base58();
                                    let len = id_str.len();
                                    format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                });
                                app.push(format!("{} sent a global message!", sender_display_name));
                            }

                            // Auto-scroll chat view IF already at the bottom
                            let current_max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height.max(1)).saturating_sub(1); // Max scroll before adding
                            if app.chat_scroll >= current_max_scroll {
                                let new_max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height.max(1));
                                app.chat_scroll = new_max_scroll;
                            }
                             // Otherwise, if user has scrolled up, don't force scroll down.

                            redraw = true;
                        }
                        AppEvent::PublishGossipsub(_) => {
                            // This event is sent TO the swarm task, should not be received here.
                            // Log if it happens, but otherwise ignore.
                            app.log("Warning: Received PublishGossipsub event in UI loop.".to_string());
                        }
                        AppEvent::SendPrivateMessage { .. } => {
                            // This event is sent TO the swarm task, should not be received here.
                            app.log("Warning: Received SendPrivateMessage event in UI loop.".to_string());
                        }
                        AppEvent::PrivateMessageReceived { sender_id, content } => {
                            // app.log(format!("[UI Task] Received PrivateMessageReceived event from {}", sender_id));
                            // Get sender's nickname from peers map (if known)
                            let sender_nickname = app.peers.get(&sender_id).and_then(|info| info.nickname.clone());

                            // Get current timestamp
                            let timestamp_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .expect("Time went backwards")
                                .as_millis() as u64;

                            // Create the chat message struct
                            let chat_msg = tui::ChatMessage {
                                sender_id,
                                sender_nickname, // May be None
                                content,
                                timestamp_ms,
                            };

                            // Add message to the corresponding private history
                            let history = app.private_chat_histories.entry(sender_id).or_default();
                            let current_len = history.len(); // Get length before adding
                            history.push(tui::PrivateChatItem::Message(chat_msg));

                            // Auto-scroll if the user is currently viewing this private chat
                            let mut notify_in_console = true; // Assume notification needed by default
                            if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                                if *target_peer_id == sender_id {
                                    notify_in_console = false; // Don't notify if already viewing
                                    let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                                    if app.chat_scroll >= current_max_scroll {
                                        let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                        app.chat_scroll = new_max_scroll;
                                    }
                                }
                            }

                            // Add notification to console log if not viewing the private chat
                            if notify_in_console {
                                let sender_display_name = app.peers.get(&sender_id)
                                    .and_then(|info| info.nickname.clone())
                                    .unwrap_or_else(|| {
                                        let id_str = sender_id.to_base58();
                                        let len = id_str.len();
                                        format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                    });
                                app.push(format!("{} sent you a private message!", sender_display_name));
                            }

                            redraw = true;
                        }
                        AppEvent::FileOfferReceived { sender_id, filename, size_bytes } => {
                            // Get sender's display name (used for console notification)
                            let sender_display_name = app.peers.get(&sender_id)
                                .and_then(|info| info.nickname.clone())
                                .unwrap_or_else(|| {
                                    let id_str = sender_id.to_base58();
                                    let len = id_str.len();
                                    format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                });

                            // Check if we are currently viewing the private chat with the sender
                            let mut is_viewing_chat = false;
                            if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                                if *target_peer_id == sender_id {
                                    is_viewing_chat = true;
                                }
                            }

                            // Store/overwrite the pending offer details globally
                            let offer_details = crate::tui::PendingOfferDetails {
                                filename: filename.clone(), // Clone filename needed for both maps
                                size_bytes,
                            };
                            app.pending_offers.insert(sender_id, offer_details.clone());

                            // ALWAYS add the offer to the specific private chat history
                            let history = app.private_chat_histories.entry(sender_id).or_default();
                            let current_len = history.len(); // Get length *before* adding for scroll calculation
                            history.push(crate::tui::PrivateChatItem::Offer(offer_details)); // Push the cloned details

                            // Decide whether to notify in console or auto-scroll chat
                            if !is_viewing_chat {
                                // Show notification in console log
                                app.push(format!(
                                    "{} sent you a file offer: {} ({})",
                                    sender_display_name,
                                    filename, // Use the original filename from the event
                                    crate::utils::format_bytes(size_bytes) // Use formatter
                                ));
                            } else {
                                // If viewing the chat, auto-scroll if we were already near the bottom
                                let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                                if app.chat_scroll >= current_max_scroll {
                                    let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                    app.chat_scroll = new_max_scroll;
                                }
                            }

                            redraw = true;
                        }
                        AppEvent::SendFileOffer { .. } => {
                            // This event is sent TO the swarm task, should not be received here.
                            app.log("Warning: Received SendFileOffer event in UI loop.".to_string());
                        }
                        AppEvent::FileOfferDeclined { peer_id, filename } => {
                            // Get peer's display name
                            let peer_display_name = app.peers.get(&peer_id)
                                .and_then(|info| info.nickname.clone())
                                .unwrap_or_else(|| {
                                    let id_str = peer_id.to_base58();
                                    let len = id_str.len();
                                    format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                });

                            // Add message to console
                            app.push(format!("{} declined your offer for '{}'.", peer_display_name, filename));

                            // Add message to the private chat history for that peer
                            if let Some(history) = app.private_chat_histories.get_mut(&peer_id) {
                                // Find the original OfferSent details to add RemoteOfferDeclined
                                // We need to iterate to find the size bytes associated with the filename
                                let mut offer_details_opt: Option<tui::PendingOfferDetails> = None;
                                for item in history.iter() {
                                    if let tui::PrivateChatItem::OfferSent(details) = item {
                                        if details.filename == filename {
                                            offer_details_opt = Some(details.clone());
                                            break; // Found the matching offer
                                        }
                                    }
                                }

                                if let Some(offer_details) = offer_details_opt {
                                    let current_len = history.len(); // Get length *before* adding
                                    history.push(tui::PrivateChatItem::RemoteOfferDeclined(offer_details));

                                    // Auto-scroll if viewing this chat
                                    if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                                        if *target_peer_id == peer_id {
                                            let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                                            if app.chat_scroll >= current_max_scroll {
                                                let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                                app.chat_scroll = new_max_scroll;
                                            }
                                        }
                                    }
                                } else {
                                    // Log if we couldn't find the original offer details (should not normally happen)
                                    app.log(format!("Warning: Could not find original OfferSent details for declined file '{}' from {}", filename, peer_display_name));
                                }
                            } else {
                                // Log if no history exists (also unusual if an offer was sent)
                                app.log(format!("Warning: No private chat history found for peer {} who declined file '{}'.", peer_display_name, filename));
                            }

                            redraw = true;
                        }
                        AppEvent::FileOfferAccepted { peer_id, filename } => {
                            // Get peer's display name
                            let peer_display_name = app.peers.get(&peer_id)
                                .and_then(|info| info.nickname.clone())
                                .unwrap_or_else(|| {
                                    let id_str = peer_id.to_base58();
                                    let len = id_str.len();
                                    format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                });

                            // Add message to console
                            app.push(format!("{} accepted your offer for '{}'.", peer_display_name, filename));

                            // Add message to the private chat history for that peer
                            if let Some(history) = app.private_chat_histories.get_mut(&peer_id) {
                                // Find the original OfferSent details to add RemoteOfferAccepted
                                let mut offer_details_opt: Option<tui::PendingOfferDetails> = None;
                                for item in history.iter() {
                                    if let tui::PrivateChatItem::OfferSent(details) = item {
                                        if details.filename == filename {
                                            offer_details_opt = Some(details.clone());
                                            break; // Found the matching offer
                                        }
                                    }
                                }

                                if let Some(offer_details) = offer_details_opt {
                                    let current_len = history.len(); // Get length *before* adding
                                    history.push(tui::PrivateChatItem::RemoteOfferAccepted(offer_details));

                                    // Auto-scroll if viewing this chat
                                    if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                                        if *target_peer_id == peer_id {
                                            let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                                            if app.chat_scroll >= current_max_scroll {
                                                let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                                app.chat_scroll = new_max_scroll;
                                            }
                                        }
                                    }
                                } else {
                                    app.log(format!("Warning: Could not find original OfferSent details for accepted file '{}' from {}", filename, peer_display_name));
                                }
                            } else {
                                app.log(format!("Warning: No private chat history found for peer {} who accepted file '{}'.", peer_display_name, filename));
                            }

                            redraw = true;
                        }
                        AppEvent::SendAcceptOffer { .. } => {
                            app.log("Warning: Received unexpected SendAcceptOffer event in UI loop.".to_string());
                            redraw = true;
                        }
                        AppEvent::DeclineFileOffer { .. } => {
                            app.log("Warning: Received unexpected DeclineFileOffer event in UI loop.".to_string());
                            redraw = true;
                        }
                    }
                } else {
                    // Channel closed, exit
                    app.exit = true;
                }
            },

            // --- Check for Stale Peers ---
            _ = check_peers_interval.tick() => {
                let mut changed = false;
                let now = Instant::now();
                let timeout = Duration::from_secs(PEER_TIMEOUT_SECS);
                let mut timed_out_peers = Vec::new(); // Collect timed out peers

                for (peer_id, peer_info) in app.peers.iter_mut() { // Iterate mutably
                    if peer_info.status == OnlineStatus::Online && now.duration_since(peer_info.last_seen) > timeout {
                        peer_info.status = OnlineStatus::Offline;
                        changed = true;
                        timed_out_peers.push(*peer_id); // Store the PeerId
                        // Log when timeout occurs - MOVED outside loop
                        // app.push(format!("[UI] Marked peer {:?} offline due to timeout (> {:?})", peer_id, timeout));
                    }
                }

                // Log timed out peers after the loop
                // if !timed_out_peers.is_empty() {
                //     let peer_ids_str = timed_out_peers.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", ");
                //     app.push(format!("[UI] Marked peers offline due to timeout (> {:?}): {}", timeout, peer_ids_str));
                // }

                if changed {
                    redraw = true;
                }
            }
        }

        if app.exit {
            break;
        }
    }


    // --- Restore terminal to original state ---
    ratatui::restore();

    Ok(())
}
