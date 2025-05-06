# Setting Up a Libp2p Rendezvous Server for SwapBytes

SwapBytes uses mDNS for peer discovery on a local network (LAN). To allow peers to discover each other across *different* networks (e.g., over the internet), a **Rendezvous Server** is needed. This server acts as a known meeting point where SwapBytes clients can register themselves and discover others.

This guide explains how to set up a standalone Rendezvous server using the pre-built binary provided in the ` directory of the SwapBytes project.

## Prerequisites

*   **Rust & Cargo:** You need Rust installed to build and run the server. If you don't have it, visit [rustup.rs](https://rustup.rs/).
*   **Modified Example:** The `Cargo.toml` file within the `rendezvous` directory has been modified from its original state to remove workspace dependencies and make it runnable as a standalone project. This change is necessary because the example is not part of the main `swapbytes` Cargo workspace.

## Running the Rendezvous Server

The simplest way to get a Rendezvous server running for SwapBytes is to use the example server included in the project's ` directory.

1.  **Navigate to the Rendezvous Example Directory:**
    Open your terminal and change to the specific directory containing the rendezvous examples within the SwapBytes project structure:
    ```bash
    cd /path/to/swapbytes/rendezvous
    ```
    *(Replace `/path/to/swapbytes` with the actual path where you cloned the repository)*

2.  **Build and Run the Server:**
    From *within* the `rendezvous` directory, use Cargo to build and run the example server binary. We use `RUST_LOG=info` to see basic activity logs.
    ```bash
    RUST_LOG=info cargo run --bin rendezvous-example
    ```

3.  **Server is Running!**
    You should see output indicating the server is running and listening for connections. By default, the example server:
    *   Listens on address: `/ip4/0.0.0.0/tcp/62649` (meaning it accepts connections on port 62649 from any network interface).
    *   Has the PeerID: `12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN`.

    Keep this terminal window open. The server needs to remain running for SwapBytes clients to connect to it.

## Important Considerations for Internet Connectivity

*   **Public IP Address/Domain:** For SwapBytes clients on *different* networks to reach your server, the machine running the server must have a stable public IP address or a domain name pointing to it.
*   **Firewall:** Ensure that your server's firewall allows incoming TCP connections on port `62649`.
*   **Server Address in SwapBytes:** When you modify the SwapBytes client code (as per the next steps), you will need to configure it with the *public* address and the PeerID (`12D3KooW...`) of *this* running server. If you run the server on `1.2.3.4`, the address clients need might be `/ip4/1.2.3.4/tcp/62649`.

## Stopping the Server

Simply press `Ctrl+C` in the terminal where the server is running.

