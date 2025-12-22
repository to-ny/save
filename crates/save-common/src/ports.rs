//! Default port constants for Save services.

/// Default HTTP/S3 API port.
pub const DEFAULT_HTTP: u16 = 9000;

/// Default Raft consensus gRPC port.
pub const DEFAULT_RAFT: u16 = 9001;

/// Default replication gRPC port.
pub const DEFAULT_REPLICATION: u16 = 9002;

/// Default internal cluster API gRPC port.
pub const DEFAULT_INTERNAL_API: u16 = 9082;

/// Default bind addresses (all interfaces).
pub const DEFAULT_HTTP_BIND: &str = "0.0.0.0:9000";
pub const DEFAULT_RAFT_BIND: &str = "0.0.0.0:9001";
pub const DEFAULT_REPLICATION_BIND: &str = "0.0.0.0:9002";
pub const DEFAULT_INTERNAL_API_BIND: &str = "0.0.0.0:9082";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_distinct() {
        assert_ne!(DEFAULT_HTTP, DEFAULT_RAFT);
        assert_ne!(DEFAULT_HTTP, DEFAULT_REPLICATION);
        assert_ne!(DEFAULT_HTTP, DEFAULT_INTERNAL_API);
        assert_ne!(DEFAULT_RAFT, DEFAULT_REPLICATION);
        assert_ne!(DEFAULT_RAFT, DEFAULT_INTERNAL_API);
        assert_ne!(DEFAULT_REPLICATION, DEFAULT_INTERNAL_API);
    }

    #[test]
    fn bind_addresses_match_ports() {
        assert!(DEFAULT_HTTP_BIND.ends_with(&DEFAULT_HTTP.to_string()));
        assert!(DEFAULT_RAFT_BIND.ends_with(&DEFAULT_RAFT.to_string()));
        assert!(DEFAULT_REPLICATION_BIND.ends_with(&DEFAULT_REPLICATION.to_string()));
        assert!(DEFAULT_INTERNAL_API_BIND.ends_with(&DEFAULT_INTERNAL_API.to_string()));
    }
}
