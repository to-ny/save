use crate::error::{LoadTestError, Result};
use chrono::{DateTime, Utc};
use prometheus_parse::{Scrape, Value};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{instrument, warn};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusSnapshot {
    pub timestamp: DateTime<Utc>,
    pub samples: Vec<PrometheusSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusSample {
    pub metric: String,
    pub labels: HashMap<String, String>,
    pub value: MetricValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MetricValue {
    Counter { value: f64 },
    Gauge { value: f64 },
    Untyped { value: f64 },
}

pub struct MetricsCollector {
    client: Client,
    endpoint: Url,
}

impl MetricsCollector {
    pub fn new(endpoint: Url) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(LoadTestError::HttpRequest)?,
            endpoint,
        })
    }

    #[instrument(skip(self))]
    pub async fn collect(&self) -> Result<PrometheusSnapshot> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .send()
            .await
            .map_err(LoadTestError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(LoadTestError::MetricsCollection(anyhow::anyhow!(
                "Prometheus endpoint returned status: {}",
                response.status()
            )));
        }

        let text = response.text().await.map_err(LoadTestError::HttpRequest)?;

        let scrape = Scrape::parse(text.lines().map(|s| Ok(s.to_owned())))
            .map_err(|e| LoadTestError::PrometheusParseError(format!("{:?}", e)))?;

        let samples = scrape
            .samples
            .into_iter()
            .filter_map(|sample| {
                let metric = sample.metric.clone();
                let labels: HashMap<String, String> = sample
                    .labels
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                let value = match sample.value {
                    Value::Counter(v) => MetricValue::Counter { value: v },
                    Value::Gauge(v) => MetricValue::Gauge { value: v },
                    Value::Untyped(v) => MetricValue::Untyped { value: v },
                    _ => {
                        warn!(metric = %metric, "Skipping unsupported metric type");
                        return None;
                    }
                };

                Some(PrometheusSample {
                    metric,
                    labels,
                    value,
                })
            })
            .collect();

        Ok(PrometheusSnapshot {
            timestamp: Utc::now(),
            samples,
        })
    }
}
