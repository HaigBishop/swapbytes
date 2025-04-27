/*
A temporary libp2p test program "ping". Now with a TUI!
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

// Local modules
mod tui;
use tui::{App, AppEvent, InputMode};

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
            // Poll for key events every 250ms (non-blocking)
            if event::poll(Duration::from_millis(250)).unwrap() {
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
    let mut redraw = true; // Force initial draw

    loop {
        // Redraw UI only if something changed
        if redraw {
            terminal.draw(|f| {
                f.render_widget(&app, f.area());
                // Set cursor visibility and position based on input mode
                match app.input_mode {
                    InputMode::Normal => {}, // Do nothing, cursor hidden by default
                    #[allow(clippy::cast_possible_truncation)]
                    InputMode::Editing => {
                        // Calculate layout again (or pass it down) to find input box coords
                        // This is slightly inefficient, maybe refactor later.
                        let block = ratatui::widgets::Block::bordered().border_set(ratatui::symbols::border::THICK);
                        let inner_area = block.inner(f.area());
                        let chunks = ratatui::layout::Layout::vertical([
                            ratatui::layout::Constraint::Min(1),
                            ratatui::layout::Constraint::Length(3),
                        ])
                        .split(inner_area);
                        let input_area = chunks[1];

                        f.set_cursor_position(ratatui::layout::Position::new(
                            // Draw the cursor at the current position in the input field.
                            input_area.x + app.cursor_position as u16 + 1,
                            // Move one line down, from the border to the input line
                            input_area.y + 1,
                        ));
                    }
                }
            })?;
            redraw = false;
        }

        // Wait for next event (from swarm or keyboard)
        if let Some(ev) = rx.recv().await {
            // LOG EVENTS
            // app.log(format!("Received event: {:?}", ev));
            match ev {
                AppEvent::Swarm(se) => {
                    match &se {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            app.push(format!("Listening on {address}"));
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
                    // Always handle Ctrl+q first regardless of mode
                    if key.kind == KeyEventKind::Press
                        && key.code == KeyCode::Char('q')
                        && key.modifiers.contains(event::KeyModifiers::CONTROL)
                    {
                        cancel.cancel();   // Signal background tasks to stop
                        app.exit = true;   // End UI loop
                        continue; // Skip further processing
                    }

                    match app.input_mode {
                        InputMode::Normal => {
                            // Enter editing mode on '/'
                            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('/') {
                                app.input_mode = InputMode::Editing;
                                app.input.clear(); // Clear previous input
                                app.input.push('/'); // Start with '/'
                                app.cursor_position = 1; // Position cursor after '/'
                                redraw = true;
                            }
                            // Other key presses ignored in normal mode
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
                                        // Optional: Clear input on Esc? No, keep it for now.
                                        redraw = true;
                                    }
                                    _ => {} // Ignore other keys in editing mode
                                }
                            }
                        }
                    }
                }
                // Handle events received from background tasks
                AppEvent::LogMessage(msg) => {
                    app.push(msg);
                    redraw = true;
                }
                // Dial is handled by the swarm task now, ignore if received here
                AppEvent::Dial(_) => {}
                // Handle quit command from the TUI input (this is unreachable) but needed for exhaustive match
                AppEvent::Quit => {}
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
