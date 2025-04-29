# SwapBytes 🪙📁

**A peer-to-peer file-bartering CLI built in Rust with [`libp2p`](https://libp2p.io) and an ergonomic TUI powered by [`ratatui`](https://github.com/ratatui-org/ratatui).**

> _"Got notes? Need notes? Swap 'em."_ – **COSC 473 (2025)** Assignment 2

---

## To Do
- small UI things
    - notify when another user's nickname changes
    - add /who <name> command which gives info about a user
- private chat
- trade offer -> accept/decline
- transfer of file
- README.md & write-up

---

## Table of Contents
1. [What is SwapBytes?](#what-is-swapbytes)
2. [Key Features](#key-features)
3. [User Interface Layout](#user-interface-layout)
4. [Command Reference](#command-reference)
5. [Startup & Peer Discovery Flow](#startup--peer-discovery-flow)
6. [Error Handling & Edge Cases](#error-handling--edge-cases)
7. [Building & Running](#building--running)
8. [Examples](#examples)
9. [Road-map](#road-map)
10. [Contributing](#contributing)
11. [License](#license)

---

## What is SwapBytes?

SwapBytes is a **CLI/TUI application** that lets users **barter files directly with one another** in a fully decentralised network.  
It satisfies all baseline requirements for COSC 473 A2 and adds a polished text-user-interface plus extra QoL commands.

- Written in **Rust 2024**.
- Networked with **libp2p** (pub-sub + request/response).
- Cross-platform (Linux, macOS, Windows).
- No central server required – discovery via **mDNS** on-LAN.

---

## Key Features

| Feature | Details |
|----------|---------|
| **User List** | Sidebar showing nicknames **+ PeerIDs**. Online status auto-refreshes every 2-8s. |
| **Global Chat** | Simple room where every message is broadcast. |
| **Private Chats** | One-on-one chat for negotiation and file offers. |
| **File Swapping** | `/offer <file>` → `/accept | /decline` → direct transfer (max 100 MB). |
| **Ergonomic TUI** | Cross-platform Text User Interface powered by `ratatui`. |
| **Auto mDNS** | Automatic mDNS connection to peers. |
| **Command Interface** | Simple slash-commands entered in the Console pane. |
| **Heartbeat Mechanism** | Regular lightweight background messages announce presence (~2s); peers offline after ~8s inactivity. |

## Extra Features

| Feature | Details |
|----------|---------|
| **Visibility Controls** | `/hide` / `/show` toggles presence in the global list. |
| **Directory Safety** | User-chosen **output directory** validated at startup and via `/setdir`. |
| **Duplicate Nicknames** | Allowed, but each peer gets a warning if a clash is detected. |
| **Help Command** | `/help` prints concise command help. |
| **Direct Peer Interaction** | `/ping` command checks reachability and latency to specific peers. |
| **Nickname Handling** | Default random name (`userXXXX`); allows user-set names; handles duplicates gracefully. |
| **Self-Information** | `/me` command displays current addresses, PeerID, download directory, nickname, and visibility. |
| **Graceful Exit** | Cleanly shut down the application using `Ctrl+Q` or `/quit`. |


---

### User Interface Layout  

**Global chat view:**
```
┌───────────────────────────────────────────────┬──────────────┐
│               Global Chat                     │ Users        │
│  [you] hello bob                              │ alice (Fp9x) │
│  [bob] hey wanna trade?                       │ bob (s8sF)   │
│  …                                            │              │
│                                               │              │
│                                               │              │
├───────────────────────────────────────────────┤              │
│               Console                         │              │
│  > /chat bob                                  │              │
└───────────────────────────────────────────────┴──────────────┘
```
**Private chat view:**

```
┌───────────────────────────────────────────────┬──────────────┐
│              Private Chat (bob)               │ Users        │
│  [you] hi, yeah let's trade!                  │ alice (Fp9x) │
│  [bob] /offer notes.pdf                       │ *bob* (s8sF) │
│  >> Accept notes.pdf? (/accept or /decline)   │              │
│                                               │              │
│                                               │              │
├───────────────────────────────────────────────┤              │
│               Console                         │              │
│  > /accept                                    │              │
└───────────────────────────────────────────────┴──────────────┘
```

#### TUI Panes
- **Users List** 
    - Always visible, shows nickname + PeerIDs and online/offline status.  
    - Asterisk marks the peer whose chat is currently in view.
- **Chat Pane**
    - Shows either: Global Chat or Private Chat
    - To enter Global Chat run `/global` or hit `ESC`
    - To enter Private Chat run `/chat <nickname>` or select a user in the Users List
- **Console Pane**
    - Always visible, shows console output and online/offline status. 

#### Trade offers
- In a private chat, `/offer <file>` posts an interactive prompt (`Accept?`) inside the same pane.  
- `/accept` begins the transfer; `/decline` cancels it.



---

## Command Reference

| Command | Scope | Description |
|---------|-------|-------------|
| `/help` | global | Print brief help. |
| `/setdir <path>` | global | Change download directory (validated absolute path). |
| `/setname <name>` | global | Change nickname (validated). |
| `/me`           | global       | Show information about you (addrs, nickname, etc.) |
| `/chat <name>` | global | Switch chat to a user (e.g. `chat bob`) or global (`chat global`) |
| `/offer <path>` | private chat | Propose a file swap in the current private chat . |
| `/accept` | private chat | Accept the latest offer in the current private chat . |
| `/decline` | private chat | Decline the latest offer in the current private chat . |
| `/hide` / `/show` | global | Toggle your visibility in the Global User List. |
| `/forget` | global | Clear the list of known peers (they will reappear on next heartbeat). |
| `/ping <multiaddr>` | global | Ping a peer by `multiaddr`. |
| `/quit`           | global       | Quit the application    |

---

## Application Usage Flow

1.  **Start-up:** The application generates a unique cryptographic identity (PeerId) and a default random nickname (e.g. `user1234`). It begins listening for incoming connections on available network interfaces. Default visibility is set to **ON**.
2.  **Peer Discovery (mDNS):** The app uses the **mDNS** protocol to broadcast its presence and discover other SwapBytes users on the **local network (LAN)**. When another peer is discovered, it's added to the Users List, initially marked as online.
3.  **Presence & Connection Maintenance (Gossipsub Heartbeats):** To maintain the online status and handle peers joining/leaving, the app uses **Gossipsub**:
    *   If **visible** (`/show`, default), a lightweight **Heartbeat** message containing the current nickname is broadcast via Gossipsub every **2 seconds** (`HEARTBEAT_INTERVAL_SECS`).
    *   Receiving any Gossipsub message (heartbeats or global chat) from a peer updates their `last_seen` time in the local state and well as that user's nickname.
    *   A background task checks periodically (every 5s). If no message has been received from a peer for more than **8 seconds** (`PEER_TIMEOUT_SECS`), that peer is marked **offline** in the Users List.
    *   Using `/hide` stops sending heartbeats, causing the user to appear offline to others after the timeout, while `/show` resumes heartbeats.
4.  **Manual Connection (Ping):** If mDNS fails (e.g., firewall, different network, specific OS issues) or for direct testing, users can establish a connection manually using `/ping <multiaddr>`. The required multiaddress can be found using `/me` on the target peer. This uses the `ping::Behaviour` to check reachability and implicitly establishes a persistent connection if successful. For testing two instances on the **same machine**, using the `/ip4/127.0.0.1/...` address is the most reliable way.
5.  **Global Chat (Gossipsub):** Messages typed into the chat pane while in the `Global Chat` context are packaged and published to the shared `"swapbytes-global-chat"` using **Gossipsub**. All connected peers receive these messages and display them.
6.  **Private Chat & Trade (Request/Response):** Interacting with specific users privately (via `/chat <name>`) and managing file trades (`/offer`, `/accept`, `/decline`) is planned to use libp2p's direct **Request/Response** protocol. This ensures messages and file transfer commands are sent only between the two involved peers.

---

## Peer Discovery Challenges

Our app can automatically find other users on a local network using mDNS. This also usually works with multiple instances on the same machine. However sometimes, especially on MacOS, this auto-discovery might not work when running instances on the *same* machine without being connected to a larger network. If the apps don't see each other automatically, it's easy to connect them manually: just use the `/me` command in one instance to get its multiaddr, and then use `/ping <multiaddr>` in the other instance to connect. 

You might also see a situation on certain restricted networks, like some university or corporate LANs, where peers *do* discover each other initially using their local network addresses. Despite this initial discovery, the network might block the specific communication protocols libp2p needs to establish a full, secure connection. This can lead to peers appearing online briefly in the user list but then quickly showing as offline because the connection handshake failed, preventing ongoing communication like heartbeats or chat messages from getting through.

**Bottom line:** If instances do not connect or maintain a connection, try using `/ping <multiaddr>`  to initiate a stable connection.

---

## Building & Running

> Requires **Rust ≥ 1.77** and **Git**.

```bash
# Clone
git clone https://github.com/HaigBishop/swapbytes.git
cd swapbytes

# Run
cargo run
```

---

## Future Features

- [ ] Reputation points (+1/-1) persisted in a DHT.
- [ ] More advanced back-and-forth trading UX.

---

## License

SwapBytes is released under the **MIT License**. See the LICENSE file.


## Useful Links
**Ratatui:** https://ratatui.rs/

