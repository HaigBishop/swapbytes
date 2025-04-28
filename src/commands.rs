use crate::tui::{App, AppEvent};
use libp2p::Multiaddr;
use std::time::Instant;

/// Processes a user command input.
///
/// Takes the command string (without the leading '/') and a mutable reference
/// to the application state. It modifies the state based on the command
/// (e.g., logging output, changing settings) and returns an optional `AppEvent`
/// if the command requires interaction with the main event loop or swarm task
/// (e.g., Dial, Quit).
pub fn process_command(command_input: &str, app: &mut App) -> Option<AppEvent> {
    let command_parts: Vec<&str> = command_input.trim().splitn(2, ' ').collect();
    let command_name = *command_parts.get(0).unwrap_or(&"");
    let args = command_parts.get(1).unwrap_or(&"").trim(); // Trim args

    let mut event_to_send = None;

    match command_name {
        "ping" => {
            if args.is_empty() {
                app.push("Usage: /ping <multiaddr>".to_string());
            } else {
                match args.parse::<Multiaddr>() {
                    Ok(addr) => {
                        app.pinging = true;
                        app.ping_start_time = Some(Instant::now());
                        app.push(format!("Attempting ping to: {}", args));
                        event_to_send = Some(AppEvent::Dial(addr));
                    }
                    Err(e) => {
                        app.push(format!("Invalid Multiaddr: {e}"));
                    }
                }
            }
        }
        "me" => {
            // Show listening addresses
            app.push("You are listening on addresses:".to_string());
            if app.listening_addresses.is_empty() {
                app.push("  (Not listening on any addresses right now)".to_string());
            } else {
                let addrs_to_print: Vec<String> = app.listening_addresses
                    .iter()
                    .map(|addr| format!("  {}", addr))
                    .collect();
                for addr_str in addrs_to_print {
                    app.push(addr_str);
                }
            }
            // Show Peer ID
            match &app.local_peer_id {
                Some(id) => app.push(format!("Peer ID: {}", id)),
                None => app.push("Peer ID: (Unknown - this shouldn't happen)".to_string()),
            }
             // Show download directory if set
            match &app.download_dir {
                Some(dir) => app.push(format!("Download directory: {}", dir.display())),
                None => app.push("Download directory: (Not set - use /setdir)".to_string()),
            }
            // Show nickname if set
            match &app.nickname {
                Some(name) => app.push(format!("Nickname: {}", name)),
                None => app.push("Nickname: (Not set - use /setname)".to_string()),
            }
            // Show visibility status
            app.push(format!("Visibility: {}", if app.is_visible { "Online" } else { "Hidden" }));
        }
        "setdir" => {
            if args.is_empty() {
                app.push("Usage: /setdir <absolute_path>".to_string());
            } else {
                // Call the verification function from utils
                // Note: This blocks briefly. For heavy I/O, consider spawning a task.
                match crate::utils::verify_download_directory(args) {
                    Ok(verified_path) => {
                        app.push(format!("Download directory set to: {}", verified_path.display()));
                        app.download_dir = Some(verified_path);
                    }
                    Err(err_msg) => {
                        app.push(format!("Error setting directory: {}", err_msg));
                    }
                }
            }
        }
        "setname" => {
            if args.is_empty() {
                app.push("Usage: /setname <nickname>".to_string());
            } else {
                // Call the verification function from utils
                match crate::utils::verify_nickname(args) {
                    Ok(verified_name) => {
                        // Update the nickname in the UI
                        app.push(format!("Nickname set to: {}", verified_name));
                        app.nickname = Some(verified_name.clone());
                        // If the nickname is already taken that is okay, but warn the user
                        if app.peers.values().any(|peer| peer.nickname == Some(verified_name.clone())) {
                            app.push(format!("Warning: Nickname '{}' is already taken by another user.", verified_name));
                        }
                        // Send update event to swarm task
                        event_to_send = Some(AppEvent::NicknameUpdated(app.local_peer_id.unwrap(), verified_name));
                    }
                    Err(err_msg) => {
                        app.push(format!("Error setting nickname: {}", err_msg));
                    }
                }
            }
        }
        "hide" => {
            if app.is_visible {
                app.is_visible = false;
                app.push("You are now hidden. Use /show to become visible again.".to_string());
                event_to_send = Some(AppEvent::VisibilityChanged(false));
            } else {
                app.push("You are already hidden.".to_string());
            }
        }
        "show" => {
            if !app.is_visible {
                app.is_visible = true;
                app.push("You are now visible.".to_string());
                event_to_send = Some(AppEvent::VisibilityChanged(true));
            } else {
                app.push("You are already visible.".to_string());
            }
        }
        "quit" | "q" => {
            event_to_send = Some(AppEvent::Quit);
        }
        "help" | "h" => {
            app.push("SwapBytes Commands:".to_string());
            app.push("  /me               - Show my info (addrs, dir, nickname).".to_string());
            app.push("  /setdir <path>    - Set the absolute path for downloads.".to_string());
            app.push("  /setname <name>   - Set your nickname (3-16 chars, a-z, A-Z, 0-9, -, _).".to_string());
            app.push("  /ping <multiaddr> - Ping a peer.".to_string());
            app.push("  /hide             - Set your status to appear offline.".to_string());
            app.push("  /show             - Set your status to appear online.".to_string());
            app.push("  /quit             - Exit SwapBytes.".to_string());
            // Add other commands here as needed
            app.push("  /help             - Show this help message.".to_string());
        }
        // Unknown command
        _ => {
             if !command_name.is_empty() { // Only show unknown if not empty
                app.push(format!("Unknown command: {}", command_name));
                app.push("Type /help for a list of commands.".to_string());
            }
        }
    }

    event_to_send
} 