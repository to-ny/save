//! Traffic simulator orchestration.

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::config::Config;
use crate::operations::{OperationResult, OperationType, Operations};
use crate::patterns::TrafficPattern;
use crate::user::VirtualUser;

pub struct Stats {
    pub ops_by_type: HashMap<OperationType, AtomicU64>,
    pub success_by_type: HashMap<OperationType, AtomicU64>,
    pub bytes_transferred: AtomicU64,
    pub latency_us_by_type: HashMap<OperationType, AtomicU64>,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    pub fn new() -> Self {
        let mut ops_by_type = HashMap::new();
        let mut success_by_type = HashMap::new();
        let mut latency_us_by_type = HashMap::new();

        for op in [
            OperationType::Put,
            OperationType::Get,
            OperationType::Delete,
            OperationType::List,
            OperationType::Head,
            OperationType::ListBuckets,
            OperationType::MultipartUpload,
        ] {
            ops_by_type.insert(op, AtomicU64::new(0));
            success_by_type.insert(op, AtomicU64::new(0));
            latency_us_by_type.insert(op, AtomicU64::new(0));
        }

        Self {
            ops_by_type,
            success_by_type,
            bytes_transferred: AtomicU64::new(0),
            latency_us_by_type,
        }
    }

    pub fn record(&self, result: &OperationResult) {
        if let Some(counter) = self.ops_by_type.get(&result.operation) {
            counter.fetch_add(1, Ordering::Relaxed);
        }

        if result.success
            && let Some(counter) = self.success_by_type.get(&result.operation)
        {
            counter.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(bytes) = result.size_bytes {
            self.bytes_transferred.fetch_add(bytes, Ordering::Relaxed);
        }

        if let Some(latency) = self.latency_us_by_type.get(&result.operation) {
            latency.fetch_add(result.duration.as_micros() as u64, Ordering::Relaxed);
        }
    }

    pub fn summary(&self, elapsed: Duration) -> StatsSummary {
        let elapsed_secs = elapsed.as_secs_f64();
        let mut ops_per_sec = HashMap::new();
        let mut error_rates = HashMap::new();
        let mut avg_latency_ms = HashMap::new();

        for op in [
            OperationType::Put,
            OperationType::Get,
            OperationType::Delete,
            OperationType::List,
            OperationType::Head,
            OperationType::ListBuckets,
            OperationType::MultipartUpload,
        ] {
            let total = self
                .ops_by_type
                .get(&op)
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0);
            let success = self
                .success_by_type
                .get(&op)
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0);
            let latency_us = self
                .latency_us_by_type
                .get(&op)
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0);

            ops_per_sec.insert(op, total as f64 / elapsed_secs);

            if total > 0 {
                error_rates.insert(op, 100.0 * (total - success) as f64 / total as f64);
                avg_latency_ms.insert(op, (latency_us as f64 / total as f64) / 1000.0);
            } else {
                error_rates.insert(op, 0.0);
                avg_latency_ms.insert(op, 0.0);
            }
        }

        let total_ops: u64 = self
            .ops_by_type
            .values()
            .map(|c| c.load(Ordering::Relaxed))
            .sum();
        let total_success: u64 = self
            .success_by_type
            .values()
            .map(|c| c.load(Ordering::Relaxed))
            .sum();

        StatsSummary {
            elapsed_secs,
            total_ops,
            total_ops_per_sec: total_ops as f64 / elapsed_secs,
            total_error_rate: if total_ops > 0 {
                100.0 * (total_ops - total_success) as f64 / total_ops as f64
            } else {
                0.0
            },
            bytes_transferred: self.bytes_transferred.load(Ordering::Relaxed),
            ops_per_sec,
            error_rates,
            avg_latency_ms,
        }
    }
}

#[derive(Debug)]
pub struct StatsSummary {
    pub elapsed_secs: f64,
    pub total_ops: u64,
    pub total_ops_per_sec: f64,
    pub total_error_rate: f64,
    pub bytes_transferred: u64,
    pub ops_per_sec: HashMap<OperationType, f64>,
    pub error_rates: HashMap<OperationType, f64>,
    pub avg_latency_ms: HashMap<OperationType, f64>,
}

impl StatsSummary {
    pub fn log(&self) {
        info!(
            elapsed_secs = self.elapsed_secs,
            total_ops = self.total_ops,
            ops_per_sec = format!("{:.2}", self.total_ops_per_sec),
            error_rate = format!("{:.2}%", self.total_error_rate),
            bytes_transferred = self.bytes_transferred,
            "Summary"
        );

        for op in [
            OperationType::Put,
            OperationType::Get,
            OperationType::Delete,
            OperationType::List,
            OperationType::Head,
            OperationType::ListBuckets,
            OperationType::MultipartUpload,
        ] {
            let ops = self.ops_per_sec.get(&op).unwrap_or(&0.0);
            let error = self.error_rates.get(&op).unwrap_or(&0.0);
            let latency = self.avg_latency_ms.get(&op).unwrap_or(&0.0);

            if *ops > 0.0 {
                info!(
                    operation = %op,
                    ops_per_sec = format!("{:.2}", ops),
                    error_rate = format!("{:.2}%", error),
                    avg_latency_ms = format!("{:.2}", latency),
                    "Operation stats"
                );
            }
        }
    }
}

pub struct Simulator {
    config: Config,
    client: Client,
    running: Arc<AtomicBool>,
    stats: Arc<Stats>,
}

impl Simulator {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        // Build AWS SDK config
        let credentials = Credentials::new(
            &config.target.access_key,
            &config.target.secret_key,
            None,
            None,
            "save-traffic-simulator",
        );

        let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(config.target.region.clone()))
            .endpoint_url(&config.target.endpoint)
            .load()
            .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
            .force_path_style(true)
            .build();

        let client = Client::from_conf(s3_config);

        Ok(Self {
            config,
            client,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Stats::new()),
        })
    }

    pub async fn ensure_bucket(&self) -> anyhow::Result<()> {
        let bucket = &self.config.target.bucket;

        // Check if bucket exists
        let result = self.client.head_bucket().bucket(bucket).send().await;

        if result.is_err() {
            // Create the bucket
            info!(bucket = bucket, "Creating bucket");
            self.client
                .create_bucket()
                .bucket(bucket)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create bucket: {}", e))?;
            info!(bucket = bucket, "Bucket created");
        } else {
            info!(bucket = bucket, "Bucket exists");
        }

        Ok(())
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let start = Instant::now();

        info!(
            endpoint = self.config.target.endpoint,
            bucket = self.config.target.bucket,
            virtual_users = self.config.simulation.virtual_users,
            "Starting traffic simulator"
        );

        // Ensure bucket exists
        self.ensure_bucket().await?;

        // Create traffic pattern
        let pattern = TrafficPattern::from(&self.config.simulation.pattern);

        // Apply config overrides for mixed pattern
        let pattern = match pattern {
            TrafficPattern::Mixed { .. } => TrafficPattern::Mixed {
                read_ratio: self.config.simulation.read_ratio,
                write_ratio: self.config.simulation.write_ratio,
                rps: self.config.simulation.requests_per_second,
            },
            other => other,
        };

        // Create operations executor
        let ops = Arc::new(Operations::new(
            self.client.clone(),
            self.config.target.bucket.clone(),
            self.config.objects.key_prefix.clone(),
            self.config.objects.size_distribution.clone(),
        ));

        // Spawn virtual users
        let mut handles = Vec::new();
        let num_users = self.config.simulation.virtual_users;

        for user_id in 0..num_users {
            let running = self.running.clone();
            let stats = self.stats.clone();
            let ops = ops.clone();
            let pattern = pattern.clone();

            let handle = tokio::spawn(async move {
                let mut user = VirtualUser::new(user_id);
                info!(user_id = user_id, personality = ?user.personality(), "Virtual user started");

                while running.load(Ordering::SeqCst) {
                    // Select and execute operation
                    let operation = user.select_operation(&pattern);
                    let result = user.execute_operation(&ops, operation).await;

                    // Record stats
                    stats.record(&result);

                    // Wait based on pattern
                    let delay = pattern.request_delay() / num_users;
                    tokio::time::sleep(delay).await;
                }

                info!(
                    user_id = user_id,
                    objects_created = user.object_count(),
                    "Virtual user stopped"
                );
            });

            handles.push(handle);
        }

        // Spawn summary reporter
        let stats = self.stats.clone();
        let running = self.running.clone();
        let summary_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.tick().await; // Skip immediate tick

            while running.load(Ordering::SeqCst) {
                interval.tick().await;
                let summary = stats.summary(start.elapsed());
                summary.log();
            }
        });

        // Wait for shutdown signal
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal");
            }
        }

        // Stop all users
        info!("Stopping virtual users...");
        self.running.store(false, Ordering::SeqCst);

        // Wait for users to finish
        for handle in handles {
            if let Err(e) = handle.await {
                warn!(error = %e, "User task panicked");
            }
        }

        summary_handle.abort();

        // Final summary
        info!("Final statistics:");
        let summary = self.stats.summary(start.elapsed());
        summary.log();

        info!("Traffic simulator stopped");
        Ok(())
    }
}
