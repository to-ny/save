use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub metadata: MetadataConfig,
    pub credentials: CredentialsConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub shutdown: ShutdownConfig,
    #[serde(default)]
    pub cluster: ClusterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub bind_address: String,
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
    /// Tokio runtime worker threads (default: 4)
    /// Should generally match CPU core count for best performance
    #[serde(default = "default_worker_threads")]
    pub worker_threads: usize,
    /// Tokio runtime max blocking threads (default: 512)
    /// Used for spawn_blocking tasks (RocksDB operations, fsync, etc.)
    /// Increase if you see "blocking operation took too long" warnings
    #[serde(default = "default_max_blocking_threads")]
    pub max_blocking_threads: usize,
    /// Bucket existence cache TTL in seconds (default: 60)
    /// Caches bucket existence checks to avoid repeated DB lookups.
    /// Lower values = fresher data but more DB load
    /// Higher values = less DB load but stale cache after bucket deletion
    #[serde(default = "default_bucket_cache_ttl_secs")]
    pub bucket_cache_ttl_secs: u64,
    /// Metrics collection interval in seconds (default: 60)
    #[serde(default = "default_metrics_interval_secs")]
    pub metrics_interval_secs: u64,
}

fn default_max_body_size() -> usize {
    100 * 1024 * 1024
}

fn default_worker_threads() -> usize {
    4
}

fn default_max_blocking_threads() -> usize {
    512
}

fn default_bucket_cache_ttl_secs() -> u64 {
    60
}

fn default_metrics_interval_secs() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageConfig {
    pub data_path: String,
    pub metadata_path: String,
    #[serde(default)]
    pub max_object_size: Option<u64>,
    #[serde(default = "default_gc_interval_secs")]
    pub gc_interval_secs: u64,
    #[serde(default = "default_gc_temp_file_max_age_secs")]
    pub gc_temp_file_max_age_secs: u64,
    /// Fsync mode: "full", "data" (default), or "none"
    #[serde(default = "default_fsync_mode")]
    pub fsync_mode: String,
}

fn default_gc_interval_secs() -> u64 {
    10 * 60 // 10 minutes
}

fn default_gc_temp_file_max_age_secs() -> u64 {
    60 * 60 // 1 hour
}

fn default_fsync_mode() -> String {
    "data".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetadataConfig {
    /// RocksDB write buffer size in MB (default: 128)
    /// Larger values reduce compaction frequency but use more memory.
    /// Total write buffer memory = write_buffer_size_mb × max_write_buffer_number
    #[serde(default = "default_write_buffer_size_mb")]
    pub write_buffer_size_mb: usize,

    /// Maximum number of write buffers (default: 4)
    /// Total write buffer memory = write_buffer_size_mb × max_write_buffer_number
    #[serde(default = "default_max_write_buffer_number")]
    pub max_write_buffer_number: i32,

    /// RocksDB block cache size in MB (default: 256)
    /// Critical for read performance. Should be ~25% of available RAM for dedicated servers.
    /// For shared deployments, adjust based on available memory.
    #[serde(default = "default_block_cache_size_mb")]
    pub block_cache_size_mb: usize,

    /// Maximum background jobs for compaction/flush (default: 4)
    /// Generally should match CPU core count. Higher values increase write throughput
    /// but consume more CPU during compactions.
    #[serde(default = "default_max_background_jobs")]
    pub max_background_jobs: i32,
}

fn default_write_buffer_size_mb() -> usize {
    128
}

fn default_max_write_buffer_number() -> i32 {
    4
}

fn default_block_cache_size_mb() -> usize {
    256
}

fn default_max_background_jobs() -> i32 {
    4
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            write_buffer_size_mb: default_write_buffer_size_mb(),
            max_write_buffer_number: default_max_write_buffer_number(),
            block_cache_size_mb: default_block_cache_size_mb(),
            max_background_jobs: default_max_background_jobs(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialsConfig {
    pub access_key: String,
    pub secret_key: String,
}

impl std::fmt::Debug for CredentialsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialsConfig")
            .field("access_key", &"[REDACTED]")
            .field("secret_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LimitsConfig {
    #[serde(default = "default_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
    #[serde(default = "default_requests_per_second")]
    pub requests_per_second: u64,
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    // TODO Phase 1 validates but doesn't enforce these limits
    //  Phase 2 will wire up Tower middleware for actual enforcement
}

fn default_max_concurrent_requests() -> usize {
    1000
}

fn default_requests_per_second() -> u64 {
    100
}

fn default_request_timeout_secs() -> u64 {
    300
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: default_max_concurrent_requests(),
            requests_per_second: default_requests_per_second(),
            request_timeout_secs: default_request_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShutdownConfig {
    #[serde(default = "default_drain_timeout_secs")]
    pub drain_timeout_secs: u64,
}

fn default_drain_timeout_secs() -> u64 {
    30
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            drain_timeout_secs: default_drain_timeout_secs(),
        }
    }
}

/// Consistency mode for cluster reads
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConsistencyMode {
    /// Read from Raft leader (linearizable, higher latency)
    #[default]
    Strong,
    /// Read from local RocksDB (may be stale, lower latency)
    Eventual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterConfig {
    /// Unique node ID in the cluster
    /// Must be > 0 and unique across all nodes in the cluster
    #[serde(default = "default_node_id")]
    pub node_id: u64,

    /// Raft gRPC bind address (e.g., "0.0.0.0:9001")
    #[serde(default = "default_raft_bind_addr")]
    pub raft_bind_addr: String,

    /// Seed nodes for initial cluster discovery (not the actual membership).
    /// Format: "node_id:host:raft_port:http_port". Empty = standalone cluster.
    #[serde(default)]
    pub seed_nodes: Vec<String>,

    /// Consistency mode for read operations
    #[serde(default)]
    pub consistency_mode: ConsistencyMode,

    /// Connection timeout for gRPC channels in seconds (default: 5)
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,

    /// RPC timeout for individual requests in seconds (default: 30)
    #[serde(default = "default_rpc_timeout_secs")]
    pub rpc_timeout_secs: u64,

    /// Raft heartbeat interval in milliseconds (default: 50)
    #[serde(default = "default_heartbeat_interval_ms")]
    pub heartbeat_interval_ms: u64,

    /// Minimum Raft election timeout in milliseconds (default: 150)
    #[serde(default = "default_election_timeout_min_ms")]
    pub election_timeout_min_ms: u64,

    /// Maximum Raft election timeout in milliseconds (default: 300)
    #[serde(default = "default_election_timeout_max_ms")]
    pub election_timeout_max_ms: u64,

    /// Timeout for cluster join on startup in seconds (default: 30)
    #[serde(default = "default_join_timeout_secs")]
    pub join_timeout_secs: u64,

    /// Timeout for graceful leave on shutdown in seconds (default: 30)
    #[serde(default = "default_leave_timeout_secs")]
    pub leave_timeout_secs: u64,

    /// Delay before promoting learner to voter in milliseconds (default: 200).
    /// Allows time for the learner to catch up with the Raft log.
    #[serde(default = "default_learner_catchup_delay_ms")]
    pub learner_catchup_delay_ms: u64,

    /// Maximum retry attempts for cluster join operations (default: 3)
    #[serde(default = "default_join_max_retries")]
    pub join_max_retries: u32,

    /// Replication configuration
    #[serde(default)]
    pub replication: ReplicationConfig,

    /// Internal API configuration for cluster management
    #[serde(default)]
    pub internal_api: InternalApiConfig,
}

/// Replication configuration for data redundancy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicationConfig {
    /// Number of copies to maintain (including local). Default: 1 (no replication)
    /// Set to 3 for typical production clusters.
    #[serde(default = "default_replication_factor")]
    pub replication_factor: usize,

    /// gRPC bind address for replication service (e.g., "0.0.0.0:9002")
    /// If empty, replication service is disabled.
    #[serde(default)]
    pub bind_addr: String,

    /// TLS configuration for secure node-to-node communication.
    #[serde(default)]
    pub tls: Option<TlsConfig>,

    /// Retry configuration for transient failures.
    #[serde(default)]
    pub retry: RetrySettings,
}

/// Internal API configuration for cluster management operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InternalApiConfig {
    /// gRPC bind address for internal cluster API (e.g., "0.0.0.0:9082")
    /// If empty, internal API is disabled.
    #[serde(default)]
    pub bind_addr: String,

    /// TLS configuration for secure admin communication.
    /// If not set, uses the replication TLS config if available.
    #[serde(default)]
    pub tls: Option<TlsConfig>,

    /// Require mTLS authentication for all requests.
    #[serde(default = "default_require_auth")]
    pub require_auth: bool,
}

fn default_require_auth() -> bool {
    true
}

impl Default for InternalApiConfig {
    fn default() -> Self {
        Self {
            bind_addr: String::new(),
            tls: None,
            require_auth: default_require_auth(),
        }
    }
}

/// Retry settings for replication operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetrySettings {
    /// Maximum number of retry attempts (0 = no retries). Default: 3.
    #[serde(default = "default_retry_max_attempts")]
    pub max_attempts: u32,

    /// Initial delay before first retry in milliseconds. Default: 100.
    #[serde(default = "default_retry_initial_delay_ms")]
    pub initial_delay_ms: u64,

    /// Maximum delay between retries in milliseconds. Default: 5000.
    #[serde(default = "default_retry_max_delay_ms")]
    pub max_delay_ms: u64,

    /// Jitter factor (0.0 to 1.0) to prevent thundering herd. Default: 0.2.
    #[serde(default = "default_retry_jitter")]
    pub jitter: f64,
}

fn default_retry_max_attempts() -> u32 {
    3
}

fn default_retry_initial_delay_ms() -> u64 {
    100
}

fn default_retry_max_delay_ms() -> u64 {
    5000
}

fn default_retry_jitter() -> f64 {
    0.2
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            max_attempts: default_retry_max_attempts(),
            initial_delay_ms: default_retry_initial_delay_ms(),
            max_delay_ms: default_retry_max_delay_ms(),
            jitter: default_retry_jitter(),
        }
    }
}

/// TLS configuration for mTLS node authentication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsConfig {
    /// Path to the server certificate (PEM format).
    pub cert_path: String,

    /// Path to the server private key (PEM format).
    pub key_path: String,

    /// Path to the CA certificate for verifying client certificates (PEM format).
    pub ca_cert_path: String,
}

fn default_replication_factor() -> usize {
    1
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            replication_factor: default_replication_factor(),
            bind_addr: "0.0.0.0:9002".to_string(),
            tls: None,
            retry: RetrySettings::default(),
        }
    }
}

impl ReplicationConfig {
    /// Returns true if replication is enabled (factor > 1)
    pub fn is_enabled(&self) -> bool {
        self.replication_factor > 1
    }

    /// Returns the replication address as a full URL (e.g., "http://0.0.0.0:9002").
    pub fn replication_addr(&self) -> String {
        format!("http://{}", self.bind_addr)
    }
}

fn default_connect_timeout_secs() -> u64 {
    5
}

fn default_rpc_timeout_secs() -> u64 {
    30
}

fn default_heartbeat_interval_ms() -> u64 {
    50
}

fn default_election_timeout_min_ms() -> u64 {
    150
}

fn default_election_timeout_max_ms() -> u64 {
    300
}

fn default_join_timeout_secs() -> u64 {
    30
}

fn default_leave_timeout_secs() -> u64 {
    30
}

fn default_learner_catchup_delay_ms() -> u64 {
    200
}

fn default_join_max_retries() -> u32 {
    3
}

fn default_node_id() -> u64 {
    1
}

fn default_raft_bind_addr() -> String {
    "0.0.0.0:9001".to_string()
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            node_id: default_node_id(),
            raft_bind_addr: default_raft_bind_addr(),
            seed_nodes: vec![],
            consistency_mode: ConsistencyMode::default(),
            connect_timeout_secs: default_connect_timeout_secs(),
            rpc_timeout_secs: default_rpc_timeout_secs(),
            heartbeat_interval_ms: default_heartbeat_interval_ms(),
            election_timeout_min_ms: default_election_timeout_min_ms(),
            election_timeout_max_ms: default_election_timeout_max_ms(),
            join_timeout_secs: default_join_timeout_secs(),
            leave_timeout_secs: default_leave_timeout_secs(),
            learner_catchup_delay_ms: default_learner_catchup_delay_ms(),
            join_max_retries: default_join_max_retries(),
            replication: ReplicationConfig::default(),
            internal_api: InternalApiConfig::default(),
        }
    }
}

impl SaveConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|e| {
            Error::config(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;

        let config: SaveConfig = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.server.bind_address.is_empty() {
            return Err(Error::validation("bind_address cannot be empty"));
        }

        if self.storage.data_path.is_empty() {
            return Err(Error::validation("data_path cannot be empty"));
        }

        if self.storage.metadata_path.is_empty() {
            return Err(Error::validation("metadata_path cannot be empty"));
        }

        if self.credentials.access_key.is_empty() {
            return Err(Error::validation("access_key cannot be empty"));
        }

        if self.credentials.secret_key.is_empty() {
            return Err(Error::validation("secret_key cannot be empty"));
        }

        if self.limits.max_concurrent_requests == 0 {
            return Err(Error::validation("max_concurrent_requests must be > 0"));
        }

        if self.limits.requests_per_second == 0 {
            return Err(Error::validation("requests_per_second must be > 0"));
        }

        if self.limits.request_timeout_secs == 0 {
            return Err(Error::validation("request_timeout_secs must be > 0"));
        }

        if self.shutdown.drain_timeout_secs == 0 {
            return Err(Error::validation("drain_timeout_secs must be > 0"));
        }

        // Cluster validation (always required)
        if self.cluster.node_id == 0 {
            return Err(Error::validation("cluster.node_id must be > 0"));
        }

        if self.cluster.raft_bind_addr.is_empty() {
            return Err(Error::validation("cluster.raft_bind_addr cannot be empty"));
        }

        // Validate peer format using shared utility
        for peer in &self.cluster.seed_nodes {
            crate::cluster::parse_peer(peer)?;
        }

        // Replication bind address is always required (cluster must be ready for replication)
        if self.cluster.replication.bind_addr.is_empty() {
            return Err(Error::validation(
                "cluster.replication.bind_addr cannot be empty",
            ));
        }

        Ok(())
    }

    #[cfg(test)]
    pub fn test_default() -> Self {
        Self::default()
    }
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "0.0.0.0:9000".to_string(),
                max_body_size: default_max_body_size(),
                worker_threads: default_worker_threads(),
                max_blocking_threads: default_max_blocking_threads(),
                bucket_cache_ttl_secs: default_bucket_cache_ttl_secs(),
                metrics_interval_secs: default_metrics_interval_secs(),
            },
            storage: StorageConfig {
                data_path: "/tmp/save/data".to_string(),
                metadata_path: "/tmp/save/metadata".to_string(),
                max_object_size: Some(5 * 1024 * 1024 * 1024),
                gc_interval_secs: default_gc_interval_secs(),
                gc_temp_file_max_age_secs: default_gc_temp_file_max_age_secs(),
                fsync_mode: default_fsync_mode(),
            },
            metadata: MetadataConfig::default(),
            credentials: CredentialsConfig {
                access_key: "test-access-key".to_string(),
                secret_key: "test-secret-key".to_string(),
            },
            limits: LimitsConfig::default(),
            shutdown: ShutdownConfig::default(),
            cluster: ClusterConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_load_from_toml() {
        let toml_content = r#"
[server]
bind_address = "0.0.0.0:9000"
max_body_size = 52428800

[storage]
data_path = "/var/lib/save/data"
metadata_path = "/var/lib/save/metadata"
max_object_size = 5368709120

[credentials]
access_key = "admin"
secret_key = "secret123"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = SaveConfig::load(temp_file.path()).unwrap();

        assert_eq!(config.server.bind_address, "0.0.0.0:9000");
        assert_eq!(config.server.max_body_size, 52428800);
        assert_eq!(config.storage.data_path, "/var/lib/save/data");
        assert_eq!(config.storage.metadata_path, "/var/lib/save/metadata");
        assert_eq!(config.storage.max_object_size, Some(5368709120));
        assert_eq!(config.credentials.access_key, "admin");
        assert_eq!(config.credentials.secret_key, "secret123");
    }

    #[test]
    fn test_config_validation_empty_bind_address() {
        let mut config = SaveConfig::test_default();
        config.server.bind_address = String::new();
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_empty_data_path() {
        let mut config = SaveConfig::test_default();
        config.storage.data_path = String::new();
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = SaveConfig::test_default();
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: SaveConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_config_validation_zero_max_concurrent_requests() {
        let mut config = SaveConfig::test_default();
        config.limits.max_concurrent_requests = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("max_concurrent_requests")
        );
    }

    #[test]
    fn test_config_validation_zero_requests_per_second() {
        let mut config = SaveConfig::test_default();
        config.limits.requests_per_second = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requests_per_second")
        );
    }

    #[test]
    fn test_config_validation_zero_request_timeout() {
        let mut config = SaveConfig::test_default();
        config.limits.request_timeout_secs = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("request_timeout_secs")
        );
    }

    #[test]
    fn test_config_validation_zero_drain_timeout() {
        let mut config = SaveConfig::test_default();
        config.shutdown.drain_timeout_secs = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("drain_timeout_secs")
        );
    }

    #[test]
    fn test_cluster_config_defaults() {
        let config = SaveConfig::test_default();
        assert_eq!(config.cluster.node_id, 1);
        assert_eq!(config.cluster.raft_bind_addr, "0.0.0.0:9001");
        assert!(config.cluster.seed_nodes.is_empty());
        assert_eq!(config.cluster.consistency_mode, ConsistencyMode::Strong);
    }

    #[test]
    fn test_cluster_config_validation_zero_node_id() {
        let mut config = SaveConfig::test_default();
        config.cluster.node_id = 0; // Invalid
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("node_id"));
    }

    #[test]
    fn test_cluster_config_validation_empty_raft_addr() {
        let mut config = SaveConfig::test_default();
        config.cluster.raft_bind_addr = String::new();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("raft_bind_addr"));
    }

    #[test]
    fn test_cluster_config_validation_invalid_peer_format() {
        let mut config = SaveConfig::test_default();
        config.cluster.seed_nodes = vec!["invalid-format".to_string()];
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid peer format")
        );
    }

    #[test]
    fn test_cluster_config_validation_invalid_peer_node_id() {
        let mut config = SaveConfig::test_default();
        config.cluster.seed_nodes = vec!["abc:192.168.1.10:9001".to_string()];
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid node_id"));
    }

    #[test]
    fn test_cluster_config_validation_invalid_peer_port() {
        let mut config = SaveConfig::test_default();
        config.cluster.seed_nodes = vec!["2:192.168.1.10:99999".to_string()];
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid raft_port")
        );
    }

    #[test]
    fn test_cluster_config_validation_valid_single_node() {
        let config = SaveConfig::test_default();
        // Single-node cluster with no peers should validate
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_cluster_config_validation_valid_multi_node() {
        let mut config = SaveConfig::test_default();
        config.cluster.seed_nodes = vec![
            "2:192.168.1.11:9001".to_string(),
            "3:192.168.1.12:9001".to_string(),
        ];
        config.cluster.consistency_mode = ConsistencyMode::Eventual;
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_cluster_config_serialization() {
        let mut config = SaveConfig::test_default();
        config.cluster.seed_nodes = vec!["2:192.168.1.11:9001".to_string()];
        config.cluster.consistency_mode = ConsistencyMode::Eventual;

        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: SaveConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_consistency_mode_serialization() {
        let strong = ConsistencyMode::Strong;
        let eventual = ConsistencyMode::Eventual;

        let strong_json = serde_json::to_string(&strong).unwrap();
        let eventual_json = serde_json::to_string(&eventual).unwrap();

        assert_eq!(strong_json, "\"strong\"");
        assert_eq!(eventual_json, "\"eventual\"");

        let strong_de: ConsistencyMode = serde_json::from_str(&strong_json).unwrap();
        let eventual_de: ConsistencyMode = serde_json::from_str(&eventual_json).unwrap();

        assert_eq!(strong_de, ConsistencyMode::Strong);
        assert_eq!(eventual_de, ConsistencyMode::Eventual);
    }
}
