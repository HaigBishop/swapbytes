# SwapBytes 🪙📁

**A peer-to-peer file-bartering CLI built in Rust with [`libp2p`](https://libp2p.io) and an ergonomic TUI powered by [`ratatui`](https://github.com/ratatui-org/ratatui).**

> _“Got notes? Need notes? Swap ‘em.”_ – **COSC 473 (2025)** Assignment 2

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

| Category | Details |
|----------|---------|
| **Global Chat** | Simple room where every message is broadcast. |
| **Global User List** | Sidebar showing nicknames **+ PeerIDs**. Online status auto-refreshes every 15 s. |
| **Private Chats** | One-on-one pane for negotiation and file offers. |
| **File Swapping** | `/offer <file>` → `/accept | /decline` → direct transfer (max 1 GB). |
| **Visibility Controls** | `/hide` / `/show` toggles presence in the global list. |
| **Directory Safety** | User-chosen **output directory** validated at startup and via `/setdir`. |
| **Duplicate Nickname Guard** | Allowed, but each peer gets a warning if a clash is detected. |
| **Auto + Manual Discovery** | mDNS; manual `/connect <peerID>` is also supported. |
| **Reset Workflow** | `/reset` drops state and re-runs the initial join wizard. |
| **Help Anywhere** | `/help` prints concise command help. |

---

### User Interface Layout  

**Global chat view:**
```
┌───────────────────────────────────────────────┬──────────────┐
│               Global Chat 📣                  │  Users List  │
│  [you] hello bob                              │  alice       │
│  [bob] hey wanna trade?                       │  (7ifp9x)    │
│  …                                            │  bob         │
│                                               │  (s8sfk9)    │
│                                               │  …           │
├───────────────────────────────────────────────┤              │
│               Console Pane ⌨️                 │              │
│  > /offer draft.md                            │              │
└───────────────────────────────────────────────┴──────────────┘
```
**Private chat view:**

```
┌───────────────────────────────────────────────┬──────────────┐
│              Private Chat 🔐 (bob)            │  Users List  │
│  [you ➜ bob] hi, yeah let's trade!            │  alice       │
│  [bob] /offer notes.pdf                       │  (7ifp9x)    │
│  >> Accept file notes.pdf?  (Y/N)             │  *bob*       │
│  …                                            │  (s8sfk9)    │
│                                               │  …           │
├───────────────────────────────────────────────┤              │
│               Console Pane ⌨️                 │              │
│  > /accept                                    │              │
└───────────────────────────────────────────────┴──────────────┘
```

#### TUI Panes
- **Users List** 
    - Occupies full height and 1/4 of width on the right side (scrollable)
    - Always visible, shows nickname + PeerIDs and online/offline status.  
    - Asterisk marks the peer whose chat is currently in view.
- **Chat Pane**
    - Occupies the top 3⁄4 of height of left side (scrollable)
    - Shows either: Global Chat or Private Chat
    - To enter Global Chat run `/global` or hit `ESC`
    - To enter Private Chat run `/chat <nickname>` or select a user in the Users List
- **Console Pane**
    - Occupies the bottom 1⁄4 of height of left side
    - Always visible, shows console output and online/offline status. 

#### Trade offers
- In a private chat, `/offer <file>` posts an interactive prompt (`Accept? Y/N`) inside the same pane.  
- `/accept` or pressing **Y** begins the transfer; `/decline` or **N** cancels it.




---

## Command Reference

| Command | Scope | Description |
|---------|-------|-------------|
| `/help` | global | Print brief help. |
| `/ping <peer>` | global | Ping a peer by `nickname`, `PeerID`, or `multiaddr`. |
| `/offer <path>` | private chat | Propose a file swap in the current private chat . |
| `/accept` | private chat | Accept the latest offer in the current private chat . |
| `/decline` | private chat | Decline the latest offer in the current private chat . |
| `/connect <peer>` | global | Manually dial a peer by`nickname`, `PeerID`, or `multiaddr`. |
| `/refresh` | global | Force immediate peer discovery refresh. |
| `/hide` / `/show` | global | Toggle your visibility in the Global User List. |
| `/setdir <path>` | global | Change download directory (validated). |
| `/setname <name>` | global | Change nickname (validated). |
| `/reset`          | global       | Wipe runtime state and restart the join wizard.              |
| `/quit`           | global       | Quit the application                                         |

---

## Startup & Peer Discovery Flow

1. **Join Wizard**  
   1. Prompt for **nickname**.  
   2. Prompt for **download directory** – must exist & be writable.  
   3. Connect to **mDNS**.
   4. Default **visibility ON**; first heartbeat sent immediately.

2. **Heartbeat**  
   - Every 15 s a lightweight pub-sub ping announces presence.  
   - Peers missing > 30 s marked **offline**.

3. **Auto-Refresh**  
   - Discovery refresh runs silently every 60 s.  
   - `/refresh` triggers the same logic manually.

---

## Error Handling & Edge Cases

| Scenario | Behaviour |
|----------|-----------|
| `nickname` clash | Allowed, but both peers print a warning. |
| File does not exist | Offer rejected, requester notified. |
| File > 1 GB | Offer rejected, sender notified. |
| Transfer failure / disconnect | Both peers see **“File transfer failed – please retry.”** |
| Invalid download dir | App blocks until a valid, writable path is provided. |
| Path traversal attempt (`../../`) | Offer rejected for safety. |

---

## Building & Running

> Requires **Rust ≥ 1.77** and **Git**.

```bash
# Clone
git clone https://github.com/your-user/swapbytes.git
cd swapbytes

# Build release binary
cargo build --release   # target/release/swapbytes
```

### Quick Start (2 local terminals)

```bash
# Terminal A:
./target/release/swapbytes --nick alice \
  --dir ~/Downloads/swapbytes_alice --bootstrap true

# Terminal B:
./target/release/swapbytes --nick bob \
  --dir ~/Downloads/swapbytes_bob
```

#### CLI Flags

| Flag | Default | Purpose |
|------|---------|---------|
| `--nick <name>` | prompt | Set nickname non-interactively. |
| `--dir <path>` | prompt | Set download directory non-interactively. |
| `--bootstrap <bool>` | `true` | Enable mDNS discovery (LAN). |

---

## Examples

```
bob> /offer notes_week4.pdf
alice> /accept
[transfer starts: 142 KB]
[transfer complete ✓]

bob> /hide
bob is now invisible to others.

bob> /show
bob is visible again.

bob> /reset   # re-run join wizard
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

