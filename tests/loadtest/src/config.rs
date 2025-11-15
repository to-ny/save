use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoadTestConfig {
    pub target: TargetConfig,
    pub workload: WorkloadConfig,
    pub scenarios: ScenariosConfig,
    pub reporting: ReportingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetConfig {
    pub endpoint: String,
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
    pub output_dir: String,
    pub prometheus_endpoint: Option<String>,
    pub collect_system_metrics: bool,
}

impl LoadTestConfig {
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: LoadTestConfig = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let total = self.workload.object_sizes.small_1kb_percent
            + self.workload.object_sizes.medium_1mb_percent
            + self.workload.object_sizes.large_10mb_percent
            + self.workload.object_sizes.xlarge_100mb_percent;

        if total != 100 {
            anyhow::bail!("Object size percentages must sum to 100, got {}", total);
        }

        if self.workload.users.start > self.workload.users.max {
            anyhow::bail!("Start users cannot exceed max users");
        }

        if self.workload.users.hatch_rate == 0 {
            anyhow::bail!("Hatch rate must be > 0");
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

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            target: TargetConfig {
                endpoint: "http://localhost:8080".to_string(),
                access_key: "test-access-key".to_string(),
                secret_key: "test-access-key".to_string(),
                bucket: "loadtest".to_string(),
            },
            workload: WorkloadConfig {
                object_sizes: ObjectSizeDistribution {
                    small_1kb_percent: 40,
                    medium_1mb_percent: 40,
                    large_10mb_percent: 15,
                    xlarge_100mb_percent: 5,
                },
                duration_secs: 60,
                users: UsersConfig {
                    start: 1,
                    max: 50,
                    hatch_rate: 5,
                },
            },
            scenarios: ScenariosConfig {
                read_heavy: ScenarioWeights {
                    put_weight: 1,
                    get_weight: 8,
                    delete_weight: 1,
                    list_weight: 0,
                },
                write_heavy: ScenarioWeights {
                    put_weight: 7,
                    get_weight: 2,
                    delete_weight: 1,
                    list_weight: 0,
                },
                mixed: ScenarioWeights {
                    put_weight: 3,
                    get_weight: 5,
                    delete_weight: 1,
                    list_weight: 1,
                },
            },
            reporting: ReportingConfig {
                output_dir: "./loadtest-results".to_string(),
                prometheus_endpoint: Some("http://localhost:8080/metrics".to_string()),
                collect_system_metrics: true,
            },
        }
    }
}
