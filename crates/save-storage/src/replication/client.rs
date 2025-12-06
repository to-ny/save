//! Replication client for making gRPC requests to other nodes.

use crate::StorageError;
use save_common::grpc::ProstCodec;
use save_proto::replication as proto;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Default connection timeout for establishing new connections.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default RPC timeout for individual requests.
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Client for replication RPCs to a single node.
/// Supports automatic reconnection on transport errors.
#[derive(Clone)]
pub struct ReplicationClient {
    node_id: u64,
    addr: String,
    connect_timeout: Duration,
    rpc_timeout: Duration,
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
            channel: Arc::new(RwLock::new(None)),
        }
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

        // Slow path: acquire write lock and connect
        let mut guard = self.channel.write().await;
        // Double-check after acquiring write lock
        if let Some(ref channel) = *guard {
            return Ok(channel.clone());
        }

        let channel = self.do_connect().await?;
        *guard = Some(channel.clone());
        Ok(channel)
    }

    async fn do_connect(&self) -> Result<tonic::transport::Channel, StorageError> {
        debug!(node_id = %self.node_id, addr = %self.addr, "Connecting to replication endpoint");
        tonic::transport::Channel::from_shared(self.addr.clone())
            .map_err(|e| StorageError::Io(std::io::Error::other(e)))?
            .connect_timeout(self.connect_timeout)
            .timeout(self.rpc_timeout)
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
        Req: prost::Message + 'static,
        Resp: prost::Message + Default + 'static,
    {
        let result = self.do_call_unary(path, req).await;

        // Reset connection on transport errors for automatic reconnect
        if let Err(ref e) = result
            && Self::is_transport_error(e)
        {
            warn!(node_id = %self.node_id, "Transport error, resetting connection");
            self.reset_connection().await;
        }

        result
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
