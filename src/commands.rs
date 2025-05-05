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
            app.push("  /offer <path>     - Offer a file to the current private chat peer.".to_string());
            app.push("  /quit             - Exit SwapBytes.".to_string());
            app.push("  /myoffers         - List pending incoming file offers.".to_string());
            app.push("  /decline          - Decline the offer from the current chat peer.".to_string());
            app.push("  /accept           - Accept the offer from the current chat peer.".to_string());
            // Add other commands here as needed
            app.push("  /help             - Show this help message.".to_string());
        }

        // Command: /offer <file_path>
        // Offers a file to the peer in the current private chat context.
        "offer" => {
            if args.is_empty() {
                app.push("Usage: /offer <file_path>".to_string());
            } else {
                // First, check if we are in a private chat context
                if let ChatContext::Private { target_peer_id, target_nickname } = app.current_chat_context.clone() {
                    // Clone the necessary parts from the context to avoid borrow issues
                    let target_peer_id_cloned = target_peer_id;
                    let target_nickname_cloned = target_nickname;

                    // Verify the file exists and is readable
                    match crate::utils::verify_offer_file(args) {
                        Ok((verified_path, size_bytes)) => {
                            let target_name = target_nickname_cloned.as_deref().unwrap_or("the peer"); // Use nickname or fallback
                            app.push(format!("Sending offer to {}...", target_name));

                            // Create details for local history and event
                            let offer_details = crate::tui::PendingOfferDetails {
                                filename: verified_path.file_name().map_or_else(
                                    || args.to_string(), // Fallback to original arg if filename extraction fails
                                    |name| name.to_string_lossy().into_owned()
                                ),
                                size_bytes,
                            };

                            // Add the sent offer to local history
                            let history = app.private_chat_histories.entry(target_peer_id_cloned).or_default();
                            let current_len = history.len();
                            history.push(crate::tui::PrivateChatItem::OfferSent(offer_details.clone()));

                            // Auto-scroll local chat if user is viewing it
                            let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                             if app.chat_scroll >= current_max_scroll {
                                let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                app.chat_scroll = new_max_scroll;
                            }

                            // Create the event to send the offer via the swarm task
                            event_to_send = Some(AppEvent::SendFileOffer { 
                                target_peer: target_peer_id_cloned, 
                                file_path: verified_path 
                            });
                        }
                        Err(err_msg) => {
                            // If invalid, show the error message.
                            app.push(format!("Error offering file: {}", err_msg));
                        }
                    }
                } else {
                    app.push("Error: /offer can only be used in a private chat. Use /chat <nickname> first.".to_string());
                }
            }
        }

        // Command: /decline
        // Declines the most recent file offer received from the peer in the current private chat.
        "decline" => {
            if !args.is_empty() {
                 app.push("Usage: /decline (takes no arguments)".to_string());
            } else {
                // Check if we are in a private chat context
                if let ChatContext::Private { target_peer_id, target_nickname } = app.current_chat_context.clone() {
                    let target_peer_id_cloned = target_peer_id; // Clone for use after borrow ends
                    let target_nickname_cloned = target_nickname.clone(); // Clone for use after borrow ends
                    let target_name = target_nickname_cloned.as_deref().unwrap_or("the peer");

                    // Check if there is a pending offer from this specific peer
                    if let Some(offer_details) = app.pending_offers.remove(&target_peer_id_cloned) {
                        // Offer found and removed, now update history and notify user
                        app.push(format!(
                            "Offer for '{}' from {} declined.",
                            offer_details.filename,
                            target_name
                        ));

                        // Add the declined event to the private chat history
                        let history = app.private_chat_histories.entry(target_peer_id_cloned).or_default();
                        let current_len = history.len(); // Length before adding
                        history.push(crate::tui::PrivateChatItem::OfferDeclined(offer_details.clone())); // Clone details again for history

                        // Auto-scroll chat view if we are viewing it
                        let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                        if app.chat_scroll >= current_max_scroll {
                            let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                            app.chat_scroll = new_max_scroll;
                        }

                        // Send event to swarm task to notify the peer
                        event_to_send = Some(AppEvent::DeclineFileOffer { 
                            target_peer: target_peer_id_cloned, 
                            filename: offer_details.filename // Include filename in event
                        });

                    } else {
                        // No pending offer found from this user
                        app.push(format!("You have no pending file offer from {}.", target_name));
                    }
                } else {
                    // Not in a private chat context
                    app.push("Error: /decline can only be used in a private chat with a pending offer.".to_string());
                }
            }
        }

        // Command: /accept
        // Accepts the most recent file offer received from the peer in the current private chat.
        "accept" => {
            if !args.is_empty() {
                 app.push("Usage: /accept (takes no arguments)".to_string());
            } else {
                // Check if we are in a private chat context
                if let ChatContext::Private { target_peer_id, target_nickname } = app.current_chat_context.clone() {
                    let target_peer_id_cloned = target_peer_id; // Clone for use after borrow ends
                    let target_nickname_cloned = target_nickname.clone(); // Clone for use after borrow ends
                    let target_name = target_nickname_cloned.as_deref().unwrap_or("the peer");

                    // Check if there is a pending offer from this specific peer
                    // Use remove() to take ownership of the offer details if it exists.
                    if let Some(offer_details) = app.pending_offers.remove(&target_peer_id_cloned) {
                        // Offer existed and was removed. Now check download dir.
                        match app.download_dir.as_deref() {
                            None => {
                                app.push("Error: Download directory not set. Use /setdir <path>.".to_string());
                                // Put the offer back since we couldn't accept it!
                                app.pending_offers.insert(target_peer_id_cloned, offer_details);
                            }
                            Some(dir_path) => {
                                match crate::utils::verify_download_directory(dir_path.to_string_lossy().as_ref()) {
                                    Ok(_) => {
                                        // Directory is valid, proceed with acceptance steps 1 & 2.
                                        app.push(format!(
                                            "Accepted offer for '{}' from {}.",
                                            offer_details.filename,
                                            target_name
                                        ));

                                        // 2. Add an item to `app.private_chat_histories` indicating acceptance.
                                        let history = app.private_chat_histories.entry(target_peer_id_cloned).or_default();
                                        let current_len = history.len();
                                        history.push(crate::tui::PrivateChatItem::OfferAccepted(offer_details.clone()));

                                        // Auto-scroll chat view if we are viewing it
                                        let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                                        if app.chat_scroll >= current_max_scroll {
                                            let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                                            app.chat_scroll = new_max_scroll;
                                        }

                                        // 3. Send an AppEvent to the swarm task to initiate the transfer.
                                        event_to_send = Some(AppEvent::SendAcceptOffer {
                                            target_peer: target_peer_id_cloned,
                                            filename: offer_details.filename // Send filename along
                                        });
                                        // TODO: File Transfer - Step 1: After accepting, prepare the UI/state for download.
                                        //    - Maybe add a status indicator to the `OfferAccepted` chat item?
                                        //    - Need to tell the swarm task to expect incoming data for this file.

                                    }
                                    Err(err_msg) => {
                                        app.push(format!(
                                            "Error: Download directory '{}' is invalid: {}. Use /setdir to set a valid one.",
                                            dir_path.display(),
                                            err_msg
                                        ));
                                        // Put the offer back since we couldn't accept it!
                                        app.pending_offers.insert(target_peer_id_cloned, offer_details);
                                    }
                                }
                            }
                        }
                    } else {
                        // No pending offer found from this user
                        app.push(format!("You have no pending file offer from {}.", target_name));
                    }
                } else {
                    // Not in a private chat context
                    app.push("Error: /accept can only be used in a private chat with a pending offer.".to_string());
                }
            }
        }

        // Command: /myoffers
        // Lists all pending incoming file offers.
        "myoffers" => {
            if app.pending_offers.is_empty() {
                app.push("You have no pending file offers.".to_string());
            } else {
                app.push("Pending file offers:".to_string());
                // Collect offer details first to avoid borrow checker issues
                let offer_summaries: Vec<(String, String, u64)> = app.pending_offers.iter().map(|(peer_id, offer)| {
                    let sender_display_name = app.peers.get(peer_id)
                        .and_then(|info| info.nickname.clone())
                        .unwrap_or_else(|| {
                            let id_str = peer_id.to_base58();
                            let len = id_str.len();
                            format!("user(...{})", &id_str[len.saturating_sub(6)..])
                        });
                    (sender_display_name, offer.filename.clone(), offer.size_bytes)
                }).collect();

                // Now print the collected summaries
                for (sender_display_name, filename, size_bytes) in offer_summaries {
                    let formatted_size = crate::utils::format_bytes(size_bytes);
                    app.push(format!("  - From {}: {} ({})", sender_display_name, filename, formatted_size));
                }
            }
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
