# Setting Up a Libp2p Rendezvous Server for SwapBytes

SwapBytes uses **mDNS** for automatic peer discovery on a local-area network. That only works when all instances live on the same LAN.  
To let peers **find each other across the public Internet** we need a small, always-on **Rendezvous Server**.  
This server is nothing more than a lightweight libp2p node that everyone knows upfront; clients "check-in" (REGISTER) and ask who else is around (DISCOVER).

> **Good news →** The libp2p team already ships an off-the-shelf binary, so we do **not** have to write any server code. 🎉

## 1 · Quick TL;DR (copy-paste)

```bash
# 1. Install Rust toolchain (if missing)
curl https://sh.rustup.rs -sSf | sh        # <1 min, press ↵ for defaults

# 2. Grab the rendezvous server binary (~15 s)
cargo install libp2p-rendezvous --locked --bin libp2p-rendezvous-server

# 3. Run it (default port 62649).  Leave this running!
libp2p-rendezvous-server

# 4. Copy the PeerId + Multiaddr the server prints, then
#    paste them into  src/constants.rs  inside your SwapBytes repo:
#    - RENDEZVOUS_PEER_ID
#    - RENDEZVOUS_POINT_ADDRESS

# 5. Re-compile & run SwapBytes. Peers will now discover each
#    other no matter where they are located.
```

That's it. The remainder of this doc explains each step in more detail and offers troubleshooting tips.

---

## 2 · Prerequisites

| Requirement | Version | Notes |
|-------------|---------|-------|
| Rust toolchain | 1.74 or newer | `rustup` installs the correct stable toolchain automatically |
| Open TCP port | 62649 (default) | Can be changed; ensure it is **exposed** in firewalls/containers |

A cheap VPS (DigitalOcean, Hetzner CPX, AWS Lightsail, etc.) with a public IPv4 address works great.

---

## 3 · Installing the Server Binary

libp2p publishes a crate containing several binaries. We only need `libp2p-rendezvous-server`.

```bash
cargo install libp2p-rendezvous --locked --bin libp2p-rendezvous-server
```

* `--locked` ensures Cargo replicates the exact dependency versions tested by the authors.
* The compiler caches artefacts, so subsequent installs/upgrades are almost instant.

The executable ends up in `~/.cargo/bin/` – make sure that directory is on your `$PATH` (rust-installer does this automatically).

---

## 4 · Running the Server

### 4.1 · Plain Terminal (Quick Try-Out)

```bash
libp2p-rendezvous-server
```

You will see log output similar to:

```text
INFO  rendezvous_server > Local PeerId  = 12D3KooWLV12WefKsw14gZp6upUnHDj6S54LShtrKzN2fFfQQCT1
INFO  rendezvous_server > Listening on  /ip4/0.0.0.0/tcp/62649
```

Copy **both** the `PeerId` and the full **Multiaddr** – we need them in the next step.

The process keeps running in the foreground.  
If you want to keep it alive after you log out, either:

* prepend `nohup … & disown`,
* or use `tmux`/`screen`,
* or move on to "4.2 Systemd Service" below.

### 4.2 · (Option A) Systemd Service

Create `/etc/systemd/system/rendezvous.service`:

```ini
[Unit]
Description=Libp2p Rendezvous Server
After=network.target

[Service]
Type=simple
ExecStart=/home/<user>/.cargo/bin/libp2p-rendezvous-server
Restart=on-failure
User=<user>

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now rendezvous.service
journalctl -fu rendezvous.service  # live logs
```

Adjust paths/usernames as needed.

### 4.3 · (Option B) Docker

If you prefer containers:

```Dockerfile
# Dockerfile (simple, ~23 MB image)
FROM rust:1.74-slim as builder
RUN cargo install libp2p-rendezvous --locked --bin libp2p-rendezvous-server

FROM debian:stable-slim
COPY --from=builder /usr/local/cargo/bin/libp2p-rendezvous-server /usr/local/bin/
EXPOSE 62649/tcp
ENTRYPOINT ["libp2p-rendezvous-server"]
```

```bash
docker build -t rendezvous-server .
docker run -d -p 62649:62649 --name rendezvous rendezvous-server
```

---

## 5 · Tell SwapBytes About Your Server

Open `src/constants.rs` inside the SwapBytes codebase and replace the placeholders:

```rust
/// Peer ID of the default Rendezvous server.
pub const RENDEZVOUS_PEER_ID: &str = "12D3KooWLV12WefKsw14gZp6upUnHDj6S54LShtrKzN2fFfQQCT1";
/// Multiaddress of the default Rendezvous server.
pub const RENDEZVOUS_POINT_ADDRESS: &str = "/ip4/203.0.113.10/tcp/62649";
```

**Important:**

1. The *PeerId* **must** match **exactly** the one printed by the server.
2. The Multiaddr **must** include the public IP/hostname reachable by clients.
3. Keep the TCP port in sync with whatever you exposed (62649 by default).

Re-compile & run SwapBytes. On startup every client will:

1. Dial the rendezvous address.
2. REGISTER under the namespace `swapbytes-poc-v1` (hard-coded for now).
3. DISCOVER other peers and automatically dial them.

You can watch the swarm's log lines confirming registration and discovery.

---

## 6 · Security & Production Notes

* The reference server **does not authenticate** clients. Anyone with the address can register.  For hobby projects this is fine; for production consider wrapping it behind authentication or a token mechanism.
* The server keeps all state **in memory**. If it restarts, registrations vanish until clients re-register (SwapBytes does this periodically).
* If you expose the server on the open Internet, rate-limit incoming connections to avoid abuse.
* **TLS/Noise encryption** happens automatically at the libp2p layer – your traffic is already encrypted.

---

## 7 · Troubleshooting

| Symptom | Possible Cause & Fix |
|---------|----------------------|
| *Clients cannot connect* | Port 62649 blocked by firewall / router. Open or forward it. |
| *`RegisterFailed` errors* | The constants in SwapBytes don't match the server's PeerId/addr. Double-check. |
| *High latency pings* | VPS far away geographically – choose a region closer to your users. |
| *`AddressInUse` when starting server* | Another process is already listening on 62649. Pick a different port with `--port <n>`. |

---

## 8 · Alternative: Build From the Example Source

This repo ships a fully-featured example rendezvous implementation under `rendezvous_examples/rendezvous/`.
Use it if you want to hack on the server logic yourself:

```bash
cd rendezvous_examples/rendezvous
RUST_LOG=info cargo run --bin rendezvous-example
```

The output is identical to the pre-built binary; configure SwapBytes the same way.

---

## 9 · Next Steps

* Expose multiple rendezvous servers and **load-balance** clients across them.
* Implement **persistent storage** (e.g. SQLite) in the server to survive restarts.
* Add **token-based registration** to limit who can use your rendezvous.

Happy hacking — and may your bytes always rendezvous! 🚀