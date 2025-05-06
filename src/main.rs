/*
Main entry point for the SwapBytes application.
*/

// --- Standard Library Imports ---
use std::{error::Error, time::Duration, time::Instant};

// --- Async and Tokio Imports ---
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio::time::interval;

// --- libp2p Imports ---
use libp2p::{noise, tcp, yamux, identity, PeerId, gossipsub};

// --- Terminal UI Imports ---
use crossterm::event;
use ratatui::{
    layout::{Constraint, Layout, Position},
    widgets::Block,
};

// --- Local Module Imports ---
mod swarm_task;
mod tui;
mod utils;
mod commands;
mod behavior;
mod protocol;
mod constants;
mod input_handler;
mod event_handler;
use tui::{App, AppEvent, InputMode, layout_chunks, OnlineStatus};
use behavior::SwapBytesBehaviour;


// --- Application Entry Point ---
/// Sets up the terminal UI, libp2p swarm, and the main application event loop.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // --- Initialize App State ---
    // Creates the central state structure for the application.
    let mut app = App::default();

    // --- Terminal UI Setup ---
    // Initializes the terminal interface using ratatui.
    let mut terminal = ratatui::init();


    // --- Generate Local Peer Identity ---
    // Creates a unique cryptographic keypair for this node.
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // --- Communication Channels ---
    // Channel for events from background tasks (Swarm, Keyboard) to the UI loop.
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();
    // Token to signal cancellation to background tasks.
    let cancel = CancellationToken::new();
    // Channel for commands from the UI loop to the Swarm task.
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<AppEvent>();


    // --- libp2p Swarm Setup ---
    // 1. Create the custom Swarm behaviour using the generated key.
    let mut behaviour = SwapBytesBehaviour::new(&local_key)?;

    // 2. Define the Gossipsub topic for chat messages and subscribe to it.
    let topic = gossipsub::IdentTopic::new(constants::SWAPBYTES_TOPIC);
    behaviour.gossipsub.subscribe(&topic)?;

    // 3. Build the libp2p Swarm, configuring transport (TCP, Noise, Yamux) and behaviour.
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .build();

    // Listen for incoming connections on all network interfaces using a random OS-assigned port.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;


    // --- Spawn Swarm Task ---
    // Clone necessary variables for the swarm task.
    let swarm_tx = tx.clone();
    let swarm_cancel = cancel.clone();
    let initial_nickname = app.nickname.clone(); // Pass initial state to the task
    let initial_visibility = app.is_visible;

    // Spawn a dedicated asynchronous task to manage the libp2p Swarm.
    // This task handles network events, peer discovery, and message propagation.
    // See `swarm_task::run_swarm_loop` for implementation details.
    tokio::spawn(swarm_task::run_swarm_loop(
        swarm,
        swarm_tx,
        cmd_rx,
        swarm_cancel,
        initial_nickname,
        initial_visibility,
    ));


    // --- Spawn Keyboard Input Task ---
    // Clone necessary variables for the keyboard input task.
    let kb_tx = tx.clone();
    let kb_cancel = cancel.clone();
    // Spawn a dedicated asynchronous task to listen for keyboard events.
    tokio::spawn(async move {
        loop {
            // Check for cancellation signal.
            if kb_cancel.is_cancelled() { break; }
            // Poll for keyboard events with a short timeout to avoid blocking.
            if event::poll(Duration::from_millis(150)).unwrap() {
                // If a key event occurs, read it and send it to the main UI loop via the channel.
                if let event::Event::Key(key) = event::read().unwrap() {
                    if kb_tx.send(AppEvent::Input(key)).is_err() {
                        // Stop the task if the channel is closed.
                        break;
                    }
                }
            }
        }
    });


    // --- Initialize Application State & Start Main Loop ---
    // Set the local peer ID in the application state.
    app.local_peer_id = Some(local_peer_id);
    // Add initial messages to the console log.
    app.push("Welcome to SwapBytes!".to_string());
    app.push("Run /help to get started.".to_string());
    // Flag to indicate whether the UI needs to be redrawn.
    let mut redraw = true;

    // --- Peer Staleness Check Timer ---
    // Set up a timer to periodically check for inactive peers.
    let mut check_peers_interval = interval(Duration::from_secs(5));

    // --- Main Event Loop ---
    loop {
        // --- Check Ping Timeout ---
        // This check must happen before drawing or event processing.
        if app.pinging {
            if let Some(start_time) = app.ping_start_time {
                // Check if the ping duration has exceeded the timeout.
                if start_time.elapsed() > constants::PINGING_DURATION {
                    app.pinging = false;
                    app.ping_start_time = None;
                    // The ping result itself will log success/failure.
                    redraw = true;
                }
            } else {
                // Reset state defensively if ping_start_time is None while pinging is true.
                app.pinging = false;
                redraw = true;
            }
        }

        // --- UI Rendering ---
        // Redraw the terminal UI only if the state has changed.
        if redraw {
            terminal.draw(|f| {
                // Draw the main application widget, which handles most UI elements.
                // Note: The `App` struct implements the `ratatui::widgets::Widget` trait.
                f.render_widget(&app, f.area());

                // --- Layout Calculation ---
                // Divide the terminal area into sections for chat, console, and users.
                let (chat_area, console_area, _users_area) = layout_chunks(f.area());

                // --- Console Area Calculation ---
                // Calculate the inner area of the console block, excluding borders.
                let console_block = Block::bordered();
                let console_inner_area = console_block.inner(console_area);
                // Divide the console area into log display and command input sections.
                let console_chunks = Layout::vertical([
                    Constraint::Min(1),      // Area for displaying logs.
                    Constraint::Length(3), // Area for command input.
                ]).split(console_inner_area);
                let log_area = console_chunks[0];

                // Update the console viewport height in the app state for scrollbar calculation.
                app.console_viewport_height = log_area.height as usize;

                // --- Chat Area Calculation ---
                // Calculate the inner area of the chat block, excluding borders.
                let chat_block = Block::bordered();
                let chat_inner_area = chat_block.inner(chat_area);
                 // Divide the chat area into message display and chat input sections.
                let chat_chunks_for_height = Layout::vertical([
                    Constraint::Min(1),      // Area for displaying chat messages.
                    Constraint::Length(3), // Area for chat input.
                ]).split(chat_inner_area);
                let messages_area = chat_chunks_for_height[0];
                // Update the chat viewport height in the app state.
                app.chat_viewport_height = messages_area.height as usize;

                // --- Cursor Positioning ---
                // Set the terminal cursor position based on the current input mode.
                match app.input_mode {
                    InputMode::Normal => {} // Cursor is hidden in Normal mode.
                    InputMode::Command => {
                        // Position cursor within the command input area.
                        let command_input_area = console_chunks[1];
                        f.set_cursor_position(Position::new(
                            command_input_area.x + app.cursor_position as u16 + 1, // +1 for left border
                            command_input_area.y + 1, // +1 for top border
                        ));
                    }
                    InputMode::Chat => {
                         // Position cursor within the chat input area.
                        let chat_input_area = chat_chunks_for_height[1];
                        f.set_cursor_position(Position::new(
                            chat_input_area.x + app.chat_cursor_position as u16 + 1, // +1 for left border
                            chat_input_area.y + 1, // +1 for top border
                        ));
                    }
                }
            })?;
            // Reset the redraw flag after drawing.
            redraw = false;
        }

        // --- Event Handling ---
        // Wait for events from different sources concurrently.
        tokio::select! {
            // Handle events received from the Swarm or Keyboard tasks.
            maybe_ev = rx.recv() => {
                if let Some(ev) = maybe_ev {
                    // Delegate event processing to the `handle_app_event` function.
                    let needs_redraw = event_handler::handle_app_event(&mut app, &cmd_tx, ev);
                    // Mark UI for redraw if the handler indicates changes.
                    redraw = redraw || needs_redraw;

                    // Check if the application should exit (e.g., user pressed Ctrl+Q).
                    if app.exit {
                        // Signal background tasks to stop.
                        cancel.cancel();
                        // The main loop's exit condition will handle breaking out.
                    }
                } else {
                    // If the channel is closed, it means producers have stopped; exit the application.
                    app.exit = true;
                }
            },

            // --- Peer Staleness Check ---
            // Triggered periodically by the `check_peers_interval`.
            _ = check_peers_interval.tick() => {
                let mut changed = false;
                let now = Instant::now();
                let timeout = constants::PEER_TIMEOUT;

                // Iterate through known peers and mark those inactive for too long as Offline.
                for (_peer_id, peer_info) in app.peers.iter_mut() {
                    if peer_info.status == OnlineStatus::Online && now.duration_since(peer_info.last_seen) > timeout {
                        peer_info.status = OnlineStatus::Offline;
                        changed = true;
                        // Logging the specific timed-out peer can be verbose; consider if needed.
                        // app.push(format!("[UI] Marked peer {:?} offline due to timeout (> {:?})", peer_id, timeout));
                    }
                }

                // Mark UI for redraw if any peer status changed.
                if changed {
                    redraw = true;
                }
            }
        }

        // Exit the main loop if the exit flag is set.
        if app.exit {
            break;
        }
    }

    // --- Terminal Restoration ---
    // Restore the terminal to its original state before the application started.
    ratatui::restore();

    Ok(())
}

