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

// --- Message Enum ---
/// Represents messages exchanged over the network, primarily for presence and public chat.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Message {
    /// Periodic presence and nickname announcement sent over gossipsub.
    Heartbeat {
        timestamp_ms: u64,
        nickname: Option<String>,
    },
    /// Public chat message sent to the global gossipsub topic.
    GlobalChatMessage {
        content: String,
        timestamp_ms: u64,
        nickname: Option<String>,
    },
}

// --- Private Protocol Definition ---
/// Defines the libp2p protocol structure for private, direct peer-to-peer interactions.
#[derive(Debug, Clone)]
pub struct PrivateProtocol();

/// Implements `UpgradeInfo` to advertise and negotiate the private protocol.
/// Peers use this information to agree on using the "/swapbytes/private/1.0.0" protocol
/// when establishing a direct request-response connection.
impl UpgradeInfo for PrivateProtocol {
    type Info = &'static str; // The protocol name is a static string.
    type InfoIter = iter::Once<Self::Info>; // We advertise exactly one protocol name.

    /// Returns the canonical name of the protocol.
    fn protocol_info(&self) -> Self::InfoIter {
        // Retrieves the protocol name defined in `crate::constants::PROTOCOL_NAME`.
        // Assumes the constant contains valid UTF-8.
        iter::once(std::str::from_utf8(crate::constants::PROTOCOL_NAME).unwrap())
    }
}

/// Provides a simple way to get the protocol name as a string slice.
/// This is needed internally by the `Codec`.
impl AsRef<str> for PrivateProtocol {
    fn as_ref(&self) -> &str {
        std::str::from_utf8(crate::constants::PROTOCOL_NAME).unwrap()
    }
}

// --- Private Request/Response Types ---
/// Defines the types of requests that can be sent over the `PrivateProtocol`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateRequest {
    /// A simple text chat message sent directly to a peer.
    ChatMessage(String),
    /// Initiates a file transfer by offering a file to the peer.
    Offer {
        filename: String,
        size_bytes: u64,
    },
    /// Informs the offering peer that the file offer is declined.
    DeclineOffer { filename: String },
    /// Informs the offering peer that the file offer is accepted.
    AcceptOffer { filename: String },
    /// Requests a specific chunk of a file during a transfer.
    RequestChunk {
        filename: String,
        chunk_index: u64,
    },
}

/// Defines the types of responses that can be received over the `PrivateProtocol`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateResponse {
    /// A generic acknowledgement that a request was received.
    Ack,
    /// A chunk of file data being sent in response to a `RequestChunk`.
    FileChunk {
        filename: String,
        chunk_index: u64,
        data: Vec<u8>,
        /// Indicates if this is the final chunk of the file.
        is_last: bool,
    },
    /// Indicates an error occurred during the file transfer process.
    TransferError {
        filename: String,
        /// A description of the error.
        error: String,
    },
}

// --- Private Message Codec ---
/// Handles encoding (`PrivateRequest`/`PrivateResponse` to bytes) and
/// decoding (bytes to `PrivateRequest`/`PrivateResponse`) for the `PrivateProtocol`.
/// Uses JSON serialization and length-delimited framing.
#[derive(Debug, Clone)]
pub struct PrivateCodec {
    /// The underlying codec that handles message framing.
    /// It prepends each message with its length, allowing the receiver
    /// to know how many bytes constitute a single, complete message.
    inner: LengthDelimitedCodec,
}

impl Default for PrivateCodec {
    fn default() -> Self {
        Self {
            // Configure the length-delimited codec.
            // Set a maximum frame length (e.g., 2 MiB) to accommodate large file chunks
            // plus serialization overhead.
            inner: LengthDelimitedCodec::builder()
                .max_frame_length(2 * 1024 * 1024) // 2 MiB
                .new_codec(),
        }
    }
}

#[async_trait]
impl Codec for PrivateCodec {
    type Protocol = PrivateProtocol; // Associates this codec with `PrivateProtocol`.
    type Request = PrivateRequest; // Specifies the request type it handles.
    type Response = PrivateResponse; // Specifies the response type it handles.

    /// Reads bytes from the network stream and decodes them into a `PrivateRequest`.
    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol, // Protocol instance not needed for decoding.
        io: &mut T,                // The asynchronous network stream.
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Adapt the tokio-based `io` stream for use with the futures-based `FramedRead`.
        // `FramedRead` uses the `LengthDelimitedCodec` to read one complete message frame.
        let mut framed = FramedRead::new(io.compat(), self.inner.clone());
        let frame = framed
            .next()
            .await
            // If `None` is returned, the stream closed unexpectedly.
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "Stream ended"))?
            // Propagate any I/O errors encountered while reading the frame.
            ?;

        // Deserialize the received bytes (expected to be JSON) into a `PrivateRequest`.
        // Map JSON deserialization errors to `io::Error` with `InvalidData` kind.
        serde_json::from_slice(&frame)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Reads bytes from the network stream and decodes them into a `PrivateResponse`.
    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Logic mirrors `read_request`, but deserializes into `PrivateResponse`.
        let mut framed = FramedRead::new(io.compat(), self.inner.clone());
        let frame = framed
            .next()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "Stream ended"))??;

        serde_json::from_slice(&frame)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Encodes a `PrivateRequest` into bytes and writes it to the network stream.
    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol, // Protocol instance not needed for encoding.
        io: &mut T,                 // The asynchronous network stream.
        req: Self::Request,        // The request object to encode and send.
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Serialize the `PrivateRequest` into JSON bytes.
        // Map JSON serialization errors to `io::Error` with `InvalidData` kind.
        let bytes = serde_json::to_vec(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Adapt the tokio-based `io` stream for use with the futures-based `FramedWrite`.
        // `FramedWrite` uses the `LengthDelimitedCodec` to write the message frame (length-prefixed).
        let mut framed = FramedWrite::new(io.compat_write(), self.inner.clone());
        // Send the serialized bytes. The codec handles adding the length prefix.
        framed.send(bytes::Bytes::from(bytes)).await
    }

    /// Encodes a `PrivateResponse` into bytes and writes it to the network stream.
    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response, // The response object to encode and send.
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Logic mirrors `write_request`, but serializes a `PrivateResponse`.
        let bytes = serde_json::to_vec(&res)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut framed = FramedWrite::new(io.compat_write(), self.inner.clone());
        framed.send(bytes::Bytes::from(bytes)).await
    }
}
