use crate::benchmark::{BenchmarkRunner, Operation, RequestMetrics};
use crate::config::LoadTestConfig;
use crate::objects::ObjectGenerator;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

pub struct WorkloadExecutor {
    runner: Arc<BenchmarkRunner>,
    config: LoadTestConfig,
    generator: ObjectGenerator,
    uploaded_keys: Arc<Mutex<Vec<String>>>,
}

impl WorkloadExecutor {
    pub fn new(config: LoadTestConfig) -> Self {
        let runner = Arc::new(BenchmarkRunner::new(config.clone()));
        let generator = ObjectGenerator::new("test-objects");
        let uploaded_keys = Arc::new(Mutex::new(Vec::new()));

        Self {
            runner,
            config,
            generator,
            uploaded_keys,
        }
    }

    pub async fn run_mixed_workload(&self) -> crate::Result<Vec<RequestMetrics>> {
        let start_time = Instant::now();
        let duration = std::time::Duration::from_secs(self.config.workload.duration_secs);
        let mut all_metrics = Vec::new();

        let put_weight = self.config.scenarios.mixed.put_weight;
        let get_weight = self.config.scenarios.mixed.get_weight;
        let delete_weight = self.config.scenarios.mixed.delete_weight;
        let list_weight = self.config.scenarios.mixed.list_weight;
        let total_weight = put_weight + get_weight + delete_weight + list_weight;

        println!("Starting mixed workload:");
        println!("  Duration: {}s", self.config.workload.duration_secs);
        println!("  Concurrent users: {}", self.config.workload.users.max);
        println!(
            "  Operation mix: PUT:{}% GET:{}% DELETE:{}% LIST:{}%",
            put_weight * 100 / total_weight,
            get_weight * 100 / total_weight,
            delete_weight * 100 / total_weight,
            list_weight * 100 / total_weight
        );

        let mut tasks = JoinSet::new();

        for _ in 0..self.config.workload.users.max {
            let runner = self.runner.clone();
            let uploaded_keys = self.uploaded_keys.clone();
            let generator = self.generator.clone();
            let config = self.config.clone();

            tasks.spawn(async move {
                let mut metrics = Vec::new();

                while start_time.elapsed() < duration {
                    let roll = rand::rng().random_range(0..total_weight);

                    let operation = if roll < put_weight {
                        Operation::Put
                    } else if roll < put_weight + get_weight {
                        Operation::Get
                    } else if roll < put_weight + get_weight + delete_weight {
                        Operation::Delete
                    } else {
                        Operation::List
                    };

                    let result = match operation {
                        Operation::Put => {
                            let size = config.workload.object_sizes.sample();
                            let key = generator.random_key();
                            let data = generator.random_data(size);
                            let metric = runner.put_object(&key, data).await?;

                            if metric.success {
                                uploaded_keys.lock().await.push(key);
                            }

                            metric
                        }
                        Operation::Get => {
                            let keys = uploaded_keys.lock().await;
                            if keys.is_empty() {
                                continue;
                            }
                            let idx = rand::rng().random_range(0..keys.len());
                            let key = keys[idx].clone();
                            drop(keys);

                            runner.get_object(&key).await?
                        }
                        Operation::Delete => {
                            let mut keys = uploaded_keys.lock().await;
                            if keys.is_empty() {
                                continue;
                            }
                            let idx = rand::rng().random_range(0..keys.len());
                            let key = keys.swap_remove(idx);
                            drop(keys);

                            runner.delete_object(&key).await?
                        }
                        Operation::List => runner.list_objects().await?,
                        Operation::Head => unreachable!(),
                    };

                    metrics.push(result);
                }

                Ok::<Vec<RequestMetrics>, anyhow::Error>(metrics)
            });
        }

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok(mut user_metrics)) => {
                    all_metrics.append(&mut user_metrics);
                }
                Ok(Err(e)) => {
                    eprintln!("User task failed: {}", e);
                }
                Err(e) => {
                    eprintln!("Task join failed: {}", e);
                }
            }
        }

        Ok(all_metrics)
    }

    pub async fn run_write_heavy_workload(&self) -> crate::Result<Vec<RequestMetrics>> {
        let start_time = Instant::now();
        let duration = std::time::Duration::from_secs(self.config.workload.duration_secs);
        let mut all_metrics = Vec::new();

        println!("Starting write-heavy workload (90% PUT, 10% GET)");
        let mut tasks = JoinSet::new();

        for _ in 0..self.config.workload.users.max {
            let runner = self.runner.clone();
            let uploaded_keys = self.uploaded_keys.clone();
            let generator = self.generator.clone();
            let config = self.config.clone();

            tasks.spawn(async move {
                let mut metrics = Vec::new();

                while start_time.elapsed() < duration {
                    let is_put = rand::rng().random_range(0..100) < 90;

                    let metric = if is_put {
                        let size = config.workload.object_sizes.sample();
                        let key = generator.random_key();
                        let data = generator.random_data(size);
                        let m = runner.put_object(&key, data).await?;

                        if m.success {
                            uploaded_keys.lock().await.push(key);
                        }
                        m
                    } else {
                        let keys = uploaded_keys.lock().await;
                        if keys.is_empty() {
                            continue;
                        }
                        let idx = rand::rng().random_range(0..keys.len());
                        let key = keys[idx].clone();
                        drop(keys);

                        runner.get_object(&key).await?
                    };

                    metrics.push(metric);
                }

                Ok::<Vec<RequestMetrics>, anyhow::Error>(metrics)
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Ok(Ok(mut user_metrics)) = result {
                all_metrics.append(&mut user_metrics);
            }
        }

        Ok(all_metrics)
    }

    pub async fn run_read_heavy_workload(&self) -> crate::Result<Vec<RequestMetrics>> {
        println!("Pre-populating objects for read-heavy test...");
        let num_objects = 100;

        for i in 0..num_objects {
            let size = self.config.workload.object_sizes.sample();
            let key = format!("read-test-{}", i);
            let data = self.generator.random_data(size);

            let metric = self.runner.put_object(&key, data).await?;
            if metric.success {
                self.uploaded_keys.lock().await.push(key);
            }
        }

        println!("Pre-populated {} objects", num_objects);
        println!("Starting read-heavy workload (90% GET, 10% LIST)");

        let start_time = Instant::now();
        let duration = std::time::Duration::from_secs(self.config.workload.duration_secs);
        let mut all_metrics = Vec::new();

        let mut tasks = JoinSet::new();

        for _ in 0..self.config.workload.users.max {
            let runner = self.runner.clone();
            let uploaded_keys = self.uploaded_keys.clone();

            tasks.spawn(async move {
                let mut metrics = Vec::new();

                while start_time.elapsed() < duration {
                    let is_get = rand::rng().random_range(0..100) < 90;

                    let metric = if is_get {
                        let keys = uploaded_keys.lock().await;
                        if keys.is_empty() {
                            continue;
                        }
                        let idx = rand::rng().random_range(0..keys.len());
                        let key = keys[idx].clone();
                        drop(keys);

                        runner.get_object(&key).await?
                    } else {
                        runner.list_objects().await?
                    };

                    metrics.push(metric);
                }

                Ok::<Vec<RequestMetrics>, anyhow::Error>(metrics)
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Ok(Ok(mut user_metrics)) = result {
                all_metrics.append(&mut user_metrics);
            }
        }

        Ok(all_metrics)
    }
}

pub fn print_progress(metrics: &[RequestMetrics], duration_secs: u64) {
    let total = metrics.len();
    let successful = metrics.iter().filter(|m| m.success).count();
    let failed = total - successful;

    let mut ops_by_type: HashMap<Operation, usize> = HashMap::new();
    for metric in metrics {
        *ops_by_type.entry(metric.operation).or_insert(0) += 1;
    }

    println!("\nProgress:");
    println!("  Total requests: {}", total);
    println!("  Successful: {}", successful);
    println!("  Failed: {}", failed);
    println!("  Requests/sec: {:.2}", total as f64 / duration_secs as f64);

    for (op, count) in ops_by_type {
        println!("  {}: {}", op, count);
    }
}
