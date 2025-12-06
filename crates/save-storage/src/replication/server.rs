//! gRPC server for handling incoming replication requests.

use super::service::ReplicationService;
use save_proto::replication as proto;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tonic::Status;
use tonic::transport::Server;
use tracing::info;

type BoxBody = http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, Status>;

/// Run the replication gRPC server with graceful shutdown support.
pub async fn run_server(
    service: Arc<ReplicationService>,
    addr: SocketAddr,
    mut shutdown_rx: Option<broadcast::Receiver<()>>,
) -> Result<(), tonic::transport::Error> {
    info!("Starting replication gRPC server on {}", addr);

    let server =
        Server::builder().add_service(ReplicationServiceServer::new(ServiceWrapper { service }));

    match shutdown_rx.take() {
        Some(mut rx) => {
            server
                .serve_with_shutdown(addr, async move {
                    let _ = rx.recv().await;
                    info!("Replication gRPC server shutting down");
                })
                .await
        }
        None => server.serve(addr).await,
    }
}

#[derive(Clone)]
struct ServiceWrapper {
    service: Arc<ReplicationService>,
}

// Manual tonic service implementation

#[derive(Clone)]
struct ReplicationServiceServer<T: Clone> {
    inner: T,
}

impl<T: Clone> ReplicationServiceServer<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Clone> tonic::server::NamedService for ReplicationServiceServer<T> {
    const NAME: &'static str = "replication";
}

impl<T, B> tower::Service<http::Request<B>> for ReplicationServiceServer<T>
where
    T: Clone + Send + Sync + 'static,
    T: ReplicationTrait,
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
                // WriteReplica
                "/replication.WriteReplica/WriteObject" => {
                    handle_write_object(&inner, body.to_vec()).await
                }
                "/replication.WriteReplica/PrepareObject" => {
                    handle_prepare_object(&inner, body.to_vec()).await
                }
                "/replication.WriteReplica/CommitObject" => {
                    handle_commit_object(&inner, body.to_vec()).await
                }
                "/replication.WriteReplica/AbortObject" => {
                    handle_abort_object(&inner, body.to_vec()).await
                }
                // ReadReplica
                "/replication.ReadReplica/ReadObject" => {
                    handle_read_object(&inner, body.to_vec()).await
                }
                "/replication.ReadReplica/ObjectExists" => {
                    handle_object_exists(&inner, body.to_vec()).await
                }
                // DeleteReplica
                "/replication.DeleteReplica/DeleteObject" => {
                    handle_delete_object(&inner, body.to_vec()).await
                }
                // ReplicationHealth
                "/replication.ReplicationHealth/HealthCheck" => handle_health_check(&inner).await,
                "/replication.ReplicationHealth/GetStats" => {
                    handle_get_stats(&inner, body.to_vec()).await
                }
                _ => create_error_response(Status::unimplemented("unknown method")),
            };

            Ok(response)
        })
    }
}

trait ReplicationTrait: Clone + Send + Sync + 'static {
    fn write_object(
        &self,
        req: proto::WriteObjectRequest,
    ) -> impl std::future::Future<Output = proto::WriteObjectResponse> + Send;

    fn prepare_object(
        &self,
        req: proto::PrepareObjectRequest,
    ) -> impl std::future::Future<Output = proto::PrepareObjectResponse> + Send;

    fn commit_object(
        &self,
        req: proto::CommitObjectRequest,
    ) -> impl std::future::Future<Output = proto::CommitObjectResponse> + Send;

    fn abort_object(
        &self,
        req: proto::AbortObjectRequest,
    ) -> impl std::future::Future<Output = proto::AbortObjectResponse> + Send;

    fn read_object(
        &self,
        req: proto::ReadObjectRequest,
    ) -> impl std::future::Future<Output = Vec<proto::ReadObjectResponse>> + Send;

    fn object_exists(
        &self,
        req: proto::ObjectExistsRequest,
    ) -> impl std::future::Future<Output = proto::ObjectExistsResponse> + Send;

    fn delete_object(
        &self,
        req: proto::DeleteObjectRequest,
    ) -> impl std::future::Future<Output = proto::DeleteObjectResponse> + Send;

    fn health_check(&self) -> proto::HealthCheckResponse;

    fn get_stats(&self) -> impl std::future::Future<Output = proto::GetStatsResponse> + Send;
}

impl ReplicationTrait for ServiceWrapper {
    async fn write_object(&self, req: proto::WriteObjectRequest) -> proto::WriteObjectResponse {
        self.service.write_object(req).await
    }

    async fn prepare_object(
        &self,
        req: proto::PrepareObjectRequest,
    ) -> proto::PrepareObjectResponse {
        self.service.prepare_object(req).await
    }

    async fn commit_object(&self, req: proto::CommitObjectRequest) -> proto::CommitObjectResponse {
        self.service.commit_object(req).await
    }

    async fn abort_object(&self, req: proto::AbortObjectRequest) -> proto::AbortObjectResponse {
        self.service.abort_object(req).await
    }

    async fn read_object(&self, req: proto::ReadObjectRequest) -> Vec<proto::ReadObjectResponse> {
        self.service.read_object(req).await
    }

    async fn object_exists(&self, req: proto::ObjectExistsRequest) -> proto::ObjectExistsResponse {
        self.service.object_exists(req).await
    }

    async fn delete_object(&self, req: proto::DeleteObjectRequest) -> proto::DeleteObjectResponse {
        self.service.delete_object(req).await
    }

    fn health_check(&self) -> proto::HealthCheckResponse {
        self.service.health_check()
    }

    async fn get_stats(&self) -> proto::GetStatsResponse {
        self.service.get_stats().await
    }
}

// Request handlers

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

async fn handle_write_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_response(service.write_object(req).await),
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_prepare_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_response(service.prepare_object(req).await),
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_commit_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_response(service.commit_object(req).await),
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_abort_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_response(service.abort_object(req).await),
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_read_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    let req: proto::ReadObjectRequest = match prost::Message::decode(data) {
        Ok(r) => r,
        Err(e) => return create_error_response(Status::invalid_argument(e.to_string())),
    };

    let chunks = service.read_object(req).await;
    create_streaming_response(chunks)
}

async fn handle_object_exists<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_response(service.object_exists(req).await),
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_delete_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_response(service.delete_object(req).await),
        Err(e) => create_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_health_check<T: ReplicationTrait>(service: &T) -> http::Response<BoxBody> {
    create_response(service.health_check())
}

async fn handle_get_stats<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_error_response(status),
    };

    let _req: proto::GetStatsRequest = match prost::Message::decode(data) {
        Ok(r) => r,
        Err(e) => return create_error_response(Status::invalid_argument(e.to_string())),
    };

    create_response(service.get_stats().await)
}

// Response helpers

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

fn create_streaming_response<T: prost::Message>(msgs: Vec<T>) -> http::Response<BoxBody> {
    use http_body_util::BodyExt;

    let mut buf = Vec::new();
    for msg in msgs {
        buf.push(0); // compression flag
        let len = msg.encoded_len() as u32;
        buf.extend_from_slice(&len.to_be_bytes());
        msg.encode(&mut buf).unwrap();
    }

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
