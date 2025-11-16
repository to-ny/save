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
            // Collect response times from all requests
            // The times BTreeMap has: key = response_time_ms, value = count
            let mut all_times: Vec<usize> = Vec::new();

            for req_metric in metrics.requests.values() {
                // Expand the histogram: for each response time, add it N times based on count
                for (&time_ms, &count) in &req_metric.raw_data.times {
                    for _ in 0..count {
                        all_times.push(time_ms);
                    }
                }
            }

            if all_times.is_empty() {
                (0.0, 0.0, 0.0, 0.0)
            } else {
                all_times.sort_unstable();
                let len = all_times.len();

                let p50_idx = (len * 50) / 100;
                let p95_idx = (len * 95) / 100;
                let p99_idx = (len * 99) / 100;

                // Times are already in milliseconds, no conversion needed
                let p50 = all_times[p50_idx.min(len - 1)] as f64;
                let p95 = all_times[p95_idx.min(len - 1)] as f64;
                let p99 = all_times[p99_idx.min(len - 1)] as f64;
                let max = all_times[len - 1] as f64;

                (p50, p95, p99, max)
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
                latency_p50_ms: p50,
                latency_p95_ms: p95,
                latency_p99_ms: p99,
                latency_max_ms: max,
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

    /// Calculate success rate as a fraction (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        if self.summary.total_requests == 0 {
            0.0
        } else {
            self.summary.successful_requests as f64 / self.summary.total_requests as f64
        }
    }

    /// Get percentile latency in milliseconds
    pub fn percentile_latency(&self, percentile: f64) -> Option<f64> {
        match percentile {
            50.0 => Some(self.summary.latency_p50_ms),
            95.0 => Some(self.summary.latency_p95_ms),
            99.0 => Some(self.summary.latency_p99_ms),
            100.0 => Some(self.summary.latency_max_ms),
            _ => None,
        }
    }

    /// Get requests per second
    pub fn requests_per_second(&self) -> Option<f64> {
        if self.summary.requests_per_second > 0.0 {
            Some(self.summary.requests_per_second)
        } else {
            None
        }
    }

    /// Convenience accessors for summary fields
    pub fn total_requests(&self) -> u64 {
        self.summary.total_requests
    }

    pub fn successful_requests(&self) -> u64 {
        self.summary.successful_requests
    }

    pub fn failed_requests(&self) -> u64 {
        self.summary.failed_requests
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
