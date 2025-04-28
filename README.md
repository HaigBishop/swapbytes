# SwapBytes 🪙📁

**A peer-to-peer file-bartering CLI built in Rust with [`libp2p`](https://libp2p.io) and an ergonomic TUI powered by [`ratatui`](https://github.com/ratatui-org/ratatui).**

> _"Got notes? Need notes? Swap 'em."_ – **COSC 473 (2025)** Assignment 2

---

## To Do
- global chat
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
| `/ping <multiaddr>` | global | Ping a peer by `multiaddr`. |
| `/quit`           | global       | Quit the application    |

---

## Startup & Peer Discovery Flow

1. **Start-up**  
   1. Uh..
   2. Default **visibility ON**; first heartbeat sent immediately.

2. **Heartbeat**  
   - Every 2s a lightweight pub-sub (gossipsub) ping announces presence.  
   - Peers missing > 8s marked **offline**.

3. **Chat**
   - At any time a user can chat on the **Global Chat** visible to anyone.
   - Users can open a **Private Chat** with a particular user, automatically notifying that user.
   - Within a private chat users can trade files.

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

