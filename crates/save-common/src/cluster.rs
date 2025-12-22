//! Cluster-related utilities and types.

use crate::error::{Error, Result};

/// Parsed peer information containing Raft, HTTP, and replication port information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Node ID (must be > 0).
    pub node_id: u64,
    /// Host address.
    pub host: String,
    /// Raft gRPC port number.
    pub raft_port: u16,
    /// HTTP API port number.
    pub http_port: u16,
    /// Replication gRPC port number.
    pub replication_port: u16,
}

impl PeerInfo {
    /// Returns the Raft gRPC address as "http://host:raft_port".
    #[must_use]
    pub fn raft_addr(&self) -> String {
        format!("http://{}:{}", self.host, self.raft_port)
    }

    /// Returns the HTTP API address as "http://host:http_port".
    #[must_use]
    pub fn http_addr(&self) -> String {
        format!("http://{}:{}", self.host, self.http_port)
    }

    /// Returns the Replication gRPC address as "http://host:replication_port".
    #[must_use]
    pub fn replication_addr(&self) -> String {
        format!("http://{}:{}", self.host, self.replication_port)
    }

    /// Converts to tuple format for legacy API compatibility.
    #[must_use]
    pub fn to_tuple(&self) -> (u64, String, String, String) {
        (
            self.node_id,
            self.raft_addr(),
            self.http_addr(),
            self.replication_addr(),
        )
    }

    /// Creates a PeerInfo from bind addresses (e.g., "0.0.0.0:9001").
    ///
    /// Parses each address to extract host and port. The host is taken from
    /// the raft bind address; HTTP and replication ports are extracted but
    /// their hosts are assumed to match (common in single-node configurations).
    ///
    /// Returns an error if any address is not in "host:port" format.
    pub fn from_bind_addrs(
        node_id: u64,
        raft_bind: &str,
        http_bind: &str,
        replication_bind: &str,
    ) -> Result<Self> {
        let (host, raft_port) = parse_bind_addr(raft_bind)?;
        let (_, http_port) = parse_bind_addr(http_bind)?;
        let (_, replication_port) = parse_bind_addr(replication_bind)?;

        Ok(Self {
            node_id,
            host,
            raft_port,
            http_port,
            replication_port,
        })
    }
}

/// Parses a bind address in "host:port" format.
///
/// Returns an error if the address is not in the expected format or if the port is invalid.
fn parse_bind_addr(addr: &str) -> Result<(String, u16)> {
    let (host, port_str) = addr.rsplit_once(':').ok_or_else(|| {
        Error::validation(format!(
            "Invalid bind address '{}'. Expected 'host:port' format",
            addr
        ))
    })?;

    let port: u16 = port_str.parse().map_err(|_| {
        Error::validation(format!(
            "Invalid port in bind address '{}'. Expected numeric value 1-65535",
            addr
        ))
    })?;

    Ok((host.to_string(), port))
}

/// Parses a peer string in the format:
/// "node_id:host:raft_port:http_port:replication_port"
///
/// All 5 parts are required; no defaults are applied.
///
/// # Examples
/// ```
/// use save_common::cluster::parse_peer;
///
/// let peer = parse_peer("1:192.168.1.10:9001:9000:9002").unwrap();
/// assert_eq!(peer.node_id, 1);
/// assert_eq!(peer.host, "192.168.1.10");
/// assert_eq!(peer.raft_port, 9001);
/// assert_eq!(peer.http_port, 9000);
/// assert_eq!(peer.replication_port, 9002);
/// ```
pub fn parse_peer(peer: &str) -> Result<PeerInfo> {
    let parts: Vec<&str> = peer.split(':').collect();
    if parts.len() != 5 {
        return Err(Error::validation(format!(
            "Invalid peer format '{}'. Expected 'node_id:host:raft_port:http_port:replication_port'",
            peer
        )));
    }

    let node_id: u64 = parts[0].parse().map_err(|_| {
        Error::validation(format!(
            "Invalid node_id in peer '{}'. Expected numeric value",
            peer
        ))
    })?;

    let host = parts[1].to_string();

    let raft_port: u16 = parts[2].parse().map_err(|_| {
        Error::validation(format!(
            "Invalid raft_port in peer '{}'. Expected numeric value 1-65535",
            peer
        ))
    })?;

    let http_port: u16 = parts[3].parse().map_err(|_| {
        Error::validation(format!(
            "Invalid http_port in peer '{}'. Expected numeric value 1-65535",
            peer
        ))
    })?;

    let replication_port: u16 = parts[4].parse().map_err(|_| {
        Error::validation(format!(
            "Invalid replication_port in peer '{}'. Expected numeric value 1-65535",
            peer
        ))
    })?;

    Ok(PeerInfo {
        node_id,
        host,
        raft_port,
        http_port,
        replication_port,
    })
}

/// Parses multiple peer strings.
pub fn parse_peers(peers: &[String]) -> Result<Vec<PeerInfo>> {
    peers.iter().map(|p| parse_peer(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_peer_full_format() {
        let peer = parse_peer("1:192.168.1.10:9001:9000:9002").unwrap();
        assert_eq!(peer.node_id, 1);
        assert_eq!(peer.host, "192.168.1.10");
        assert_eq!(peer.raft_port, 9001);
        assert_eq!(peer.http_port, 9000);
        assert_eq!(peer.replication_port, 9002);
    }

    #[test]
    fn test_parse_peer_incomplete_format_rejected() {
        // 4-part format should be rejected (no defaults)
        let result = parse_peer("1:192.168.1.10:9001:9000");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid peer format")
        );

        // 3-part format should be rejected (no defaults)
        let result = parse_peer("1:192.168.1.10:9001");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid peer format")
        );
    }

    #[test]
    fn test_parse_peer_localhost() {
        let peer = parse_peer("2:localhost:8080:8000:8002").unwrap();
        assert_eq!(peer.node_id, 2);
        assert_eq!(peer.host, "localhost");
        assert_eq!(peer.raft_port, 8080);
        assert_eq!(peer.http_port, 8000);
        assert_eq!(peer.replication_port, 8002);
    }

    #[test]
    fn test_parse_peer_raft_addr() {
        let peer = parse_peer("1:10.0.0.1:9001:9000:9002").unwrap();
        assert_eq!(peer.raft_addr(), "http://10.0.0.1:9001");
    }

    #[test]
    fn test_parse_peer_http_addr() {
        let peer = parse_peer("1:10.0.0.1:9001:9000:9002").unwrap();
        assert_eq!(peer.http_addr(), "http://10.0.0.1:9000");
    }

    #[test]
    fn test_parse_peer_replication_addr() {
        let peer = parse_peer("1:10.0.0.1:9001:9000:9002").unwrap();
        assert_eq!(peer.replication_addr(), "http://10.0.0.1:9002");
    }

    #[test]
    fn test_parse_peer_different_ports() {
        let peer = parse_peer("3:myhost:5001:5000:5002").unwrap();
        assert_eq!(peer.raft_addr(), "http://myhost:5001");
        assert_eq!(peer.http_addr(), "http://myhost:5000");
        assert_eq!(peer.replication_addr(), "http://myhost:5002");
    }

    #[test]
    fn test_parse_peer_invalid_format() {
        let result = parse_peer("invalid");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid peer format")
        );
    }

    #[test]
    fn test_parse_peer_too_many_parts() {
        let result = parse_peer("1:host:9001:9000:9002:extra");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid peer format")
        );
    }

    #[test]
    fn test_parse_peer_invalid_node_id() {
        let result = parse_peer("abc:192.168.1.10:9001:9000:9002");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid node_id"));
    }

    #[test]
    fn test_parse_peer_invalid_raft_port() {
        let result = parse_peer("1:192.168.1.10:invalid:9000:9002");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid raft_port")
        );
    }

    #[test]
    fn test_parse_peer_invalid_http_port() {
        let result = parse_peer("1:192.168.1.10:9001:invalid:9002");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid http_port")
        );
    }

    #[test]
    fn test_parse_peer_invalid_replication_port() {
        let result = parse_peer("1:192.168.1.10:9001:9000:invalid");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid replication_port")
        );
    }

    #[test]
    fn test_parse_peer_raft_port_overflow() {
        let result = parse_peer("1:192.168.1.10:99999:9000:9002");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peer_http_port_overflow() {
        let result = parse_peer("1:192.168.1.10:9001:99999:9002");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peer_replication_port_overflow() {
        let result = parse_peer("1:192.168.1.10:9001:9000:99999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peers_full_format() {
        let peers = vec![
            "1:192.168.1.10:9001:9000:9002".to_string(),
            "2:192.168.1.11:9001:9000:9002".to_string(),
        ];
        let result = parse_peers(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].node_id, 1);
        assert_eq!(result[0].raft_port, 9001);
        assert_eq!(result[0].http_port, 9000);
        assert_eq!(result[0].replication_port, 9002);
        assert_eq!(result[1].node_id, 2);
    }

    #[test]
    fn test_parse_peers_incomplete_format_rejected() {
        // Incomplete formats should be rejected
        let peers = vec![
            "1:192.168.1.10:9001".to_string(),
            "2:192.168.1.11:9001".to_string(),
        ];
        let result = parse_peers(&peers);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peers_one_invalid() {
        let peers = vec![
            "1:192.168.1.10:9001:9000:9002".to_string(),
            "invalid".to_string(),
        ];
        let result = parse_peers(&peers);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_bind_addrs() {
        let peer =
            PeerInfo::from_bind_addrs(1, "0.0.0.0:9001", "0.0.0.0:9000", "0.0.0.0:9002").unwrap();
        assert_eq!(peer.node_id, 1);
        assert_eq!(peer.host, "0.0.0.0");
        assert_eq!(peer.raft_port, 9001);
        assert_eq!(peer.http_port, 9000);
        assert_eq!(peer.replication_port, 9002);
    }

    #[test]
    fn test_from_bind_addrs_different_ports() {
        let peer =
            PeerInfo::from_bind_addrs(42, "127.0.0.1:5001", "127.0.0.1:5000", "127.0.0.1:5002")
                .unwrap();
        assert_eq!(peer.node_id, 42);
        assert_eq!(peer.host, "127.0.0.1");
        assert_eq!(peer.raft_port, 5001);
        assert_eq!(peer.http_port, 5000);
        assert_eq!(peer.replication_port, 5002);
        assert_eq!(peer.raft_addr(), "http://127.0.0.1:5001");
        assert_eq!(peer.http_addr(), "http://127.0.0.1:5000");
        assert_eq!(peer.replication_addr(), "http://127.0.0.1:5002");
    }

    #[test]
    fn test_from_bind_addrs_missing_port_rejected() {
        // Address without port should be rejected
        let result = PeerInfo::from_bind_addrs(1, "0.0.0.0", "0.0.0.0:9000", "0.0.0.0:9002");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid bind address")
        );
    }

    #[test]
    fn test_from_bind_addrs_invalid_port_rejected() {
        let result =
            PeerInfo::from_bind_addrs(1, "0.0.0.0:invalid", "0.0.0.0:9000", "0.0.0.0:9002");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid port"));
    }
}
