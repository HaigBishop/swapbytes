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

// --- 1. Protocol Definition ---
// This defines our custom protocol for private messages.
#[derive(Debug, Clone)]
pub struct PrivateProtocol();
// This is the unique name that identifies our protocol on the network.
// Think of it like a specific channel or API endpoint name.
pub const PROTOCOL_NAME: &[u8] = b"/swapbytes/private/1.0.0";

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
        iter::once(std::str::from_utf8(PROTOCOL_NAME).unwrap())
    }
}

// Although `UpgradeInfo` handles negotiation, the `Codec` (defined below)
// still needs a way to easily get the protocol name as a string.
// This `AsRef<str>` implementation provides that.
impl AsRef<str> for PrivateProtocol {
    fn as_ref(&self) -> &str {
        std::str::from_utf8(PROTOCOL_NAME).unwrap()
    }
}

// --- 2. Request/Response Message Types ---
// These enums define the different kinds of messages we can send (Request)
// and expect to receive back (Response) within our private protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateRequest {
    ChatMessage(String),
    // TODO: Add Offer(String) for file transfer negotiation
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateResponse {
    Ack, // A simple "I got your message" acknowledgement.
    // TODO: Add Accept/Decline for file transfer offers
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
            // Sensible default: Allow messages up to 1MB.
            // Prevents accidentally sending/receiving huge messages.
            inner: LengthDelimitedCodec::builder()
                .max_frame_length(1024 * 1024) // 1 MiB
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

// --- 4. Basic Tests ---
// These tests ensure that we can correctly encode (serialize) our messages
// and then decode (deserialize) them back into the original form.
#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::Cursor; // Use an in-memory buffer for testing instead of a real network stream.

    #[tokio::test]
    async fn test_codec_round_trip() {
        let mut codec = PrivateCodec::default();
        let protocol = PrivateProtocol(); // Needed for the codec methods, although not used directly by them.

        // --- Test Request Encoding/Decoding ---
        let request = PrivateRequest::ChatMessage("hello world".to_string());
        let mut buffer_req_vec: Vec<u8> = Vec::new(); // Our in-memory "network" buffer.
        {
            // Create a writer cursor targeting the buffer.
            let mut cursor_req = Cursor::new(&mut buffer_req_vec);
            // Encode the request into the buffer.
            codec.write_request(&protocol, &mut cursor_req, request.clone()).await.unwrap();
        } // `cursor_req` goes out of scope, releasing the mutable borrow.

        // Create a reader cursor over the buffer containing the encoded data.
        let mut reader_cursor_req = Cursor::new(&buffer_req_vec);
        // Decode the request from the buffer.
        let decoded_request = codec.read_request(&protocol, &mut reader_cursor_req).await.unwrap();
        // Check that the decoded request is identical to the original.
        assert_eq!(request, decoded_request);

        // --- Test Response Encoding/Decoding ---
        let response = PrivateResponse::Ack;
        let mut buffer_res_vec: Vec<u8> = Vec::new();
        {
            let mut cursor_res = Cursor::new(&mut buffer_res_vec);
            codec.write_response(&protocol, &mut cursor_res, response.clone()).await.unwrap();
        }

        let mut reader_cursor_res = Cursor::new(&buffer_res_vec);
        let decoded_response = codec.read_response(&protocol, &mut reader_cursor_res).await.unwrap();
        assert_eq!(response, decoded_response);
    }
}
