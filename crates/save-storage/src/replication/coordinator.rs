//! Replication coordinator for quorum writes across nodes.

use super::client::ReplicationClient;
use crate::StorageError;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Quorum configuration.
#[derive(Debug, Clone)]
pub struct QuorumConfig {
    /// Number of replicas to write (replication factor).
    pub replication_factor: usize,
    /// Minimum successful writes for quorum.
    pub write_quorum: usize,
    /// Minimum successful reads for quorum (unused in Phase 2).
    pub read_quorum: usize,
}

impl QuorumConfig {
    /// Create config with N replicas, requiring majority for quorum.
    pub fn with_replication_factor(n: usize) -> Self {
        let quorum = (n / 2) + 1;
        Self {
            replication_factor: n,
            write_quorum: quorum,
            read_quorum: 1, // Eventual consistency by default
        }
    }
}

impl Default for QuorumConfig {
    fn default() -> Self {
        Self::with_replication_factor(3)
    }
}

/// Result of a replicated operation.
#[derive(Debug)]
pub struct ReplicationResult {
    /// Number of nodes that succeeded.
    pub success_count: usize,
    /// Number of nodes that failed.
    pub failure_count: usize,
    /// Whether quorum was achieved.
    pub quorum_achieved: bool,
    /// Node IDs that succeeded.
    pub successful_nodes: Vec<u64>,
    /// Node IDs that failed with error messages.
    pub failed_nodes: Vec<(u64, String)>,
}

/// Prepared write state for 2PC.
struct PreparedWrite {
    key: String,
    #[allow(dead_code)]
    checksum: String,
    request_id: u64,
    temp_ids: HashMap<u64, String>, // node_id -> temp_id
}

/// Coordinates replicated writes across cluster nodes.
pub struct ReplicationCoordinator {
    local_node_id: u64,
    config: QuorumConfig,
    connect_timeout: std::time::Duration,
    rpc_timeout: std::time::Duration,
    clients: Arc<RwLock<HashMap<u64, ReplicationClient>>>,
    request_counter: AtomicU64,
}

impl ReplicationCoordinator {
    /// Create a new coordinator with default timeouts.
    pub fn new(local_node_id: u64, config: QuorumConfig) -> Self {
        Self::with_timeouts(
            local_node_id,
            config,
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(30),
        )
    }

    /// Create a new coordinator with custom timeouts.
    pub fn with_timeouts(
        local_node_id: u64,
        config: QuorumConfig,
        connect_timeout: std::time::Duration,
        rpc_timeout: std::time::Duration,
    ) -> Self {
        Self {
            local_node_id,
            config,
            connect_timeout,
            rpc_timeout,
            clients: Arc::new(RwLock::new(HashMap::new())),
            request_counter: AtomicU64::new(1),
        }
    }

    /// Add a replica node client.
    pub async fn add_node(&self, node_id: u64, addr: String) -> Result<(), StorageError> {
        if node_id == self.local_node_id {
            return Ok(()); // Skip self
        }

        let client = ReplicationClient::connect_with_timeouts(
            node_id,
            addr,
            self.connect_timeout,
            self.rpc_timeout,
        )
        .await?;
        self.clients.write().await.insert(node_id, client);
        info!(node_id = %node_id, "Added replication client");
        Ok(())
    }

    /// Remove a replica node client.
    pub async fn remove_node(&self, node_id: u64) {
        self.clients.write().await.remove(&node_id);
        info!(node_id = %node_id, "Removed replication client");
    }

    /// Get connected node IDs (excluding self).
    pub async fn connected_nodes(&self) -> Vec<u64> {
        self.clients.read().await.keys().copied().collect()
    }

    /// Select nodes for placement (round-robin over healthy nodes).
    pub async fn select_replica_nodes(&self, count: usize) -> Vec<u64> {
        let clients = self.clients.read().await;
        let mut nodes: Vec<u64> = clients.keys().copied().collect();
        nodes.truncate(count.saturating_sub(1)); // -1 for local
        nodes
    }

    /// Replicate object write to quorum of nodes using 2PC.
    ///
    /// Returns Ok if quorum achieved, Err otherwise.
    pub async fn replicate_write(
        &self,
        key: &str,
        data: Vec<u8>,
        local_write: impl std::future::Future<Output = Result<(), StorageError>>,
    ) -> Result<ReplicationResult, StorageError> {
        let request_id = self.request_counter.fetch_add(1, Ordering::SeqCst);
        let checksum = compute_sha256(&data);

        // Select replica nodes
        let replica_nodes = self
            .select_replica_nodes(self.config.replication_factor)
            .await;

        debug!(
            key = %key,
            request_id = %request_id,
            replica_count = replica_nodes.len(),
            "Starting replicated write"
        );

        // Phase 1: Prepare on all replicas
        let mut prepared = PreparedWrite {
            key: key.to_string(),
            checksum: checksum.clone(),
            request_id,
            temp_ids: HashMap::new(),
        };

        let prepare_results = self
            .prepare_all(&replica_nodes, key, data.clone(), &checksum, request_id)
            .await;

        // Track prepare successes/failures
        let mut success_count = 0;
        let mut successful_nodes = vec![self.local_node_id]; // Local always participates
        let mut failed_nodes = Vec::new();

        for (node_id, result) in prepare_results {
            match result {
                Ok(temp_id) => {
                    prepared.temp_ids.insert(node_id, temp_id);
                    successful_nodes.push(node_id);
                    success_count += 1;
                }
                Err(e) => {
                    failed_nodes.push((node_id, e.to_string()));
                }
            }
        }

        // Include local in count
        success_count += 1;

        // Check quorum before proceeding
        if success_count < self.config.write_quorum {
            // Abort all prepared writes
            self.abort_all(&prepared).await;

            return Ok(ReplicationResult {
                success_count,
                failure_count: failed_nodes.len(),
                quorum_achieved: false,
                successful_nodes,
                failed_nodes,
            });
        }

        // Write locally first
        if let Err(e) = local_write.await {
            // Abort all prepared writes
            self.abort_all(&prepared).await;
            return Err(e);
        }

        // Phase 2: Commit on all prepared replicas
        let commit_results = self.commit_all(&prepared).await;

        // Track commit failures (shouldn't happen normally)
        for (node_id, result) in commit_results {
            if let Err(e) = result {
                warn!(node_id = %node_id, error = %e, "Commit failed after prepare");
                // Don't fail overall - local write succeeded
            }
        }

        debug!(
            key = %key,
            request_id = %request_id,
            success_count = success_count,
            "Replicated write completed"
        );

        Ok(ReplicationResult {
            success_count,
            failure_count: failed_nodes.len(),
            quorum_achieved: true,
            successful_nodes,
            failed_nodes,
        })
    }

    /// Replicate object deletion to all nodes.
    pub async fn replicate_delete(
        &self,
        key: &str,
        local_delete: impl std::future::Future<Output = Result<(), StorageError>>,
    ) -> Result<ReplicationResult, StorageError> {
        let request_id = self.request_counter.fetch_add(1, Ordering::SeqCst);

        // Delete locally first
        local_delete.await?;

        // Delete on all replicas (best effort)
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        for (node_id, client) in clients.iter() {
            let node_id = *node_id;
            let client = client.clone();
            let key = key.to_string();

            handles.push(tokio::spawn(async move {
                let result = client.delete_object(&key, request_id).await;
                (node_id, result)
            }));
        }

        drop(clients);

        let mut success_count = 1; // Local
        let mut successful_nodes = vec![self.local_node_id];
        let mut failed_nodes = Vec::new();

        for handle in handles {
            if let Ok((node_id, result)) = handle.await {
                match result {
                    Ok(_) => {
                        success_count += 1;
                        successful_nodes.push(node_id);
                    }
                    Err(e) => {
                        failed_nodes.push((node_id, e.to_string()));
                    }
                }
            }
        }

        Ok(ReplicationResult {
            success_count,
            failure_count: failed_nodes.len(),
            quorum_achieved: success_count >= self.config.write_quorum,
            successful_nodes,
            failed_nodes,
        })
    }

    async fn prepare_all(
        &self,
        nodes: &[u64],
        key: &str,
        data: Vec<u8>,
        checksum: &str,
        request_id: u64,
    ) -> Vec<(u64, Result<String, StorageError>)> {
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        for node_id in nodes {
            if let Some(client) = clients.get(node_id) {
                let node_id = *node_id;
                let client = client.clone();
                let key = key.to_string();
                let data = data.clone();
                let checksum = checksum.to_string();

                handles.push(tokio::spawn(async move {
                    let result = client
                        .prepare_object(&key, data, &checksum, request_id)
                        .await;
                    (node_id, result)
                }));
            }
        }

        drop(clients);

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    async fn commit_all(&self, prepared: &PreparedWrite) -> Vec<(u64, Result<(), StorageError>)> {
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        for (node_id, temp_id) in &prepared.temp_ids {
            if let Some(client) = clients.get(node_id) {
                let node_id = *node_id;
                let client = client.clone();
                let key = prepared.key.clone();
                let temp_id = temp_id.clone();
                let request_id = prepared.request_id;

                handles.push(tokio::spawn(async move {
                    let result = client.commit_object(&key, &temp_id, request_id).await;
                    (node_id, result)
                }));
            }
        }

        drop(clients);

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    async fn abort_all(&self, prepared: &PreparedWrite) {
        let clients = self.clients.read().await;

        // Collect all abort tasks upfront to avoid borrowing issues
        let abort_tasks: Vec<_> = prepared
            .temp_ids
            .iter()
            .filter_map(|(node_id, temp_id)| {
                clients.get(node_id).map(|client| {
                    let node_id = *node_id;
                    let client = client.clone();
                    let key = prepared.key.clone();
                    let temp_id = temp_id.clone();
                    let request_id = prepared.request_id;
                    (node_id, client, key, temp_id, request_id)
                })
            })
            .collect();

        drop(clients);

        // Fire and forget aborts
        for (node_id, client, key, temp_id, request_id) in abort_tasks {
            tokio::spawn(async move {
                if let Err(e) = client.abort_object(&key, &temp_id, request_id).await {
                    warn!(node_id = %node_id, error = %e, "Abort failed");
                }
            });
        }
    }
}

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

impl std::fmt::Debug for ReplicationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicationCoordinator")
            .field("local_node_id", &self.local_node_id)
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quorum_config_defaults() {
        let config = QuorumConfig::default();
        assert_eq!(config.replication_factor, 3);
        assert_eq!(config.write_quorum, 2);
        assert_eq!(config.read_quorum, 1);
    }

    #[test]
    fn test_quorum_config_factor_5() {
        let config = QuorumConfig::with_replication_factor(5);
        assert_eq!(config.replication_factor, 5);
        assert_eq!(config.write_quorum, 3);
    }

    #[test]
    fn test_quorum_config_factor_1() {
        let config = QuorumConfig::with_replication_factor(1);
        assert_eq!(config.replication_factor, 1);
        assert_eq!(config.write_quorum, 1);
    }

    #[tokio::test]
    async fn test_coordinator_creation() {
        let coord = ReplicationCoordinator::new(1, QuorumConfig::default());
        assert!(coord.connected_nodes().await.is_empty());
    }

    #[tokio::test]
    async fn test_select_replica_nodes_empty() {
        let coord = ReplicationCoordinator::new(1, QuorumConfig::default());
        let nodes = coord.select_replica_nodes(3).await;
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn test_replication_result_quorum_achieved() {
        let result = ReplicationResult {
            success_count: 2,
            failure_count: 1,
            quorum_achieved: true,
            successful_nodes: vec![1, 2],
            failed_nodes: vec![(3, "timeout".to_string())],
        };

        assert!(result.quorum_achieved);
        assert_eq!(result.success_count, 2);
        assert_eq!(result.failure_count, 1);
    }

    #[tokio::test]
    async fn test_local_only_write_achieves_quorum_for_factor_1() {
        let config = QuorumConfig::with_replication_factor(1);
        let coord = ReplicationCoordinator::new(1, config);

        let result = coord
            .replicate_write("test/key", b"data".to_vec(), async { Ok(()) })
            .await
            .unwrap();

        assert!(result.quorum_achieved);
        assert_eq!(result.success_count, 1);
        assert!(result.successful_nodes.contains(&1));
    }

    #[tokio::test]
    async fn test_local_write_failure_aborts() {
        let config = QuorumConfig::with_replication_factor(1);
        let coord = ReplicationCoordinator::new(1, config);

        let result = coord
            .replicate_write("test/key", b"data".to_vec(), async {
                Err(StorageError::Io(std::io::Error::other("disk full")))
            })
            .await;

        assert!(result.is_err());
    }
}
