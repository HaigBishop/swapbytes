/*
The main loop for the Swarm task.
*/

use crate::{AppEvent, behavior::{SwapBytesBehaviour, SwapBytesBehaviourEvent}, protocol, constants, tui::DownloadState};
use libp2p::{
    gossipsub::{self, IdentTopic}, // Added IdentTopic here
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

#[allow(clippy::too_many_lines)]
pub async fn run_swarm_loop(
    mut swarm: Swarm<SwapBytesBehaviour>,
    swarm_tx: mpsc::UnboundedSender<AppEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<AppEvent>,
    swarm_cancel: CancellationToken,
    initial_nickname: Option<String>,
    initial_visibility: bool,
) {
    // Store the current nickname locally within the swarm task
    let mut current_nickname = initial_nickname;
    // Store the current visibility state locally within the swarm task
    let mut is_visible = initial_visibility;
    // Initialize local state for download directory and outgoing transfers
    let mut download_dir: Option<PathBuf> = None;
    let mut outgoing_transfers: HashMap<(PeerId, String), PathBuf> = HashMap::new();
    // Initialize map for incoming transfer states within the swarm task
    let mut incoming_transfers_state: HashMap<PeerId, HashMap<String, crate::tui::DownloadState>> = HashMap::new();
    // Heartbeat interval timer
    let mut heartbeat_timer = interval(constants::HEARTBEAT_INTERVAL);
    // Define the topic here once
    let topic = IdentTopic::new(constants::SWAPBYTES_TOPIC);

    loop {
        tokio::select! {
            // Graceful shutdown
            _ = swarm_cancel.cancelled() => break,

            // --- Heartbeat Broadcaster ---
            _ = heartbeat_timer.tick() => {
                // Only send heartbeat if visible
                if is_visible {
                    // Log heartbeat sending
                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Sending heartbeat (visible: {})", is_visible)));
                    // Get current timestamp in milliseconds
                    let timestamp_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("Time went backwards")
                        .as_millis() as u64; // Use u64

                    let heartbeat_msg = protocol::Message::Heartbeat {
                        timestamp_ms,
                        nickname: current_nickname.clone(),
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
                    // Handle publishing gossipsub messages
                    AppEvent::PublishGossipsub(data) => {
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), data) {
                            // Ignore InsufficientPeers, log other messages though
                            if e.to_string() != "InsufficientPeers" {
                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("Failed to publish chat message: {e}")));
                            }
                        }
                    }
                    // Handle sending private messages via Request/Response
                    AppEvent::SendPrivateMessage { target_peer, message } => {
                        let request = protocol::PrivateRequest::ChatMessage(message);
                        // Send the request
                        swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                        // Log the attempt (optional)
                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("Sent private message request to {}", target_peer)));
                    }
                    // Handle sending file offers
                    AppEvent::SendFileOffer { target_peer, file_path } => {
                        match std::fs::metadata(&file_path) {
                            Ok(metadata) => {
                                // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Preparing SendFileOffer event for {} to {}", file_path.display(), target_peer)));
                                if metadata.is_file() {
                                    let size_bytes = metadata.len();
                                    // Extract filename from path
                                    let filename = file_path.file_name().map_or_else(
                                        || "unknown_file".to_string(), // Fallback filename
                                        |os_name| os_name.to_string_lossy().into_owned()
                                    );

                                    // <<< Store mapping immediately so RequestChunk can be served even before AcceptOffer arrives >>>
                                    outgoing_transfers.insert((target_peer, filename.clone()), file_path.clone());
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Stored pending offer for '{}' to {}", filename, target_peer)));

                                    let request = protocol::PrivateRequest::Offer { filename: filename.clone(), size_bytes };
                                    // Send the request
                                    swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                                    // Log after attempting send
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Attempted send_request (Offer) to {} for {}", target_peer, file_path.display())));
                                } else {
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("Error: Offer path is not a file: {}", file_path.display())));
                                }
                            }
                            Err(e) => {
                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("Error getting file metadata for offer: {} ({})", file_path.display(), e)));
                            }
                        }
                    }
                    // Handle declining file offers
                    AppEvent::DeclineFileOffer { target_peer, filename } => {
                        // <<< Remove mapping if exists >>>
                        outgoing_transfers.remove(&(target_peer, filename.clone()));
                        let request = protocol::PrivateRequest::DeclineOffer { filename };
                        // Send the request
                        swarm.behaviour_mut().request_response.send_request(&target_peer, request);
                        // Log the attempt (optional)
                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Sent DeclineOffer request to {} for file {}", target_peer, filename)));
                    }
                    // Handle accepting file offers
                    AppEvent::SendAcceptOffer { target_peer, filename, size_bytes } => {
                        // Send the AcceptOffer network request *first*
                        let accept_request = protocol::PrivateRequest::AcceptOffer { filename: filename.clone() };
                        swarm.behaviour_mut().request_response.send_request(&target_peer, accept_request);

                        // Now, initiate the download process
                        let filename_c = filename.clone(); // Clone for state map key
                        // Use the local, mutable download_dir variable now
                        match download_dir.as_deref() {
                            Some(dir) => {
                                // Construct temporary file path
                                let temp_filename = format!("{}.tmp", filename);
                                let mut temp_path = PathBuf::from(dir);
                                temp_path.push(&temp_filename);

                                // Attempt to create the file
                                match TokioFile::create(&temp_path).await {
                                    Ok(file) => {
                                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                        //     "[Swarm Task] Created temp file for download: {}",
                                        //     temp_path.display()
                                        // )));

                                        // Create DownloadState
                                        let download_state = DownloadState {
                                            local_path: temp_path.clone(), // Store the temp path
                                            total_size: size_bytes,
                                            received: 0,
                                            next_chunk: 0,
                                            file, // Move file handle into state
                                        };

                                        // Store state in the task's map
                                        incoming_transfers_state
                                            .entry(target_peer)
                                            .or_default()
                                            .insert(filename_c, download_state);

                                        // Send RequestChunk for chunk 0
                                        let chunk_request = protocol::PrivateRequest::RequestChunk {
                                            filename: filename.clone(),
                                            chunk_index: 0
                                        };
                                        swarm.behaviour_mut().request_response.send_request(&target_peer, chunk_request);
                                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                        //     "[Swarm Task] Sent RequestChunk 0 for '{}' to {}",
                                        //     filename, target_peer
                                        // )));
                                    }
                                    Err(e) => {
                                        // Failed to create temp file
                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                            "[Swarm Task] Error creating temp file '{}': {}",
                                            temp_path.display(), e
                                        )));
                                        // TODO: Send FileTransferFailed event back to UI?
                                    }
                                }
                            }
                            None => {
                                // Download directory not set
                                let _ = swarm_tx.send(AppEvent::LogMessage(
                                    "[Swarm Task] Error: Cannot accept offer because download directory is not set.".to_string()
                                ));
                                // TODO: Send FileTransferFailed event back to UI?
                            }
                        }
                    }
                    // Handle download directory changes
                    AppEvent::DownloadDirChanged(new_dir) => {
                        download_dir = new_dir;
                        // Optional: log the change
                        // let log_msg = format!("[Swarm Task] Download directory updated: {:?}", download_dir);
                        // let _ = swarm_tx.send(AppEvent::LogMessage(log_msg));
                    }
                    // Handle registering outgoing transfers
                    AppEvent::RegisterOutgoingTransfer { peer_id, filename, path } => {
                        // <<< Log receipt of RegisterOutgoingTransfer command >>>
                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Received RegisterOutgoingTransfer command for '{}' to {} (Path: {:?})", filename, peer_id, path)));
                        outgoing_transfers.insert((peer_id, filename.clone()), path);
                        // Optional: log the registration
                        // let log_msg = format!("[Swarm Task] Registered outgoing transfer for '{}' to {}", filename, peer_id);
                        // let _ = swarm_tx.send(AppEvent::LogMessage(log_msg));
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
                        // --- Handle Gossipsub Messages ---
                        SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                            propagation_source: peer_id, // The peer who forwarded us the message
                            message_id: _id,
                            message,
                        })) => {
                            // Attempt to deserialize the message
                            match serde_json::from_slice::<protocol::Message>(&message.data) {
                                Ok(deserialized_msg) => {
                                    // Log raw message reception before processing
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Received Gossipsub msg ({} bytes) from {}", message.data.len(), peer_id)));
                                    match deserialized_msg {
                                        protocol::Message::Heartbeat { timestamp_ms: _, nickname } => {
                                            // Send NicknameUpdated event if nickname is present
                                            if let Some(nick) = nickname {
                                                // Use the actual source peer ID from the message if available,
                                                // otherwise, we assume the forwarder is the source for nickname updates.
                                                // NOTE: For gossipsub, message.source is usually None unless message signing is enabled.
                                                let source_peer_id = message.source.unwrap_or(peer_id);
                                                let _ = swarm_tx.send(AppEvent::NicknameUpdated(source_peer_id, nick));
                                            }
                                            // Also forward the raw event to update the forwarder's last_seen
                                            let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                                propagation_source: peer_id,
                                                message_id: _id,
                                                message: message.clone(), // Clone message needed here
                                            }))));
                                        }
                                        protocol::Message::GlobalChatMessage { content, timestamp_ms, nickname } => {
                                            // Send the specific GlobalMessageReceived event
                                            // Use the actual source peer ID if available, otherwise use the forwarder
                                            let source_peer_id = message.source.unwrap_or(peer_id);
                                            let _ = swarm_tx.send(AppEvent::GlobalMessageReceived {
                                                sender_id: source_peer_id,
                                                sender_nickname: nickname,
                                                content,
                                                timestamp_ms,
                                            });
                                            // Also forward the raw event to update the forwarder's last_seen
                                             let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                                propagation_source: peer_id,
                                                message_id: _id,
                                                message: message.clone(), // Clone message needed here
                                            }))));
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Log deserialization error, but still forward raw event for presence
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Failed to deserialize gossipsub msg from {}: {}", peer_id, e)));
                                    let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(SwapBytesBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                                        propagation_source: peer_id,
                                        message_id: _id,
                                        message: message.clone(), // Clone message needed here
                                    }))));
                                }
                            }
                        }
                        // --- Handle Request/Response Events ---
                        SwarmEvent::Behaviour(SwapBytesBehaviourEvent::RequestResponse(event)) => {
                            match event {
                                RequestResponseEvent::Message { peer, message, .. } => match message {
                                    RequestResponseMessage::Request { request, channel, .. } => {
                                        match request {
                                            protocol::PrivateRequest::ChatMessage(text) => {
                                                // Send PrivateMessageReceived event to UI task
                                                if let Err(e) = swarm_tx.send(AppEvent::PrivateMessageReceived {
                                                    sender_id: peer,
                                                    content: text,
                                                }) {
                                                    // Log if sending to UI fails
                                                    eprintln!("[Swarm] Error sending PrivateMessageReceived to UI: {}", e);
                                                }

                                                // Send Ack response
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // Handle incoming file offers
                                            protocol::PrivateRequest::Offer { filename, size_bytes } => {
                                                // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Received Offer request from {}: File '{}' ({} bytes)", peer, filename, size_bytes)));
                                                // Send FileOfferReceived event to UI task
                                                if let Err(e) = swarm_tx.send(AppEvent::FileOfferReceived {
                                                    sender_id: peer,
                                                    filename: filename.clone(), // Clone filename
                                                    size_bytes,
                                                }) {
                                                    eprintln!("[Swarm] Error sending FileOfferReceived to UI: {}", e);
                                                }

                                                // Send Ack response to confirm receipt of the offer message
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // Handle incoming decline messages
                                            protocol::PrivateRequest::DeclineOffer { filename } => {
                                                // Notify the UI that the offer was declined by the peer
                                                if let Err(e) = swarm_tx.send(AppEvent::FileOfferDeclined { peer_id: peer, filename }) {
                                                    eprintln!("[Swarm] Error sending FileOfferDeclined to UI: {}", e);
                                                }
                                                // Acknowledge receipt of the decline message
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // Handle incoming accept offer messages
                                            protocol::PrivateRequest::AcceptOffer { filename } => {
                                                // <<< Log receipt of AcceptOffer >>>
                                                // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Received AcceptOffer from {} for '{}'", peer, filename)));
                                                // Notify the UI that the offer was accepted by the peer
                                                if let Err(e) = swarm_tx.send(AppEvent::FileOfferAccepted { peer_id: peer, filename: filename.clone() }) { // Clone filename here
                                                    eprintln!("[Swarm] Error sending FileOfferAccepted to UI: {}", e);
                                                }
                                                // Acknowledge receipt of the accept message
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, protocol::PrivateResponse::Ack) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending Ack response to {}: {:?}", peer, e)));
                                                }
                                            }
                                            // --- Handle incoming chunk requests ---
                                            protocol::PrivateRequest::RequestChunk { filename, chunk_index } => {
                                                // <<< Log before lookup >>>
                                                // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Handling RequestChunk({}, {}) from {}. Checking map...", filename, chunk_index, peer)));

                                                // Use the local, mutable outgoing_transfers variable now
                                                let response = match outgoing_transfers.get(&(peer, filename.clone())) {
                                                    Some(file_path) => {
                                                        // File path found, attempt to read the chunk
                                                        match tokio::fs::File::open(file_path).await {
                                                            Ok(mut file) => {
                                                                // Get file size first, handling potential error
                                                                let file_size = match file.metadata().await {
                                                                    Ok(meta) => meta.len(),
                                                                    Err(e) => {
                                                                        // Failed to get metadata, send error response and stop processing this request
                                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error getting metadata for '{}': {}", filename, e)));
                                                                        let error_response = protocol::PrivateResponse::TransferError {
                                                                            filename: filename.clone(),
                                                                            error: format!("Failed to get file metadata: {}", e),
                                                                        };
                                                                        // Send error response immediately
                                                                        if let Err(send_err) = swarm.behaviour_mut().request_response.send_response(channel, error_response) {
                                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending metadata TransferError response to {}: {:?}", peer, send_err)));
                                                                        }
                                                                        return; // Stop processing this RequestChunk
                                                                    }
                                                                };

                                                                let offset = chunk_index * crate::constants::CHUNK_SIZE as u64;
                                                                if offset >= file_size {
                                                                    // Requested chunk beyond file size (shouldn't normally happen if receiver tracks size)
                                                                    protocol::PrivateResponse::TransferError {
                                                                        filename: filename.clone(),
                                                                        error: "Requested chunk index out of bounds".to_string(),
                                                                    }
                                                                } else {
                                                                    let mut buffer = vec![0u8; crate::constants::CHUNK_SIZE];
                                                                    // Seek to the correct position
                                                                    if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error seeking file '{}' at offset {}: {}", filename, offset, e)));
                                                                         protocol::PrivateResponse::TransferError {
                                                                            filename: filename.clone(),
                                                                            error: format!("Failed to seek file: {}", e),
                                                                        }
                                                                    } else {
                                                                        // Read the chunk
                                                                        match file.read(&mut buffer).await {
                                                                            Ok(bytes_read) => {
                                                                                // Resize buffer to actual bytes read
                                                                                buffer.truncate(bytes_read);
                                                                                let is_last = (offset + bytes_read as u64) >= file_size;
                                                                                protocol::PrivateResponse::FileChunk {
                                                                                    filename: filename.clone(),
                                                                                    chunk_index,
                                                                                    data: buffer,
                                                                                    is_last,
                                                                                }
                                                                            }
                                                                            Err(e) => {
                                                                                // Failed to read chunk
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
                                                                // Failed to open file
                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error opening file '{}' for transfer: {}", filename, e)));
                                                                protocol::PrivateResponse::TransferError {
                                                                    filename: filename.clone(),
                                                                    error: format!("Failed to open file: {}", e),
                                                                }
                                                            }
                                                        }
                                                    }
                                                    None => {
                                                        // No outgoing transfer registered for this peer/filename combination
                                                        // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Received RequestChunk for unknown transfer: Peer {}, File '{}'", peer, filename)));
                                                        protocol::PrivateResponse::TransferError {
                                                            filename: filename.clone(),
                                                            error: "No active transfer found for this file".to_string(),
                                                        }
                                                    }
                                                };

                                                // Send the response (FileChunk or TransferError)
                                                if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, response) {
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending chunk/error response to {}: {:?}", peer, e)));
                                                }
                                            }
                                        }
                                    }
                                    RequestResponseMessage::Response { request_id, response } => {
                                        match response {
                                            protocol::PrivateResponse::Ack => {
                                                // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Received Ack for request {:?} from {}", request_id, peer)));
                                            }
                                            // --- Handle incoming file chunks ---
                                            protocol::PrivateResponse::FileChunk { filename, chunk_index, data, is_last } => {
                                                // Look up DownloadState.
                                                // Write data to file.
                                                // Update progress.
                                                // Request next chunk or finalize.
                                                // Get the mutable state for this peer's downloads
                                                if let Some(peer_downloads) = incoming_transfers_state.get_mut(&peer) {
                                                    // Get the mutable state for the specific file download
                                                    if let Some(state) = peer_downloads.get_mut(&filename) {
                                                        // Verify chunk index
                                                        if chunk_index != state.next_chunk {
                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                                "[Swarm Task] Error: Received out-of-order chunk for '{}' from {}. Expected {}, Got {}. Ignoring.",
                                                                filename, peer, state.next_chunk, chunk_index
                                                            )));
                                                            // Optional: Send TransferError back?
                                                            return; // Stop processing this chunk
                                                        }

                                                        // Write data to file
                                                        match state.file.write_all(&data).await {
                                                            Ok(_) => {
                                                                // Update state
                                                                let bytes_written = data.len() as u64;
                                                                let previous_progress_marker = state.received / crate::constants::PROGRESS_UPDATE_BYTES;
                                                                state.received += bytes_written;
                                                                state.next_chunk += 1;
                                                                let current_progress_marker = state.received / crate::constants::PROGRESS_UPDATE_BYTES;

                                                                // Send progress update if threshold crossed
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

                                                                // Request next or finalize
                                                                if is_last {
                                                                    // Final chunk received
                                                                    // Flush and sync file
                                                                    if let Err(e) = state.file.flush().await {
                                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error flushing file '{}': {}", state.local_path.display(), e)));
                                                                    }
                                                                    if let Err(e) = state.file.sync_all().await {
                                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error syncing file '{}': {}", state.local_path.display(), e)));
                                                                    }

                                                                    // Remove the state *before* rename/cleanup attempts
                                                                    let state_owned = peer_downloads.remove(&filename).expect("State should exist here");

                                                                    // Construct final path with collision handling
                                                                    let mut final_path = state_owned.local_path.clone(); // Use state_owned now
                                                                    final_path.set_extension(""); // Remove .tmp extension conceptually
                                                                    let original_final_path = final_path.clone(); // Keep original for potential renaming

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
                                                                        // Safety break after too many attempts (highly unlikely)
                                                                        if counter > 10 {
                                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Error: Could not find unique filename for '{}' after {} attempts. Aborting rename.", original_final_path.display(), counter)));
                                                                            let fail_event = AppEvent::FileTransferFailed {
                                                                                 peer_id: peer,
                                                                                 filename: filename.clone(),
                                                                                 error: "Failed to create unique final filename".to_string(),
                                                                             };
                                                                             let _ = swarm_tx.send(fail_event);
                                                                            // Attempt cleanup of temp file
                                                                            let _ = tokio::fs::remove_file(&state_owned.local_path).await; // Use state_owned
                                                                            return;
                                                                        }
                                                                    }

                                                                    // Rename temp file to final path
                                                                    match tokio::fs::rename(&state_owned.local_path, &final_path).await { // Use state_owned
                                                                        Ok(_) => {
                                                                            // Send completion event to UI
                                                                             let success_event = AppEvent::FileTransferComplete {
                                                                                peer_id: peer,
                                                                                filename: filename.clone(),
                                                                                path: final_path.clone(), // Send final path to UI
                                                                                total_size: state_owned.total_size, // <<< Include total size
                                                                            };
                                                                            if let Err(e) = swarm_tx.send(success_event) {
                                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Error sending completion event to UI: {}", e)));
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            // Failed to rename file
                                                                            let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                                                "[Swarm Task] Error renaming temp file '{}' to '{}': {}. Download failed.",
                                                                                state_owned.local_path.display(), final_path.display(), e // Use state_owned
                                                                            )));
                                                                            // Send failure event to UI
                                                                            let fail_event = AppEvent::FileTransferFailed {
                                                                                 peer_id: peer,
                                                                                 filename: filename.clone(),
                                                                                 error: format!("Failed to rename temp file: {}", e),
                                                                             };
                                                                            let _ = swarm_tx.send(fail_event);
                                                                             // Attempt cleanup of temp file
                                                                            let _ = tokio::fs::remove_file(&state_owned.local_path).await; // Use state_owned
                                                                        }
                                                                    }
                                                                } else {
                                                                    // Not the last chunk, request the next one
                                                                    let chunk_request = protocol::PrivateRequest::RequestChunk {
                                                                        filename: filename.clone(),
                                                                        chunk_index: state.next_chunk
                                                                    };
                                                                    swarm.behaviour_mut().request_response.send_request(&peer, chunk_request);
                                                                }
                                                            }
                                                            Err(e) => {
                                                                // Failed to write chunk to file
                                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                                    "[Swarm Task] Error writing chunk {} for file '{}' to '{}': {}. Download failed.",
                                                                    chunk_index, filename, state.local_path.display(), e
                                                                )));
                                                                // Send failure event to UI
                                                                 let fail_event = AppEvent::FileTransferFailed {
                                                                     peer_id: peer,
                                                                     filename: filename.clone(),
                                                                     error: format!("Failed to write to file: {}", e),
                                                                 };
                                                                let _ = swarm_tx.send(fail_event);

                                                                // Remove state to allow file handle to drop before attempting removal
                                                                let state_owned_err = peer_downloads.remove(&filename).expect("State should exist here on write error");
                                                                // Attempt cleanup of temp file
                                                                let _ = tokio::fs::remove_file(&state_owned_err.local_path).await; // Use state_owned_err
                                                            }
                                                        }
                                                    } else {
                                                        // No download state found for this filename
                                                        let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                            "[Swarm Task] Received FileChunk for unknown download '{}' from {}. Ignoring.",
                                                            filename, peer
                                                        )));
                                                    }
                                                } else {
                                                    // No download state found for this peer
                                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                        "[Swarm Task] Received FileChunk for unknown peer {} (File '{}'). Ignoring.",
                                                        peer, filename
                                                    )));
                                                }
                                            }
                                            // --- Handle transfer errors ---
                                            protocol::PrivateResponse::TransferError { filename, error } => {
                                                let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                    "[Swarm Task] Received TransferError from {} for file '{}': {}",
                                                    peer, filename, error
                                                )));
                                                // Attempt to find and clean up the download state
                                                if let Some(peer_downloads) = incoming_transfers_state.get_mut(&peer) {
                                                    if let Some(state_owned) = peer_downloads.remove(&filename) {
                                                         // Send failure event to UI
                                                        let fail_event = AppEvent::FileTransferFailed {
                                                            peer_id: peer,
                                                            filename: filename.clone(),
                                                            error: format!("Transfer failed on sender side: {}", error),
                                                        };
                                                        let _ = swarm_tx.send(fail_event);
                                                        // Attempt cleanup of temp file
                                                        let _ = tokio::fs::remove_file(&state_owned.local_path).await;
                                                    } else {
                                                         let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                            "[Swarm Task] Received TransferError for unknown download '{}' from {}. No cleanup needed.",
                                                            filename, peer
                                                        )));
                                                    }
                                                } else {
                                                     let _ = swarm_tx.send(AppEvent::LogMessage(format!(
                                                        "[Swarm Task] Received TransferError for unknown peer {} (File '{}'). No cleanup needed.",
                                                        peer, filename
                                                    )));
                                                }
                                            }
                                        }
                                        // Avoid request_id unused warning
                                        let _ = request_id;
                                    }
                                }
                                RequestResponseEvent::OutboundFailure { peer, request_id, error, .. } => {
                                    // Explicitly log outbound failures for RequestResponse
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm Task] Outbound RequestResponse Failure to {}: ReqID {:?}, Error: {}", peer, request_id, error)));
                                }
                                RequestResponseEvent::InboundFailure { peer, request_id, error, .. } => {
                                    let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Inbound RequestResponse failure from {}: Request {:?}, Error: {}", peer, request_id, error)));
                                }
                                RequestResponseEvent::ResponseSent { peer, request_id, .. } => {
                                    // Optional: Log when response is successfully sent
                                    // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Sent response for request {:?} to {}", request_id, peer)));

                                    let _ = peer;
                                    let _ = request_id;
                                }
                            }
                        }
                        // Forward other behaviour events (like Ping) generically
                        SwarmEvent::Behaviour(other_behaviour_event) => {
                            // Check if it's NOT a Gossipsub or RequestResponse event before forwarding generically
                            if !matches!(other_behaviour_event, SwapBytesBehaviourEvent::Gossipsub(_) | SwapBytesBehaviourEvent::RequestResponse(_)) {
                                 let _ = swarm_tx.send(AppEvent::Swarm(SwarmEvent::Behaviour(other_behaviour_event)));
                            } else {
                                // Log ignored Gossipsub/ReqRes events
                                // let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] Ignored specific Gossipsub/ReqRes event: {:?}", other_behaviour_event)));
                            }
                        }
                        // Forward non-behaviour swarm events (like NewListenAddr, ConnectionEstablished, etc.)
                        other_swarm_event => {
                            // Log specific connection events
                            // match &other_swarm_event {
                            //     SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] ConnectionEstablished: {} ({:?})", peer_id, endpoint.get_remote_address())));
                            //     },
                            //     SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                            //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] ConnectionClosed: {} (Cause: {:?})", peer_id, cause)));
                            //     },
                            //     SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] OutgoingConnectionError: {:?} ({})", peer_id, error)));
                            //     },
                            //     SwarmEvent::IncomingConnectionError { error, local_addr, send_back_addr, .. } => {
                            //         let _ = swarm_tx.send(AppEvent::LogMessage(format!("[Swarm] IncomingConnectionError: {} (Local: {:?}, Remote: {:?})", error, local_addr, send_back_addr)));
                            //     },
                            //     _ => {} // Ignore others for now
                            // }

                            let _ = swarm_tx.send(AppEvent::Swarm(other_swarm_event));
                        }
                    }
                } else {
                    break; // Swarm stream ended
                }
            }
        }
    }
} 