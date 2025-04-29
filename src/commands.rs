/*
Handles all the slash commands (e.g. /ping, /setname, /chat).
*/


use crate::tui::{App, AppEvent, ChatContext, FocusPane, InputMode, OnlineStatus};
use libp2p::{Multiaddr, PeerId};
use std::time::Instant;

/// Processes a command entered by the user in the console input.
///
/// Takes the raw text after the '/' and the main application state.
/// It figures out the command and any arguments, updates the app state accordingly
/// (like adding messages to the console log), and sometimes returns an `AppEvent`.
/// These events signal actions that need to be handled elsewhere, like sending network
/// messages (Dial, NicknameUpdate) or quitting the app.
pub fn process_command(command_input: &str, app: &mut App) -> Option<AppEvent> {
    // Split the input into the command name (like "ping") and the rest (arguments).
    let command_parts: Vec<&str> = command_input.trim().splitn(2, ' ').collect();
    let command_name = *command_parts.get(0).unwrap_or(&""); // The command itself, e.g., "ping"
    let args = command_parts.get(1).unwrap_or(&"").trim(); // The arguments, e.g., "12D..."

    let mut event_to_send = None; // This will hold an event if the command needs to trigger a network action or quit.

    // Figure out which command was entered and run the corresponding code.
    match command_name {
        // Command: /ping <multiaddr>
        // Tries to connect to and ping another peer using their network address.
        "ping" => {
            if args.is_empty() {
                app.push("Usage: /ping <multiaddr>".to_string());
            } else {
                // Try to understand the address the user provided.
                match args.parse::<Multiaddr>() {
                    Ok(addr) => {
                        // If the address is valid, start the ping process.
                        app.pinging = true; // Set a flag to show we're waiting for a ping reply.
                        app.ping_start_time = Some(Instant::now()); // Record when we started.
                        app.push(format!("Attempting ping to: {}", args));
                        // Prepare an event to tell the network task to actually send the ping.
                        event_to_send = Some(AppEvent::Dial(addr));
                    }
                    Err(e) => {
                        // If the address is not valid, tell the user.
                        app.push(format!("Invalid Multiaddr: {e}"));
                    }
                }
            }
        }

        // Command: /me
        // Shows the user's own information like network addresses, Peer ID, download directory, and nickname.
        "me" => {
            // Show listening addresses
            app.push("You are listening on addresses:".to_string());
            if app.listening_addresses.is_empty() {
                app.push("  (Not listening on any addresses right now)".to_string());
            } else {
                // Format and print each address neatly.
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

        // Command: /setdir <absolute_path>
        // Sets the directory where downloaded files will be saved.
        "setdir" => {
            if args.is_empty() {
                app.push("Usage: /setdir <absolute_path>".to_string());
            } else {
                // Check if the provided path is a valid, writable directory.
                match crate::utils::verify_download_directory(args) {
                    Ok(verified_path) => {
                        // If valid, update the app state and tell the user.
                        app.push(format!("Download directory set to: {}", verified_path.display()));
                        app.download_dir = Some(verified_path);
                    }
                    Err(err_msg) => {
                        // If invalid, show the error message.
                        app.push(format!("Error setting directory: {}", err_msg));
                    }
                }
            }
        }

        // Command: /setname <nickname>
        // Sets the user's nickname, which others will see in chat and user lists.
        "setname" => {
            if args.is_empty() {
                app.push("Usage: /setname <nickname>".to_string());
            } else {
                // Check if the nickname meets the required format (length, characters).
                match crate::utils::verify_nickname(args) {
                    Ok(verified_name) => {
                        // If valid, update the nickname in the app state.
                        app.push(format!("Nickname set to: {}", verified_name));
                        app.nickname = Some(verified_name.clone());

                        // Check if someone else already has this nickname and warn the user.
                        if app.peers.values().any(|peer| peer.nickname == Some(verified_name.clone())) {
                            app.push(format!("Warning: Nickname '{}' is already taken by another user.", verified_name));
                        }

                        // Prepare an event to tell the network task about the new nickname,
                        // so it can be included in heartbeats.
                        event_to_send = Some(AppEvent::NicknameUpdated(app.local_peer_id.unwrap(), verified_name));
                    }
                    Err(err_msg) => {
                        // If the nickname is invalid, show the error message.
                        app.push(format!("Error setting nickname: {}", err_msg));
                    }
                }
            }
        }

        // Command: /chat <nickname|global>
        // Switches the chat view to either a private chat with a specific user (by nickname) or the global chat.
        "chat" => {
            if args.is_empty() {
                app.push("Usage: /chat <nickname|global>".to_string());
            } else if args.eq_ignore_ascii_case("global") {
                // Switch to the global chat context.
                app.current_chat_context = ChatContext::Global;
                app.push("Switched to global chat.".to_string());
                // Automatically focus the chat pane and enter chat input mode.
                app.focused_pane = FocusPane::Chat;
                app.input_mode = InputMode::Chat;
                app.chat_input.clear();
                app.reset_chat_cursor();
            } else {
                // Try to find the peer by the provided nickname (case-insensitive).
                let target_name_lower = args.to_lowercase();
                let matches: Vec<(PeerId, Option<String>)> = app
                    .peers
                    .iter()
                    .filter_map(|(id, info)| {
                        info.nickname
                            .as_ref()
                            .filter(|nick| nick.to_lowercase() == target_name_lower)
                            .map(|nick| (*id, Some(nick.clone()))) // Grab the PeerId and the actual nickname casing.
                    })
                    .collect();

                match matches.len() {
                    0 => {
                        // No user found with that nickname.
                        app.push(format!("Error: User '{}' not found.", args));
                    }
                    1 => {
                        // Exactly one user found. Switch to private chat with them.
                        let (peer_id, nickname) = matches.into_iter().next().unwrap();
                        let display_name = nickname.clone().unwrap_or_else(|| "Unknown User".to_string());
                        app.current_chat_context = ChatContext::Private { target_peer_id: peer_id, target_nickname: nickname };
                        app.push(format!("Switched chat to {}", display_name));
                        // Focus chat pane and enter chat input mode.
                        app.focused_pane = FocusPane::Chat;
                        app.input_mode = InputMode::Chat;
                        app.chat_input.clear();
                        app.reset_chat_cursor();
                    }
                    _ => {
                        // Multiple users found with similar nicknames. Pick the first one and warn.
                        // TODO: Maybe list the matches and ask the user to be more specific?
                        app.push(format!("Warning: Multiple users found matching '{}'. Connecting to the first one.", args));
                        let (peer_id, nickname) = matches.into_iter().next().unwrap();
                        let display_name = nickname.clone().unwrap_or_else(|| "Unknown User".to_string());
                        app.current_chat_context = ChatContext::Private { target_peer_id: peer_id, target_nickname: nickname };
                        app.push(format!("Switched chat to {}", display_name));
                        // Focus chat pane and enter chat input mode.
                        app.focused_pane = FocusPane::Chat;
                        app.input_mode = InputMode::Chat;
                        app.chat_input.clear();
                        app.reset_chat_cursor();
                    }
                }
            }
        }

        // Command: /global
        // A shortcut to switch to the global chat view.
        "global" => {
            // Same logic as `/chat global`.
            app.current_chat_context = ChatContext::Global;
            app.push("Switched to global chat.".to_string());
            // Focus chat pane and enter chat input mode.
            app.focused_pane = FocusPane::Chat;
            app.input_mode = InputMode::Chat;
            app.chat_input.clear();
            app.reset_chat_cursor();
        }

        // Command: /forget
        // Clears the list of known peers. Useful if the list gets cluttered or outdated.
        "forget" => {
            let num_peers = app.peers.len();
            app.peers.clear();
            app.push(format!("Forgot {} known peers.", num_peers));
        }

        // Command: /hide
        // Makes the user appear offline to others by stopping heartbeat broadcasts.
        "hide" => {
            if app.is_visible {
                app.is_visible = false; // Update the local visibility state.
                app.push("You are now hidden. Use /show to become visible again.".to_string());
                // Tell the network task to stop sending heartbeats.
                event_to_send = Some(AppEvent::VisibilityChanged(false));
            } else {
                app.push("You are already hidden.".to_string());
            }
        }

        // Command: /show
        // Makes the user appear online again by resuming heartbeat broadcasts.
        "show" => {
            if !app.is_visible {
                app.is_visible = true; // Update the local visibility state.
                app.push("You are now visible.".to_string());
                // Tell the network task to start sending heartbeats again.
                event_to_send = Some(AppEvent::VisibilityChanged(true));
            } else {
                app.push("You are already visible.".to_string());
            }
        }

        // Command: /quit or /q
        // Exits the application gracefully.
        "quit" | "q" => {
            // Prepare an event to signal the main loop to shut down.
            event_to_send = Some(AppEvent::Quit);
        }

        // Command: /help or /h
        // Displays a list of available commands and their usage.
        "help" | "h" => {
            app.push("SwapBytes Commands:".to_string());
            app.push("  /me               - Show my info (addrs, dir, nickname).".to_string());
            app.push("  /setdir <path>    - Set the absolute path for downloads.".to_string());
            app.push("  /setname <name>   - Set your nickname (3-16 chars, a-z, A-Z, 0-9, -, _).".to_string());
            app.push("  /chat <name>      - Switch chat (e.g. 'bob' or 'global').".to_string());
            app.push("  /global           - Switch to the global chat view.".to_string());
            app.push("  /ping <multiaddr> - Ping a peer.".to_string());
            app.push("  /forget           - Forget all known peers.".to_string());
            app.push("  /hide             - Set your status to appear offline.".to_string());
            app.push("  /show             - Set your status to appear online.".to_string());
            app.push("  /who <name>       - Show information about a specific user.".to_string());
            app.push("  /quit             - Exit SwapBytes.".to_string());
            // Add other commands here as needed
            app.push("  /help             - Show this help message.".to_string());
        }

        // Command: /who <nickname>
        // Shows details about a specific user identified by their nickname.
        "who" => {
            if args.is_empty() {
                app.push("Usage: /who <nickname>".to_string());
            } else {
                let target_name_lower = args.to_lowercase();

                // Check if the user is asking about themselves.
                if Some(target_name_lower.clone()) == app.nickname.as_ref().map(|n| n.to_lowercase()) {
                    app.push("That is your nickname. Use /me to see your own information.".to_string());
                } else {
                    // Find all peers matching the nickname (case-insensitive).
                    let now = Instant::now(); // Get current time to calculate 'last seen' duration.
                    let matches: Vec<_> = app
                        .peers
                        .iter()
                        .filter_map(|(id, info)| {
                            info.nickname
                                .as_ref()
                                .filter(|nick| nick.to_lowercase() == target_name_lower)
                                .map(|nick| (*id, nick.clone(), info.status.clone(), info.last_seen)) // Collect necessary info.
                        })
                        .collect();

                    // Display the information found.
                    match matches.len() {
                        0 => {
                            app.push(format!("Error: User '{}' not found.", args));
                        }
                        count => {
                            app.push(format!("Found {} users matching '{}':", count, args));
                            for (peer_id, nickname, status, last_seen) in matches {
                                app.push("--- User ---".to_string());
                                app.push(format!("  Nickname: {}", nickname));
                                app.push(format!("  Peer ID: {}", peer_id));
                                // Format the status string to include how long ago they were last seen if offline.
                                let status_str = match status {
                                    OnlineStatus::Online => "Online".to_string(),
                                    OnlineStatus::Offline => {
                                        let secs_ago = now.duration_since(last_seen).as_secs();
                                        format!("Offline (Last seen: {} seconds ago)", secs_ago)
                                    }
                                };
                                app.push(format!("  Status: {}", status_str));
                            }
                        }
                    }
                }
            }
        }

        // Handle any input that doesn't match a known command.
        _ => {
             // Only show the error if the user actually typed something (not just "/").
             if !command_name.is_empty() {
                app.push(format!("Unknown command: {}", command_name));
                app.push("Type /help for a list of commands.".to_string());
            }
            // If the input was empty (e.g., user just typed "/" and hit enter), do nothing.
        }
    }

    // Return the event we prepared, if any.
    event_to_send
}
