//! gRPC server for internal cluster admin API.

use super::service::ClusterAdminService;
use crate::AppState;
use crate::metrics::{grpc_request_duration_seconds, grpc_requests_total};
use prost::Message;
use save_common::TlsConfig;
use save_common::{BoxBody, create_grpc_error_response, create_grpc_response, parse_grpc_frame};
use save_proto::cluster as proto;
use socket2::{Domain, Socket, Type};
use std::net::SocketAddr;
use std::time::Instant;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Status;
use tonic::server::NamedService;
use tonic::transport::Server;
use tracing::{debug, info};

const SERVICE_NAME: &str = "ClusterAdmin";

/// Run the internal gRPC server with optional mTLS.
///
/// # Arguments
/// * `state` - Application state
/// * `addr` - The address to bind to
/// * `shutdown_rx` - Optional broadcast receiver for shutdown signal
/// * `tls_config` - Optional TLS configuration for mTLS
/// * `require_auth` - Whether to require mTLS authentication
/// * `ready_tx` - Optional oneshot sender to signal when server is ready to accept connections
pub async fn run_server(
    state: AppState,
    addr: SocketAddr,
    mut shutdown_rx: Option<broadcast::Receiver<()>>,
    tls_config: Option<&TlsConfig>,
    require_auth: bool,
    ready_tx: Option<oneshot::Sender<()>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(
        "Starting internal gRPC server on {} (TLS: {}, require_auth: {})",
        addr,
        tls_config.is_some(),
        require_auth
    );

    let service = ClusterAdminService::new(state);
    let wrapper = ServiceWrapper { service };

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

    let mut builder = Server::builder();

    if let Some(tls) = tls_config {
        let server_tls = save_common::load_server_tls_config(tls)?;
        builder = builder.tls_config(server_tls)?;
    }

    let server = builder.add_service(ClusterAdminServer::new(wrapper, require_auth));

    match shutdown_rx.take() {
        Some(mut rx) => {
            server
                .serve_with_incoming_shutdown(incoming, async move {
                    let _ = rx.recv().await;
                    info!("Internal gRPC server shutting down");
                })
                .await?
        }
        None => server.serve_with_incoming(incoming).await?,
    }

    Ok(())
}

#[derive(Clone)]
struct ServiceWrapper {
    service: ClusterAdminService,
}

#[derive(Clone)]
struct ClusterAdminServer<T: Clone> {
    inner: T,
    require_auth: bool,
}

impl<T: Clone> ClusterAdminServer<T> {
    fn new(inner: T, require_auth: bool) -> Self {
        Self {
            inner,
            require_auth,
        }
    }
}

impl<T: Clone> NamedService for ClusterAdminServer<T> {
    const NAME: &'static str = "cluster.ClusterAdmin";
}

impl<T, B> tower::Service<http::Request<B>> for ClusterAdminServer<T>
where
    T: Clone + Send + Sync + 'static,
    T: ClusterAdminTrait,
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
        let require_auth = self.require_auth;
        let path = req.uri().path().to_string();

        Box::pin(async move {
            let start = Instant::now();
            let method = extract_method(&path);

            // Check for mTLS authentication if required
            if require_auth {
                // In mTLS, the TLS layer validates the client certificate.
                // If we get here with TLS enabled, the client is authenticated.
                // The tonic server handles TLS validation, so we just log here.
                debug!(path = %path, "Processing authenticated request");
            }

            let (_parts, body) = req.into_parts();
            let body = match http_body_util::BodyExt::collect(body).await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    record_request(method, "invalid_argument", start.elapsed().as_secs_f64());
                    return Ok(create_grpc_error_response(Status::internal(
                        "failed to read body",
                    )));
                }
            };

            let (response, grpc_status) = match path.as_str() {
                "/cluster.ClusterAdmin/GetStatus" => handle_get_status(&inner, body.to_vec()).await,
                "/cluster.ClusterAdmin/AddLearner" => {
                    handle_add_learner(&inner, body.to_vec()).await
                }
                "/cluster.ClusterAdmin/PromoteVoters" => {
                    handle_promote_voters(&inner, body.to_vec()).await
                }
                "/cluster.ClusterAdmin/RemoveNode" => {
                    handle_remove_node(&inner, body.to_vec()).await
                }
                "/cluster.ClusterAdmin/DrainNode" => handle_drain_node(&inner, body.to_vec()).await,
                "/cluster.ClusterAdmin/GetDebugInfo" => {
                    handle_get_debug_info(&inner, body.to_vec()).await
                }
                "/cluster.ClusterAdmin/TriggerElect" => {
                    handle_trigger_elect(&inner, body.to_vec()).await
                }
                _ => {
                    record_request(method, "unimplemented", start.elapsed().as_secs_f64());
                    return Ok(create_grpc_error_response(Status::unimplemented(
                        "unknown method",
                    )));
                }
            };

            record_request(method, grpc_status, start.elapsed().as_secs_f64());
            Ok(response)
        })
    }
}

fn extract_method(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("unknown")
}

fn record_request(method: &str, status: &str, duration: f64) {
    grpc_requests_total()
        .with_label_values(&[SERVICE_NAME, method, status])
        .inc();
    grpc_request_duration_seconds()
        .with_label_values(&[SERVICE_NAME, method])
        .observe(duration);
}

trait ClusterAdminTrait: Clone + Send + Sync + 'static {
    fn get_status(&self) -> impl Future<Output = proto::GetStatusResponse> + Send;

    fn add_learner(
        &self,
        req: proto::AddLearnerRequest,
    ) -> impl Future<Output = proto::AddLearnerResponse> + Send;

    fn promote_voters(
        &self,
        req: proto::PromoteVotersRequest,
    ) -> impl Future<Output = proto::PromoteVotersResponse> + Send;

    fn remove_node(
        &self,
        req: proto::RemoveNodeRequest,
    ) -> impl Future<Output = proto::RemoveNodeResponse> + Send;

    fn drain_node(
        &self,
        req: proto::DrainNodeRequest,
    ) -> impl Future<Output = proto::DrainNodeResponse> + Send;

    fn get_debug_info(
        &self,
        req: proto::GetDebugInfoRequest,
    ) -> impl Future<Output = proto::GetDebugInfoResponse> + Send;

    fn trigger_elect(
        &self,
        req: proto::TriggerElectRequest,
    ) -> impl Future<Output = proto::TriggerElectResponse> + Send;
}

impl ClusterAdminTrait for ServiceWrapper {
    async fn get_status(&self) -> proto::GetStatusResponse {
        self.service.get_status().await
    }

    async fn add_learner(&self, req: proto::AddLearnerRequest) -> proto::AddLearnerResponse {
        self.service.add_learner(req).await
    }

    async fn promote_voters(
        &self,
        req: proto::PromoteVotersRequest,
    ) -> proto::PromoteVotersResponse {
        self.service.promote_voters(req).await
    }

    async fn remove_node(&self, req: proto::RemoveNodeRequest) -> proto::RemoveNodeResponse {
        self.service.remove_node(req).await
    }

    async fn drain_node(&self, req: proto::DrainNodeRequest) -> proto::DrainNodeResponse {
        self.service.drain_node(req).await
    }

    async fn get_debug_info(&self, req: proto::GetDebugInfoRequest) -> proto::GetDebugInfoResponse {
        self.service.get_debug_info(req).await
    }

    async fn trigger_elect(&self, req: proto::TriggerElectRequest) -> proto::TriggerElectResponse {
        self.service.trigger_elect(req).await
    }
}

// Request handler type alias
type HandlerResult = (http::Response<BoxBody>, &'static str);

/// Macro to generate gRPC request handlers with consistent error handling and metrics.
macro_rules! define_grpc_handler {
    // Handler for empty request (no decode needed)
    ($name:ident, $trait_bound:ident, $method:ident, empty) => {
        async fn $name<T: $trait_bound>(service: &T, body: Vec<u8>) -> HandlerResult {
            if !body.is_empty() && parse_grpc_frame(&body).is_err() {
                return (
                    create_grpc_error_response(Status::invalid_argument("invalid grpc frame")),
                    "3", // INVALID_ARGUMENT
                );
            }
            (create_grpc_response(service.$method().await), "0")
        }
    };
    // Handler with request decode
    ($name:ident, $trait_bound:ident, $method:ident, $req_type:ty) => {
        async fn $name<T: $trait_bound>(service: &T, body: Vec<u8>) -> HandlerResult {
            let data = match parse_grpc_frame(&body) {
                Ok(d) => d,
                Err(status) => {
                    let code = status_to_code(&status);
                    return (create_grpc_error_response(status), code);
                }
            };

            match <$req_type>::decode(data) {
                Ok(req) => (create_grpc_response(service.$method(req).await), "0"),
                Err(e) => (
                    create_grpc_error_response(Status::invalid_argument(e.to_string())),
                    "3", // INVALID_ARGUMENT
                ),
            }
        }
    };
}

fn status_to_code(status: &Status) -> &'static str {
    match status.code() {
        tonic::Code::Ok => "0",
        tonic::Code::Cancelled => "1",
        tonic::Code::Unknown => "2",
        tonic::Code::InvalidArgument => "3",
        tonic::Code::DeadlineExceeded => "4",
        tonic::Code::NotFound => "5",
        tonic::Code::AlreadyExists => "6",
        tonic::Code::PermissionDenied => "7",
        tonic::Code::ResourceExhausted => "8",
        tonic::Code::FailedPrecondition => "9",
        tonic::Code::Aborted => "10",
        tonic::Code::OutOfRange => "11",
        tonic::Code::Unimplemented => "12",
        tonic::Code::Internal => "13",
        tonic::Code::Unavailable => "14",
        tonic::Code::DataLoss => "15",
        tonic::Code::Unauthenticated => "16",
    }
}

define_grpc_handler!(handle_get_status, ClusterAdminTrait, get_status, empty);
define_grpc_handler!(
    handle_add_learner,
    ClusterAdminTrait,
    add_learner,
    proto::AddLearnerRequest
);
define_grpc_handler!(
    handle_promote_voters,
    ClusterAdminTrait,
    promote_voters,
    proto::PromoteVotersRequest
);
define_grpc_handler!(
    handle_remove_node,
    ClusterAdminTrait,
    remove_node,
    proto::RemoveNodeRequest
);
define_grpc_handler!(
    handle_drain_node,
    ClusterAdminTrait,
    drain_node,
    proto::DrainNodeRequest
);
define_grpc_handler!(
    handle_get_debug_info,
    ClusterAdminTrait,
    get_debug_info,
    proto::GetDebugInfoRequest
);
define_grpc_handler!(
    handle_trigger_elect,
    ClusterAdminTrait,
    trigger_elect,
    proto::TriggerElectRequest
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_method() {
        assert_eq!(
            extract_method("/cluster.ClusterAdmin/GetStatus"),
            "GetStatus"
        );
        assert_eq!(
            extract_method("/cluster.ClusterAdmin/AddLearner"),
            "AddLearner"
        );
        assert_eq!(extract_method("unknown"), "unknown");
        assert_eq!(extract_method("/"), "");
        assert_eq!(extract_method(""), "");
    }

    #[tokio::test]
    async fn test_create_response_encoding() {
        use http_body_util::BodyExt;

        let msg = proto::GetStatusResponse {
            node_id: 1,
            initialized: true,
            state: proto::RaftState::Leader.into(),
            current_term: 5,
            leader_id: Some(1),
            voters: vec![1],
            learners: vec![],
            last_applied_index: Some(10),
            last_log_index: Some(10),
        };

        let response = create_grpc_response(msg);
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/grpc"
        );

        // Collect body to get trailers
        let collected = response.into_body().collect().await.unwrap();
        let trailers = collected.trailers().unwrap();
        assert_eq!(trailers.get("grpc-status").unwrap(), "0");
    }

    #[test]
    fn test_create_error_response() {
        let response = create_grpc_error_response(Status::invalid_argument("test error"));
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/grpc"
        );
        // Error responses use trailers-only format (grpc-status in headers)
        assert_eq!(response.headers().get("grpc-status").unwrap(), "3");
        assert_eq!(
            response.headers().get("grpc-message").unwrap(),
            "test error"
        );
    }
}
