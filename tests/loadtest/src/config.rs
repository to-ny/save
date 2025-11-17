use crate::error::{LoadTestError, Result};
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;
use url::Url;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoadTestConfig {
    pub target: TargetConfig,
    pub workload: WorkloadConfig,
    pub scenarios: ScenariosConfig,
    pub reporting: ReportingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetConfig {
    #[serde(deserialize_with = "deserialize_url")]
    #[serde(serialize_with = "serialize_url")]
    pub endpoint: Url,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkloadConfig {
    pub object_sizes: ObjectSizeDistribution,
    pub duration_secs: u64,
    pub users: UsersConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObjectSizeDistribution {
    pub small_1kb_percent: u32,
    pub medium_1mb_percent: u32,
    pub large_10mb_percent: u32,
    pub xlarge_100mb_percent: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsersConfig {
    pub start: usize,
    pub max: usize,
    pub hatch_rate: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenariosConfig {
    pub read_heavy: ScenarioWeights,
    pub write_heavy: ScenarioWeights,
    pub mixed: ScenarioWeights,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioWeights {
    pub put_weight: usize,
    pub get_weight: usize,
    pub delete_weight: usize,
    pub list_weight: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReportingConfig {
    pub output_dir: PathBuf,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_optional_url")]
    #[serde(serialize_with = "serialize_optional_url")]
    pub prometheus_endpoint: Option<Url>,
    pub collect_system_metrics: bool,
}

impl LoadTestConfig {
    /// Load configuration with layered precedence:
    /// 1. config.toml file (if exists)
    /// 2. Environment variables with SAVE_ prefix
    /// 3. Validation checks
    pub fn load() -> Result<Self> {
        let config_path =
            std::env::var("LOADTEST_CONFIG").unwrap_or_else(|_| "config.toml".to_string());

        info!("Loading config from: {}", config_path);

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .add_source(
                Environment::with_prefix("SAVE")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()
            .map_err(|e| LoadTestError::Config(e.to_string()))?;

        let mut cfg: Self = config
            .try_deserialize()
            .map_err(|e| LoadTestError::Config(e.to_string()))?;

        cfg.apply_overrides()?;
        cfg.validate()?;

        info!(
            endpoint = %cfg.target.endpoint,
            users = cfg.workload.users.max,
            duration_secs = cfg.workload.duration_secs,
            "Configuration loaded successfully"
        );

        Ok(cfg)
    }

    /// Load from specific file path (for backwards compatibility)
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| LoadTestError::Config(format!("Failed to read config file: {}", e)))?;

        let mut config: LoadTestConfig = toml::from_str(&content)
            .map_err(|e| LoadTestError::Config(format!("Failed to parse TOML: {}", e)))?;

        config.apply_overrides()?;
        config.validate()?;
        Ok(config)
    }

    /// Apply environment variable overrides for remote testing
    fn apply_overrides(&mut self) -> Result<()> {
        if let Ok(endpoint) = std::env::var("SAVE_ENDPOINT") {
            self.target.endpoint = endpoint.parse().map_err(LoadTestError::UrlParse)?;
            info!(endpoint = %self.target.endpoint, "Overriding endpoint from SAVE_ENDPOINT");
        }

        if let Ok(access_key) = std::env::var("SAVE_ACCESS_KEY") {
            self.target.access_key = access_key;
        }

        if let Ok(secret_key) = std::env::var("SAVE_SECRET_KEY") {
            self.target.secret_key = secret_key;
        }

        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let total = self.workload.object_sizes.small_1kb_percent
            + self.workload.object_sizes.medium_1mb_percent
            + self.workload.object_sizes.large_10mb_percent
            + self.workload.object_sizes.xlarge_100mb_percent;

        if total != 100 {
            return Err(LoadTestError::ConfigValidation(format!(
                "Object size percentages must sum to 100, got {}",
                total
            )));
        }

        if self.workload.users.start > self.workload.users.max {
            return Err(LoadTestError::ConfigValidation(
                "Start users cannot exceed max users".to_string(),
            ));
        }

        if self.workload.users.hatch_rate == 0 {
            return Err(LoadTestError::ConfigValidation(
                "Hatch rate must be > 0".to_string(),
            ));
        }

        let scheme = self.target.endpoint.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(LoadTestError::InvalidEndpoint(format!(
                "Invalid endpoint scheme '{}': must be http or https",
                scheme
            )));
        }

        if self.target.access_key.is_empty() {
            return Err(LoadTestError::ConfigValidation(
                "Access key cannot be empty".to_string(),
            ));
        }

        if self.target.secret_key.is_empty() {
            return Err(LoadTestError::ConfigValidation(
                "Secret key cannot be empty".to_string(),
            ));
        }

        if self.target.bucket.is_empty() {
            return Err(LoadTestError::ConfigValidation(
                "Bucket name cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl ObjectSizeDistribution {
    pub fn sample(&self) -> usize {
        let rand_val = rand::random::<u32>() % 100;
        let mut cumulative = 0;

        cumulative += self.small_1kb_percent;
        if rand_val < cumulative {
            return 1024;
        }

        cumulative += self.medium_1mb_percent;
        if rand_val < cumulative {
            return 1_048_576;
        }

        cumulative += self.large_10mb_percent;
        if rand_val < cumulative {
            return 10_485_760;
        }

        104_857_600
    }
}

fn deserialize_url<'de, D>(deserializer: D) -> std::result::Result<Url, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn serialize_url<S>(url: &Url, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(url.as_str())
}

fn deserialize_optional_url<'de, D>(deserializer: D) -> std::result::Result<Option<Url>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    opt.map(|s| s.parse().map_err(serde::de::Error::custom))
        .transpose()
}

fn serialize_optional_url<S>(
    opt: &Option<Url>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match opt {
        Some(url) => serializer.serialize_some(url.as_str()),
        None => serializer.serialize_none(),
    }
}
