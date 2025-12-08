//! Replication client for making gRPC requests to other nodes.

use crate::StorageError;
use save_common::TlsConfig;
use save_common::grpc::ProstCodec;
use save_common::retry::{RetryConfig, retry_with_backoff};
use save_proto::replication as proto;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tonic::transport::ClientTlsConfig;
use tracing::{debug, warn};

/// Default connection timeout for establishing new connections.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default RPC timeout for individual requests.
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Client for replication RPCs to a single node.
/// Supports automatic reconnection, retry with exponential backoff, and mTLS.
#[derive(Clone)]
pub struct ReplicationClient {
    node_id: u64,
    addr: String,
    connect_timeout: Duration,
    rpc_timeout: Duration,
    retry_config: RetryConfig,
    tls_config: Option<ClientTlsConfig>,
    channel: Arc<RwLock<Option<tonic::transport::Channel>>>,
}

impl ReplicationClient {
    /// Create a new client for a replication endpoint with default timeouts.
    pub fn new(node_id: u64, addr: String) -> Self {
        Self::with_timeouts(node_id, addr, DEFAULT_CONNECT_TIMEOUT, DEFAULT_RPC_TIMEOUT)
    }

    /// Create a new client with custom timeouts.
    pub fn with_timeouts(
        node_id: u64,
        addr: String,
        connect_timeout: Duration,
        rpc_timeout: Duration,
    ) -> Self {
        Self {
            node_id,
            addr,
            connect_timeout,
            rpc_timeout,
            retry_config: RetryConfig::default(),
            tls_config: None,
            channel: Arc::new(RwLock::new(None)),
        }
    }

    /// Set retry configuration.
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Enable mTLS with the given configuration.
    pub fn with_tls(mut self, config: &TlsConfig) -> Result<Self, StorageError> {
        let tls = save_common::load_client_tls_config(config)
            .map_err(|e| StorageError::Tls(e.to_string()))?;
        self.tls_config = Some(tls);
        Ok(self)
    }

    /// Connect to a replication endpoint (eagerly) with default timeouts.
    pub async fn connect(node_id: u64, addr: String) -> Result<Self, StorageError> {
        Self::connect_with_timeouts(node_id, addr, DEFAULT_CONNECT_TIMEOUT, DEFAULT_RPC_TIMEOUT)
            .await
    }

    /// Connect to a replication endpoint (eagerly) with custom timeouts.
    pub async fn connect_with_timeouts(
        node_id: u64,
        addr: String,
        connect_timeout: Duration,
        rpc_timeout: Duration,
    ) -> Result<Self, StorageError> {
        let client = Self::with_timeouts(node_id, addr, connect_timeout, rpc_timeout);
        // Pre-connect to verify endpoint is reachable
        client.get_channel().await?;
        Ok(client)
    }

    /// Reset the connection, forcing reconnect on next call.
    pub async fn reset_connection(&self) {
        *self.channel.write().await = None;
    }

    async fn get_channel(&self) -> Result<tonic::transport::Channel, StorageError> {
        // Fast path: check if we have a channel
        {
            let guard = self.channel.read().await;
            if let Some(ref channel) = *guard {
                return Ok(channel.clone());
            }
        }

        // Slow path: acquire write lock and connect with retry
        let mut guard = self.channel.write().await;
        // Double-check after acquiring write lock
        if let Some(ref channel) = *guard {
            return Ok(channel.clone());
        }

        // Retry connection establishment for transient network issues
        let channel = retry_with_backoff(
            &self.retry_config,
            || async { self.do_connect().await },
            Self::is_transport_error,
        )
        .await?;

        *guard = Some(channel.clone());
        Ok(channel)
    }

    async fn do_connect(&self) -> Result<tonic::transport::Channel, StorageError> {
        debug!(node_id = %self.node_id, addr = %self.addr, tls = self.tls_config.is_some(), "Connecting to replication endpoint");

        let mut endpoint = tonic::transport::Channel::from_shared(self.addr.clone())
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?
            .connect_timeout(self.connect_timeout)
            .timeout(self.rpc_timeout);

        if let Some(ref tls) = self.tls_config {
            endpoint = endpoint
                .tls_config(tls.clone())
                .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;
        }

        endpoint
            .connect()
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))
    }

    /// Node ID for this client.
    pub fn node_id(&self) -> u64 {
        self.node_id
    }

    /// Address of the remote node.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Prepare object on remote node (2PC phase 1).
    pub async fn prepare_object(
        &self,
        key: &str,
        data: Vec<u8>,
        checksum: &str,
        request_id: u64,
    ) -> Result<String, StorageError> {
        let req = proto::PrepareObjectRequest {
            key: key.to_string(),
            data,
            checksum: checksum.to_string(),
            request_id,
        };

        let resp = self.call_prepare(req).await?;

        if resp.success {
            debug!(node_id = %self.node_id, key = %key, "Prepare succeeded");
            Ok(resp.temp_id)
        } else {
            warn!(node_id = %self.node_id, error = %resp.error_message, "Prepare failed");
            Err(StorageError::Io(std::io::Error::other(resp.error_message)))
        }
    }

    /// Commit prepared object on remote node (2PC phase 2).
    pub async fn commit_object(
        &self,
        key: &str,
        temp_id: &str,
        request_id: u64,
    ) -> Result<(), StorageError> {
        let req = proto::CommitObjectRequest {
            key: key.to_string(),
            temp_id: temp_id.to_string(),
            request_id,
        };

        let resp = self.call_commit(req).await?;

        if resp.success {
            debug!(node_id = %self.node_id, key = %key, "Commit succeeded");
            Ok(())
        } else {
            warn!(node_id = %self.node_id, error = %resp.error_message, "Commit failed");
            Err(StorageError::Io(std::io::Error::other(resp.error_message)))
        }
    }

    /// Abort prepared object on remote node (2PC rollback).
    pub async fn abort_object(
        &self,
        key: &str,
        temp_id: &str,
        request_id: u64,
    ) -> Result<(), StorageError> {
        let req = proto::AbortObjectRequest {
            key: key.to_string(),
            temp_id: temp_id.to_string(),
            request_id,
        };

        let resp = self.call_abort(req).await?;

        if resp.success {
            debug!(node_id = %self.node_id, key = %key, "Abort succeeded");
            Ok(())
        } else {
            warn!(node_id = %self.node_id, error = %resp.error_message, "Abort failed");
            Err(StorageError::Io(std::io::Error::other(resp.error_message)))
        }
    }

    /// Delete object on remote node.
    pub async fn delete_object(&self, key: &str, request_id: u64) -> Result<bool, StorageError> {
        let req = proto::DeleteObjectRequest {
            key: key.to_string(),
            request_id,
        };

        let resp = self.call_delete(req).await?;

        if resp.success {
            Ok(resp.was_present)
        } else {
            Err(StorageError::Io(std::io::Error::other(resp.error_message)))
        }
    }

    /// Check object exists on remote node.
    pub async fn object_exists(&self, key: &str) -> Result<(bool, u64, String), StorageError> {
        let req = proto::ObjectExistsRequest {
            key: key.to_string(),
        };

        let resp = self.call_exists(req).await?;
        Ok((resp.exists, resp.size, resp.checksum))
    }

    /// Health check remote node.
    pub async fn health_check(&self) -> Result<proto::HealthCheckResponse, StorageError> {
        self.call_health().await
    }

    /// Streaming prepare for large objects. Reads from file path in chunks.
    pub async fn stream_prepare_object(
        &self,
        key: &str,
        request_id: u64,
        file_path: &std::path::Path,
        checksum: &str,
    ) -> Result<String, StorageError> {
        use tokio::io::AsyncReadExt;

        let channel = self.get_channel().await?;
        let mut client = tonic::client::Grpc::new(channel);

        client
            .ready()
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;

        let (tx, rx) = tokio::sync::mpsc::channel::<proto::PrepareChunkRequest>(4);

        // Spawn task to read file and send chunks
        let key_owned = key.to_string();
        let checksum_owned = checksum.to_string();
        let path_owned = file_path.to_path_buf();

        let send_task = tokio::spawn(async move {
            let mut file = match tokio::fs::File::open(&path_owned).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to open file for streaming");
                    return;
                }
            };

            let mut buf = vec![0u8; 64 * 1024]; // 64KB chunks
            let mut is_first = true;

            loop {
                let n = match file.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to read chunk");
                        break;
                    }
                };

                let chunk = proto::PrepareChunkRequest {
                    key: if is_first {
                        key_owned.clone()
                    } else {
                        String::new()
                    },
                    request_id: if is_first { request_id } else { 0 },
                    chunk: buf[..n].to_vec(),
                    is_last: false,
                    checksum: String::new(),
                };
                is_first = false;

                if tx.send(chunk).await.is_err() {
                    break;
                }
            }

            // Send final marker with checksum
            let final_chunk = proto::PrepareChunkRequest {
                key: String::new(),
                request_id: 0,
                chunk: Vec::new(),
                is_last: true,
                checksum: checksum_owned,
            };
            let _ = tx.send(final_chunk).await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let path =
            http::uri::PathAndQuery::from_static("/replication.WriteReplica/StreamPrepareObject");
        let request = tonic::Request::new(stream);

        let response = client
            .client_streaming(request, path, ProstCodec::default())
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

        let _ = send_task.await;

        let resp: proto::PrepareObjectResponse = response.into_inner();
        if resp.success {
            debug!(node_id = %self.node_id, key = %key, "Streaming prepare succeeded");
            Ok(resp.temp_id)
        } else {
            warn!(node_id = %self.node_id, error = %resp.error_message, "Streaming prepare failed");
            Err(StorageError::Io(std::io::Error::other(resp.error_message)))
        }
    }

    /// Read object from remote node with streaming.
    /// Returns an AsyncRead that streams data without full buffering.
    pub async fn read_object_stream(&self, key: &str) -> Result<StreamingReader, StorageError> {
        let req = proto::ReadObjectRequest {
            key: key.to_string(),
            offset: 0,
            length: 0, // 0 = read all
        };

        let channel = self.get_channel().await?;
        let mut client = tonic::client::Grpc::new(channel);

        client
            .ready()
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;

        let path = http::uri::PathAndQuery::from_static("/replication.ReadReplica/ReadObject");
        let request = tonic::Request::new(req);

        let response = client
            .server_streaming(request, path, ProstCodec::default())
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

        let stream: tonic::Streaming<proto::ReadObjectResponse> = response.into_inner();
        Ok(StreamingReader::new(stream))
    }
}

/// Adapter that wraps a tonic::Streaming into an AsyncRead using a channel.
pub struct StreamingReader {
    rx: tokio::sync::mpsc::Receiver<Result<bytes::Bytes, std::io::Error>>,
    buffer: bytes::Bytes,
    _task: tokio::task::JoinHandle<()>,
}

impl StreamingReader {
    fn new(mut stream: tonic::Streaming<proto::ReadObjectResponse>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(4);

        let task = tokio::spawn(async move {
            loop {
                match stream.message().await {
                    Ok(Some(chunk)) => {
                        let is_last = chunk.is_last;
                        if !chunk.chunk.is_empty()
                            && tx.send(Ok(bytes::Bytes::from(chunk.chunk))).await.is_err()
                        {
                            break;
                        }
                        if is_last {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                        break;
                    }
                }
            }
        });

        Self {
            rx,
            buffer: bytes::Bytes::new(),
            _task: task,
        }
    }
}

impl tokio::io::AsyncRead for StreamingReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;

        // Return buffered data first
        if !self.buffer.is_empty() {
            let to_copy = std::cmp::min(self.buffer.len(), buf.remaining());
            buf.put_slice(&self.buffer[..to_copy]);
            self.buffer = self.buffer.slice(to_copy..);
            return Poll::Ready(Ok(()));
        }

        // Try to receive more data
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(data))) => {
                let to_copy = std::cmp::min(data.len(), buf.remaining());
                buf.put_slice(&data[..to_copy]);
                if to_copy < data.len() {
                    self.buffer = data.slice(to_copy..);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Err(e)),
            Poll::Ready(None) => Poll::Ready(Ok(())), // EOF
            Poll::Pending => Poll::Pending,
        }
    }
}

impl std::fmt::Debug for StreamingReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingReader")
            .field("buffer_len", &self.buffer.len())
            .finish()
    }
}

impl ReplicationClient {
    // Internal gRPC call implementations

    async fn call_prepare(
        &self,
        req: proto::PrepareObjectRequest,
    ) -> Result<proto::PrepareObjectResponse, StorageError> {
        self.call_unary("/replication.WriteReplica/PrepareObject", req)
            .await
    }

    async fn call_commit(
        &self,
        req: proto::CommitObjectRequest,
    ) -> Result<proto::CommitObjectResponse, StorageError> {
        self.call_unary("/replication.WriteReplica/CommitObject", req)
            .await
    }

    async fn call_abort(
        &self,
        req: proto::AbortObjectRequest,
    ) -> Result<proto::AbortObjectResponse, StorageError> {
        self.call_unary("/replication.WriteReplica/AbortObject", req)
            .await
    }

    async fn call_delete(
        &self,
        req: proto::DeleteObjectRequest,
    ) -> Result<proto::DeleteObjectResponse, StorageError> {
        self.call_unary("/replication.DeleteReplica/DeleteObject", req)
            .await
    }

    async fn call_exists(
        &self,
        req: proto::ObjectExistsRequest,
    ) -> Result<proto::ObjectExistsResponse, StorageError> {
        self.call_unary("/replication.ReadReplica/ObjectExists", req)
            .await
    }

    async fn call_health(&self) -> Result<proto::HealthCheckResponse, StorageError> {
        self.call_unary(
            "/replication.ReplicationHealth/HealthCheck",
            proto::HealthCheckRequest {},
        )
        .await
    }

    async fn call_unary<Req, Resp>(
        &self,
        path: &'static str,
        req: Req,
    ) -> Result<Resp, StorageError>
    where
        Req: prost::Message + Clone + 'static,
        Resp: prost::Message + Default + 'static,
    {
        retry_with_backoff(
            &self.retry_config,
            || {
                let req = req.clone();
                async move {
                    let result = self.do_call_unary(path, req).await;
                    // Reset connection on transport errors for automatic reconnect
                    if let Err(ref e) = result
                        && Self::is_transport_error(e)
                    {
                        warn!(node_id = %self.node_id, error = %e, "Transport error, will retry");
                        self.reset_connection().await;
                    }
                    result
                }
            },
            Self::is_transport_error,
        )
        .await
    }

    async fn do_call_unary<Req, Resp>(
        &self,
        path: &'static str,
        req: Req,
    ) -> Result<Resp, StorageError>
    where
        Req: prost::Message + 'static,
        Resp: prost::Message + Default + 'static,
    {
        let channel = self.get_channel().await?;
        let mut client = tonic::client::Grpc::new(channel);

        client
            .ready()
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;

        let path = http::uri::PathAndQuery::from_static(path);
        let request = tonic::Request::new(req);

        let response: tonic::Response<Resp> = client
            .unary(request, path, ProstCodec::<Req, Resp>::default())
            .await
            .map_err(|e| StorageError::Io(std::io::Error::other(e.to_string())))?;

        Ok(response.into_inner())
    }

    fn is_transport_error(e: &StorageError) -> bool {
        match e {
            StorageError::Io(io_err) => {
                let msg = io_err.to_string().to_lowercase();
                msg.contains("connection")
                    || msg.contains("unavailable")
                    || msg.contains("transport")
                    || msg.contains("timeout")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
            }
            _ => false,
        }
    }
}

impl std::fmt::Debug for ReplicationClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicationClient")
            .field("node_id", &self.node_id)
            .field("addr", &self.addr)
            .finish()
    }
}
