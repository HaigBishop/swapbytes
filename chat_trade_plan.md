# Chat and Trade Implementation Plan

This document outlines the design and implementation steps for the following features in the SwapBytes application:

- Global Chat (broadcast chat via Gossipsub)
- Private Chat (point-to-point chat via Request/Response)
- Trade Offer Workflow (send and receive file offers)
- Accept/Decline Workflow (user interaction for offers)
- File Transfer Workflow (chunked file streaming via Request/Response)

It covers:
- Networking protocols and behaviors
- App state and data structures
- UI/TUI changes and rendering logic
- Command parsing and inter-task communication
- New event types and behavior extensions

---

## 1. Global Chat

### 1.1 Networking
- Use `libp2p::gossipsub` on topic `"swapbytes-global-chat"` (defined by `SWAPBYTES_TOPIC`).
- Extend the `Message` enum in `src/main.rs` to include:
  ```rust
  enum Message {
    Heartbeat { timestamp_ms: u64, nickname: Option<String> },
    GlobalChatMessage { content: String, timestamp_ms: u64, nickname: Option<String> },
    // ... other variants
  }
  ```
- In the Swarm task, handle `AppEvent::PublishGossipsub(Vec<u8>)` by calling:
  ```rust
  swarm.behaviour_mut().gossipsub.publish(topic.clone(), data);
  ```

### 1.2 App State & Storage
- In `tui::App`, add:
  ```rust
  pub struct ChatMessage { pub sender: PeerId, pub content: String, pub timestamp_ms: u64 }
  
  pub struct App {
    // ... existing fields ...
    pub global_chat_history: Vec<ChatMessage>,
    pub chat_scroll: usize,
    // private histories in section 2
  }
  ```
- Append new incoming and outgoing global chat messages to `app.global_chat_history` and auto-scroll to bottom (update `chat_scroll`).

### 1.3 Events and Commands
- Define new `AppEvent` variants:
  ```rust
  enum AppEvent {
    GlobalMessageReceived(PeerId, String, u64),
    PublishGossipsub(Vec<u8>),
    // ...
  }
  ```
- In the UI loop, on `AppEvent::GlobalMessageReceived`, push to `global_chat_history` and `redraw = true`.
- **UI Task (`src/main.rs`) Event Loop:**
  - On `AppEvent::GlobalMessageReceived { sender_id, ... }`, create `tui::ChatMessage`, push to `global_chat_history`, update `app.chat_scroll`, and set `redraw = true`.
  - On `AppEvent::NicknameUpdated { peer_id, nickname }`, update the nickname in `app.peers`. Set `redraw = true`.
  - On `AppEvent::Swarm(SwarmEvent::Behaviour(Gossipsub::Message { propagation_source, message, .. }))`:
    - Update the `last_seen` and `status` for the `propagation_source` (forwarding peer) in `app.peers`.
    - **Do not** deserialize `message.data` here. Content handling (chat, heartbeats) is done via specific `AppEvents` sent by the Swarm task.
    - Set `redraw = true`.
- In `commands::process_chat_input`, if `current_chat_context == Global`, serialize `GlobalChatMessage`, send `PublishGossipsub`, and locally append to history.

### 1.4 UI Rendering
- In `tui::render_chat_pane`, detect global context and render `app.global_chat_history`:
  - For each `ChatMessage`, format: `[{HH:MM}] <nick>: content`.
  - Support vertical scrolling using `app.chat_scroll`.

---

## 2. Private Chat

### 2.1 Networking (Request/Response)
- Add `libp2p::request_response` behavior in `behavior.rs`:
  ```rust
  use libp2p::request_response::{ProtocolName, RequestResponse, RequestResponseCodec};
  
  #[derive(NetworkBehaviour)]
  pub struct SwapBytesBehaviour {
    // ... existing fields ...
    pub request_response: RequestResponse<ReqResCodec>,
  }
  
  #[derive(Debug, Clone)]
  pub enum ReqResMessage {
    PrivateChat { content: String, timestamp_ms: u64 },
    FileOffer { filename: String, size: u64, timestamp_ms: u64 },
    FileAccept { timestamp_ms: u64 },
    FileDecline { timestamp_ms: u64 },
    FileChunk { seq: u32, data: Vec<u8> },
    FileTransferComplete { timestamp_ms: u64 },
  }
  
  // Implement ProtocolName and RequestResponseCodec for `ReqResMessage`
  ```
- Extend `SwapBytesBehaviourEvent` in `behavior.rs`:
  ```rust
  pub enum SwapBytesBehaviourEvent {
    // ... existing variants ...
    RequestResponse(request_response::Event<ReqResMessage, ReqResMessage>),
  }
  ```

### 2.2 App State & Storage
- In `tui::App`, add:
  ```rust
  pub struct App {
    // ... existing fields ...
    pub private_chat_histories: HashMap<PeerId, Vec<ChatMessage>>,
    pub pending_offers: HashMap<PeerId, FileOffer>,
    pub active_transfers: HashMap<PeerId, FileTransferState>,
    // reuse chat_scroll for private or add per-peer scroll map
  }
  
  pub struct FileOffer { pub filename: String, pub size: u64, pub timestamp_ms: u64 }
  pub struct FileTransferState {
    pub file: std::fs::File,
    pub bytes_received: u64,
    pub total_size: u64,
    pub chunk_size: usize,
  }
  ```

### 2.3 Events and Commands
- Define `AppEvent` variants:
  ```rust
  enum AppEvent {
    SendRequest(PeerId, ReqResMessage),
    PrivateMessageReceived(PeerId, String, u64),
    FileOfferReceived(PeerId, FileOffer),
    FileAcceptReceived(PeerId),
    FileDeclineReceived(PeerId),
    FileChunkReceived(PeerId, u32, Vec<u8>),
    FileTransferCompleted(PeerId),
    // ...
  }
  ```
- In `commands.rs`, handle `/chat <nick>` to set private context.
- In chat input handler (when sending):
  ```rust
  if let ChatContext::Private { target_peer_id, .. } = app.current_chat_context {
    let msg = ReqResMessage::PrivateChat { content: chat_input.clone(), timestamp_ms };
    event_to_send = Some(AppEvent::SendRequest(target_peer_id, msg));
    app.private_chat_histories.get_mut(&target_peer_id).unwrap().push(...);
  }
  ```
- Handle `/offer <path>`:
  1. Verify file path via `utils::verify_file_exists`.
  2. Create `FileOffer { filename, size, timestamp_ms }`.
  3. `SendRequest(target_peer_id, ReqResMessage::FileOffer{...})`.
  4. Store in `app.pending_offers.insert(target_peer_id, FileOffer)`.
  5. Display interactive prompt in chat pane.
- Handle `/accept` and `/decline` in private mode:
  - Look up `pending_offers` for `target_peer_id`.
  - Send `ReqResMessage::FileAccept` or `FileDecline` via `SendRequest`.
  - On accept: initialize `FileTransferState`, insert into `active_transfers`.

### 2.4 Swarm Task Handling
- In the Swarm loop, handle `SwapBytesBehaviourEvent::RequestResponse(event)`:
  ```rust
  match event {
    RequestResponseEvent::Message { peer, message: Request { request, channel, .. } } => {
      match request {
        ReqResMessage::PrivateChat{content, ts} => {
          tx.send(AppEvent::PrivateMessageReceived(peer, content, ts));
          // send an empty ack response if desired
        }
        ReqResMessage::FileOffer{filename, size, ts} => {
          tx.send(AppEvent::FileOfferReceived(peer, FileOffer{filename, size, ts}));
        }
        ReqResMessage::FileAccept{..} => {
          tx.send(AppEvent::FileAcceptReceived(peer));
        }
        ReqResMessage::FileDecline{..} => {
          tx.send(AppEvent::FileDeclineReceived(peer));
        }
        ReqResMessage::FileChunk{seq, data} => {
          tx.send(AppEvent::FileChunkReceived(peer, seq, data));
        }
        ReqResMessage::FileTransferComplete{..} => {
          tx.send(AppEvent::FileTransferCompleted(peer));
        }
      }
    }
    RequestResponseEvent::OutboundFailure{peer, error, request_id} => { /* log */ }
    RequestResponseEvent::InboundFailure{peer, error, request_id} => { /* log */ }
    _ => {}
  }
  ```
- On `OutboundResponse` for file requests, handle writing chunks.

### 2.5 UI Rendering for Private Chat
- In `tui::render_chat_pane`, when `ChatContext::Private`, use `app.private_chat_histories[&peer]`.
- Render messages with sender prefix (`[you]` vs `[nick]`).
- Below the messages, render interactive prompts for pending offers:
  ```text
  >> Offer: notes.pdf (12 KB)? (/accept or /decline)
  ```
- Support scrolling identical to global chat.

---

## 3. Trade Offer Workflow
1. User types `/offer <path>` in private chat.
2. Validate path and file metadata in `commands.rs`.
3. Create `ReqResMessage::FileOffer` and send via `AppEvent::SendRequest`.
4. Store the offer in `app.pending_offers`.
5. Swarm task forwards to the recipient.
6. Recipient UI sees `AppEvent::FileOfferReceived` and displays prompt.

## 4. Accept / Decline Workflow
1. In private chat,  user enters `/accept` or `/decline`.
2. `commands.rs` processes command:
   - Look up pending offer.
   - Send `FileAccept` or `FileDecline` via `SendRequest`.
   - On accept: create download file handle and `FileTransferState`.
3. The other peer receives `FileAcceptReceived` or `FileDeclineReceived`.
4. On accept:
   - Sender's swarm task begins sending file chunks: read file, split into `FileChunk` messages, send via request-response.

## 5. File Transfer Workflow
1. **Chunking:** Define a fixed chunk size (e.g., 16 KiB).
2. **Sender**: For each chunk, send `ReqResMessage::FileChunk{seq, data}` to recipient.
3. **Receiver**: On `FileChunkReceived`, write `data` to open file and update `bytes_received`.
4. **Completion:** After last chunk, send `ReqResMessage::FileTransferComplete`, then UI `AppEvent::FileTransferCompleted`.
5. **UI Feedback:** Show progress updates in console or chat pane (`bytes_received / total_size`).
6. **Error Handling:** On failure events, clean up state and show error messages.

---

## 6. Summary of New AppEvents
```rust
enum AppEvent {
  // Global
  GlobalMessageReceived(PeerId, String, u64),
  PublishGossipsub(Vec<u8>),

  // Private / Trade
  SendRequest(PeerId, ReqResMessage),
  PrivateMessageReceived(PeerId, String, u64),
  FileOfferReceived(PeerId, FileOffer),
  FileAcceptReceived(PeerId),
  FileDeclineReceived(PeerId),
  FileChunkReceived(PeerId, u32, Vec<u8>),
  FileTransferCompleted(PeerId),
  
  // ... existing variants ...
}
```

---

