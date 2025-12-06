//! Shared gRPC utilities for internal node-to-node communication.

use std::marker::PhantomData;

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
