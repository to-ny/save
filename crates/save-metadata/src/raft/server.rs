//! Raft gRPC server for handling incoming Raft RPCs.

use super::rpc::RaftRpcServer;
use super::types::Raft;
use save_proto::raft as proto;
use socket2::{Domain, Socket, Type};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::info;

type BoxBody = http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, Status>;

/// Runs the Raft gRPC server with graceful shutdown support.
///
/// # Arguments
/// * `raft` - The Raft instance to handle RPCs for
/// * `addr` - The address to bind to
/// * `shutdown_rx` - Optional broadcast receiver for shutdown signal
/// * `ready_tx` - Optional oneshot sender to signal when server is ready to accept connections
///
/// Returns early error if binding fails, allowing caller to handle startup failures.
pub async fn run_server(
    raft: Arc<Raft>,
    addr: SocketAddr,
    mut shutdown_rx: Option<broadcast::Receiver<()>>,
    ready_tx: Option<oneshot::Sender<()>>,
) -> anyhow::Result<()> {
    let service = RaftRpcService::new(raft);

    info!("Starting Raft gRPC server on {}", addr);

    // Create socket with SO_REUSEADDR for faster restart after crash
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    let std_listener: std::net::TcpListener = socket.into();
    let listener = tokio::net::TcpListener::from_std(std_listener)?;

    let incoming = TcpListenerStream::new(listener);

    // Signal that we're ready to accept connections
    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }

    let server = Server::builder().add_service(RaftRpcServiceServer::new(service));

    match shutdown_rx.take() {
        Some(mut rx) => {
            server
                .serve_with_incoming_shutdown(incoming, async move {
                    let _ = rx.recv().await;
                    info!("Raft gRPC server shutting down");
                })
                .await?
        }
        None => server.serve_with_incoming(incoming).await?,
    }

    Ok(())
}

/// Raft RPC service implementation for tonic.
#[derive(Clone)]
struct RaftRpcService {
    raft: Arc<Raft>,
}

impl RaftRpcService {
    fn new(raft: Arc<Raft>) -> Self {
        Self { raft }
    }

    async fn append_entries(
        &self,
        request: Request<proto::AppendEntriesRequest>,
    ) -> Result<Response<proto::AppendEntriesResponse>, Status> {
        let inner = RaftRpcServer::new(self.raft.clone());
        inner.append_entries(request).await
    }

    async fn vote(
        &self,
        request: Request<proto::VoteRequest>,
    ) -> Result<Response<proto::VoteResponse>, Status> {
        let inner = RaftRpcServer::new(self.raft.clone());
        inner.vote(request).await
    }

    async fn install_snapshot_from_frames(
        &self,
        frames: Vec<proto::InstallSnapshotRequest>,
    ) -> Result<Response<proto::InstallSnapshotResponse>, Status> {
        let inner = RaftRpcServer::new(self.raft.clone());
        inner.install_snapshot_from_frames(frames).await
    }
}

// Manual tonic service implementation since we're not using codegen

#[derive(Clone)]
struct RaftRpcServiceServer<T: Clone> {
    inner: T,
}

impl<T: Clone> RaftRpcServiceServer<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Clone> tonic::server::NamedService for RaftRpcServiceServer<T> {
    const NAME: &'static str = "raft.RaftRpc";
}

impl<T, B> tower::Service<http::Request<B>> for RaftRpcServiceServer<T>
where
    T: Clone + Send + Sync + 'static,
    T: RaftRpcTrait,
    B: http_body::Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = http::Response<BoxBody>;
    type Error = std::convert::Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let inner = self.inner.clone();
        let path = req.uri().path().to_string();

        Box::pin(async move {
            let (_parts, body) = req.into_parts();
            let body = match http_body_util::BodyExt::collect(body).await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    return Ok(create_error_response(Status::internal(
                        "failed to read body",
                    )));
                }
            };

            let response = match path.as_str() {
                "/raft.RaftRpc/AppendEntries" => handle_append_entries(&inner, body.to_vec()).await,
                "/raft.RaftRpc/Vote" => handle_vote(&inner, body.to_vec()).await,
                "/raft.RaftRpc/InstallSnapshot" => {
                    handle_install_snapshot(&inner, body.to_vec()).await
                }
                _ => create_error_response(Status::unimplemented("unknown method")),
            };

            Ok(response)
        })
    }
}

trait RaftRpcTrait: Clone + Send + Sync + 'static {
    fn append_entries(
        &self,
        req: Request<proto::AppendEntriesRequest>,
    ) -> impl std::future::Future<Output = Result<Response<proto::AppendEntriesResponse>, Status>> + Send;

    fn vote(
        &self,
        req: Request<proto::VoteRequest>,
    ) -> impl std::future::Future<Output = Result<Response<proto::VoteResponse>, Status>> + Send;

    fn install_snapshot_from_frames(
        &self,
        frames: Vec<proto::InstallSnapshotRequest>,
    ) -> impl std::future::Future<Output = Result<Response<proto::InstallSnapshotResponse>, Status>> + Send;
}

impl RaftRpcTrait for RaftRpcService {
    async fn append_entries(
        &self,
        req: Request<proto::AppendEntriesRequest>,
    ) -> Result<Response<proto::AppendEntriesResponse>, Status> {
        RaftRpcService::append_entries(self, req).await
    }

    async fn vote(
        &self,
        req: Request<proto::VoteRequest>,
    ) -> Result<Response<proto::VoteResponse>, Status> {
        RaftRpcService::vote(self, req).await
    }

    async fn install_snapshot_from_frames(
        &self,
        frames: Vec<proto::InstallSnapshotRequest>,
    ) -> Result<Response<proto::InstallSnapshotResponse>, Status> {
        RaftRpcService::install_snapshot_from_frames(self, frames).await
    }
}

/// Parses gRPC frame: 1-byte compression flag + 4-byte big-endian length + message.
fn parse_grpc_frame(body: &[u8]) -> Result<&[u8], Status> {
    const HEADER_LEN: usize = 5;
    if body.len() < HEADER_LEN {
        return Err(Status::invalid_argument("incomplete grpc frame header"));
    }

    let _compression = body[0];
    let len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;

    if body.len() < HEADER_LEN + len {
        return Err(Status::invalid_argument("incomplete grpc frame body"));
    }

    Ok(&body[HEADER_LEN..HEADER_LEN + len])
}

async fn handle_append_entries<T: RaftRpcTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => match service.append_entries(Request::new(req)).await {
            Ok(resp) => create_response(resp.into_inner()),
            Err(status) => create_error_response(status),
        },
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_vote<T: RaftRpcTrait>(service: &T, body: Vec<u8>) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => match service.vote(Request::new(req)).await {
            Ok(resp) => create_response(resp.into_inner()),
            Err(status) => create_error_response(status),
        },
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_install_snapshot<T: RaftRpcTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let frames = match parse_streaming_frames(&body) {
        Ok(f) => f,
        Err(status) => return create_error_response(status),
    };

    match service.install_snapshot_from_frames(frames).await {
        Ok(resp) => create_response(resp.into_inner()),
        Err(status) => create_error_response(status),
    }
}

/// Parses multiple gRPC frames from a streaming request body.
fn parse_streaming_frames(body: &[u8]) -> Result<Vec<proto::InstallSnapshotRequest>, Status> {
    const HEADER_LEN: usize = 5;
    let mut frames = Vec::new();
    let mut offset = 0;

    while offset < body.len() {
        if body.len() - offset < HEADER_LEN {
            return Err(Status::invalid_argument("incomplete grpc frame header"));
        }

        let _compression = body[offset];
        let len = u32::from_be_bytes([
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
            body[offset + 4],
        ]) as usize;

        if body.len() - offset < HEADER_LEN + len {
            return Err(Status::invalid_argument("incomplete grpc frame body"));
        }

        let data = &body[offset + HEADER_LEN..offset + HEADER_LEN + len];
        let req: proto::InstallSnapshotRequest =
            prost::Message::decode(data).map_err(|e| Status::invalid_argument(e.to_string()))?;
        frames.push(req);

        offset += HEADER_LEN + len;
    }

    Ok(frames)
}

fn create_response<T: prost::Message>(msg: T) -> http::Response<BoxBody> {
    use http_body_util::BodyExt;

    let mut buf = Vec::with_capacity(msg.encoded_len() + 5);
    buf.push(0); // compression flag
    let len = msg.encoded_len() as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    msg.encode(&mut buf).unwrap();

    let body = http_body_util::Full::new(bytes::Bytes::from(buf))
        .map_err(|_: std::convert::Infallible| Status::internal("body error"))
        .boxed_unsync();

    http::Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .header("grpc-status", "0")
        .body(body)
        .unwrap()
}

fn create_error_response(status: Status) -> http::Response<BoxBody> {
    use http_body_util::BodyExt;

    let body = http_body_util::Empty::new()
        .map_err(|_: std::convert::Infallible| Status::internal("body error"))
        .boxed_unsync();

    http::Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .header("grpc-status", status.code() as i32)
        .header("grpc-message", status.message())
        .body(body)
        .unwrap()
}
