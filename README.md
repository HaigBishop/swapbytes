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

SwapBytes is a **CLI/TUI application** that lets students (or anyone) **barter files directly with one another** in a fully decentralised network.  
It satisfies all baseline requirements for COSC 473 A2 and adds a polished text-user-interface plus extra QoL commands.

- Written in **Rust 2021**.
- Networked with **libp2p** (pub-sub + request/response).
- Cross-platform (Linux, macOS, Windows).
- No central server required – discovery via **mDNS** on-LAN and an optional **rendezvous server** for wider internet peers.

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
| **Auto + Manual Discovery** | mDNS + rendezvous; manual `/connect <peerID>` is also supported. |
| **Reset Workflow** | `/reset` drops state and re-runs the initial join wizard. |
| **Help Anywhere** | `/help` prints concise command help. |

---

## User Interface Layout

```
┌──────────────────────────────────┬──────────────────────────────┐
│          Global Chat 📣          │      Private Chat 🔐         │
│  [nick] hello everyone           │  [you ➜ bob] hi, trade?      │
│  ...                             │  [bob] /offer notes.pdf      │
├───────────────┬──────────────────┤  >> Accept file notes.pdf ?  │
│  Global Users │ Input ⌨️          │  >> (Y)es / (N)o             │
│ ───────────── │ /accept          │                              │
│  alice  (12d…)| /offer draft.md  │                              │
│  bob    (k2…) │ ...              │                              │
└───────────────┴──────────────────┴──────────────────────────────┘
```

*Left top–bottom*  
- **Global Chat** scrolls.  
- **Global Users** lists online peers (`nickname` + `PeerID`-short).  

*Right*  
- **Private Chat** pane (switch with `Tab`).  
- File-offer pop-up overlays in this pane.

*Bottom*  
- **Input box** captures commands or plain messages.

---

## Command Reference

| Command | Scope | Description |
|---------|-------|-------------|
| `/help` | anywhere | Print brief help. |
| `/offer <path>` | private chat | Propose a file swap (must ≤ 1 GB). |
| `/accept` | private chat | Accept the latest offer. |
| `/decline` | private chat | Decline the latest offer. |
| `/connect <peerID>` | global | Manually dial a peer by ID. |
| `/refresh` | global | Force immediate peer discovery refresh. |
| `/hide` / `/show` | global | Toggle your visibility in the Global User List. |
| `/setdir <path>` | global | Change download directory (validated). |
| `/reset` | global | Wipe runtime state and restart the join wizard. |

---

## Startup & Peer Discovery Flow

1. **Join Wizard**  
   1. Prompt for **nickname**.  
   2. Prompt for **download directory** – must exist & be writable.  
   3. Connect to **mDNS** + optional **rendezvous server** (flag `--rendezvous <multiaddr>`).  
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
# Terminal A (acts as rendezvous too):
./target/release/swapbytes --nick alice \
  --dir ~/Downloads/swapbytes_alice --bootstrap false

# Terminal B:
./target/release/swapbytes --nick bob \
  --dir ~/Downloads/swapbytes_bob \
  --rendezvous /ip4/127.0.0.1/tcp/7654
```

#### CLI Flags

| Flag | Default | Purpose |
|------|---------|---------|
| `--nick <name>` | prompt | Set nickname non-interactively. |
| `--dir <path>` | prompt | Set download directory non-interactively. |
| `--rendezvous <addr>` | _none_ | Multi-addr of rendezvous server. |
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

## Road-map

- [ ] Reputation points (+1/-1) persisted in a DHT.  
- [ ] Themed rooms for topic-specific swaps.  
- [ ] File-encryption using recipient’s public key before transfer.  
- [ ] Resume-on-disconnect restartable transfers.  

---

## Contributing

PRs welcome! Please open an issue first to discuss changes.  
Code style: `rustfmt + clippy --all -- -D warnings`.

---

## License

SwapBytes is released under the **MIT License**.

```
MIT License

Copyright (c) 2025 Haig Bishop

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

(…full standard MIT text…)
```



## Useful Links
**Ratatui:** https://ratatui.rs/

