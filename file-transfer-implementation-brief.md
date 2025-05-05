# File-Transfer Implementation Brief for SwapBytes

## 0. Purpose
This document is meant for the engineer who will bring **actual file data transfer** to completion in the `swapbytes` application.  It assumes you are already familiar with Rust, libp2p, and the current SwapBytes codebase.  Everything below distils the current state, the gaps, and **concrete, ordered steps** to finish the feature, along with notes on uncertainties and strong recommendations.  See `tutorial.md` in the repo for the reference mini-project that inspired part of this design.

---

## 1. Current Status (Negotiation Only)
* **Offer / Accept / Decline flows** are _fully wired_ through:
  * `/offer`, `/accept`, `/decline` commands in `commands.rs`  
    → generate `AppEvent::{SendFileOffer, SendAcceptOffer, DeclineFileOffer}`
  * Swarm task in `main.rs` converts these UI events to `PrivateRequest::{Offer, AcceptOffer, DeclineOffer}` messages.
  * Receiving peer replies with `PrivateResponse::Ack` and UI gets the right console / chat history items (`PrivateChatItem::*`).
* **What's missing**: _no bytes ever leave disk_.  The "offer accepted" handshake ends with a TODO.

---

## 2. Goal & Design Direction
We want a **robust, resumable, progress-reporting transfer** that can handle files far larger than memory and the current 1 MiB codec frame cap.  Following `tutorial.md` literally (single-shot `Vec<u8>` response) is acceptable only for _tiny_ files and will break in real use.  Therefore we will implement **chunked transfer over the existing Request/Response protocol**.  libp2p streams (e.g. `RequestResponse::new_stream`) could be explored later, but chunked RR keeps the mental model simple and reuses the already-wired infra.

### Key Principles
1. **Fixed Chunk Size** (recommend 64 KiB – tweakable via constant).
2. **Pull-based**: receiver asks for each chunk (`RequestChunk`) so back-pressure is natural.
3. **Single outstanding request** per peer/file simplifies state.
4. **Resumable** by tracking last confirmed chunk; sender re-serves idempotently.

---

## 3. Required Code Changes
### 3.1 Protocol (`src/protocol.rs`)
Add variants:
```rust
// In PrivateRequest
RequestChunk { filename: String, chunk_index: u64 },

// In PrivateResponse
FileChunk {
    filename: String,
    chunk_index: u64,
    data: Vec<u8>,      // <= CHUNK_SIZE bytes
    is_last: bool,
},
TransferError { filename: String, error: String },
```
* **Codec**: keep using JSON + length-delimited.  Bump `max_frame_length` to e.g. `2 * 1024 * 1024` to allow 1 MiB chunks if we later increase size.

### 3.2 Constants (create `const CHUNK_SIZE: usize = 64 * 1024;`).

### 3.3 State Management (`tui.rs` & new structs)
* **Sender**: when an offer is _accepted_, map `(peer_id, filename) → PathBuf` so we can read on demand.
* **Receiver**: introduce struct
  ```rust
  struct DownloadState {
      local_path: PathBuf,
      total_size: u64,
      received: u64,
      next_chunk: u64,
      file: tokio::fs::File,
  }
  ```
  Keep `HashMap<PeerId, HashMap<String, DownloadState>>` in `App` _or_ inside the swarm task (preferred to avoid UI borrow hell).

### 3.4 Swarm-Task Logic (`main.rs`)
#### Sender Side
```text
On PrivateRequest::RequestChunk { filename, idx }:
  • look up PathBuf; error → TransferError.
  • read exactly CHUNK_SIZE bytes at offset idx * CHUNK_SIZE.
  • send FileChunk { idx, data, is_last }.
```
Use `tokio::fs::File::open`, `seek`, `read`.  Cache open handles if perf desired.

#### Receiver Side
```text
After we send AcceptOffer:
  • create DownloadState, open file for write (+tmp ext).
  • immediately send RequestChunk { idx: 0 }.

On FileChunk:
  • write data at expected offset (using file.write_all). Verify idx == state.next_chunk.
  • update `received`, `next_chunk += 1`.
  • if !is_last → send next RequestChunk.
  • if is_last → flush+sync, rename tmp → final, emit FileTransferComplete.
```
Emit progress every N chunks via new `AppEvent::FileTransferProgress` so UI can auto-scroll a status line.

### 3.5 `AppEvent` Additions
```rust
FileTransferProgress { peer_id: PeerId, filename: String, received: u64, total: u64 },
FileTransferComplete { peer_id: PeerId, filename: String, path: PathBuf },
FileTransferFailed   { peer_id: PeerId, filename: String, error: String },
```

### 3.6 UI (`tui.rs`)
* Extend `PrivateChatItem` with `TransferProgress`, `TransferComplete`, `TransferFailed`.
* In `render_chat_pane`, render progress as e.g.
  `<< Downloading 'big.iso' 512 KiB / 1.4 GiB (3%)`.
* Auto-scroll like existing message logic; optionally add a dedicated "Transfers" area later.

---

## 4. User Flow (Happy Path)
1. **Alice** `/offer /path/to/vid.mp4` in private chat with Bob.
2. **Bob** sees offer → `/accept`.
3. **Bob UI** verifies download dir, creates `DownloadState`, asks chunk 0.
4. **Alice** streams chunks; **Bob** writes to file.
5. On last chunk Bob emits `TransferComplete`; chat shows green "✅ Download finished: vid.mp4 (1.2 GiB)".

---

## 5. Error Handling & Edge-Cases
| Scenario | Recommended Handling |
|----------|----------------------|
| Peer disconnects mid-transfer | Receiver keeps `DownloadState`, sets status `Stalled`. If peer reconnects, resume from `next_chunk`. Might require trigger `/resume`. |
| Sender can't read file | Send `TransferError`; receiver emits `FileTransferFailed`. |
| Receiver disk full | Detect write failure, emit `FileTransferFailed`, delete partial file, optionally `/retry`. |
| Filename collision | Save to `filename.part` then rename when successful, or append numeric suffix. |

---

## 6. Open Questions / Uncertainties
1. **Large file support limits**: JSON + base64 adds ~33 % overhead.  For big transfers consider switching codec to `cbor` (see tutorial) or raw `Bytes` on a sub-stream.
2. **Multiple concurrent transfers**: current plan is single outstanding chunk per peer/file.  Scaling to many transfers may need a queue.
3. **Security**: No checksum verification yet.  Add SHA-256 hash in Offer to verify on completion.
4. **Resume after reboot**: Would need persistence of `DownloadState`.

---

## 7. Implementation Checklist (TL;DR)
- [ ] Add protocol variants + bump frame length.
- [ ] Create `CHUNK_SIZE` constant.
- [ ] Sender mapping for accepted offers.
- [ ] Receiver `DownloadState` + hashmap.
- [ ] Swarm-task sender: serve `RequestChunk`.
- [ ] Swarm-task receiver: drive chunk loop, write file, events.
- [ ] Extend `AppEvent`.
- [ ] UI: progress & completion rendering.
- [ ] Robust error paths.
- [ ] (Nice-to-have) SHA-256 verification after download.

Happy coding 🚀 