//! Shared gRPC utilities for internal node-to-node communication.

use bytes::Bytes;
use http::HeaderMap;
use http_body::Frame;
use http_body_util::BodyExt;
use prost::Message;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tonic::Status;

// ============================================================================
// Client-side utilities
// ============================================================================

/// Prost-based codec for tonic gRPC calls.
pub struct ProstCodec<T, U>(PhantomData<(T, U)>);

impl<T, U> Default for ProstCodec<T, U> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T, U> tonic::codec::Codec for ProstCodec<T, U>
where
    T: prost::Message + Send + 'static,
    U: prost::Message + Default + Send + 'static,
{
    type Encode = T;
    type Decode = U;
    type Encoder = ProstEncoder<T>;
    type Decoder = ProstDecoder<U>;

    fn encoder(&mut self) -> Self::Encoder {
        ProstEncoder(PhantomData)
    }

    fn decoder(&mut self) -> Self::Decoder {
        ProstDecoder(PhantomData)
    }
}

/// Prost message encoder.
pub struct ProstEncoder<T>(PhantomData<T>);

impl<T: prost::Message> tonic::codec::Encoder for ProstEncoder<T> {
    type Item = T;
    type Error = tonic::Status;

    fn encode(
        &mut self,
        item: Self::Item,
        dst: &mut tonic::codec::EncodeBuf<'_>,
    ) -> Result<(), Self::Error> {
        item.encode(dst)
            .map_err(|e| tonic::Status::internal(e.to_string()))
    }
}

/// Prost message decoder.
pub struct ProstDecoder<T>(PhantomData<T>);

impl<T: prost::Message + Default> tonic::codec::Decoder for ProstDecoder<T> {
    type Item = T;
    type Error = tonic::Status;

    fn decode(
        &mut self,
        src: &mut tonic::codec::DecodeBuf<'_>,
    ) -> Result<Option<Self::Item>, Self::Error> {
        let item = T::decode(src).map_err(|e| tonic::Status::internal(e.to_string()))?;
        Ok(Some(item))
    }
}

/// Send a unary gRPC request.
pub async fn send_unary<Req, Resp>(
    channel: &tonic::transport::Channel,
    path: &'static str,
    request: tonic::Request<Req>,
) -> Result<tonic::Response<Resp>, tonic::Status>
where
    Req: prost::Message + 'static,
    Resp: prost::Message + Default + 'static,
{
    let mut client = tonic::client::Grpc::new(channel.clone());
    client
        .ready()
        .await
        .map_err(|e| tonic::Status::unavailable(e.to_string()))?;

    let path = http::uri::PathAndQuery::from_static(path);
    client
        .unary(request, path, ProstCodec::<Req, Resp>::default())
        .await
}

/// Send a client-streaming gRPC request.
pub async fn send_client_streaming<Req, Resp, S>(
    channel: &tonic::transport::Channel,
    path: &'static str,
    stream: S,
) -> Result<tonic::Response<Resp>, tonic::Status>
where
    Req: prost::Message + 'static,
    Resp: prost::Message + Default + 'static,
    S: tonic::IntoStreamingRequest<Message = Req>,
{
    let mut client = tonic::client::Grpc::new(channel.clone());
    client
        .ready()
        .await
        .map_err(|e| tonic::Status::unavailable(e.to_string()))?;

    let path = http::uri::PathAndQuery::from_static(path);
    client
        .client_streaming(
            stream.into_streaming_request(),
            path,
            ProstCodec::<Req, Resp>::default(),
        )
        .await
}

// ============================================================================
// Server-side utilities
// ============================================================================

/// gRPC frame header length (1 byte compression flag + 4 bytes length).
const GRPC_HEADER_LEN: usize = 5;

/// Type alias for boxed body used in gRPC responses.
pub type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, Status>;

/// A body that yields data frames followed by trailers.
struct GrpcBody {
    data: Option<Bytes>,
    trailers: Option<HeaderMap>,
}

impl GrpcBody {
    fn new(data: Bytes, trailers: HeaderMap) -> Self {
        Self {
            data: Some(data),
            trailers: Some(trailers),
        }
    }
}

impl http_body::Body for GrpcBody {
    type Data = Bytes;
    type Error = Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if let Some(data) = self.data.take() {
            return Poll::Ready(Some(Ok(Frame::data(data))));
        }
        if let Some(trailers) = self.trailers.take() {
            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
        }
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        self.data.is_none() && self.trailers.is_none()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match &self.data {
            Some(data) => http_body::SizeHint::with_exact(data.len() as u64),
            None => http_body::SizeHint::with_exact(0),
        }
    }
}

/// Parse a gRPC length-prefixed frame, returning the message bytes.
///
/// A gRPC frame consists of:
/// - 1 byte: compression flag (0 = uncompressed)
/// - 4 bytes: big-endian message length
/// - N bytes: protobuf message
pub fn parse_grpc_frame(body: &[u8]) -> Result<&[u8], Status> {
    if body.len() < GRPC_HEADER_LEN {
        return Err(Status::invalid_argument("incomplete grpc frame header"));
    }

    let _compression = body[0];
    let len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;

    if body.len() < GRPC_HEADER_LEN + len {
        return Err(Status::invalid_argument("incomplete grpc frame body"));
    }

    Ok(&body[GRPC_HEADER_LEN..GRPC_HEADER_LEN + len])
}

/// Create a successful gRPC response with a protobuf message.
/// Sends grpc-status as a trailer per gRPC spec.
pub fn create_grpc_response<T: Message>(msg: T) -> http::Response<BoxBody> {
    let mut buf = Vec::with_capacity(msg.encoded_len() + GRPC_HEADER_LEN);
    buf.push(0); // compression flag = uncompressed
    let len = msg.encoded_len() as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    msg.encode(&mut buf).unwrap();

    let mut trailers = HeaderMap::new();
    trailers.insert("grpc-status", "0".parse().unwrap());

    let body = GrpcBody::new(Bytes::from(buf), trailers).boxed_unsync();

    http::Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .body(body)
        .unwrap()
}

/// Create a gRPC error response.
/// Per gRPC spec, trailers-only responses send grpc-status in headers.
pub fn create_grpc_error_response(status: Status) -> http::Response<BoxBody> {
    let body = http_body_util::Empty::new()
        .map_err(|_: std::convert::Infallible| Status::internal("body error"))
        .boxed_unsync();

    let mut builder = http::Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .header("grpc-status", (status.code() as i32).to_string());

    if !status.message().is_empty() {
        builder = builder.header("grpc-message", status.message());
    }

    builder.body(body).unwrap()
}

/// Create a successful gRPC streaming response with multiple protobuf messages.
/// Sends grpc-status as a trailer per gRPC spec.
pub fn create_grpc_streaming_response<T: Message>(msgs: Vec<T>) -> http::Response<BoxBody> {
    let mut buf = Vec::new();
    for msg in msgs {
        buf.push(0); // compression flag = uncompressed
        let len = msg.encoded_len() as u32;
        buf.extend_from_slice(&len.to_be_bytes());
        msg.encode(&mut buf).unwrap();
    }

    let mut trailers = HeaderMap::new();
    trailers.insert("grpc-status", "0".parse().unwrap());

    let body = GrpcBody::new(Bytes::from(buf), trailers).boxed_unsync();

    http::Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .body(body)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_grpc_frame_valid_empty() {
        let frame = vec![0, 0, 0, 0, 0];
        let result = parse_grpc_frame(&frame);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_grpc_frame_valid_with_data() {
        let mut frame = vec![0, 0, 0, 0, 3];
        frame.extend_from_slice(&[1, 2, 3]);
        let result = parse_grpc_frame(&frame);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn test_parse_grpc_frame_header_too_short() {
        let frame = vec![0, 0, 0];
        let result = parse_grpc_frame(&frame);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_grpc_frame_body_incomplete() {
        let mut frame = vec![0, 0, 0, 0, 10];
        frame.extend_from_slice(&[1, 2, 3, 4, 5]);
        let result = parse_grpc_frame(&frame);
        assert!(result.is_err());
    }
}
