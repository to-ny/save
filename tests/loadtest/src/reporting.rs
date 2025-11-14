use crate::metrics::PrometheusMetrics;
use crate::system_metrics::SystemMetrics;
use goose::metrics::GooseMetrics;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct TestReport {
    pub test_name: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub summary: TestSummary,
    pub prometheus_samples: Vec<PrometheusMetrics>,
    pub system_samples: Vec<SystemMetrics>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestSummary {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub requests_per_second: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub latency_max_ms: f64,
}

impl TestReport {
    pub fn from_goose_metrics(
        test_name: impl Into<String>,
        metrics: &GooseMetrics,
        prometheus_samples: Vec<PrometheusMetrics>,
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

        let (p50, p95, p99, max) = {
            let mut agg_p50 = 0u64;
            let mut agg_p95 = 0u64;
            let mut agg_p99 = 0u64;
            let mut agg_max = 0u64;
            let mut count = 0;

            for req_metric in metrics.requests.values() {
                if let (Some(p50), Some(p95), Some(p99), Some(max)) = (
                    req_metric.raw_data.times.get(&50),
                    req_metric.raw_data.times.get(&95),
                    req_metric.raw_data.times.get(&99),
                    req_metric.raw_data.times.get(&100),
                ) {
                    agg_p50 += *p50 as u64;
                    agg_p95 += *p95 as u64;
                    agg_p99 += *p99 as u64;
                    agg_max += *max as u64;
                    count += 1;
                }
            }

            if count > 0 {
                (
                    agg_p50 / count / 1_000_000,
                    agg_p95 / count / 1_000_000,
                    agg_p99 / count / 1_000_000,
                    agg_max / count / 1_000_000,
                )
            } else {
                (0, 0, 0, 0)
            }
        };

        Self {
            test_name: test_name.into(),
            start_time: chrono::Utc::now() - chrono::Duration::seconds(metrics.duration as i64),
            end_time: chrono::Utc::now(),
            summary: TestSummary {
                total_requests,
                successful_requests,
                failed_requests,
                requests_per_second,
                latency_p50_ms: p50 as f64,
                latency_p95_ms: p95 as f64,
                latency_p99_ms: p99 as f64,
                latency_max_ms: max as f64,
            },
            prometheus_samples,
            system_samples,
        }
    }

    pub fn save_json(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn save_markdown(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let md = self.to_markdown();
        std::fs::write(path, md)?;
        Ok(())
    }

    pub fn save_csv(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let csv = format!(
            "metric,value\n\
             test_name,{}\n\
             total_requests,{}\n\
             successful_requests,{}\n\
             failed_requests,{}\n\
             requests_per_second,{:.2}\n\
             latency_p50_ms,{:.2}\n\
             latency_p95_ms,{:.2}\n\
             latency_p99_ms,{:.2}\n\
             latency_max_ms,{:.2}\n",
            self.test_name,
            self.summary.total_requests,
            self.summary.successful_requests,
            self.summary.failed_requests,
            self.summary.requests_per_second,
            self.summary.latency_p50_ms,
            self.summary.latency_p95_ms,
            self.summary.latency_p99_ms,
            self.summary.latency_max_ms,
        );
        std::fs::write(path, csv)?;
        Ok(())
    }

    fn to_markdown(&self) -> String {
        format!(
            "# Load Test Report: {}\n\n\
             **Start**: {}\n\
             **End**: {}\n\
             **Duration**: {:.2}s\n\n\
             ## Summary\n\n\
             | Metric | Value |\n\
             |--------|-------|\n\
             | Total Requests | {} |\n\
             | Successful | {} |\n\
             | Failed | {} |\n\
             | Requests/sec | {:.2} |\n\n\
             ## Latency\n\n\
             | Percentile | Latency (ms) |\n\
             |------------|-------------|\n\
             | p50 | {:.2} |\n\
             | p95 | {:.2} |\n\
             | p99 | {:.2} |\n\
             | max | {:.2} |\n",
            self.test_name,
            self.start_time.format("%Y-%m-%d %H:%M:%S UTC"),
            self.end_time.format("%Y-%m-%d %H:%M:%S UTC"),
            (self.end_time - self.start_time).num_seconds(),
            self.summary.total_requests,
            self.summary.successful_requests,
            self.summary.failed_requests,
            self.summary.requests_per_second,
            self.summary.latency_p50_ms,
            self.summary.latency_p95_ms,
            self.summary.latency_p99_ms,
            self.summary.latency_max_ms,
        )
    }
}
