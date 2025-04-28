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
    symbols,
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
                            // Forward other events (including Gossipsub messages, Ping, etc.) to the UI task
                            other_event => {
                                let _ = swarm_tx.send(AppEvent::Swarm(other_event));
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

                // Calculate Console Log Area using layout_chunks helper
                let (_chat_area, console_area, _users_area) = layout_chunks(f.area());

                let console_block = Block::bordered().border_set(symbols::border::THICK);
                let console_inner_area = console_block.inner(console_area);
                let console_chunks = Layout::vertical([
                    Constraint::Min(1),
                    Constraint::Length(3),
                ])
                .split(console_inner_area);
                let log_area = console_chunks[0];

                // --- Render Console Scrollbar --- 
                // Update scrollbar state stored in app (ensure content len & viewport are correct)
                app.console_viewport_height = log_area.height as usize;

                // --- Set cursor position (only in Command mode) ---
                match app.input_mode {
                    InputMode::Normal => {} // Cursor hidden by default
                    InputMode::Command => {
                        // Input area calculation (already done above for scrollbar)
                        let input_area = console_chunks[1];
                        f.set_cursor_position(Position::new(
                            input_area.x + app.cursor_position as u16 + 1,
                            input_area.y + 1,
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

                                SwarmEvent::Behaviour(
                                    SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                        propagation_source: peer_id,
                                        message_id: id,
                                        message,
                                    })
                                ) => {
                                    // Use `propagation_source` (peer_id) to update the status of the *forwarding* peer.
                                    let forwarder_peer_id = peer_id;
                                    // Avoid the unused warnings
                                    let _ = id;

                                    // Update the forwarder's status/last_seen
                                    let now = Instant::now();
                                    let forwarder_info = app.peers.entry(forwarder_peer_id).or_insert_with(|| {
                                        // Insert if new
                                        PeerInfo {
                                            nickname: None, // Nickname unknown until exchanged
                                            status: OnlineStatus::Online,
                                            last_seen: now,
                                        }
                                    });
                                    // Modify the entry (whether newly inserted or existing)
                                    forwarder_info.last_seen = now;
                                    forwarder_info.status = OnlineStatus::Online;

                                    // Attempt to deserialize the message data
                                    match serde_json::from_slice::<Message>(&message.data) {
                                        Ok(msg_content) => {
                                            // Use message.source (original sender) for nickname association
                                            if let Some(original_sender_peer_id) = message.source {
                                                // Also update original sender's status/last_seen
                                                let original_sender_info = app.peers.entry(original_sender_peer_id).or_insert_with(|| {
                                                    PeerInfo {
                                                        nickname: None, // Nickname unknown until exchanged
                                                        status: OnlineStatus::Online,
                                                        last_seen: now,
                                                    }
                                                });
                                                original_sender_info.last_seen = now;
                                                original_sender_info.status = OnlineStatus::Online;

                                                match msg_content {
                                                    Message::Heartbeat { nickname: Some(received_nickname), .. } => {
                                                        // Update nickname for the *original sender* if different
                                                        if original_sender_info.nickname.as_ref() != Some(&received_nickname) {
                                                            original_sender_info.nickname = Some(received_nickname);
                                                            // redraw will be set outside the match
                                                        }
                                                    }
                                                    // Handle other message types here later (e.g., Chat)
                                                    _ => {}
                                                }
                                            } else {
                                                // This shouldn't happen with signed messages, but handle defensively.
                                                app.push(format!(
                                                    "Received message without source from {forwarder_peer_id}"
                                                ));
                                            }
                                        }
                                        Err(e) => {
                                            // Log deserialization errors, but don't crash
                                            app.push(format!(
                                                "Error deserializing message from {forwarder_peer_id}: {e}"
                                            ));
                                        }
                                    }

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
                            // Always handle Ctrl+q first
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
                                InputMode::Normal => {
                                    if key.kind == KeyEventKind::Press {
                                        match key.code {
                                            // Focus Switching
                                            // (if user hits Tab)
                                            KeyCode::Tab => {
                                                app.focused_pane = match app.focused_pane {
                                                    FocusPane::Chat => FocusPane::Console,
                                                    FocusPane::Console => FocusPane::UsersList,
                                                    FocusPane::UsersList => FocusPane::Chat,
                                                };
                                                redraw = true;
                                            }
                                            // Enter Command Mode 
                                            // (if user hits / and Console focused)
                                            KeyCode::Char('/') if app.focused_pane == FocusPane::Console => {
                                                app.input_mode = InputMode::Command;
                                                app.input.clear();
                                                app.input.push('/');
                                                app.cursor_position = 1;
                                                redraw = true;
                                            }
                                            // Scrolling 
                                            // (if user hits Up or Down and Console focused)
                                            KeyCode::Up if app.focused_pane == FocusPane::Console => {
                                                app.console_scroll = app.console_scroll.saturating_sub(1);
                                                redraw = true;
                                            }
                                            KeyCode::Down if app.focused_pane == FocusPane::Console => {
                                                let max_scroll = app.log.len().saturating_sub(app.console_viewport_height);
                                                app.console_scroll = app.console_scroll.saturating_add(1).min(max_scroll);
                                                redraw = true;
                                            }
                                            _ => {} // Ignore other keys in normal mode
                                        }
                                    }
                                }
                                InputMode::Command => {
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
                                            KeyCode::Esc => {
                                                app.input_mode = InputMode::Normal;
                                                app.input.clear();
                                                app.reset_cursor();
                                                redraw = true;
                                            }
                                            _ => {} // Ignore other keys in Command mode
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
