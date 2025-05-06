# SwapBytes 

**A peer-to-peer file-bartering CLI built in Rust with [`libp2p`](https://libp2p.io) and an TUI powered by [`ratatui`](https://github.com/ratatui-org/ratatui).**


---

## What is SwapBytes?

SwapBytes is a **CLI/TUI application** that lets users **barter files directly with one another** in a fully decentralised network.  

- Written in **Rust 2024**.
- Networked with **libp2p** (pub-sub + request/response).
- Cross-platform (Linux, macOS, Windows).
- No central server required – discovery via **mDNS** on-LAN.


---

## Building & Running

To install and run SwapBytes  simplying clone the GitHub repository and run `cargo run`. Rust is required.

```bash
# Clone
git clone https://github.com/HaigBishop/swapbytes.git
cd swapbytes

# Run
cargo run
```

---

## Basic Usage

Once in the SwapBytes TUI, you can use keyboard controls:

-  `Tab` to toggle pane focus
- `↑`/`↓` to scroll
- `Ctrl + Q` or the `/quit` command to quit 
- `/` to start typing a command (see below)

Using SwapBytes involves running commands such as `/setname <name>` in the console and sending chat messages in the chat. 

---

## Demo

**See the [demo.md](demo.md) for a visual walk-through.**

---

## Commands

| Command             | Scope        | Description                                                  |
| ------------------- | ------------ | ------------------------------------------------------------ |
| `/help` or `/h`     | global       | Print help text.                                             |
| `/setdir <path>`    | global       | Change download directory (validated absolute path).         |
| `/setname <name>`   | global       | Change nickname.                                             |
| `/me`               | global       | Show information about you (addrs, nickname, etc.)           |
| `/chat <name>`      | global       | Switch chat to a user (e.g. `/chat bob`) or global (`/chat global`) |
| `/offer <path>`     | private chat | Propose a file swap in the current private chat .            |
| `/accept`           | private chat | Accept the latest offer in the current private chat .        |
| `/decline`          | private chat | Decline the latest offer in the current private chat .       |
| `/hide` / `/show`   | global       | Toggle your visibility in the Global User List.              |
| `/forget`           | global       | Clear the list of known peers (they will reconnect on next heartbeat). |
| `/ping <multiaddr>` | global       | Ping a peer by `multiaddr` (obtainable using `/me`).         |
| `/who <name>`       | global       | Show information about a specific user by nickname.          |
| `/myoffers`         | global       | List pending incoming file offers.                           |
| `/quit` or `/q`     | global       | Quit the application.                                        |

---

## Key Features

| Feature | Details |
|----------|---------|
| **User List** | Sidebar showing nicknames **+ PeerIDs**. Online status auto-refreshes every 2-8s. |
| **Global Chat** | Simple room where every message is broadcast. |
| **Private Chats** | One-on-one chat for negotiation and file offers. |
| **File Swapping** | `/offer <file>` → `/accept | /decline` → direct transfer. |
| **Ergonomic TUI** | Cross-platform Text User Interface powered by `ratatui`. |
| **Auto mDNS** | Automatic mDNS connection to peers. |
| **Rendezvous** | Can connect to peers on different networks via a Rendezvous server. See `how-to-rendezvous.md` for a guide. |
| **Command Interface** | Simple slash-commands entered in the Console pane. |
| **Heartbeat Mechanism** | Regular lightweight background messages announce presence (~2s); peers offline after ~8s inactivity. |

## Extra Features

| Feature | Details |
|----------|---------|
| **Visibility Controls** | `/hide` / `/show` toggles presence in the global list. |
| **Directory Safety** | User-chosen **output directory** validated at startup and via `/setdir`. |
| **Duplicate Nicknames** | Allowed, but each peer gets a warning if a clash is detected. |
| **Help Command** | `/help` prints concise command help. |
| **Direct Peer Connection** | `/ping` command provides a secondary method to connect to peers. |
| **Nickname Handling** | Default random name (`userXXXX`); allows user-set names; handles duplicates gracefully. |
| **Self-Information** | `/me` command displays current addresses, PeerID, download directory, nickname, and visibility. |
| **Graceful Exit** | Cleanly shut down the application using `Ctrl+Q` or `/q`. |

---

## Peer Discovery

#### Peers on the same network

The app can automatically find other users on a local network using mDNS and if a rendezvous server is active, through that mechanism as well. 

However sometimes, this auto-discovery might not work when running instances on the *same* machine without being connected to a larger network. You might also see a situation on certain restricted networks, like some university LANs, where peers *do* discover each other initially but then quickly showing as offline.

If peers on the same network don't maintian a connection automatically, it's easy to connect them manually: just use the `/me` command in one instance to get its multiaddr, and then use `/ping <multiaddr>` in the other instance to connect. 

#### Peers on different networks

To connect to peers on another network, a rendezvous server is required. The file `how-to-rendezvous.md` provides a full guide on setting up a server. After a server is properly hosted, the address just needs to be entered into the constant `RENDEZVOUS_ADDR` in the client's `constants.rs` file to connect.

---

## Project Structure
 * **main.rs** - Entry point, initializes the application state and starts the main event loop.
 * **tui.rs** - Manages the Terminal User Interface display and layout.
 * **swarm_task.rs** - Runs the main libp2p swarm event loop in a separate task.
 * **behavior.rs** - Defines the combined libp2p network behaviors (Gossipsub, etc.).
 * **event_handler.rs** - Handles events from the swarm, UI, and other sources.
 * **input_handler.rs** - Parses and processes user input from the TUI.
 * **commands.rs** - Implements logic for user-executable commands.
 * **protocol.rs** - Defines data structures for network communication or internal state.
 * **constants.rs** - Contains application-wide constant values.
 * **utils.rs** - Provides miscellaneous helper functions and utilities.

---

## License

SwapBytes is released under the **MIT License**. See the LICENSE file.

Note that the contents of `rendezvous/` is derived from https://github.com/libp2p/rust-libp2p and is not apart of SwapBytes.

---
