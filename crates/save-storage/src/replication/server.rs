//! gRPC server for handling incoming replication requests.

use super::service::ReplicationService;
use save_common::TlsConfig;
use save_common::{
    BoxBody, create_grpc_error_response, create_grpc_response, create_grpc_streaming_response,
    parse_grpc_frame,
};
use save_proto::replication as proto;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tonic::Status;
use tonic::server::NamedService;
use tonic::transport::Server;
use tracing::info;

/// Run the replication gRPC server with graceful shutdown support.
pub async fn run_server(
    service: Arc<ReplicationService>,
    addr: SocketAddr,
    shutdown_rx: Option<broadcast::Receiver<()>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_server_with_tls(service, addr, shutdown_rx, None).await
}

/// Run the replication gRPC server with optional mTLS.
pub async fn run_server_with_tls(
    service: Arc<ReplicationService>,
    addr: SocketAddr,
    mut shutdown_rx: Option<broadcast::Receiver<()>>,
    tls_config: Option<&TlsConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(
        "Starting replication gRPC server on {} (TLS: {})",
        addr,
        tls_config.is_some()
    );

    let inner = ServiceWrapper {
        service: service.clone(),
    };

    // Add services for all gRPC paths we handle
    let mut builder = Server::builder();

    if let Some(tls) = tls_config {
        let server_tls = save_common::load_server_tls_config(tls)?;
        builder = builder.tls_config(server_tls)?;
    }

    let server = builder
        .add_service(WriteReplicaServer::new(inner.clone()))
        .add_service(ReadReplicaServer::new(inner.clone()))
        .add_service(DeleteReplicaServer::new(inner.clone()))
        .add_service(ReplicationHealthServer::new(inner));

    match shutdown_rx.take() {
        Some(mut rx) => {
            server
                .serve_with_shutdown(addr, async move {
                    let _ = rx.recv().await;
                    info!("Replication gRPC server shutting down");
                })
                .await?
        }
        None => server.serve(addr).await?,
    }

    Ok(())
}

#[derive(Clone)]
struct ServiceWrapper {
    service: Arc<ReplicationService>,
}

// Macro to generate gRPC service servers with different NamedService::NAME values
macro_rules! define_grpc_server {
    ($name:ident, $service_name:expr) => {
        #[derive(Clone)]
        struct $name<T: Clone> {
            inner: T,
        }

        impl<T: Clone> $name<T> {
            fn new(inner: T) -> Self {
                Self { inner }
            }
        }

        impl<T: Clone> NamedService for $name<T> {
            const NAME: &'static str = $service_name;
        }

        impl<T, B> tower::Service<http::Request<B>> for $name<T>
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
                            return Ok(create_grpc_error_response(Status::internal(
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
                        "/replication.WriteReplica/StreamPrepareObject" => {
                            handle_stream_prepare_object(&inner, body.to_vec()).await
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
                        "/replication.ReplicationHealth/HealthCheck" => {
                            handle_health_check(&inner).await
                        }
                        "/replication.ReplicationHealth/GetStats" => {
                            handle_get_stats(&inner, body.to_vec()).await
                        }
                        _ => create_grpc_error_response(Status::unimplemented("unknown method")),
                    };

                    Ok(response)
                })
            }
        }
    };
}

// Generate server types for each gRPC service
define_grpc_server!(WriteReplicaServer, "replication.WriteReplica");
define_grpc_server!(ReadReplicaServer, "replication.ReadReplica");
define_grpc_server!(DeleteReplicaServer, "replication.DeleteReplica");
define_grpc_server!(ReplicationHealthServer, "replication.ReplicationHealth");

// Keep the original for backwards compatibility (used by existing code)
#[derive(Clone)]
#[allow(dead_code)]
struct ReplicationServiceServer<T: Clone> {
    inner: T,
}

#[allow(dead_code)]
impl<T: Clone> ReplicationServiceServer<T> {
    fn new(inner: T) -> Self {
        Self { inner }
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
    ) -> impl std::future::Future<
        Output = Result<Vec<proto::ReadObjectResponse>, crate::StorageError>,
    > + Send;

    fn stream_prepare_object(
        &self,
        chunks: Vec<proto::PrepareChunkRequest>,
    ) -> impl Future<Output = proto::PrepareObjectResponse> + Send;

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

    async fn read_object(
        &self,
        req: proto::ReadObjectRequest,
    ) -> Result<Vec<proto::ReadObjectResponse>, crate::StorageError> {
        self.service.read_object(req).await
    }

    async fn stream_prepare_object(
        &self,
        chunks: Vec<proto::PrepareChunkRequest>,
    ) -> proto::PrepareObjectResponse {
        self.service.stream_prepare_object(chunks).await
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

async fn handle_write_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_grpc_response(service.write_object(req).await),
        Err(e) => create_grpc_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_prepare_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_grpc_response(service.prepare_object(req).await),
        Err(e) => create_grpc_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_commit_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_grpc_response(service.commit_object(req).await),
        Err(e) => create_grpc_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_abort_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_grpc_response(service.abort_object(req).await),
        Err(e) => create_grpc_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_stream_prepare_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    // Parse multiple grpc frames from the streaming request body
    let mut chunks = Vec::new();
    let mut offset = 0;

    while offset < body.len() {
        const HEADER_LEN: usize = 5;
        if body.len() - offset < HEADER_LEN {
            break;
        }

        let len = u32::from_be_bytes([
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
            body[offset + 4],
        ]) as usize;

        if body.len() - offset < HEADER_LEN + len {
            break;
        }

        let data = &body[offset + HEADER_LEN..offset + HEADER_LEN + len];
        match prost::Message::decode(data) {
            Ok(chunk) => chunks.push(chunk),
            Err(e) => return create_grpc_error_response(Status::invalid_argument(e.to_string())),
        }
        offset += HEADER_LEN + len;
    }

    create_grpc_response(service.stream_prepare_object(chunks).await)
}

async fn handle_read_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    let req: proto::ReadObjectRequest = match prost::Message::decode(data) {
        Ok(r) => r,
        Err(e) => return create_grpc_error_response(Status::invalid_argument(e.to_string())),
    };

    match service.read_object(req).await {
        Ok(chunks) => create_grpc_streaming_response(chunks),
        Err(crate::StorageError::NotFound(key)) => {
            create_grpc_error_response(Status::not_found(format!("Object not found: {}", key)))
        }
        Err(e) => create_grpc_error_response(Status::internal(e.to_string())),
    }
}

async fn handle_object_exists<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_grpc_response(service.object_exists(req).await),
        Err(e) => create_grpc_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_delete_object<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    match prost::Message::decode(data) {
        Ok(req) => create_grpc_response(service.delete_object(req).await),
        Err(e) => create_grpc_error_response(Status::invalid_argument(e.to_string())),
    }
}

async fn handle_health_check<T: ReplicationTrait>(service: &T) -> http::Response<BoxBody> {
    create_grpc_response(service.health_check())
}

async fn handle_get_stats<T: ReplicationTrait>(
    service: &T,
    body: Vec<u8>,
) -> http::Response<BoxBody> {
    let data = match parse_grpc_frame(&body) {
        Ok(d) => d,
        Err(status) => return create_grpc_error_response(status),
    };

    let _req: proto::GetStatsRequest = match prost::Message::decode(data) {
        Ok(r) => r,
        Err(e) => return create_grpc_error_response(Status::invalid_argument(e.to_string())),
    };

    create_grpc_response(service.get_stats().await)
}
