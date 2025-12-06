//! Raft gRPC service implementation.

use super::types::{NodeId, NodeTypeConfig, Raft};
use openraft::BasicNode;
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use save_proto::raft as proto;
use std::sync::Arc;
use tonic::{Request, Response, Status};

/// Raft RPC server handling incoming consensus requests.
pub struct RaftRpcServer {
    raft: Arc<Raft>,
}

impl RaftRpcServer {
    pub fn new(raft: Arc<Raft>) -> Self {
        Self { raft }
    }

    pub fn raft(&self) -> &Arc<Raft> {
        &self.raft
    }

    pub async fn append_entries(
        &self,
        request: Request<proto::AppendEntriesRequest>,
    ) -> Result<Response<proto::AppendEntriesResponse>, Status> {
        let req = request.into_inner();
        let raft_req = convert_append_entries_request(req)?;

        let resp = self
            .raft
            .append_entries(raft_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(convert_append_entries_response(resp)))
    }

    pub async fn vote(
        &self,
        request: Request<proto::VoteRequest>,
    ) -> Result<Response<proto::VoteResponse>, Status> {
        let req = request.into_inner();
        let raft_req = convert_vote_request(req)?;

        let resp = self
            .raft
            .vote(raft_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(convert_vote_response(resp)))
    }

    pub async fn install_snapshot(
        &self,
        request: Request<tonic::Streaming<proto::InstallSnapshotRequest>>,
    ) -> Result<Response<proto::InstallSnapshotResponse>, Status> {
        use tokio_stream::StreamExt;

        let mut stream = request.into_inner();
        let mut chunks = Vec::new();

        while let Some(chunk) = stream.next().await {
            chunks.push(chunk?);
        }

        self.do_install_snapshot(chunks).await
    }

    pub async fn install_snapshot_from_frames(
        &self,
        frames: Vec<proto::InstallSnapshotRequest>,
    ) -> Result<Response<proto::InstallSnapshotResponse>, Status> {
        self.do_install_snapshot(frames).await
    }

    async fn do_install_snapshot(
        &self,
        chunks: Vec<proto::InstallSnapshotRequest>,
    ) -> Result<Response<proto::InstallSnapshotResponse>, Status> {
        let mut snapshot_data = Vec::new();
        let mut meta: Option<proto::SnapshotMeta> = None;
        let mut vote: Option<proto::Vote> = None;

        for chunk in chunks {
            if meta.is_none() {
                meta = chunk.meta;
                vote = chunk.vote;
            }
            snapshot_data.extend(chunk.data);
        }

        let vote = vote.ok_or_else(|| Status::invalid_argument("missing vote"))?;
        let meta = meta.ok_or_else(|| Status::invalid_argument("missing snapshot meta"))?;

        let raft_req = convert_install_snapshot_request(vote, meta, snapshot_data)?;

        let resp = self
            .raft
            .install_snapshot(raft_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(convert_install_snapshot_response(resp)))
    }
}

// --- Proto to OpenRaft conversions ---

fn convert_vote(v: proto::Vote) -> openraft::Vote<NodeId> {
    openraft::Vote::new_committed(v.term, v.node_id)
}

fn convert_log_id(id: proto::LogId) -> openraft::LogId<NodeId> {
    let leader_id = openraft::CommittedLeaderId::new(id.term, id.node_id);
    openraft::LogId::new(leader_id, id.index)
}

fn convert_append_entries_request(
    req: proto::AppendEntriesRequest,
) -> Result<AppendEntriesRequest<NodeTypeConfig>, Status> {
    let vote = req
        .vote
        .ok_or_else(|| Status::invalid_argument("missing vote"))?;

    let entries: Vec<openraft::Entry<NodeTypeConfig>> = req
        .entries
        .into_iter()
        .map(|e| {
            let log_id = e
                .log_id
                .map(convert_log_id)
                .ok_or_else(|| Status::invalid_argument("missing log_id"))?;
            let payload: openraft::EntryPayload<NodeTypeConfig> =
                serde_json::from_slice(&e.payload)
                    .map_err(|e| Status::invalid_argument(format!("invalid payload: {}", e)))?;
            Ok(openraft::Entry { log_id, payload })
        })
        .collect::<Result<Vec<_>, Status>>()?;

    Ok(AppendEntriesRequest {
        vote: convert_vote(vote),
        prev_log_id: req.prev_log_id.map(convert_log_id),
        entries,
        leader_commit: req.leader_commit.map(convert_log_id),
    })
}

fn convert_vote_request(req: proto::VoteRequest) -> Result<VoteRequest<NodeId>, Status> {
    let vote = req
        .vote
        .ok_or_else(|| Status::invalid_argument("missing vote"))?;

    Ok(VoteRequest {
        vote: convert_vote(vote),
        last_log_id: req.last_log_id.map(convert_log_id),
    })
}

fn convert_install_snapshot_request(
    vote: proto::Vote,
    meta: proto::SnapshotMeta,
    data: Vec<u8>,
) -> Result<InstallSnapshotRequest<NodeTypeConfig>, Status> {
    let membership: openraft::StoredMembership<NodeId, BasicNode> = meta
        .last_membership
        .map(|m| {
            serde_json::from_slice(&m.config)
                .map_err(|e| Status::invalid_argument(format!("invalid membership: {}", e)))
        })
        .transpose()?
        .unwrap_or_default();

    let snapshot_meta = openraft::SnapshotMeta {
        last_log_id: meta.last_log_id.map(convert_log_id),
        last_membership: membership,
        snapshot_id: meta.snapshot_id,
    };

    Ok(InstallSnapshotRequest {
        vote: convert_vote(vote),
        meta: snapshot_meta,
        offset: 0,
        data,
        done: true,
    })
}

// --- OpenRaft to Proto conversions ---

fn to_proto_vote(v: &openraft::Vote<NodeId>) -> proto::Vote {
    proto::Vote {
        term: v.leader_id().term,
        node_id: v.leader_id().node_id,
        committed: v.is_committed(),
    }
}

fn to_proto_log_id(id: &openraft::LogId<NodeId>) -> proto::LogId {
    proto::LogId {
        term: id.leader_id.term,
        node_id: id.leader_id.node_id,
        index: id.index,
    }
}

fn convert_append_entries_response(
    resp: AppendEntriesResponse<NodeId>,
) -> proto::AppendEntriesResponse {
    match resp {
        AppendEntriesResponse::Success => proto::AppendEntriesResponse {
            vote: None,
            success: true,
            conflict: None,
        },
        AppendEntriesResponse::PartialSuccess(opt) => proto::AppendEntriesResponse {
            vote: None,
            success: true,
            conflict: opt.map(|id| to_proto_log_id(&id)),
        },
        AppendEntriesResponse::HigherVote(vote) => proto::AppendEntriesResponse {
            vote: Some(to_proto_vote(&vote)),
            success: false,
            conflict: None,
        },
        AppendEntriesResponse::Conflict => proto::AppendEntriesResponse {
            vote: None,
            success: false,
            conflict: None,
        },
    }
}

fn convert_vote_response(resp: VoteResponse<NodeId>) -> proto::VoteResponse {
    proto::VoteResponse {
        vote: Some(to_proto_vote(&resp.vote)),
        vote_granted: resp.vote_granted,
        last_log_id: resp.last_log_id.map(|id| to_proto_log_id(&id)),
    }
}

fn convert_install_snapshot_response(
    resp: InstallSnapshotResponse<NodeId>,
) -> proto::InstallSnapshotResponse {
    proto::InstallSnapshotResponse {
        vote: Some(to_proto_vote(&resp.vote)),
    }
}

// --- gRPC client for outgoing Raft RPCs ---

use std::time::Duration;
use tokio::sync::RwLock;
use tracing::warn;

/// Default connection timeout for establishing new connections.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default RPC timeout for individual requests.
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// Raft RPC client for making outbound requests to peers.
/// Caches the gRPC channel for connection reuse with automatic reconnection.
#[derive(Clone)]
pub struct RaftRpcClient {
    endpoint: String,
    connect_timeout: Duration,
    rpc_timeout: Duration,
    channel: std::sync::Arc<RwLock<Option<tonic::transport::Channel>>>,
}

impl RaftRpcClient {
    pub fn new(endpoint: String) -> Self {
        Self::with_timeouts(endpoint, DEFAULT_CONNECT_TIMEOUT, DEFAULT_RPC_TIMEOUT)
    }

    pub fn with_timeouts(
        endpoint: String,
        connect_timeout: Duration,
        rpc_timeout: Duration,
    ) -> Self {
        Self {
            endpoint,
            connect_timeout,
            rpc_timeout,
            channel: std::sync::Arc::new(RwLock::new(None)),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Reset the connection, forcing reconnect on next call.
    pub async fn reset_connection(&self) {
        *self.channel.write().await = None;
    }

    pub async fn append_entries(
        &self,
        req: AppendEntriesRequest<NodeTypeConfig>,
    ) -> Result<AppendEntriesResponse<NodeId>, tonic::Status> {
        let result = self.do_append_entries(req).await;
        self.handle_result(result).await
    }

    async fn do_append_entries(
        &self,
        req: AppendEntriesRequest<NodeTypeConfig>,
    ) -> Result<AppendEntriesResponse<NodeId>, tonic::Status> {
        let channel = self.get_channel().await?;
        let proto_req = to_proto_append_entries_request(req)?;

        let request = tonic::Request::new(proto_req);
        let response = send_unary(&channel, "/raft.RaftRpc/AppendEntries", request).await?;

        convert_proto_append_entries_response(response.into_inner())
    }

    pub async fn vote(
        &self,
        req: VoteRequest<NodeId>,
    ) -> Result<VoteResponse<NodeId>, tonic::Status> {
        let result = self.do_vote(req).await;
        self.handle_result(result).await
    }

    async fn do_vote(
        &self,
        req: VoteRequest<NodeId>,
    ) -> Result<VoteResponse<NodeId>, tonic::Status> {
        let channel = self.get_channel().await?;
        let proto_req = to_proto_vote_request(req);

        let request = tonic::Request::new(proto_req);
        let response = send_unary(&channel, "/raft.RaftRpc/Vote", request).await?;

        convert_proto_vote_response(response.into_inner())
    }

    pub async fn install_snapshot(
        &self,
        req: InstallSnapshotRequest<NodeTypeConfig>,
    ) -> Result<InstallSnapshotResponse<NodeId>, tonic::Status> {
        let result = self.do_install_snapshot(req).await;
        self.handle_result(result).await
    }

    async fn do_install_snapshot(
        &self,
        req: InstallSnapshotRequest<NodeTypeConfig>,
    ) -> Result<InstallSnapshotResponse<NodeId>, tonic::Status> {
        let channel = self.get_channel().await?;

        let vote = Some(to_proto_vote(&req.vote));
        let meta = Some(proto::SnapshotMeta {
            last_log_id: req.meta.last_log_id.map(|id| to_proto_log_id(&id)),
            last_membership: Some(proto::Membership {
                config: serde_json::to_vec(&req.meta.last_membership).unwrap_or_default(),
            }),
            snapshot_id: req.meta.snapshot_id.clone(),
        });

        let data = req.data;

        let stream = tokio_stream::once(proto::InstallSnapshotRequest {
            vote,
            meta,
            offset: 0,
            data,
            done: true,
        });

        let response =
            send_client_streaming(&channel, "/raft.RaftRpc/InstallSnapshot", stream).await?;

        convert_proto_install_snapshot_response(response.into_inner())
    }

    async fn get_channel(&self) -> Result<tonic::transport::Channel, tonic::Status> {
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

    async fn do_connect(&self) -> Result<tonic::transport::Channel, tonic::Status> {
        tonic::transport::Channel::from_shared(self.endpoint.clone())
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .connect_timeout(self.connect_timeout)
            .timeout(self.rpc_timeout)
            .connect()
            .await
            .map_err(|e| Status::unavailable(e.to_string()))
    }

    fn is_transport_error(status: &tonic::Status) -> bool {
        matches!(
            status.code(),
            tonic::Code::Unavailable
                | tonic::Code::Cancelled
                | tonic::Code::DeadlineExceeded
                | tonic::Code::Aborted
        )
    }

    async fn handle_result<T>(&self, result: Result<T, tonic::Status>) -> Result<T, tonic::Status> {
        if let Err(ref e) = result
            && Self::is_transport_error(e)
        {
            warn!(endpoint = %self.endpoint, "Transport error, resetting connection");
            self.reset_connection().await;
        }
        result
    }
}

fn to_proto_append_entries_request(
    req: AppendEntriesRequest<NodeTypeConfig>,
) -> Result<proto::AppendEntriesRequest, Status> {
    let entries: Vec<proto::LogEntry> = req
        .entries
        .into_iter()
        .map(|e| {
            let payload = serde_json::to_vec(&e.payload)
                .map_err(|e| Status::internal(format!("serialize error: {}", e)))?;
            Ok(proto::LogEntry {
                log_id: Some(to_proto_log_id(&e.log_id)),
                payload,
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;

    Ok(proto::AppendEntriesRequest {
        vote: Some(to_proto_vote(&req.vote)),
        prev_log_id: req.prev_log_id.map(|id| to_proto_log_id(&id)),
        entries,
        leader_commit: req.leader_commit.map(|id| to_proto_log_id(&id)),
    })
}

fn to_proto_vote_request(req: VoteRequest<NodeId>) -> proto::VoteRequest {
    proto::VoteRequest {
        vote: Some(to_proto_vote(&req.vote)),
        last_log_id: req.last_log_id.map(|id| to_proto_log_id(&id)),
    }
}

fn convert_proto_append_entries_response(
    resp: proto::AppendEntriesResponse,
) -> Result<AppendEntriesResponse<NodeId>, Status> {
    if resp.success {
        Ok(AppendEntriesResponse::Success)
    } else if let Some(vote) = resp.vote {
        Ok(AppendEntriesResponse::HigherVote(convert_vote(vote)))
    } else {
        Ok(AppendEntriesResponse::Conflict)
    }
}

fn convert_proto_vote_response(resp: proto::VoteResponse) -> Result<VoteResponse<NodeId>, Status> {
    let vote = resp
        .vote
        .ok_or_else(|| Status::invalid_argument("missing vote"))?;

    Ok(VoteResponse {
        vote: convert_vote(vote),
        vote_granted: resp.vote_granted,
        last_log_id: resp.last_log_id.map(convert_log_id),
    })
}

fn convert_proto_install_snapshot_response(
    resp: proto::InstallSnapshotResponse,
) -> Result<InstallSnapshotResponse<NodeId>, Status> {
    let vote = resp
        .vote
        .ok_or_else(|| Status::invalid_argument("missing vote"))?;

    Ok(InstallSnapshotResponse {
        vote: convert_vote(vote),
    })
}

// Re-export shared gRPC helpers
use save_common::grpc::{send_client_streaming, send_unary};
