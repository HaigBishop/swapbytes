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
use libp2p::{noise, ping, swarm::SwarmEvent, tcp, yamux, Multiaddr};

// Terminal UI imports
use crossterm::event;

// Local modules
mod tui;
use tui::{App, AppEvent};

/// Entry point: sets up TUI, libp2p, and event loop.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // --- Terminal UI setup ---
    let mut terminal = ratatui::init();


    // --- Event channel and cancellation token ---
    // Used to communicate between background tasks and the UI loop.
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();


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


    // --- Optional: dial a peer if address is given as CLI argument ---
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        println!("Dialed {addr}");
    }


    // --- Background task: forward swarm events to UI ---
    let swarm_tx = tx.clone();
    let swarm_cancel = cancel.clone();
    tokio::spawn(async move {
        let mut swarm = swarm;
        loop {
            tokio::select! {
                _ = swarm_cancel.cancelled() => break, // Graceful shutdown
                ev = swarm.next() => {
                    if let Some(ev) = ev {
                        let _ = swarm_tx.send(AppEvent::Swarm(ev));
                    } else {
                        break;
                    }
                }
            }
        }
    });


    // --- Background task: handle keyboard input ---
    let kb_cancel = cancel.clone();
    tokio::spawn(async move {
        loop {
            if kb_cancel.is_cancelled() { break; }
            // Poll for key events every 250ms (non-blocking)
            if event::poll(Duration::from_millis(250)).unwrap() {
                if let event::Event::Key(key) = event::read().unwrap() {
                    if tx.send(AppEvent::Input(key)).is_err() {
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
            terminal.draw(|f| f.render_widget(&app, f.area()))?;
            redraw = false;
        }

        // Wait for next event (from swarm or keyboard)
        if let Some(ev) = rx.recv().await {
            match ev {
                AppEvent::Swarm(se) => {
                    match &se {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            app.push(format!("Listening on {address}"));
                        }
                        SwarmEvent::Behaviour(be) => {
                            app.push(format!("{be:?}"));
                        }
                        _ => {}
                    }
                    redraw = true;
                }
                AppEvent::Input(key) => {
                    // Exit on 'q' key press
                    if key.kind == event::KeyEventKind::Press
                        && key.code == event::KeyCode::Char('q')
                    {
                        cancel.cancel();   // Signal background tasks to stop
                        app.exit = true;   // End UI loop
                    }
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
