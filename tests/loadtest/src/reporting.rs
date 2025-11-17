use crate::error::{LoadTestError, Result};
use crate::metrics::PrometheusSnapshot;
use crate::system_metrics::SystemMetrics;
use chrono::{DateTime, Utc};
use goose::metrics::GooseMetrics;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, instrument};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub metadata: TestMetadata,
    pub execution: ExecutionMetrics,
    pub observability: ObservabilityData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMetadata {
    pub test_name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub target: TargetEnvironment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetEnvironment {
    #[serde(serialize_with = "serialize_url")]
    #[serde(deserialize_with = "deserialize_url")]
    pub endpoint: Url,
    pub deployment_mode: DeploymentMode,
    pub resources: Option<ResourceSpec>,
    pub build_info: BuildInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeploymentMode {
    Local {
        #[serde(skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    Remote {
        server_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub vcpu: usize,
    pub memory_gb: usize,
    pub os: String,
    pub os_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub rust_version: String,
    pub save_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub requests_per_second: f64,
    pub latency: LatencyMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMetrics {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityData {
    pub prometheus_samples: Vec<PrometheusSnapshot>,
    pub system_samples: Vec<SystemMetrics>,
}

impl TestReport {
    pub fn from_goose_metrics(
        test_name: impl Into<String>,
        metrics: &GooseMetrics,
        target: TargetEnvironment,
        prometheus_samples: Vec<PrometheusSnapshot>,
        system_samples: Vec<SystemMetrics>,
    ) -> Self {
        let total_requests: u64 = metrics
            .requests
            .values()
            .map(|r| r.success_count as u64 + r.fail_count as u64)
            .sum();
        let successful_requests: u64 = metrics
            .requests
            .values()
            .map(|r| r.success_count as u64)
            .sum();
        let failed_requests: u64 = metrics.requests.values().map(|r| r.fail_count as u64).sum();

        let duration_secs = metrics.duration as f64;
        let requests_per_second = if duration_secs > 0.0 {
            total_requests as f64 / duration_secs
        } else {
            0.0
        };

        let latency = Self::calculate_latency_metrics(metrics);

        let end_time = Utc::now();
        let start_time = end_time - chrono::Duration::seconds(metrics.duration as i64);

        Self {
            metadata: TestMetadata {
                test_name: test_name.into(),
                start_time,
                end_time,
                target,
            },
            execution: ExecutionMetrics {
                total_requests,
                successful_requests,
                failed_requests,
                requests_per_second,
                latency,
            },
            observability: ObservabilityData {
                prometheus_samples,
                system_samples,
            },
        }
    }

    fn calculate_latency_metrics(metrics: &GooseMetrics) -> LatencyMetrics {
        let mut all_times: Vec<usize> = Vec::new();

        for req_metric in metrics.requests.values() {
            for (&time_ms, &count) in &req_metric.raw_data.times {
                for _ in 0..count {
                    all_times.push(time_ms);
                }
            }
        }

        if all_times.is_empty() {
            return LatencyMetrics {
                p50_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                max_ms: 0.0,
            };
        }

        all_times.sort_unstable();
        let len = all_times.len();

        let p50_idx = (len * 50) / 100;
        let p95_idx = (len * 95) / 100;
        let p99_idx = (len * 99) / 100;

        LatencyMetrics {
            p50_ms: all_times[p50_idx.min(len - 1)] as f64,
            p95_ms: all_times[p95_idx.min(len - 1)] as f64,
            p99_ms: all_times[p99_idx.min(len - 1)] as f64,
            max_ms: all_times[len - 1] as f64,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.execution.total_requests == 0 {
            0.0
        } else {
            self.execution.successful_requests as f64 / self.execution.total_requests as f64
        }
    }

    pub fn duration(&self) -> Duration {
        (self.metadata.end_time - self.metadata.start_time)
            .to_std()
            .unwrap_or_default()
    }

    pub fn total_requests(&self) -> u64 {
        self.execution.total_requests
    }

    pub fn successful_requests(&self) -> u64 {
        self.execution.successful_requests
    }

    pub fn failed_requests(&self) -> u64 {
        self.execution.failed_requests
    }

    pub fn requests_per_second(&self) -> Option<f64> {
        if self.execution.requests_per_second > 0.0 {
            Some(self.execution.requests_per_second)
        } else {
            None
        }
    }
}

impl TargetEnvironment {
    pub fn detect(endpoint: Url) -> Result<Self> {
        let (deployment_mode, resources) = if let (Some(server_type), Some(vcpu), Some(memory_gb)) = (
            std::env::var("SAVE_SERVER_TYPE").ok(),
            std::env::var("SAVE_SERVER_VCPU")
                .ok()
                .and_then(|s| s.parse().ok()),
            std::env::var("SAVE_SERVER_RAM_GB")
                .ok()
                .and_then(|s| s.parse().ok()),
        ) {
            let os = std::env::var("SAVE_SERVER_OS").unwrap_or_else(|_| "Linux".to_string());
            let os_version = std::env::var("SAVE_SERVER_OS_VERSION")
                .unwrap_or_else(|_| "Ubuntu 24.04".to_string());

            let location = std::env::var("SAVE_SERVER_LOCATION").ok();

            (
                DeploymentMode::Remote {
                    server_type,
                    location,
                },
                Some(ResourceSpec {
                    vcpu,
                    memory_gb,
                    os,
                    os_version,
                }),
            )
        } else {
            let pid = std::env::var("SERVER_PID")
                .ok()
                .and_then(|s| s.parse().ok());

            let resources = Some(ResourceSpec {
                vcpu: num_cpus::get(),
                memory_gb: (sys_info::mem_info()
                    .map(|m| m.total / 1024 / 1024)
                    .unwrap_or(0)) as usize,
                os: std::env::consts::OS.to_string(),
                os_version: detect_os_version(),
            });

            (DeploymentMode::Local { pid }, resources)
        };

        let build_info = BuildInfo::detect();

        Ok(Self {
            endpoint,
            deployment_mode,
            resources,
            build_info,
        })
    }
}

impl BuildInfo {
    pub fn detect() -> Self {
        let git_commit = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string());

        let git_branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string());

        Self {
            rust_version: format!("rustc {}", rustc_version_runtime::version()),
            save_version: env!("CARGO_PKG_VERSION").to_string(),
            git_commit,
            git_branch,
        }
    }
}

fn detect_os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        sys_info::linux_os_release()
            .ok()
            .map(|info| {
                format!(
                    "{} {}",
                    info.name.unwrap_or_default(),
                    info.version.unwrap_or_default()
                )
            })
            .unwrap_or_else(|| "Linux".to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        format!("{} (unknown version)", std::env::consts::OS)
    }
}

pub trait ReportFormatter: Send + Sync {
    fn format(&self, report: &TestReport) -> Result<String>;
    fn file_extension(&self) -> &str;
}

pub struct JsonFormatter;

impl ReportFormatter for JsonFormatter {
    fn format(&self, report: &TestReport) -> Result<String> {
        serde_json::to_string_pretty(report).map_err(LoadTestError::JsonSerialization)
    }

    fn file_extension(&self) -> &str {
        "json"
    }
}

pub struct MarkdownFormatter;

impl ReportFormatter for MarkdownFormatter {
    fn format(&self, report: &TestReport) -> Result<String> {
        let duration_secs = (report.metadata.end_time - report.metadata.start_time).num_seconds();

        let mut md = format!(
            "# Load Test Report: {}\n\n\
             **Start**: {}\n\
             **End**: {}\n\
             **Duration**: {:.2}s\n\n",
            report.metadata.test_name,
            report.metadata.start_time.format("%Y-%m-%d %H:%M:%S UTC"),
            report.metadata.end_time.format("%Y-%m-%d %H:%M:%S UTC"),
            duration_secs,
        );

        md.push_str("## Target Environment\n\n");
        md.push_str("| Property | Value |\n");
        md.push_str("|----------|-------|\n");
        md.push_str(&format!(
            "| Endpoint | {} |\n",
            report.metadata.target.endpoint
        ));

        match &report.metadata.target.deployment_mode {
            DeploymentMode::Local { pid } => {
                md.push_str("| Deployment | Local |\n");
                if let Some(pid) = pid {
                    md.push_str(&format!("| PID | {} |\n", pid));
                }
            }
            DeploymentMode::Remote {
                server_type,
                location,
            } => {
                md.push_str("| Deployment | Remote |\n");
                md.push_str(&format!("| Server Type | {} |\n", server_type));
                if let Some(loc) = location {
                    md.push_str(&format!("| Location | {} |\n", loc));
                }
            }
            DeploymentMode::Unknown => {
                md.push_str("| Deployment | Unknown |\n");
            }
        }

        if let Some(ref resources) = report.metadata.target.resources {
            md.push_str(&format!("| vCPU | {} |\n", resources.vcpu));
            md.push_str(&format!("| Memory | {}GB |\n", resources.memory_gb));
            md.push_str(&format!(
                "| OS | {} {} |\n",
                resources.os, resources.os_version
            ));
        }

        md.push_str(&format!(
            "| Rust | {} |\n",
            report.metadata.target.build_info.rust_version
        ));
        md.push_str(&format!(
            "| Save | {} |\n",
            report.metadata.target.build_info.save_version
        ));

        if let Some(ref commit) = report.metadata.target.build_info.git_commit {
            md.push_str(&format!(
                "| Git Commit | {} |\n",
                &commit[..7.min(commit.len())]
            ));
        }
        if let Some(ref branch) = report.metadata.target.build_info.git_branch {
            md.push_str(&format!("| Git Branch | {} |\n", branch));
        }

        md.push_str(&format!(
            "\n## Summary\n\n\
             | Metric | Value |\n\
             |--------|-------|\n\
             | Total Requests | {} |\n\
             | Successful | {} |\n\
             | Failed | {} |\n\
             | Success Rate | {:.2}% |\n\
             | Requests/sec | {:.2} |\n\n",
            report.execution.total_requests,
            report.execution.successful_requests,
            report.execution.failed_requests,
            report.success_rate() * 100.0,
            report.execution.requests_per_second,
        ));

        md.push_str(&format!(
            "## Latency\n\n\
             | Percentile | Latency (ms) |\n\
             |------------|-------------|\n\
             | p50 | {:.2} |\n\
             | p95 | {:.2} |\n\
             | p99 | {:.2} |\n\
             | max | {:.2} |\n",
            report.execution.latency.p50_ms,
            report.execution.latency.p95_ms,
            report.execution.latency.p99_ms,
            report.execution.latency.max_ms,
        ));

        Ok(md)
    }

    fn file_extension(&self) -> &str {
        "md"
    }
}

pub struct CsvFormatter;

impl ReportFormatter for CsvFormatter {
    fn format(&self, report: &TestReport) -> Result<String> {
        Ok(format!(
            "metric,value\n\
             test_name,{}\n\
             total_requests,{}\n\
             successful_requests,{}\n\
             failed_requests,{}\n\
             success_rate,{:.4}\n\
             requests_per_second,{:.2}\n\
             latency_p50_ms,{:.2}\n\
             latency_p95_ms,{:.2}\n\
             latency_p99_ms,{:.2}\n\
             latency_max_ms,{:.2}\n",
            report.metadata.test_name,
            report.execution.total_requests,
            report.execution.successful_requests,
            report.execution.failed_requests,
            report.success_rate(),
            report.execution.requests_per_second,
            report.execution.latency.p50_ms,
            report.execution.latency.p95_ms,
            report.execution.latency.p99_ms,
            report.execution.latency.max_ms,
        ))
    }

    fn file_extension(&self) -> &str {
        "csv"
    }
}

pub struct ReportWriter {
    output_dir: PathBuf,
    formatters: Vec<Box<dyn ReportFormatter>>,
}

impl ReportWriter {
    pub fn new(output_dir: PathBuf) -> Self {
        let formatters: Vec<Box<dyn ReportFormatter>> = vec![
            Box::new(JsonFormatter),
            Box::new(MarkdownFormatter),
            Box::new(CsvFormatter),
        ];

        Self {
            output_dir,
            formatters,
        }
    }

    #[instrument(skip(self, report), fields(test_name = %report.metadata.test_name))]
    pub fn save(&self, report: &TestReport) -> Result<Vec<PathBuf>> {
        std::fs::create_dir_all(&self.output_dir).map_err(LoadTestError::Io)?;

        let timestamp = Utc::now().format("%Y%m%d-%H%M%S-%6f");
        let base_name = format!("{}-{}", report.metadata.test_name, timestamp);

        let mut paths = Vec::new();

        for formatter in &self.formatters {
            let filename = format!("{}.{}", base_name, formatter.file_extension());
            let path = self.output_dir.join(filename);

            let content = formatter.format(report)?;
            std::fs::write(&path, content).map_err(LoadTestError::Io)?;

            info!(path = %path.display(), format = formatter.file_extension(), "Wrote report");
            paths.push(path);
        }

        Ok(paths)
    }
}

fn serialize_url<S>(url: &Url, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(url.as_str())
}

fn deserialize_url<'de, D>(deserializer: D) -> std::result::Result<Url, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}
