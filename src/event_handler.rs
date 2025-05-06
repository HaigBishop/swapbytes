/*
Handles application events received from the main loop or the swarm task.
*/

// --- Standard Library Imports ---
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::path::PathBuf;

// --- Async and Tokio Imports ---
use tokio::sync::mpsc;

// --- libp2p Imports ---
use libp2p::{swarm::SwarmEvent, gossipsub, ping};

// --- Local Crate Imports ---
use crate::{
    App, AppEvent,
    tui::{self, PeerInfo, OnlineStatus, ChatMessage, PrivateChatItem, PendingOfferDetails},
    behavior::SwapBytesBehaviourEvent,
    input_handler,
    utils,
};

// --- Function Definition ---

/// Handles application-level events (`AppEvent`).
///
/// This function takes the current application state (`App`), a sender channel (`cmd_tx`)
/// to send commands back to the swarm task, and the `AppEvent` to process.
///
/// It updates the `App` state based on the event and returns `true` if the
/// UI needs to be redrawn, `false` otherwise.
pub fn handle_app_event(
    app: &mut App,
    cmd_tx: &mpsc::UnboundedSender<AppEvent>, // Used for sending commands back to swarm_task
    event: AppEvent,
) -> bool {
    let mut redraw = false; // Assume no UI redraw needed initially

    match event {
        // --- Swarm Events ---
        // Handle events forwarded from the libp2p Swarm task.
        AppEvent::Swarm(se) => {
            match se {
                SwarmEvent::NewListenAddr { address, .. } => {
                    // Store new listening addresses announced by the Swarm.
                    app.listening_addresses.push(address.clone());
                }
                SwarmEvent::Behaviour(
                    SwapBytesBehaviourEvent::Ping(ping::Event { peer, result, .. })
                ) => {
                    // Handle results of ping requests initiated by the user.
                    match result {
                        Ok(latency) => {
                            // Only log success if we explicitly initiated the ping.
                            if app.pinging {
                                app.push(format!("Successfully pinged peer: {peer} ({latency:?})"));
                                // The ping timer in `App` handles resetting `app.pinging`.
                            }
                        }
                        Err(e) => {
                            // Only log failure if we explicitly initiated the ping.
                            if app.pinging {
                                app.push(format!("Ping failed for peer: {peer} ({e:?})"));
                            }
                        }
                    }
                    // Any ping activity (even incoming) updates the peer's status.
                    if let Some(peer_info) = app.peers.get_mut(&peer) {
                        peer_info.last_seen = Instant::now();
                        peer_info.status = OnlineStatus::Online;
                    }
                }
                // Note: mDNS events (discovery/expiration) are now primarily handled
                // directly in the `swarm_task` and result in `PeerDiscovered`/`PeerExpired`
                // `AppEvent`s being sent to the UI thread.

                // Handle forwarded Gossipsub messages primarily to update peer activity.
                SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    ..
                })) => {
                    // Update the last seen time and status for the peer sending the message.
                    let now = Instant::now();
                    let peer_info = app.peers.entry(peer_id).or_insert_with(|| PeerInfo {
                        nickname: None,
                        status: OnlineStatus::Online,
                        last_seen: now,
                    });
                    peer_info.last_seen = now;
                    peer_info.status = OnlineStatus::Online;
                }

                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    // Optionally log or handle outgoing connection errors.
                    let _ = error; // Avoid unused variable warning
                }
                SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                    // Update peer status when a connection is established.
                     if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                        peer_info.status = OnlineStatus::Online;
                        peer_info.last_seen = Instant::now();
                    }
                    let _ = endpoint; // Avoid unused variable warning
                    let _ = num_established; // Avoid unused variable warning
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, num_established, .. } => {
                    // Handle connection closures. Peer status is primarily managed by the
                    // heartbeat/timeout mechanism, not just connection closure.
                    let _ = peer_id; // Avoid unused variable warning
                    let _ = cause; // Avoid unused variable warning
                    let _ = num_established; // Avoid unused variable warning
                }
                // Catch-all for other SwarmEvents we don't explicitly handle here.
                _ => {}
            }
            redraw = true; // Most swarm events likely require a UI redraw.
        }

        // --- User Input ---
        AppEvent::Input(key) => {
            // Delegate keyboard input handling to the dedicated input handler module.
            let needs_redraw = input_handler::handle_key_event(app, cmd_tx, key);
            redraw = redraw || needs_redraw;
            // Note: Checking `app.exit` happens after this function returns in main.rs
        }

        // --- Logging ---
        AppEvent::LogMessage(msg) => {
            // Add a message to the application's console/log buffer.
            app.push(msg);
            redraw = true;
        }

        // --- Peer Discovery and Status ---
        AppEvent::PeerDiscovered(peer_id) => {
            // Handle peers discovered via mDNS or other mechanisms.
            if peer_id != app.local_peer_id.expect("Local peer ID should be set") {
                let now = Instant::now();
                // Add or update the peer in the peer list, marking as online.
                let peer_info = app.peers.entry(peer_id).or_insert_with(|| PeerInfo {
                    nickname: None,
                    status: OnlineStatus::Online,
                    last_seen: now,
                });
                peer_info.last_seen = now; // Update last_seen even if peer already exists.
                peer_info.status = OnlineStatus::Online;
                redraw = true;
            }
        }
        AppEvent::PeerExpired(peer_id) => {
            // Peer expiration is primarily handled by the heartbeat timeout check now.
            // This event might be used for logging or immediate removal if needed later.
            let _ = peer_id; // Avoid unused variable warning
        }

        // --- Nickname Updates ---
        AppEvent::NicknameUpdated(peer_id, new_nickname) => {
            // Update a peer's nickname based on received gossipsub messages.
            if Some(peer_id) == app.local_peer_id {
                // Ignore updates for the local user (handled directly by input command).
            } else if let Some(peer_info) = app.peers.get_mut(&peer_id) {
                let old_nickname_opt = peer_info.nickname.clone();
                let new_nickname_opt = Some(new_nickname.clone());

                // Only log if the nickname changed *from* a known value.
                let should_log = old_nickname_opt != new_nickname_opt && old_nickname_opt.is_some();

                // Update the nickname in the main peer list.
                peer_info.nickname = Some(new_nickname.clone());
                redraw = true; // Always redraw on nickname update.

                if should_log {
                    let old_name = old_nickname_opt.unwrap_or_else(|| "Unknown".to_string());
                    let id_str = peer_id.to_base58();
                    let len = id_str.len();
                    let id_suffix = format!("(...{})", &id_str[len.saturating_sub(6)..]);
                    app.push(format!("Peer changed nickname: {} → {} {}", old_name, new_nickname, id_suffix));
                }

                // Update Chat Title if viewing private chat with this peer.
                if let tui::ChatContext::Private { target_peer_id, target_nickname } = &mut app.current_chat_context {
                    if *target_peer_id == peer_id {
                        *target_nickname = Some(new_nickname.clone());
                    }
                }

                // Update nickname in Private Chat History for this peer.
                if let Some(history) = app.private_chat_histories.get_mut(&peer_id) {
                    for item in history.iter_mut() {
                        if let PrivateChatItem::Message(message) = item {
                            if message.sender_id == peer_id {
                                message.sender_nickname = Some(new_nickname.clone());
                            }
                        }
                        // Future: Update Offer/OfferSent/etc. if they store nicknames.
                    }
                }

                // Update nickname in Global Chat History.
                for message in app.global_chat_history.iter_mut() {
                    if message.sender_id == peer_id {
                        message.sender_nickname = Some(new_nickname.clone());
                    }
                }
            }
        }

        // --- Global Chat ---
        AppEvent::GlobalMessageReceived { sender_id, sender_nickname, content, timestamp_ms } => {
            // Handle incoming global chat messages received via gossipsub.
            let chat_msg = ChatMessage {
                sender_id,
                sender_nickname: sender_nickname.clone(),
                content,
                timestamp_ms,
            };
            app.global_chat_history.push(chat_msg);

            // Notify in the console if the user is currently in a private chat view.
            if let tui::ChatContext::Private { .. } = app.current_chat_context {
                let sender_display_name = sender_nickname.clone().unwrap_or_else(|| {
                    let id_str = sender_id.to_base58();
                    let len = id_str.len();
                    format!("user(...{})", &id_str[len.saturating_sub(6)..])
                });
                app.push(format!("{} sent a global message!", sender_display_name));
            }

            // Auto-scroll the global chat view if it's already scrolled to the bottom.
            let current_max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height.max(1)).saturating_sub(1);
            if app.chat_scroll >= current_max_scroll {
                let new_max_scroll = app.global_chat_history.len().saturating_sub(app.chat_viewport_height.max(1));
                app.chat_scroll = new_max_scroll;
            }
            redraw = true;
        }

        // --- Private Chat ---
        AppEvent::PrivateMessageReceived { sender_id, content } => {
            // Handle incoming private messages received via direct send.
            let sender_nickname = app.peers.get(&sender_id).and_then(|info| info.nickname.clone());
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis() as u64;

            let chat_msg = ChatMessage {
                sender_id,
                sender_nickname: sender_nickname.clone(), // Store nickname with the message.
                content,
                timestamp_ms,
            };

            // Add the message to the specific private chat history for this sender.
            let history = app.private_chat_histories.entry(sender_id).or_default();
            let current_len = history.len(); // Length before adding the new message.
            history.push(PrivateChatItem::Message(chat_msg));

            // Determine if a notification should be shown in the main console.
            let mut notify_in_console = true;
            if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                // Don't notify if the user is already viewing the chat with this sender.
                if *target_peer_id == sender_id {
                    notify_in_console = false;
                    // Auto-scroll the private chat view if it's already at the bottom.
                    let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                    if app.chat_scroll >= current_max_scroll {
                        let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                        app.chat_scroll = new_max_scroll;
                    }
                }
            }

            if notify_in_console {
                 let sender_display_name = sender_nickname.unwrap_or_else(|| {
                        let id_str = sender_id.to_base58();
                        let len = id_str.len();
                        format!("user(...{})", &id_str[len.saturating_sub(6)..])
                    });
                app.push(format!("{} sent you a private message!", sender_display_name));
            }
            redraw = true;
        }

        // --- File Transfer Offers ---
        AppEvent::FileOfferReceived { sender_id, filename, size_bytes } => {
            // Handle an incoming file transfer offer.
            let sender_display_name = app.peers.get(&sender_id)
                .and_then(|info| info.nickname.clone())
                .unwrap_or_else(|| utils::peer_id_to_short_string(&sender_id)); // Use utility function

            // Check if the user is currently viewing the chat with this sender.
            let mut is_viewing_chat = false;
            if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                if *target_peer_id == sender_id {
                    is_viewing_chat = true;
                }
            }

            // Store the details of the pending offer.
            let offer_details = PendingOfferDetails {
                filename: filename.clone(),
                size_bytes,
                path: PathBuf::new(), // Path is irrelevant for received offers.
            };
            // Overwrite any previous pending offer from this sender.
            app.pending_offers.insert(sender_id, offer_details.clone());

            // Add the offer to the private chat history for this sender.
            let history = app.private_chat_histories.entry(sender_id).or_default();
            let current_len = history.len(); // Length before adding the offer.
            history.push(PrivateChatItem::Offer(offer_details));

            // Notify the user in the console if they aren't viewing the chat.
            if !is_viewing_chat {
                app.push(format!(
                    "{} sent you a file offer: {} ({})",
                    sender_display_name,
                    filename,
                    utils::format_bytes(size_bytes)
                ));
            } else {
                // Auto-scroll the private chat view if it's already at the bottom.
                let current_max_scroll = current_len.saturating_sub(app.chat_viewport_height.max(1));
                if app.chat_scroll >= current_max_scroll {
                    let new_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                    app.chat_scroll = new_max_scroll;
                }
            }
            redraw = true;
        }
        AppEvent::FileOfferDeclined { peer_id, filename } => {
            // Handle notification that a peer declined our file offer.
            let peer_display_name = app.peers.get(&peer_id)
                .and_then(|info| info.nickname.clone())
                .unwrap_or_else(|| utils::peer_id_to_short_string(&peer_id));

            // Log the decline event in the console.
            app.push(format!("{} declined your offer for '{}'.", peer_display_name, filename));

            // Add a record of the decline to the private chat history.
            if let Some(history) = app.private_chat_histories.get_mut(&peer_id) {
                // Find the original `OfferSent` details to include in the Declined item.
                let mut offer_details_opt: Option<PendingOfferDetails> = None;
                for item in history.iter() {
                    if let PrivateChatItem::OfferSent(details) = item {
                        if details.filename == filename {
                            offer_details_opt = Some(details.clone());
                            break;
                        }
                    }
                }

                if let Some(offer_details) = offer_details_opt {
                    let current_len = history.len(); // Length before adding decline item.
                    history.push(PrivateChatItem::RemoteOfferDeclined(offer_details));

                    // Auto-scroll if viewing this chat and already at the bottom.
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
                    // Log a warning if the original OfferSent item couldn't be found.
                    app.log(format!("Warning: Could not find OfferSent details for declined file '{}' from {}", filename, peer_display_name));
                }
            } else {
                // Log a warning if no chat history exists for this peer.
                 app.log(format!("Warning: No private chat history for peer {} who declined file '{}'.", peer_display_name, filename));
            }
            redraw = true;
        }
        AppEvent::FileOfferAccepted { peer_id, filename } => {
            // Handle notification that a peer accepted our file offer.
            let peer_display_name = app.peers.get(&peer_id)
                .and_then(|info| info.nickname.clone())
                .unwrap_or_else(|| utils::peer_id_to_short_string(&peer_id));

            // Log the acceptance event in the console.
            app.push(format!("{} accepted your offer for '{}'.", peer_display_name, filename));

            let mut found_path: Option<PathBuf> = None;
            // Add a record of the acceptance to the private chat history.
            if let Some(history) = app.private_chat_histories.get_mut(&peer_id) {
                // Find the original `OfferSent` details, including the file path.
                let mut offer_details_opt: Option<PendingOfferDetails> = None;
                for item in history.iter() {
                    if let PrivateChatItem::OfferSent(details) = item {
                        if details.filename == filename {
                            offer_details_opt = Some(details.clone());
                            found_path = Some(details.path.clone()); // Extract the path here.
                            break;
                        }
                    }
                }

                if let Some(offer_details) = offer_details_opt {
                    let current_len = history.len(); // Length before adding accepted item.
                    history.push(PrivateChatItem::RemoteOfferAccepted(offer_details));

                    // Auto-scroll if viewing this chat and already at the bottom.
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
                    // Log warning if the original OfferSent item couldn't be found.
                    app.log(format!("Warning: Could not find OfferSent details for accepted file '{}' from {}", filename, peer_display_name));
                }
            } else {
                 // Log warning if no chat history exists for this peer.
                 app.log(format!("Warning: No private chat history for peer {} who accepted file '{}'.", peer_display_name, filename));
            }

            // If the path was found, store the mapping for the outgoing transfer
            // and notify the swarm task to prepare for sending the file data.
            if let Some(path) = found_path {
                app.outgoing_transfers.insert((peer_id, filename.clone()), path.clone());
                // Send event to swarm task to register the stream handler for this transfer.
                let _ = cmd_tx.send(AppEvent::RegisterOutgoingTransfer {
                    peer_id,
                    filename: filename.clone(),
                    path
                });
            } else {
                 // Log an error if the path is missing, as the transfer cannot start.
                 app.log(format!("Error: Missing path for accepted offer '{}' from {}. Cannot start transfer.", filename, peer_display_name));
            }
            redraw = true;
        }

        // --- File Transfer Progress & Status ---
        AppEvent::FileTransferProgress { peer_id, filename, received, total } => {
            // Update the UI with the progress of an incoming or outgoing file transfer.
            let history = app.private_chat_histories.entry(peer_id).or_default();
            let current_len = history.len(); // Length before potential update/add.
            let mut updated_existing = false;

            // Check if the last item in the history is a progress update for the same file.
            // If so, update it in place to avoid cluttering the history.
            if let Some(last_item) = history.last_mut() {
                if let PrivateChatItem::TransferProgress { filename: item_filename, received: item_received, .. } = last_item {
                    if *item_filename == filename {
                        *item_received = received; // Update bytes received.
                        updated_existing = true;
                    }
                }
            }

            // If no existing progress item was updated, add a new one.
            if !updated_existing {
                history.push(PrivateChatItem::TransferProgress {
                    filename: filename.clone(),
                    received,
                    total,
                });
            }

            // Auto-scroll if viewing this chat and near the bottom.
            if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                if *target_peer_id == peer_id {
                    let scroll_target = if updated_existing { current_len } else { history.len() };
                    let current_max_scroll = scroll_target.saturating_sub(app.chat_viewport_height.max(1));
                    // Scroll if current scroll position is at or just above the last item.
                    if app.chat_scroll >= current_max_scroll.saturating_sub(1) {
                        app.chat_scroll = current_max_scroll;
                    }
                }
            }
            redraw = true;
        }
        AppEvent::FileTransferComplete { peer_id, filename, path, total_size } => {
            // Handle the completion of a file download (transfer received).
            let history = app.private_chat_histories.entry(peer_id).or_default();

            // Optional: Remove the last progress item for this file to clean up history.
            if let Some(last_item) = history.last() {
                if matches!(last_item, PrivateChatItem::TransferProgress { filename: item_filename, .. } if *item_filename == filename) {
                    history.pop();
                }
            }

            // Add a completion record to the private chat history.
            history.push(PrivateChatItem::TransferComplete {
                filename: filename.clone(),
                final_path: path, // Store the final path where the file was saved.
                size: total_size,
            });

             // Auto-scroll if viewing this chat and near the bottom.
             if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                if *target_peer_id == peer_id {
                    let current_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                     if app.chat_scroll >= current_max_scroll.saturating_sub(1) { // Scroll if near bottom
                        app.chat_scroll = current_max_scroll;
                    }
                }
            }

            // Notify the user in the console about the successful download.
            let peer_display_name = app.peers.get(&peer_id)
                .and_then(|info| info.nickname.clone())
                .unwrap_or_else(|| utils::peer_id_to_short_string(&peer_id));
            app.push(format!("✅ Download finished: '{}' ({}) from {}", filename, utils::format_bytes(total_size), peer_display_name));
            redraw = true;
        }
        AppEvent::FileTransferFailed { peer_id, filename, error } => {
           // Handle the failure of a file transfer (incoming or outgoing).
           let history = app.private_chat_histories.entry(peer_id).or_default();

            // Optional: Remove the last progress item for this file.
            if let Some(last_item) = history.last() {
                if matches!(last_item, PrivateChatItem::TransferProgress { filename: item_filename, .. } if *item_filename == filename) {
                    history.pop();
                }
            }

            // Add a failure record to the private chat history.
             history.push(PrivateChatItem::TransferFailed {
                filename: filename.clone(),
                error: error.clone(), // Store the error message.
            });

             // Auto-scroll if viewing this chat and near the bottom.
             if let tui::ChatContext::Private { target_peer_id, .. } = &app.current_chat_context {
                if *target_peer_id == peer_id {
                     let current_max_scroll = history.len().saturating_sub(app.chat_viewport_height.max(1));
                     if app.chat_scroll >= current_max_scroll.saturating_sub(1) { // Scroll if near bottom
                        app.chat_scroll = current_max_scroll;
                    }
                }
            }

            // Notify the user in the console about the transfer failure.
            let peer_display_name = app.peers.get(&peer_id)
                .and_then(|info| info.nickname.clone())
                .unwrap_or_else(|| utils::peer_id_to_short_string(&peer_id));
            app.push(format!("❌ Transfer failed for '{}' from {}: {}", filename, peer_display_name, error));
            redraw = true;
        }

        // --- Events Primarily Handled by Swarm Task ---
        // These events represent commands sent *from* the UI/Input handler *to* the
        // swarm task, or events that should be directly handled by the swarm task.
        // If they arrive here, it's likely an unexpected state or logic error.
        AppEvent::SendAcceptOffer { target_peer, filename, size_bytes } => {
             // Forward this command directly to the swarm task.
            let _ = cmd_tx.send(AppEvent::SendAcceptOffer { target_peer, filename, size_bytes });
            // No UI state change needed here, so no redraw.
        }
        AppEvent::PublishGossipsub(_) |
        AppEvent::SendPrivateMessage { .. } |
        AppEvent::SendFileOffer { .. } |
        AppEvent::DeclineFileOffer { .. } | // This is a command *to* the swarm task.
        AppEvent::DownloadDirChanged(_) |
        AppEvent::RegisterOutgoingTransfer { .. } | // Handled in FileOfferAccepted above.
        AppEvent::VisibilityChanged(_) |
        AppEvent::Dial(_) |
        AppEvent::EnterChat(_) | // Handled by input_handler.
        AppEvent::Quit => { // Handled after this function returns in main loop.
            // Log a warning if these events are received unexpectedly in the UI event handler.
            app.log(format!("Warning: Received unexpected event meant for swarm task or input handler in UI handler: {:?}", event));
            // Decide if redraw is needed for unexpected events; default to false.
            // redraw = true;
        }
    }

    redraw // Return whether the UI needs to be redrawn.
} 