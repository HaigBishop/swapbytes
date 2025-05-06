/*
Sets up the protocol for private messages.
*/

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt};
use libp2p::core::UpgradeInfo;
use libp2p::request_response::Codec;
use serde::{Deserialize, Serialize};
use std::{io, iter};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use tokio_util::compat::{FuturesAsyncReadCompatExt, FuturesAsyncWriteCompatExt};

// --- Define Message enum here ---
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)] // Added Clone, PartialEq, Eq
#[serde(tag = "type")]
pub enum Message {
    /// Periodic presence and nickname announcement.
    Heartbeat {
        timestamp_ms: u64,
        nickname: Option<String>,
    },
    /// Public chat message sent to the global topic.
    GlobalChatMessage {
        content: String,
        timestamp_ms: u64,
        nickname: Option<String>,
    },
}
// --- End of Message enum definition ---

// --- 1. Protocol Definition ---
// This defines our custom protocol for private messages.
#[derive(Debug, Clone)]
pub struct PrivateProtocol();
// This is the unique name that identifies our protocol on the network.
// Think of it like a specific channel or API endpoint name.

// Tell libp2p how to identify and negotiate this protocol.
// When two peers connect, they use this info to agree on speaking "/swapbytes/private/1.0.0".
impl UpgradeInfo for PrivateProtocol {
    // The protocol name type will be a string that lives for the whole program ('static).
    type Info = &'static str;
    // We only provide one protocol name.
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        // Convert the byte array `PROTOCOL_NAME` into a proper string slice.
        // This is safe because we know `PROTOCOL_NAME` is valid UTF-8 text and static.
        iter::once(std::str::from_utf8(crate::constants::PROTOCOL_NAME).unwrap())
    }
}

// Although `UpgradeInfo` handles negotiation, the `Codec` (defined below)
// still needs a way to easily get the protocol name as a string.
// This `AsRef<str>` implementation provides that.
impl AsRef<str> for PrivateProtocol {
    fn as_ref(&self) -> &str {
        std::str::from_utf8(crate::constants::PROTOCOL_NAME).unwrap()
    }
}

// --- 2. Request/Response Message Types ---
// These enums define the different kinds of messages we can send (Request)
// and expect to receive back (Response) within our private protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateRequest {
    ChatMessage(String),
    // File offer including filename and size
    Offer {
        filename: String,
        size_bytes: u64,
    },
    /// Peer declines a file offer we previously sent them.
    DeclineOffer { filename: String },
    /// Peer accepts a file offer we previously sent them.
    AcceptOffer { filename: String },
    // Request a specific chunk of a file.
    RequestChunk {
        filename: String,
        chunk_index: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateResponse {
    Ack, // General acknowledgement of message receipt
    // Removed AcceptOffer/DeclineOffer variants as they are requests now.
    // Response indicating a file chunk is being sent.
    FileChunk {
        filename: String,
        chunk_index: u64,
        data: Vec<u8>,
        is_last: bool,
    },
    // Response indicating an error occurred during transfer.
    TransferError {
        filename: String,
        error: String, // Simple error string for now
    },
}

// --- 3. Message Codec ---
// The Codec handles turning our `PrivateRequest` and `PrivateResponse` objects
// into bytes to send over the network (encoding) and turning received bytes
// back into our objects (decoding). We're using JSON for this.
#[derive(Debug, Clone)]
pub struct PrivateCodec {
    // We use LengthDelimitedCodec to solve the "framing" problem.
    // It prefixes each message with its length, so the receiver knows
    // exactly how many bytes to read to get one complete message.
    inner: LengthDelimitedCodec,
}

impl Default for PrivateCodec {
    fn default() -> Self {
        Self {
            // Use LengthDelimitedCodec for framing.
            // Increase max frame length to 2 MiB to allow for larger chunks (e.g., 1MiB + overhead).
            inner: LengthDelimitedCodec::builder()
                .max_frame_length(2 * 1024 * 1024) // 2 MiB
                .new_codec(),
        }
    }
}

#[async_trait]
impl Codec for PrivateCodec {
    type Protocol = PrivateProtocol; // The protocol this codec is for.
    type Request = PrivateRequest; // The request type it handles.
    type Response = PrivateResponse; // The response type it handles.

    // Decode bytes from the network stream into a `PrivateRequest`.
    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol, // We don't need the protocol instance here.
        io: &mut T,         // The network stream to read from.
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Wrap the raw stream `io` with our length-delimited codec.
        let mut framed = FramedRead::new(io.compat(), self.inner.clone());
        // Read the next complete frame (message). `compat()` adapts the tokio stream for futures::io.
        let frame = framed
            .next()
            .await
            // If `next()` returns `None`, the stream ended unexpectedly.
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "Stream ended"))?
            // The inner `?` handles potential errors from the stream itself.
            ?;

        // Deserialize the received bytes (which should be JSON) into our `PrivateRequest` enum.
        serde_json::from_slice(&frame)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    // Decode bytes from the network stream into a `PrivateResponse`.
    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Same logic as `read_request`, but deserializes into `PrivateResponse`.
        let mut framed = FramedRead::new(io.compat(), self.inner.clone());
        let frame = framed
            .next()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "Stream ended"))??;

        serde_json::from_slice(&frame)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    // Encode a `PrivateRequest` into bytes and write it to the network stream.
    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,          // The network stream to write to.
        req: Self::Request, // The request object to send.
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Serialize the `PrivateRequest` object into JSON bytes.
        let bytes = serde_json::to_vec(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Wrap the raw stream `io` with our length-delimited codec for writing.
        // `compat_write()` adapts the tokio stream for futures::io.
        let mut framed = FramedWrite::new(io.compat_write(), self.inner.clone());
        // Send the bytes. The codec automatically prefixes it with the length.
        framed.send(bytes::Bytes::from(bytes)).await
    }

    // Encode a `PrivateResponse` into bytes and write it to the network stream.
    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response, // The response object to send.
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Same logic as `write_request`, but serializes a `PrivateResponse`.
        let bytes = serde_json::to_vec(&res)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut framed = FramedWrite::new(io.compat_write(), self.inner.clone());
        framed.send(bytes::Bytes::from(bytes)).await
    }
}
