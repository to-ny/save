//! Configuration for the traffic simulator.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub target: TargetConfig,
    pub simulation: SimulationConfig,
    pub objects: ObjectConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetConfig {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_region() -> String {
    "us-east-1".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimulationConfig {
    #[serde(default)]
    pub pattern: PatternConfig,
    #[serde(default = "default_virtual_users")]
    pub virtual_users: u32,
    #[serde(default = "default_read_ratio")]
    pub read_ratio: f64,
    #[serde(default = "default_write_ratio")]
    pub write_ratio: f64,
    #[serde(default = "default_rps")]
    pub requests_per_second: f64,
}

fn default_virtual_users() -> u32 {
    10
}

fn default_read_ratio() -> f64 {
    0.7
}

fn default_write_ratio() -> f64 {
    0.3
}

fn default_rps() -> f64 {
    5.0
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternConfig {
    Baseline {
        requests_per_second: f64,
    },
    BusinessHours {
        peak_rps: f64,
        duration_secs: u64,
    },
    Bursty {
        base_rps: f64,
        spike_rps: f64,
        spike_probability: f64,
    },
    #[default]
    Mixed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObjectConfig {
    #[serde(default = "default_size_distribution")]
    pub size_distribution: Vec<SizeWeight>,
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
}

impl Default for ObjectConfig {
    fn default() -> Self {
        Self {
            size_distribution: default_size_distribution(),
            key_prefix: default_key_prefix(),
        }
    }
}

fn default_size_distribution() -> Vec<SizeWeight> {
    vec![
        SizeWeight {
            weight: 50,
            min_bytes: 1024,
            max_bytes: 10240,
        },
        SizeWeight {
            weight: 30,
            min_bytes: 10240,
            max_bytes: 102400,
        },
        SizeWeight {
            weight: 15,
            min_bytes: 102400,
            max_bytes: 1048576,
        },
        SizeWeight {
            weight: 5,
            min_bytes: 1048576,
            max_bytes: 10485760,
        },
    ]
}

fn default_key_prefix() -> String {
    "sim/".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SizeWeight {
    pub weight: u32,
    pub min_bytes: u64,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "json".to_string()
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load(config_path: Option<&str>) -> anyhow::Result<Self> {
        let mut builder = config::Config::builder();

        // Load from file if provided
        if let Some(path) = config_path {
            builder = builder.add_source(config::File::with_name(path));
        }

        // Override with environment variables
        builder = builder.add_source(
            config::Environment::with_prefix("SAVE_SIM")
                .separator("__")
                .try_parsing(true),
        );

        let config = builder.build()?;
        Ok(config.try_deserialize()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SimulationConfig {
            pattern: PatternConfig::default(),
            virtual_users: default_virtual_users(),
            read_ratio: default_read_ratio(),
            write_ratio: default_write_ratio(),
            requests_per_second: default_rps(),
        };

        assert_eq!(config.virtual_users, 10);
        assert!((config.read_ratio - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[target]
endpoint = "http://localhost:9000"
access_key = "minioadmin"
secret_key = "minioadmin"
bucket = "test"

[simulation]
pattern = { type = "mixed" }
virtual_users = 5

[objects]
key_prefix = "test/"

[logging]
level = "debug"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.target.bucket, "test");
        assert_eq!(config.simulation.virtual_users, 5);
    }
}
