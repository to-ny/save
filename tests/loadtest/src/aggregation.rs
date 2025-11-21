use crate::benchmark::{Operation, RequestMetrics};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AggregatedMetrics {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub requests_per_second: f64,
    pub total_bytes: u64,
    pub throughput_mbps: f64,
    pub end_to_end_latency: LatencyStats,
    pub server_storage_latency: Option<LatencyStats>,
    pub network_latency: Option<LatencyStats>,
    pub by_operation: HashMap<Operation, OperationStats>,
}

#[derive(Debug, Clone)]
pub struct OperationStats {
    pub count: usize,
    pub success_count: usize,
    pub fail_count: usize,
    pub total_bytes: u64,
    pub end_to_end_latency: LatencyStats,
    pub server_storage_latency: Option<LatencyStats>,
    pub network_latency: Option<LatencyStats>,
}

#[derive(Debug, Clone)]
pub struct LatencyStats {
    pub min_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
    pub max_ms: u64,
    pub mean_ms: f64,
}

impl AggregatedMetrics {
    pub fn from_metrics(metrics: &[RequestMetrics], duration_secs: u64) -> Self {
        let total_requests = metrics.len();
        let successful_requests = metrics.iter().filter(|m| m.success).count();
        let failed_requests = total_requests - successful_requests;
        let requests_per_second = total_requests as f64 / duration_secs as f64;

        let total_bytes: u64 = metrics.iter().map(|m| m.object_size as u64).sum();
        let throughput_mbps = (total_bytes as f64 / 1_048_576.0) / duration_secs as f64;

        let end_to_end_latency =
            calculate_latency_stats(&metrics.iter().map(|m| m.end_to_end_ms).collect::<Vec<_>>());

        let server_storage_values: Vec<u64> =
            metrics.iter().filter_map(|m| m.server_storage_ms).collect();

        let server_storage_latency = if !server_storage_values.is_empty() {
            Some(calculate_latency_stats(&server_storage_values))
        } else {
            None
        };

        let network_values: Vec<u64> = metrics.iter().filter_map(|m| m.network_ms()).collect();

        let network_latency = if !network_values.is_empty() {
            Some(calculate_latency_stats(&network_values))
        } else {
            None
        };

        let mut by_operation: HashMap<Operation, Vec<&RequestMetrics>> = HashMap::new();
        for metric in metrics {
            by_operation
                .entry(metric.operation)
                .or_default()
                .push(metric);
        }

        let by_operation = by_operation
            .into_iter()
            .map(|(op, op_metrics)| {
                let count = op_metrics.len();
                let success_count = op_metrics.iter().filter(|m| m.success).count();
                let fail_count = count - success_count;
                let total_bytes = op_metrics.iter().map(|m| m.object_size as u64).sum();

                let end_to_end_latency = calculate_latency_stats(
                    &op_metrics
                        .iter()
                        .map(|m| m.end_to_end_ms)
                        .collect::<Vec<_>>(),
                );

                let server_storage_values: Vec<u64> = op_metrics
                    .iter()
                    .filter_map(|m| m.server_storage_ms)
                    .collect();

                let server_storage_latency = if !server_storage_values.is_empty() {
                    Some(calculate_latency_stats(&server_storage_values))
                } else {
                    None
                };

                let network_values: Vec<u64> =
                    op_metrics.iter().filter_map(|m| m.network_ms()).collect();

                let network_latency = if !network_values.is_empty() {
                    Some(calculate_latency_stats(&network_values))
                } else {
                    None
                };

                (
                    op,
                    OperationStats {
                        count,
                        success_count,
                        fail_count,
                        total_bytes,
                        end_to_end_latency,
                        server_storage_latency,
                        network_latency,
                    },
                )
            })
            .collect();

        Self {
            total_requests,
            successful_requests,
            failed_requests,
            requests_per_second,
            total_bytes,
            throughput_mbps,
            end_to_end_latency,
            server_storage_latency,
            network_latency,
            by_operation,
        }
    }
}

fn calculate_latency_stats(values: &[u64]) -> LatencyStats {
    if values.is_empty() {
        return LatencyStats {
            min_ms: 0,
            p50_ms: 0,
            p95_ms: 0,
            p99_ms: 0,
            max_ms: 0,
            mean_ms: 0.0,
        };
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable();

    let min_ms = sorted[0];
    let max_ms = sorted[sorted.len() - 1];
    let p50_ms = percentile(&sorted, 50.0);
    let p95_ms = percentile(&sorted, 95.0);
    let p99_ms = percentile(&sorted, 99.0);
    let mean_ms = sorted.iter().sum::<u64>() as f64 / sorted.len() as f64;

    LatencyStats {
        min_ms,
        p50_ms,
        p95_ms,
        p99_ms,
        max_ms,
        mean_ms,
    }
}

fn percentile(sorted_values: &[u64], p: f64) -> u64 {
    if sorted_values.is_empty() {
        return 0;
    }

    let len = sorted_values.len();
    let rank = (p / 100.0 * len as f64).ceil() as usize;
    let index = (rank.saturating_sub(1)).min(len - 1);
    sorted_values[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_calculation() {
        let values = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&values, 50.0), 5);
        assert_eq!(percentile(&values, 95.0), 10);
        assert_eq!(percentile(&values, 99.0), 10);
    }

    #[test]
    fn test_latency_stats_empty() {
        let values: Vec<u64> = vec![];
        let stats = calculate_latency_stats(&values);
        assert_eq!(stats.min_ms, 0);
        assert_eq!(stats.max_ms, 0);
    }

    #[test]
    fn test_latency_stats() {
        let values = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        let stats = calculate_latency_stats(&values);
        assert_eq!(stats.min_ms, 10);
        assert_eq!(stats.max_ms, 100);
        assert_eq!(stats.p50_ms, 50);
        assert_eq!(stats.mean_ms, 55.0);
    }
}
