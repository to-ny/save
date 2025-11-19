use crate::aggregation::{AggregatedMetrics, LatencyStats};
use crate::benchmark::Operation;
use crate::config::LoadTestConfig;
use crate::reporting::TargetEnvironment;
use chrono::{DateTime, Utc};
use std::fs;
use std::path::PathBuf;

pub struct BenchmarkReport {
    pub test_name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub duration_secs: u64,
    pub target: TargetEnvironment,
    pub config: WorkloadSummary,
    pub metrics: AggregatedMetrics,
}

#[derive(Debug, Clone)]
pub struct WorkloadSummary {
    pub concurrent_users: usize,
    pub duration_secs: u64,
    pub object_size_distribution: String,
    pub operation_mix: String,
}

impl BenchmarkReport {
    pub fn new(
        test_name: impl Into<String>,
        config: &LoadTestConfig,
        metrics: AggregatedMetrics,
        target: TargetEnvironment,
        start_time: DateTime<Utc>,
    ) -> Self {
        let end_time = Utc::now();
        let duration_secs = config.workload.duration_secs;

        let object_size_distribution = format!(
            "Small(1KB):{}% Medium(1MB):{}% Large(10MB):{}% XLarge(100MB):{}%",
            config.workload.object_sizes.small_1kb_percent,
            config.workload.object_sizes.medium_1mb_percent,
            config.workload.object_sizes.large_10mb_percent,
            config.workload.object_sizes.xlarge_100mb_percent
        );

        let total_weight = config.scenarios.mixed.put_weight
            + config.scenarios.mixed.get_weight
            + config.scenarios.mixed.delete_weight
            + config.scenarios.mixed.list_weight;

        let operation_mix = format!(
            "PUT:{}% GET:{}% DELETE:{}% LIST:{}%",
            config.scenarios.mixed.put_weight * 100 / total_weight,
            config.scenarios.mixed.get_weight * 100 / total_weight,
            config.scenarios.mixed.delete_weight * 100 / total_weight,
            config.scenarios.mixed.list_weight * 100 / total_weight
        );

        Self {
            test_name: test_name.into(),
            start_time,
            end_time,
            duration_secs,
            target,
            config: WorkloadSummary {
                concurrent_users: config.workload.users.max,
                duration_secs,
                object_size_distribution,
                operation_mix,
            },
            metrics,
        }
    }

    pub fn save(&self, output_dir: &PathBuf) -> crate::Result<PathBuf> {
        fs::create_dir_all(output_dir)?;

        let timestamp = self.start_time.format("%Y%m%d-%H%M%S-%6f");
        let filename = format!("{}-{}.md", self.test_name, timestamp);
        let path = output_dir.join(&filename);

        let markdown = self.to_markdown();
        fs::write(&path, markdown)?;

        Ok(path)
    }

    fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# Benchmark Report: {}\n\n", self.test_name));
        md.push_str(&format!(
            "**Start**: {}\n",
            self.start_time.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        md.push_str(&format!(
            "**End**: {}\n",
            self.end_time.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        md.push_str(&format!("**Duration**: {}s\n\n", self.duration_secs));

        md.push_str("## Target Environment\n\n");
        md.push_str(&format!("- **Endpoint**: {}\n", self.target.endpoint));
        md.push_str(&format!(
            "- **Deployment**: {:?}\n",
            self.target.deployment_mode
        ));
        if let Some(resources) = &self.target.resources {
            md.push_str(&format!("- **vCPU**: {}\n", resources.vcpu));
            md.push_str(&format!("- **Memory**: {}GB\n", resources.memory_gb));
            md.push_str(&format!(
                "- **OS**: {} {}\n",
                resources.os, resources.os_version
            ));
        }
        if let Some(storage) = &self.target.storage {
            md.push_str(&format!("- **Storage**: {}\n", storage.storage_type));
        }
        md.push('\n');

        md.push_str("## Workload Configuration\n\n");
        md.push_str(&format!("- **Duration**: {}s\n", self.config.duration_secs));
        md.push_str(&format!(
            "- **Concurrent Users**: {}\n",
            self.config.concurrent_users
        ));
        md.push_str(&format!(
            "- **Object Sizes**: {}\n",
            self.config.object_size_distribution
        ));
        md.push_str(&format!(
            "- **Operation Mix**: {}\n\n",
            self.config.operation_mix
        ));

        md.push_str("## Overall Results\n\n");
        md.push_str("| Metric | Value |\n");
        md.push_str("|--------|-------|\n");
        md.push_str(&format!(
            "| Total Requests | {} |\n",
            self.metrics.total_requests
        ));
        md.push_str(&format!(
            "| Successful | {} |\n",
            self.metrics.successful_requests
        ));
        md.push_str(&format!("| Failed | {} |\n", self.metrics.failed_requests));
        md.push_str(&format!(
            "| Success Rate | {:.2}% |\n",
            self.metrics.successful_requests as f64 / self.metrics.total_requests as f64 * 100.0
        ));
        md.push_str(&format!(
            "| Requests/sec | {:.2} |\n",
            self.metrics.requests_per_second
        ));
        md.push_str(&format!(
            "| Throughput | {:.2} MB/s |\n",
            self.metrics.throughput_mbps
        ));
        md.push_str(&format!(
            "| Total Data | {:.2} MB |\n\n",
            self.metrics.total_bytes as f64 / 1_048_576.0
        ));

        md.push_str("## Latency Breakdown\n\n");
        md.push_str("### End-to-End (Client Perspective)\n\n");
        md.push_str(&self.format_latency_table(&self.metrics.end_to_end_latency));

        if let Some(storage_latency) = &self.metrics.server_storage_latency {
            md.push_str("### Server-Side Storage\n\n");
            md.push_str(&self.format_latency_table(storage_latency));
        }

        if let Some(network_latency) = &self.metrics.network_latency {
            md.push_str("### Network Transfer\n\n");
            md.push_str(&self.format_latency_table(network_latency));
        }

        md.push_str("## By Operation\n\n");
        for (op, stats) in &self.metrics.by_operation {
            md.push_str(&format!("### {}\n\n", op));
            md.push_str(&format!("- **Count**: {}\n", stats.count));
            md.push_str(&format!("- **Success**: {}\n", stats.success_count));
            md.push_str(&format!("- **Failed**: {}\n", stats.fail_count));

            if matches!(op, Operation::Put | Operation::Get) {
                md.push_str(&format!(
                    "- **Total Data**: {:.2} MB\n",
                    stats.total_bytes as f64 / 1_048_576.0
                ));
                md.push_str(&format!(
                    "- **Throughput**: {:.2} MB/s\n",
                    (stats.total_bytes as f64 / 1_048_576.0) / self.duration_secs as f64
                ));
            }

            md.push_str("\n**End-to-End Latency:**\n\n");
            md.push_str(&self.format_latency_table(&stats.end_to_end_latency));

            if let Some(storage_latency) = &stats.server_storage_latency {
                md.push_str("**Server Storage Latency:**\n\n");
                md.push_str(&self.format_latency_table(storage_latency));
            }

            if let Some(network_latency) = &stats.network_latency {
                md.push_str("**Network Latency:**\n\n");
                md.push_str(&self.format_latency_table(network_latency));
            }
        }

        md
    }

    fn format_latency_table(&self, stats: &LatencyStats) -> String {
        format!(
            "| Metric | Latency (ms) |\n\
             |--------|-------------:|\n\
             | Min | {:.2} |\n\
             | Mean | {:.2} |\n\
             | p50 (median) | {} |\n\
             | p95 | {} |\n\
             | p99 | {} |\n\
             | Max | {} |\n\n",
            stats.min_ms, stats.mean_ms, stats.p50_ms, stats.p95_ms, stats.p99_ms, stats.max_ms
        )
    }
}
