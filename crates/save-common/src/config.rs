use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub credentials: CredentialsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub bind_address: String,
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
}

fn default_max_body_size() -> usize {
    100 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageConfig {
    pub data_path: String,
    pub metadata_path: String,
    #[serde(default)]
    pub max_object_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialsConfig {
    pub access_key: String,
    pub secret_key: String,
}

impl SaveConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|e| {
            Error::config(format!("Failed to read config file {}: {}", path.display(), e))
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

        Ok(())
    }

    #[cfg(test)]
    pub fn test_default() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "127.0.0.1:9000".to_string(),
                max_body_size: default_max_body_size(),
            },
            storage: StorageConfig {
                data_path: "/tmp/save/data".to_string(),
                metadata_path: "/tmp/save/metadata".to_string(),
                max_object_size: Some(5 * 1024 * 1024 * 1024), // 5 GB
            },
            credentials: CredentialsConfig {
                access_key: "saveadmin".to_string(),
                secret_key: "savepass".to_string(),
            },
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
}
