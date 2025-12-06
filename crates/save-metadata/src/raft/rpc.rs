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
use tokio::sync::OnceCell;

/// Connection timeout for establishing new connections.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// RPC timeout for individual requests.
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// Raft RPC client for making outbound requests to peers.
/// Caches the gRPC channel for connection reuse.
#[derive(Clone)]
pub struct RaftRpcClient {
    endpoint: String,
    channel: std::sync::Arc<OnceCell<tonic::transport::Channel>>,
}

impl RaftRpcClient {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            channel: std::sync::Arc::new(OnceCell::new()),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub async fn append_entries(
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
        self.channel
            .get_or_try_init(|| self.connect())
            .await
            .cloned()
    }

    async fn connect(&self) -> Result<tonic::transport::Channel, tonic::Status> {
        tonic::transport::Channel::from_shared(self.endpoint.clone())
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(RPC_TIMEOUT)
            .connect()
            .await
            .map_err(|e| Status::unavailable(e.to_string()))
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

// --- Low-level gRPC helpers ---

async fn send_unary<Req, Resp>(
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
        .map_err(|e| Status::unavailable(e.to_string()))?;

    let path = http::uri::PathAndQuery::from_static(path);
    client
        .unary(request, path, ProstCodec::<Req, Resp>::default())
        .await
}

struct ProstCodec<T, U>(std::marker::PhantomData<(T, U)>);

impl<T, U> Default for ProstCodec<T, U> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
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
        ProstEncoder(std::marker::PhantomData)
    }

    fn decoder(&mut self) -> Self::Decoder {
        ProstDecoder(std::marker::PhantomData)
    }
}

struct ProstEncoder<T>(std::marker::PhantomData<T>);

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

struct ProstDecoder<T>(std::marker::PhantomData<T>);

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

async fn send_client_streaming<Req, Resp, S>(
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
        .map_err(|e| Status::unavailable(e.to_string()))?;

    let path = http::uri::PathAndQuery::from_static(path);
    client
        .client_streaming(
            stream.into_streaming_request(),
            path,
            ProstCodec::<Req, Resp>::default(),
        )
        .await
}
