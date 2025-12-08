//! TLS utilities for mTLS node authentication.

use crate::config::TlsConfig;
use crate::error::{Error, Result};
use std::path::Path;
use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};

/// Load TLS configuration for a gRPC server (mTLS).
pub fn load_server_tls_config(config: &TlsConfig) -> Result<ServerTlsConfig> {
    let cert = load_file(&config.cert_path)?;
    let key = load_file(&config.key_path)?;
    let ca_cert = load_file(&config.ca_cert_path)?;

    let identity = Identity::from_pem(&cert, &key);
    let ca = Certificate::from_pem(&ca_cert);

    Ok(ServerTlsConfig::new().identity(identity).client_ca_root(ca))
}

/// Load TLS configuration for a gRPC client (mTLS).
pub fn load_client_tls_config(config: &TlsConfig) -> Result<ClientTlsConfig> {
    let cert = load_file(&config.cert_path)?;
    let key = load_file(&config.key_path)?;
    let ca_cert = load_file(&config.ca_cert_path)?;

    let identity = Identity::from_pem(&cert, &key);
    let ca = Certificate::from_pem(&ca_cert);

    Ok(ClientTlsConfig::new().identity(identity).ca_certificate(ca))
}

fn load_file(path: &str) -> Result<Vec<u8>> {
    std::fs::read(Path::new(path))
        .map_err(|e| Error::config(format!("Failed to read TLS file '{}': {}", path, e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_file_not_found() {
        let result = load_file("/nonexistent/path/cert.pem");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read TLS file")
        );
    }

    #[test]
    fn test_load_file_success() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"test content").unwrap();
        temp.flush().unwrap();

        let result = load_file(temp.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"test content");
    }

    #[test]
    fn test_tls_config_creation() {
        // Just verify the TlsConfig struct can be created
        let config = TlsConfig {
            cert_path: "/path/to/cert.pem".to_string(),
            key_path: "/path/to/key.pem".to_string(),
            ca_cert_path: "/path/to/ca.pem".to_string(),
        };
        assert_eq!(config.cert_path, "/path/to/cert.pem");
    }
}
