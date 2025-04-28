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
use tui::{App, AppEvent, InputMode, FocusPane, layout_chunks, PeerInfo, OnlineStatus}; // Add PeerInfo/OnlineStatus
use behavior::{SwapBytesBehaviour, SwapBytesBehaviourEvent};

/// Gossipsub topic for SwapBytes
const SWAPBYTES_TOPIC: &str = "swapbytes-global-chat";
/// Interval for sending heartbeats
const HEARTBEAT_INTERVAL_SECS: u64 = 2;
/// Timeout duration for marking peers offline
const PEER_TIMEOUT_SECS: u64 = 8;

// --- Define Message Types ---
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")] // Use a 'type' field to distinguish message types
enum Message {
    Heartbeat {
        timestamp_ms: u64, // Add timestamp field
        nickname: Option<String>, // Add optional nickname
    },
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
                        // Log
                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("Sending heartbeat.")));
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
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to deserialize gossipsub message: {e}")));
                                        let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                            propagation_source: peer_id,
                                            message_id: _id,
                                            message: message.clone(), // Clone message needed here
                                        }))));
                                    }
                                }
                            }
                            // Forward other behaviour events (like Ping) generically
                            SwarmEvent::Behaviour(other_behaviour_event) => {
                                // Check if it's NOT a Gossipsub event before forwarding generically
                                // (We handled Gossipsub::Message specifically above)
                                if !matches!(other_behaviour_event, SwapBytesBehaviourEvent::Gossipsub(_)) {
                                     let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(other_behaviour_event)));
                                }
                                // Ignore other Gossipsub event types for now (like Subscribed, Unsubscribed)
                            }
                            // Forward non-behaviour swarm events (like NewListenAddr, ConnectionEstablished, etc.)
                            other_swarm_event => {
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
                                    // This logging is commented out to hide a harmless "Failed to negotiate transport protocol(s)" error
                                    // Connection eventually succeeds via the LAN address, so the error can be safely ignored.
                                    // This only happens during same-machine mDNS testing, therefore is not a real issue.
                                    // app.push(format!("Outgoing connection error: {error}"));

                                    // do something with the error to avoid unused warning
                                    let _ = error;

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
                                                        // Ignore any other event types for now
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
                                                } else if !app.chat_input.is_empty() && matches!(app.current_chat_context, tui::ChatContext::Private { .. }) {
                                                    // Placeholder for Private Chat sending logic
                                                    app.push(format!("[PRIVATE CHAT (not implemented)] {}", app.chat_input));
                                                    // Clear input and return to normal mode (temporary)
                                                    app.chat_input.clear();
                                                    app.reset_chat_cursor();
                                                    app.input_mode = InputMode::Normal;
                                                    redraw = true;
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
                        AppEvent::NicknameUpdated(peer_id, nickname) => {
                            if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                                peer_info.nickname = Some(nickname);
                                redraw = true;
                            }
                            // Optionally log if the peer wasn't found, but for now, just ignore it.
                        }
                        AppEvent::Dial(_) => {} // Handled by swarm task
                        AppEvent::Quit => {} // Already handled in Command mode Enter
                        AppEvent::VisibilityChanged(_) => {} // Handled by swarm task
                        AppEvent::EnterChat(msg) => {
                            // Handle submitting a chat message
                            // For now, just log it to the console
                            // TODO: Send this message over gossipsub
                            app.push(format!("[CHAT SUBMITTED] {}", msg));
                            redraw = true;
                        }
                        AppEvent::GlobalMessageReceived { sender_id, sender_nickname, content, timestamp_ms } => {
                            // app.log(format!("GlobalMessageReceived: {content} {timestamp_ms} from {sender_id}"));
                            // Create the chat message struct
                            let chat_msg = tui::ChatMessage {
                                sender_id,
                                sender_nickname,
                                content,
                                timestamp_ms,
                            };
                            // Add to history
                            app.global_chat_history.push(chat_msg);

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

                for peer_info in app.peers.values_mut() {
                    if peer_info.status == OnlineStatus::Online && now.duration_since(peer_info.last_seen) > timeout {
                        peer_info.status = OnlineStatus::Offline;
                        changed = true;
                    }
                }

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
