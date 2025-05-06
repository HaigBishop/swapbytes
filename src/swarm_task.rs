/*
The main loop for the Swarm task.
*/

// --- Imports ---
use crate::{AppEvent, behavior::{SwapBytesBehaviour, SwapBytesBehaviourEvent}, protocol, constants, tui::DownloadState};
use libp2p::{
    gossipsub::{self, IdentTopic},
    mdns,
    request_response::{Event as RequestResponseEvent, Message as RequestResponseMessage},
    swarm::{Swarm, SwarmEvent},
    PeerId,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio::time::interval;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use futures::prelude::*;
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::Local;

// --- Main Swarm Task Function ---
#[allow(clippy::too_many_lines)]
pub async fn run_swarm_loop(
    mut swarm: Swarm<SwapBytesBehaviour>,
    swarm_tx: mpsc::UnboundedSender<AppEvent>, // Channel to send events back to the UI/main task
    mut cmd_rx: mpsc::UnboundedReceiver<AppEvent>, // Channel to receive commands from the UI/main task
    swarm_cancel: CancellationToken, // Used for graceful shutdown
    initial_nickname: Option<String>,
    initial_visibility: bool,
) {
    // --- Local State ---
    let mut current_nickname = initial_nickname; // User's current nickname for gossipsub messages
    let mut is_visible = initial_visibility; // Whether the user is broadcasting presence
    let mut download_dir: Option<PathBuf> = None; // Directory for saving incoming files
    // Stores the local path of files being offered to peers. Key: (PeerId, filename)
    let mut outgoing_transfers: HashMap<(PeerId, String), PathBuf> = HashMap::new();
    // Stores the state of incoming file transfers. Key: PeerId -> (filename -> DownloadState)
    let mut incoming_transfers_state: HashMap<PeerId, HashMap<String, crate::tui::DownloadState>> = HashMap::new();
    let mut heartbeat_timer = interval(constants::HEARTBEAT_INTERVAL); // Timer for periodic heartbeat broadcasts
    let topic = IdentTopic::new(constants::SWAPBYTES_TOPIC); // Gossipsub topic for general communication

    // --- Main Event Loop ---
    loop {
        tokio::select! {
            // --- Graceful Shutdown ---
            // Listen for cancellation signal
            _ = swarm_cancel.cancelled() => break,

            // --- Heartbeat Broadcaster ---
            // Periodically send heartbeat messages if visible
            _ = heartbeat_timer.tick() => {
                if is_visible {
                    // Get current timestamp in milliseconds
                    let timestamp_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("Time went backwards")
                        .as_millis() as u64;

                    // Construct the heartbeat message
                    let heartbeat_msg = protocol::Message::Heartbeat {
                        timestamp_ms,
                        nickname: current_nickname.clone(),
                    };

                    // Serialize and publish the heartbeat message via gossipsub
                    match serde_json::to_vec(&heartbeat_msg) {
                        Ok(encoded_msg) => {
                            if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), encoded_msg) {
                                // Log errors, ignoring "InsufficientPeers" as it's expected sometimes
                                if e.to_string() != "InsufficientPeers" {
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to publish heartbeat: {e}")));
                                }
                            }
                        }
                        Err(e) => {
                             let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to serialize heartbeat: {e}")));
                        }
                    }
                }
            }

            // --- Command Handling ---
            // Process commands received from the UI task via the command channel
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    // --- Dial Command ---
                    AppEvent::Dial(addr) => {
                        let log_msg = match swarm.dial(addr.clone()) {
                            Ok(()) => format!("Dialing {addr}"),
                            Err(e) => format!("Dial error: {e}"),
                        };
                        let _ = swarm_tx.send(AppEvent::LogMessage(log_msg));
                    }
                    // --- Nickname Update Command ---
                    AppEvent::NicknameUpdated(_peer_id, nickname) => {
                        // Update the local nickname state
                        current_nickname = Some(nickname);
                    }
                    // --- Visibility Change Command ---
                    AppEvent::VisibilityChanged(new_visibility) => {
                        is_visible = new_visibility;
                    }
                    // --- Publish Gossipsub Command ---
                    AppEvent::PublishGossipsub(data) => {
                        // Publish raw data provided by the UI (e.g., global chat messages)
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), data) {
                            if e.to_string() != "InsufficientPeers" {
                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to publish chat message: {e}")));
                            }
                        }
                    }
                    // --- Send Private Message Command ---
                    AppEvent::SendPrivateMessage { target_peer, message } => {
                        let request = protocol::PrivateRequest::ChatMessage(message);
                        // Use the request-response protocol to send a private message
                        swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                    }
                    // --- Send File Offer Command ---
                    AppEvent::SendFileOffer { target_peer, file_path } => {
                        match std::fs::metadata(&file_path) {
                            Ok(metadata) => {
                                if metadata.is_file() {
                                    let size_bytes = metadata.len();
                                    let filename = file_path.file_name().map_or_else(
                                        || "unknown_file".to_string(), // Fallback filename
                                        |os_name| os_name.to_string_lossy().into_owned()
                                    );

                                    // Store the file path immediately to handle potential RequestChunk before AcceptOffer
                                    outgoing_transfers.insert((target_peer, filename.clone()), file_path.clone());

                                    // Construct and send the Offer request via request-response
                                    let request = protocol::PrivateRequest::Offer { filename: filename.clone(), size_bytes };
                                    swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                                } else {
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("Error: Offer path is not a file: {}", file_path.display())));
                                }
                            }
                            Err(e) => {
                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("Error getting file metadata for offer: {} ({})", file_path.display(), e)));
                            }
                        }
                    }
                    // --- Decline File Offer Command ---
                    AppEvent::DeclineFileOffer { target_peer, filename } => {
                        // Remove the pending offer from local state if it exists
                        outgoing_transfers.remove(&(target_peer, filename.clone()));
                        // Construct and send the DeclineOffer request via request-response
                        let request = protocol::PrivateRequest::DeclineOffer { filename };
                        swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                    }
                    // --- Accept File Offer Command ---
                    AppEvent::SendAcceptOffer { target_peer, filename, size_bytes } => {
                        // Send the AcceptOffer network request *first* to notify the sender
                        let accept_request = protocol::PrivateRequest::AcceptOffer { filename: filename.clone() };
                        swarm.behaviour_mut().request_response.send_request(&target_peer, accept_request);

                        // Begin the local download process
                        let filename_c = filename.clone(); // Clone needed for map key
                        match download_dir.as_deref() {
                            Some(dir) => {
                                // Construct temporary file path (e.g., file.ext.tmp)
                                let temp_filename = format!("{}.tmp", filename);
                                let mut temp_path = PathBuf::from(dir);
                                temp_path.push(&temp_filename);

                                // Attempt to create the temporary file
                                match TokioFile::create(&temp_path).await {
                                    Ok(file) => {
                                        // Create the initial download state
                                        let download_state = DownloadState {
                                            local_path: temp_path.clone(), // Store the temp path
                                            total_size: size_bytes,
                                            received: 0,
                                            next_chunk: 0,
                                            file, // Move file handle into state
                                        };

                                        // Store the download state associated with the peer and filename
                                        incoming_transfers_state
                                            .entry(target_peer)
                                            .or_default()
                                            .insert(filename_c, download_state);

                                        // Send the initial RequestChunk (chunk 0) to start the transfer
                                        let chunk_request = protocol::PrivateRequest::RequestChunk {
                                            filename: filename.clone(),
                                            chunk_index: 0
                                        };
                                        swarm.behaviour_mut().request_response.send_request(&target_peer, chunk_request);
                                    }
                                    Err(e) => {
                                        // Failed to create the temporary file, log error
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                            "[Swarm Task] Error creating temp file '{}': {}",
                                            temp_path.display(), e
                                        )));
                                        // TODO: Consider sending FileTransferFailed to UI
                                    }
                                }
                            }
                            None => {
                                // Download directory is not set, cannot accept offer
                                let _ = swarm_tx.send(AppEvent::LogMessage(
                                    "[Swarm Task] Error: Cannot accept offer because download directory is not set.".to_string()
                                ));
                                // TODO: Consider sending FileTransferFailed to UI
                            }
                        }
                    }
                    // --- Download Directory Change Command ---
                    AppEvent::DownloadDirChanged(new_dir) => {
                        // Update the local download directory path
                        download_dir = new_dir;
                    }
                    // --- Register Outgoing Transfer Command ---
                    // This is typically used when an offer is accepted by a remote peer
                    AppEvent::RegisterOutgoingTransfer { peer_id, filename, path } => {
                        // Store the mapping for an active outgoing transfer
                        outgoing_transfers.insert((peer_id, filename.clone()), path);
                    }
                    // Ignore any other unexpected commands
                    _ => {}
                }
            }


            // --- Swarm Event Handling ---
            // Process events generated by the libp2p Swarm
            ev = swarm.next() => {
                if let Some(event) = ev {
                    match event {
                        // --- mDNS Events ---
                        SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                            for (peer_id, _multiaddr) in list {
                                // Add newly discovered peers to Gossipsub for routing
                                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                // Notify the UI about the discovered peer
                                let _ = swarm_tx.send(AppEvent::PeerDiscovered(peer_id));
                            }
                        }
                        SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                            for (peer_id, _multiaddr) in list {
                                // Remove expired peers from Gossipsub
                                swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                            }
                        }
                        // --- Gossipsub Message Events ---
                        SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                            propagation_source: peer_id, // The peer who forwarded the message
                            message_id: _id,
                            message,
                        })) => {
                            // Attempt to deserialize the incoming gossipsub message data
                            match serde_json::from_slice::<protocol::Message>(&message.data) {
                                Ok(deserialized_msg) => {
                                    match deserialized_msg {
                                        // Handle Heartbeat messages
                                        protocol::Message::Heartbeat { timestamp_ms: _, nickname } => {
                                            // Update nickname if provided
                                            if let Some(nick) = nickname {
                                                // Use the message source if available (requires signing), else use the forwarder
                                                let source_peer_id = message.source.unwrap_or(peer_id);
                                                let _ = swarm_tx.send(AppEvent::NicknameUpdated(source_peer_id, nick));
                                            }
                                            // Forward the raw event to update the forwarder's last_seen time in the UI
                                            let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                                propagation_source: peer_id,
                                                message_id: _id,
                                                message: message.clone(),
                                            }))));
                                        }
                                        // Handle Global Chat messages
                                        protocol::Message::GlobalChatMessage { content, timestamp_ms, nickname } => {
                                            // Send a specific event to the UI for global chat messages
                                            let source_peer_id = message.source.unwrap_or(peer_id);
                                            let _ = swarm_tx.send(AppEvent::GlobalMessageReceived {
                                                sender_id: source_peer_id,
                                                sender_nickname: nickname,
                                                content,
                                                timestamp_ms,
                                            });
                                            // Forward the raw event to update the forwarder's last_seen time
                                             let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                                propagation_source: peer_id,
                                                message_id: _id,
                                                message: message.clone(),
                                            }))));
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Log deserialization error, but still forward the raw event for presence tracking
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Failed to deserialize gossipsub msg from {}: {}", peer_id, e)));
                                    let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                        propagation_source: peer_id,
                                        message_id: _id,
                                        message: message.clone(),
                                    }))));
                                }
                            }
                        }
                        // --- Request/Response Events ---
                        SwarmEvent::Behaviour(SwapBytesBehaviourEvent::RequestResponse(event)) => {
                            match event {
                                // --- Incoming Request ---
                                RequestResponseEvent::Message { peer, message, .. } => match message {
                                    RequestResponseMessage::Request { request, channel, .. } => {
                                        match request {
                                            // --- Handle Incoming Private Chat Message ---
                                            protocol::PrivateRequest::ChatMessage(text) => {
                                                // Notify UI of the received private message
                                                if let Err(e) = swarm_tx.send(AppEvent::PrivateMessageReceived {
                                                    sender_id: peer,
                                                    content: text,
                                                }) {
                                                    eprintln!("[Swarm] Error sending PrivateMessageReceived to UI: {}", e);
                                                }
                                                // Send an acknowledgement response
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // --- Handle Incoming File Offer ---
                                            protocol::PrivateRequest::Offer { filename, size_bytes } => {
                                                // Notify UI of the received file offer
                                                if let Err(e) = swarm_tx.send(AppEvent::FileOfferReceived {
                                                    sender_id: peer,
                                                    filename: filename.clone(),
                                                    size_bytes,
                                                }) {
                                                    eprintln!("[Swarm] Error sending FileOfferReceived to UI: {}", e);
                                                }
                                                // Send an acknowledgement response
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // --- Handle Incoming Decline Offer Message ---
                                            protocol::PrivateRequest::DeclineOffer { filename } => {
                                                // Notify UI that the remote peer declined an offer we sent
                                                if let Err(e) = swarm_tx.send(AppEvent::FileOfferDeclined { peer_id: peer, filename }) {
                                                    eprintln!("[Swarm] Error sending FileOfferDeclined to UI: {}", e);
                                                }
                                                // Send an acknowledgement response
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // --- Handle Incoming Accept Offer Message ---
                                            protocol::PrivateRequest::AcceptOffer { filename } => {
                                                // Notify UI that the remote peer accepted an offer we sent
                                                // This typically triggers the `RegisterOutgoingTransfer` command handler
                                                if let Err(e) = swarm_tx.send(AppEvent::FileOfferAccepted { peer_id: peer, filename: filename.clone() }) {
                                                    eprintln!("[Swarm] Error sending FileOfferAccepted to UI: {}", e);
                                                }
                                                // Send an acknowledgement response
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // --- Handle Incoming Chunk Request ---
                                            protocol::PrivateRequest::RequestChunk { filename, chunk_index } => {
                                                // Check if we have an active outgoing transfer registered for this peer/file
                                                let response = match outgoing_transfers.get(&(peer, filename.clone())) {
                                                    Some(file_path) => {
                                                        // Attempt to open the local file
                                                        match tokio::fs::File::open(file_path).await {
                                                            Ok(mut file) => {
                                                                // Get file size to calculate offset and check bounds
                                                                let file_size = match file.metadata().await {
                                                                    Ok(meta) => meta.len(),
                                                                    Err(e) => {
                                                                        // Error getting metadata, send error response
                                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error getting metadata for '{}': {}", filename, e)));
                                                                        let error_response = protocol::PrivateResponse::TransferError {
                                                                            filename: filename.clone(),
                                                                            error: format!("Failed to get file metadata: {}", e),
                                                                        };
                                                                        if let Err(send_err) = swarm.behaviour_mut().request_response.send_response(channel, error_response) {
                                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending metadata TransferError response to {}: {:?}", peer, send_err)));
                                                                        }
                                                                        return; // Stop processing this request
                                                                    }
                                                                };

                                                                // Calculate the byte offset for the requested chunk
                                                                let offset = chunk_index * crate::constants::CHUNK_SIZE as u64;
                                                                if offset >= file_size {
                                                                    // Chunk index is out of bounds, send error
                                                                    protocol::PrivateResponse::TransferError {
                                                                        filename: filename.clone(),
                                                                        error: "Requested chunk index out of bounds".to_string(),
                                                                    }
                                                                } else {
                                                                    // Prepare buffer for reading the chunk
                                                                    let mut buffer = vec![0u8; crate::constants::CHUNK_SIZE];
                                                                    // Seek to the calculated offset in the file
                                                                    if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                                                                         // Error seeking, send error response
                                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error seeking file '{}' at offset {}: {}", filename, offset, e)));
                                                                         protocol::PrivateResponse::TransferError {
                                                                            filename: filename.clone(),
                                                                            error: format!("Failed to seek file: {}", e),
                                                                        }
                                                                    } else {
                                                                        // Read the chunk data into the buffer
                                                                        match file.read(&mut buffer).await {
                                                                            Ok(bytes_read) => {
                                                                                // Adjust buffer size to actual bytes read
                                                                                buffer.truncate(bytes_read);
                                                                                // Determine if this is the last chunk
                                                                                let is_last = (offset + bytes_read as u64) >= file_size;
                                                                                // Construct the FileChunk response
                                                                                protocol::PrivateResponse::FileChunk {
                                                                                    filename: filename.clone(),
                                                                                    chunk_index,
                                                                                    data: buffer,
                                                                                    is_last,
                                                                                }
                                                                            }
                                                                            Err(e) => {
                                                                                // Error reading chunk, send error response
                                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error reading chunk {} for file '{}': {}", chunk_index, filename, e)));
                                                                                protocol::PrivateResponse::TransferError {
                                                                                    filename: filename.clone(),
                                                                                    error: format!("Failed to read file chunk: {}", e),
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            Err(e) => {
                                                                // Error opening file, send error response
                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error opening file '{}' for transfer: {}", filename, e)));
                                                                protocol::PrivateResponse::TransferError {
                                                                    filename: filename.clone(),
                                                                    error: format!("Failed to open file: {}", e),
                                                                }
                                                            }
                                                        }
                                                    }
                                                    None => {
                                                        // No active transfer found for this request, send error
                                                        protocol::PrivateResponse::TransferError {
                                                            filename: filename.clone(),
                                                            error: "No active transfer found for this file".to_string(),
                                                        }
                                                    }
                                                };

                                                // Send the constructed response (either FileChunk or TransferError)
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, response) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending chunk/error response to {}: {:?}", peer, e)));
                                                }
                                            }
                                        }
                                    }
                                    // --- Incoming Response ---
                                    RequestResponseMessage::Response { request_id, response } => {
                                        match response {
                                            // --- Handle Acknowledgement Response ---
                                            protocol::PrivateResponse::Ack => {
                                                // Acknowledge responses are typically for confirming receipt of messages like Offer, Decline, Accept, ChatMessage.
                                                // Usually no specific action needed here other than potentially logging.
                                            }
                                            // --- Handle Incoming File Chunk Response ---
                                            protocol::PrivateResponse::FileChunk { filename, chunk_index, data, is_last } => {
                                                // Find the download state for this peer and filename
                                                if let Some(peer_downloads) = incoming_transfers_state.get_mut(&peer) {
                                                    if let Some(state) = peer_downloads.get_mut(&filename) {
                                                        // Verify if the received chunk is the expected one
                                                        if chunk_index != state.next_chunk {
                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                                "[Swarm Task] Error: Received out-of-order chunk for '{}' from {}. Expected {}, Got {}. Ignoring.",
                                                                filename, peer, state.next_chunk, chunk_index
                                                            )));
                                                            // Consider sending a TransferError back or re-requesting the correct chunk
                                                            return; // Stop processing this chunk
                                                        }

                                                        // Write the received data chunk to the temporary file
                                                        match state.file.write_all(&data).await {
                                                            Ok(_) => {
                                                                // Update the download state (bytes received, next expected chunk)
                                                                let bytes_written = data.len() as u64;
                                                                let previous_progress_marker = state.received / crate::constants::PROGRESS_UPDATE_BYTES;
                                                                state.received += bytes_written;
                                                                state.next_chunk += 1;
                                                                let current_progress_marker = state.received / crate::constants::PROGRESS_UPDATE_BYTES;

                                                                // Send progress update to UI if a threshold is crossed or if it's the last chunk
                                                                if current_progress_marker > previous_progress_marker || is_last {
                                                                     if let Err(e) = swarm_tx.send(AppEvent::FileTransferProgress {
                                                                        peer_id: peer,
                                                                        filename: filename.clone(),
                                                                        received: state.received,
                                                                        total: state.total_size,
                                                                    }) {
                                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending progress update to UI: {}", e)));
                                                                    }
                                                                }

                                                                // Handle completion or request the next chunk
                                                                if is_last {
                                                                    // --- Finalize Download ---
                                                                    // Flush and sync the file ensure data is written to disk
                                                                    if let Err(e) = state.file.flush().await {
                                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error flushing file '{}': {}", state.local_path.display(), e)));
                                                                    }
                                                                    if let Err(e) = state.file.sync_all().await {
                                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error syncing file '{}': {}", state.local_path.display(), e)));
                                                                    }

                                                                    // Remove the download state (implicitly drops the file handle)
                                                                    let state_owned = peer_downloads.remove(&filename).expect("State should exist here");

                                                                    // --- Rename Temporary File ---
                                                                    // Construct final path and handle potential collisions
                                                                    let mut final_path = state_owned.local_path.clone();
                                                                    final_path.set_extension(""); // Remove .tmp conceptually
                                                                    let original_final_path = final_path.clone();

                                                                    // Add timestamp if filename collision occurs
                                                                    let mut counter = 0;
                                                                    while final_path.exists() {
                                                                        counter += 1;
                                                                        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
                                                                        let new_stem = format!(
                                                                            "{}_({})",
                                                                            original_final_path.file_stem().unwrap_or_default().to_string_lossy(),
                                                                            timestamp
                                                                        );
                                                                        final_path.set_file_name(new_stem);
                                                                        if let Some(ext) = original_final_path.extension() {
                                                                            final_path.set_extension(ext);
                                                                        }
                                                                        // Safety break to prevent infinite loops
                                                                        if counter > 10 {
                                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error: Could not find unique filename for '{}' after {} attempts. Aborting rename.", original_final_path.display(), counter)));
                                                                            let fail_event = AppEvent::FileTransferFailed {
                                                                                 peer_id: peer,
                                                                                 filename: filename.clone(),
                                                                                 error: "Failed to create unique final filename".to_string(),
                                                                             };
                                                                            let _ = swarm_tx.send(fail_event);
                                                                            // Attempt cleanup of temp file
                                                                            let _ = tokio::fs::remove_file(&state_owned.local_path).await;
                                                                            return; // Stop processing
                                                                        }
                                                                    }

                                                                    // Perform the rename from .tmp to final name
                                                                    match tokio::fs::rename(&state_owned.local_path, &final_path).await {
                                                                        Ok(_) => {
                                                                            // --- Download Successful ---
                                                                            // Notify UI of completion, including the final path and size
                                                                             let success_event = AppEvent::FileTransferComplete {
                                                                                peer_id: peer,
                                                                                filename: filename.clone(),
                                                                                path: final_path.clone(),
                                                                                total_size: state_owned.total_size,
                                                                            };
                                                                            if let Err(e) = swarm_tx.send(success_event) {
                                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending completion event to UI: {}", e)));
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            // --- Download Failed (Rename Error) ---
                                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                                                "[Swarm Task] Error renaming temp file '{}' to '{}': {}. Download failed.",
                                                                                state_owned.local_path.display(), final_path.display(), e
                                                                            )));
                                                                            // Notify UI of failure
                                                                            let fail_event = AppEvent::FileTransferFailed {
                                                                                 peer_id: peer,
                                                                                 filename: filename.clone(),
                                                                                 error: format!("Failed to rename temp file: {}", e),
                                                                             };
                                                                            let _ = swarm_tx.send(fail_event);
                                                                             // Attempt cleanup of temp file
                                                                            let _ = tokio::fs::remove_file(&state_owned.local_path).await;
                                                                        }
                                                                    }
                                                                } else {
                                                                    // --- Request Next Chunk ---
                                                                    // Not the last chunk, send a request for the next one
                                                                    let chunk_request = protocol::PrivateRequest::RequestChunk {
                                                                        filename: filename.clone(),
                                                                        chunk_index: state.next_chunk
                                                                    };
                                                                    swarm.behaviour_mut().request_response.send_request(&peer, chunk_request);
                                                                }
                                                            }
                                                            Err(e) => {
                                                                // --- Download Failed (Write Error) ---
                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                                    "[Swarm Task] Error writing chunk {} for file '{}' to '{}': {}. Download failed.",
                                                                    chunk_index, filename, state.local_path.display(), e
                                                                )));
                                                                // Notify UI of failure
                                                                 let fail_event = AppEvent::FileTransferFailed {
                                                                     peer_id: peer,
                                                                     filename: filename.clone(),
                                                                     error: format!("Failed to write to file: {}", e),
                                                                 };
                                                                let _ = swarm_tx.send(fail_event);

                                                                // Remove state to allow file handle to drop before attempting removal
                                                                let state_owned_err = peer_downloads.remove(&filename).expect("State should exist here on write error");
                                                                // Attempt cleanup of temp file
                                                                let _ = tokio::fs::remove_file(&state_owned_err.local_path).await;
                                                            }
                                                        }
                                                    } else {
                                                        // Received a chunk for a download we don't have state for (filename mismatch)
                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                            "[Swarm Task] Received FileChunk for unknown download '{}' from {}. Ignoring.",
                                                            filename, peer
                                                        )));
                                                    }
                                                } else {
                                                    // Received a chunk from a peer we don't have any active downloads with
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                        "[Swarm Task] Received FileChunk for unknown peer {} (File '{}'). Ignoring.",
                                                        peer, filename
                                                    )));
                                                }
                                            }
                                            // --- Handle Transfer Error Response ---
                                            protocol::PrivateResponse::TransferError { filename, error } => {
                                                // The remote peer reported an error during the transfer
                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                    "[Swarm Task] Received TransferError from {} for file '{}': {}",
                                                    peer, filename, error
                                                )));
                                                // --- Clean up Failed Download ---
                                                // Attempt to find and remove the download state
                                                if let Some(peer_downloads) = incoming_transfers_state.get_mut(&peer) {
                                                    if let Some(state_owned) = peer_downloads.remove(&filename) {
                                                         // Notify UI of failure
                                                        let fail_event = AppEvent::FileTransferFailed {
                                                            peer_id: peer,
                                                            filename: filename.clone(),
                                                            error: format!("Transfer failed on sender side: {}", error),
                                                        };
                                                        let _ = swarm_tx.send(fail_event);
                                                        // Attempt cleanup of the partial temp file
                                                        let _ = tokio::fs::remove_file(&state_owned.local_path).await;
                                                    } else {
                                                        // Error received for a download we didn't know about (filename)
                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                            "[Swarm Task] Received TransferError for unknown download '{}' from {}. No cleanup needed.",
                                                            filename, peer
                                                        )));
                                                    }
                                                } else {
                                                     // Error received for a peer we didn't know about
                                                     let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                        "[Swarm Task] Received TransferError for unknown peer {} (File '{}'). No cleanup needed.",
                                                        peer, filename
                                                    )));
                                                }
                                            }
                                        }
                                        // Prevent unused variable warning for request_id
                                        let _ = request_id;
                                    }
                                }
                                // --- Outbound Request Failure ---
                                RequestResponseEvent::OutboundFailure { peer, request_id, error, .. } => {
                                    // Log failures when sending requests (e.g., network issues, peer disconnected)
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Outbound RequestResponse Failure to {}: ReqID {:?}, Error: {}", peer, request_id, error)));
                                    // TODO: Potentially map request_id back to transfer state and trigger failure cleanup
                                }
                                // --- Inbound Request Failure ---
                                RequestResponseEvent::InboundFailure { peer, request_id, error, .. } => {
                                    // Log failures processing incoming requests (e.g., deserialization error on our side)
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Inbound RequestResponse failure from {}: Request {:?}, Error: {}", peer, request_id, error)));
                                }
                                // --- Response Sent Confirmation ---
                                RequestResponseEvent::ResponseSent { peer, request_id, .. } => {
                                    // Optional: Log confirmation that a response was successfully sent
                                    // Usually not needed unless debugging specific request/response flows
                                    let _ = peer;
                                    let _ = request_id;
                                }
                            }
                        }
                        // --- Forward Other Behaviour Events ---
                        // Handle other behaviour events not specifically matched above (e.g., Ping)
                        SwarmEvent::Behaviour(other_behaviour_event) => {
                            // Avoid forwarding Gossipsub or RequestResponse events again if they fall through
                            if !matches!(other_behaviour_event, SwapBytesBehaviourEvent::Gossipsub(_) | SwapBytesBehaviourEvent::RequestResponse(_)) {
                                 // Forward the generic behaviour event to the UI/main task
                                 let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(other_behaviour_event)));
                            }
                        }
                        // --- Forward Non-Behaviour Swarm Events ---
                        // Handle core Swarm events (Connections, Listen Addresses, etc.)
                        other_swarm_event => {
                            // Forward the generic Swarm event to the UI/main task
                            let _ = swarm_tx.send(AppEvent::Swarm(other_swarm_event));
                        }
                    }
                } else {
                    // The swarm event stream has ended, break the loop
                    break;
                }
            }
        }
    }
} 