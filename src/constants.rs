/*
Constants for the SwapBytes application.
*/

use std::time::Duration;

/// Public topic for global chat messages via Gossipsub.
pub const SWAPBYTES_TOPIC: &str = "swapbytes-global-chat";
/// Unique identifier for the private messaging and file transfer protocol.
pub const PROTOCOL_NAME: &[u8] = b"/swapbytes/private/1.0.0";
/// Interval for sending heartbeat messages.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
/// Time before a peer is considered offline if no heartbeat is received.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(8);
/// File transfer chunk size in bytes.
pub const CHUNK_SIZE: usize = 64 * 1024; // 64 KiB
/// Number of bytes transferred before sending a progress update to the UI.
pub const PROGRESS_UPDATE_BYTES: u64 = 512 * 1024; // 512 KiB 
/// How long the "Pinging..." indicator stays visible after sending a ping.
pub const PINGING_DURATION: Duration = Duration::from_millis(2000);
// Limit how many lines we keep in the console log to prevent using too much memory.
pub const MAX_LOG_LINES: usize = 1000;

// --- Rendezvous Configuration ---
/// Peer ID of the default Rendezvous server.
pub const RENDEZVOUS_PEER_ID: &str = "12D3KooWExampleRendezvousPeerIDString12345";
/// Multiaddress of the default Rendezvous server.
pub const RENDEZVOUS_POINT_ADDRESS: &str = "/ip4/127.0.0.1/tcp/62648";
