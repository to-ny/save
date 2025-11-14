use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusMetrics {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metrics: HashMap<String, f64>,
}

pub struct MetricsCollector {
    client: Client,
    endpoint: String,
}

impl MetricsCollector {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.into(),
        }
    }

    pub async fn collect(&self) -> anyhow::Result<PrometheusMetrics> {
        let response = self.client.get(&self.endpoint).send().await?;
        let text = response.text().await?;

        let mut metrics = HashMap::new();
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }

            if let Some((name, value)) = parse_metric_line(line) {
                metrics.insert(name, value);
            }
        }

        Ok(PrometheusMetrics {
            timestamp: chrono::Utc::now(),
            metrics,
        })
    }
}

fn parse_metric_line(line: &str) -> Option<(String, f64)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let name = parts[0].to_string();
    let value = parts[1].parse::<f64>().ok()?;

    Some((name, value))
}
