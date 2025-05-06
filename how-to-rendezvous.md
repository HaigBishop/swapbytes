# Setting Up a Libp2p Rendezvous Server for SwapBytes

SwapBytes uses mDNS for peer discovery on a local network (LAN). To allow peers to discover each other across *different* networks (e.g., over the internet), a **Rendezvous Server** is needed. This server acts as a known meeting point where SwapBytes clients can register themselves and discover others.

This guide explains how to set up a standalone Rendezvous server using the binary provided by the `libp2p-rendezvous` crate.

## 1. Installation

You need Rust and Cargo installed. Install the server binary using Cargo:

```bash
cargo install libp2p-rendezvous --bin libp2p-rendezvous-server
```

This command downloads the source code for the `libp2p-rendezvous` crate, compiles the `libp2p-rendezvous-server` binary, and installs it into your Cargo binary path (usually `~/.cargo/bin/`). You might need to add `~/.cargo/bin` to your system's `PATH` if it's not already there.

## 2. Running the Server

Once installed, you can run the server simply by executing its name:

```bash
libp2p-rendezvous-server
```

By default, it will listen on `/ip4/0.0.0.0/tcp/62648`. You can specify a different listen address using the `--listen` flag:

```bash
# Example: Listen on all interfaces, TCP port 50000
libp2p-rendezvous-server --listen /ip4/0.0.0.0/tcp/50000
```

## 3. Important Output (PeerId and Address)

When the server starts, it will print output similar to this:

```
INFO libp2p_rendezvous_server::server > Local Peer Id: 12D3KooWAbCdEfGhIjKlMnOpQrStUvWxYz1234567890
INFO libp2p_rendezvous_server::server > Listening on "/ip4/127.0.0.1/tcp/62648"
INFO libp2p_rendezvous_server::server > Listening on "/ip4/192.168.1.100/tcp/62648" # Example local IP
INFO libp2p_rendezvous_server::server > Listening on "/ip6/::1/tcp/62648"
# ... potentially more addresses ...
```

**You MUST note down two crucial pieces of information:**

1.  **The Server's PeerId:** In the example above, it's `12D3KooWAbCdEfGhIjKlMnOpQrStUvWxYz1234567890`. This uniquely identifies the server node on the libp2p network.
2.  **A Publicly Reachable Multiaddress:** This is the address other SwapBytes clients will use to connect to the server. If you're running the server on a machine behind a NAT/firewall (like a home router), you'll need to use its public IP address and ensure the chosen port is forwarded. For local testing, `/ip4/127.0.0.1/tcp/62648` (or your `--listen` port) is usually sufficient. A typical public address might look like `/ip4/YOUR_PUBLIC_IP/tcp/62648`.

## 4. Configuring SwapBytes Clients

SwapBytes clients need to know the `PeerId` and `Multiaddr` of the Rendezvous server to connect to it.

Currently, these are hardcoded as *default* values in `src/constants.rs`:

```rust
// src/constants.rs
pub const RENDEZVOUS_PEER_ID: &str = "12D3KooWExampleRendezvousPeerIDString12345"; // REPLACE ME
pub const RENDEZVOUS_POINT_ADDRESS: &str = "/ip4/127.0.0.1/tcp/62648"; // REPLACE ME
```

**You MUST replace these placeholder values** in `src/constants.rs` with the **actual `PeerId` and publicly reachable `Multiaddr`** you obtained from running *your* `libp2p-rendezvous-server` in Step 3 before compiling and running SwapBytes.

*(Future versions of SwapBytes might allow specifying the rendezvous point via command-line arguments or a configuration file).*

## 5. Persistence (Running Long-Term)

For the Rendezvous server to be useful, it needs to run continuously on a machine that is accessible from the internet (e.g., a Virtual Private Server - VPS). You would typically run it as a background service using tools like:

*   `systemd` (Linux)
*   `launchd` (macOS)
*   Docker
*   `screen` or `tmux` (simpler, but less robust)

Make sure any firewalls (on the server machine or cloud provider) allow incoming connections on the TCP port the server is listening on.

## 6. Alternative: Running the Example Server

The `rendezvous_examples/rendezvous/` directory contains example code, including a server (`main.rs`). You *can* run this for local testing:

```bash
# Navigate to the example directory
cd rendezvous_examples/rendezvous

# Run the example server (listens on port 62649 by default)
RUST_LOG=info cargo run --bin rendezvous-example
```

**However:**

*   This example server uses a **fixed PeerId** (`12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN`) derived from a hardcoded key in `main.rs`.
*   It listens on port `62649` by default.
*   Using the standalone `libp2p-rendezvous-server` (Steps 1-4) is the **recommended approach** as it generates a unique keypair/PeerId each time (unless you provide one) and is easier to manage.