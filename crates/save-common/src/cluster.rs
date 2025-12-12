//! Cluster-related utilities and types.

use crate::error::{Error, Result};

/// Parsed peer information containing both Raft and HTTP port information.
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
}

impl PeerInfo {
    /// Returns the Raft gRPC address as "http://host:raft_port".
    pub fn raft_addr(&self) -> String {
        format!("http://{}:{}", self.host, self.raft_port)
    }

    /// Returns the HTTP API address as "http://host:http_port".
    pub fn http_addr(&self) -> String {
        format!("http://{}:{}", self.host, self.http_port)
    }
}

/// Parses a peer string in "node_id:host:raft_port" or "node_id:host:raft_port:http_port" format.
///
/// If HTTP port is omitted, it defaults to 9000 for backward compatibility.
///
/// # Examples
/// ```
/// use save_common::cluster::parse_peer;
///
/// // New format with both ports
/// let peer = parse_peer("1:192.168.1.10:9001:9000").unwrap();
/// assert_eq!(peer.node_id, 1);
/// assert_eq!(peer.host, "192.168.1.10");
/// assert_eq!(peer.raft_port, 9001);
/// assert_eq!(peer.http_port, 9000);
/// assert_eq!(peer.raft_addr(), "http://192.168.1.10:9001");
/// assert_eq!(peer.http_addr(), "http://192.168.1.10:9000");
///
/// // Legacy format (HTTP port defaults to 9000)
/// let peer = parse_peer("2:192.168.1.11:9001").unwrap();
/// assert_eq!(peer.raft_port, 9001);
/// assert_eq!(peer.http_port, 9000);
/// ```
pub fn parse_peer(peer: &str) -> Result<PeerInfo> {
    let parts: Vec<&str> = peer.split(':').collect();
    if parts.len() < 3 || parts.len() > 4 {
        return Err(Error::validation(format!(
            "Invalid peer format '{}'. Expected 'node_id:host:raft_port' or 'node_id:host:raft_port:http_port'",
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

    let http_port: u16 = if parts.len() == 4 {
        parts[3].parse().map_err(|_| {
            Error::validation(format!(
                "Invalid http_port in peer '{}'. Expected numeric value 1-65535",
                peer
            ))
        })?
    } else {
        9000 // Default HTTP port for backward compatibility
    };

    Ok(PeerInfo {
        node_id,
        host,
        raft_port,
        http_port,
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
    fn test_parse_peer_new_format() {
        let peer = parse_peer("1:192.168.1.10:9001:9000").unwrap();
        assert_eq!(peer.node_id, 1);
        assert_eq!(peer.host, "192.168.1.10");
        assert_eq!(peer.raft_port, 9001);
        assert_eq!(peer.http_port, 9000);
    }

    #[test]
    fn test_parse_peer_legacy_format() {
        let peer = parse_peer("1:192.168.1.10:9001").unwrap();
        assert_eq!(peer.node_id, 1);
        assert_eq!(peer.host, "192.168.1.10");
        assert_eq!(peer.raft_port, 9001);
        assert_eq!(peer.http_port, 9000); // Default
    }

    #[test]
    fn test_parse_peer_localhost() {
        let peer = parse_peer("2:localhost:8080:8000").unwrap();
        assert_eq!(peer.node_id, 2);
        assert_eq!(peer.host, "localhost");
        assert_eq!(peer.raft_port, 8080);
        assert_eq!(peer.http_port, 8000);
    }

    #[test]
    fn test_parse_peer_raft_addr() {
        let peer = parse_peer("1:10.0.0.1:9001:9000").unwrap();
        assert_eq!(peer.raft_addr(), "http://10.0.0.1:9001");
    }

    #[test]
    fn test_parse_peer_http_addr() {
        let peer = parse_peer("1:10.0.0.1:9001:9000").unwrap();
        assert_eq!(peer.http_addr(), "http://10.0.0.1:9000");
    }

    #[test]
    fn test_parse_peer_different_ports() {
        let peer = parse_peer("3:myhost:5001:5000").unwrap();
        assert_eq!(peer.raft_addr(), "http://myhost:5001");
        assert_eq!(peer.http_addr(), "http://myhost:5000");
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
        let result = parse_peer("1:host:9001:9000:extra");
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
        let result = parse_peer("abc:192.168.1.10:9001");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid node_id"));
    }

    #[test]
    fn test_parse_peer_invalid_raft_port() {
        let result = parse_peer("1:192.168.1.10:invalid");
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
        let result = parse_peer("1:192.168.1.10:9001:invalid");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid http_port")
        );
    }

    #[test]
    fn test_parse_peer_raft_port_overflow() {
        let result = parse_peer("1:192.168.1.10:99999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peer_http_port_overflow() {
        let result = parse_peer("1:192.168.1.10:9001:99999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peers_new_format() {
        let peers = vec![
            "1:192.168.1.10:9001:9000".to_string(),
            "2:192.168.1.11:9001:9000".to_string(),
        ];
        let result = parse_peers(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].node_id, 1);
        assert_eq!(result[0].raft_port, 9001);
        assert_eq!(result[0].http_port, 9000);
        assert_eq!(result[1].node_id, 2);
    }

    #[test]
    fn test_parse_peers_legacy_format() {
        let peers = vec![
            "1:192.168.1.10:9001".to_string(),
            "2:192.168.1.11:9001".to_string(),
        ];
        let result = parse_peers(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].http_port, 9000); // Default
        assert_eq!(result[1].http_port, 9000); // Default
    }

    #[test]
    fn test_parse_peers_one_invalid() {
        let peers = vec!["1:192.168.1.10:9001".to_string(), "invalid".to_string()];
        let result = parse_peers(&peers);
        assert!(result.is_err());
    }
}
