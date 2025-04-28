/*
The main file for the SwapBytes CLI file-sharing application.
*/


// Standard library imports
use std::{error::Error, time::Duration};

// Async imports
use futures::prelude::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// libp2p imports
use libp2p::{noise, ping, swarm::SwarmEvent, tcp, yamux};

// Terminal UI imports
use crossterm::event;
use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout, Position},
    symbols,
    widgets::Block,
};

// Local modules
mod tui;
mod utils;
use tui::{App, AppEvent, InputMode, FocusPane};

/// Entry point: sets up TUI, libp2p, and event loop.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // --- Terminal UI setup ---
    let mut terminal = ratatui::init();


    // --- Event channel and cancellation token ---
    // Used to communicate between background tasks and the UI loop.
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>(); // Specify type
    let cancel = CancellationToken::new();

    // Channel for commands from UI loop to Swarm task
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<AppEvent>();


    // --- libp2p Swarm setup ---
    // 1. Generate a new identity.
    // 2. Use TCP transport, Noise encryption, Yamux multiplexing.
    // 3. Add ping behaviour.
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| ping::Behaviour::default())?
        .build();

    // Listen on all interfaces, random OS-assigned port.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;


    // --- Background task: forward swarm events to UI ----
    let swarm_tx = tx.clone(); // For Swarm->UI events
    let swarm_cancel = cancel.clone();
    tokio::spawn(async move {
        // Swarm task owns swarm and swarm_tx
        let mut swarm = swarm;
        loop {
            tokio::select! {
                _ = swarm_cancel.cancelled() => break, // Graceful shutdown
                
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
                        // Ignore other commands if any were sent here by mistake
                        _ => {}
                    }
                }

                // Forward swarm events to the UI
                ev = swarm.next() => {
                    if let Some(ev) = ev {
                        // Use swarm_tx to send Swarm events
                        let _ = swarm_tx.send(AppEvent::Swarm(ev));
                    } else {
                        break;
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
    let mut app = App::default();
    app.push("Welcome to SwapBytes!".to_string());
    app.push("Run /help to get started.".to_string());
    let mut redraw = true; // Force initial draw

    loop {
        // Redraw UI only if something changed
        if redraw {
            terminal.draw(|f| {
                // --- Draw main application widget --- 
                // We draw the app first using its Widget impl
                // This draws everything EXCEPT the stateful scrollbar
                f.render_widget(&app, f.area());

                // --- Calculate Console Log Area (needs to match tui.rs layout) --- 
                // This is slightly duplicated logic, but needed here to place the scrollbar
                let main_chunks = Layout::horizontal([
                    Constraint::Percentage(75), 
                    Constraint::Percentage(25),
                ])
                .split(f.area());
                let left_area = main_chunks[0];
                let left_chunks = Layout::vertical([
                    Constraint::Percentage(67),
                    Constraint::Percentage(33),
                ])
                .split(left_area);
                let console_area = left_chunks[1];
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

                // Scrollbar rendering removed - we'll keep just the keyboard scrolling functionality

                // --- Set cursor position (only in Editing mode) ---
                match app.input_mode {
                    InputMode::Normal => {} // Cursor hidden by default
                    InputMode::Editing => {
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

        // Wait for next event (from swarm or keyboard)
        if let Some(ev) = rx.recv().await {
            match ev {
                AppEvent::Swarm(se) => {
                    match &se {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            // Store the address
                            app.listening_addresses.push(address.clone());
                            // Print the address to the console
                            // app.push(format!("Listening on {address}"));
                        }
                        SwarmEvent::Behaviour(ping::Event { peer, result, .. }) => {
                            match result {
                                Ok(latency) => {
                                    app.push(format!("Successfully pinged peer: {peer} ({latency:?})"));
                                }
                                Err(e) => {
                                    // Log ping errors as well
                                    app.push(format!("Ping failed for peer: {peer} ({e:?})"));
                                }
                            }
                        }
                        SwarmEvent::OutgoingConnectionError { error, .. } => {
                            app.push(format!("Outgoing connection error: {error}"));
                        }
                        _ => {
                            // Log other swarm events if needed
                        }
                    }
                    redraw = true;
                }
                AppEvent::Input(key) => {
                    // Always handle Ctrl+q first
                    if key.kind == KeyEventKind::Press
                        && key.code == KeyCode::Char('q')
                        && key.modifiers.contains(event::KeyModifiers::CONTROL)
                    {
                        cancel.cancel();
                        app.exit = true;
                        continue;
                    }

                    match app.input_mode {
                        InputMode::Normal => {
                            if key.kind == KeyEventKind::Press {
                                match key.code {
                                    // Focus Switching
                                    KeyCode::Tab => {
                                        app.focused_pane = match app.focused_pane {
                                            FocusPane::Chat => FocusPane::Console,
                                            FocusPane::Console => FocusPane::UsersList,
                                            FocusPane::UsersList => FocusPane::Chat,
                                        };
                                        redraw = true;
                                    }
                                    // Enter Editing Mode
                                    KeyCode::Char('/') => {
                                        app.input_mode = InputMode::Editing;
                                        app.input.clear();
                                        app.input.push('/');
                                        app.cursor_position = 1;
                                        redraw = true;
                                    }
                                    // Scrolling (only if Console focused)
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
                        InputMode::Editing => {
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
                                                // Ignore any other event types returned by submit_command
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
                                    _ => {} // Ignore other keys in editing mode
                                }
                            }
                        }
                    }
                }
                AppEvent::LogMessage(msg) => {
                    app.push(msg);
                    redraw = true;
                }
                AppEvent::Dial(_) => {}
                AppEvent::Quit => {} // Already handled in Editing mode Enter
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
