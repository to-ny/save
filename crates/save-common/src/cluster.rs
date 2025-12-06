//! Cluster-related utilities and types.

use crate::error::{Error, Result};

/// Parsed peer information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Node ID (must be > 0).
    pub node_id: u64,
    /// Host address.
    pub host: String,
    /// Port number.
    pub port: u16,
}

impl PeerInfo {
    /// Returns the full address as "http://host:port".
    pub fn http_addr(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Returns the address as "host:port".
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Parses a peer string in "node_id:host:port" format.
///
/// # Examples
/// ```
/// use save_common::cluster::parse_peer;
///
/// let peer = parse_peer("1:192.168.1.10:9001").unwrap();
/// assert_eq!(peer.node_id, 1);
/// assert_eq!(peer.host, "192.168.1.10");
/// assert_eq!(peer.port, 9001);
/// assert_eq!(peer.http_addr(), "http://192.168.1.10:9001");
/// ```
pub fn parse_peer(peer: &str) -> Result<PeerInfo> {
    let parts: Vec<&str> = peer.split(':').collect();
    if parts.len() != 3 {
        return Err(Error::validation(format!(
            "Invalid peer format '{}'. Expected 'node_id:host:port'",
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

    let port: u16 = parts[2].parse().map_err(|_| {
        Error::validation(format!(
            "Invalid port in peer '{}'. Expected numeric value 1-65535",
            peer
        ))
    })?;

    Ok(PeerInfo {
        node_id,
        host,
        port,
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
    fn test_parse_peer_valid() {
        let peer = parse_peer("1:192.168.1.10:9001").unwrap();
        assert_eq!(peer.node_id, 1);
        assert_eq!(peer.host, "192.168.1.10");
        assert_eq!(peer.port, 9001);
    }

    #[test]
    fn test_parse_peer_localhost() {
        let peer = parse_peer("2:localhost:8080").unwrap();
        assert_eq!(peer.node_id, 2);
        assert_eq!(peer.host, "localhost");
        assert_eq!(peer.port, 8080);
    }

    #[test]
    fn test_parse_peer_http_addr() {
        let peer = parse_peer("1:10.0.0.1:9001").unwrap();
        assert_eq!(peer.http_addr(), "http://10.0.0.1:9001");
    }

    #[test]
    fn test_parse_peer_addr() {
        let peer = parse_peer("1:10.0.0.1:9001").unwrap();
        assert_eq!(peer.addr(), "10.0.0.1:9001");
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
    fn test_parse_peer_invalid_node_id() {
        let result = parse_peer("abc:192.168.1.10:9001");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid node_id"));
    }

    #[test]
    fn test_parse_peer_invalid_port() {
        let result = parse_peer("1:192.168.1.10:invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid port"));
    }

    #[test]
    fn test_parse_peer_port_overflow() {
        let result = parse_peer("1:192.168.1.10:99999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_peers_valid() {
        let peers = vec![
            "1:192.168.1.10:9001".to_string(),
            "2:192.168.1.11:9001".to_string(),
        ];
        let result = parse_peers(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].node_id, 1);
        assert_eq!(result[1].node_id, 2);
    }

    #[test]
    fn test_parse_peers_one_invalid() {
        let peers = vec!["1:192.168.1.10:9001".to_string(), "invalid".to_string()];
        let result = parse_peers(&peers);
        assert!(result.is_err());
    }
}
