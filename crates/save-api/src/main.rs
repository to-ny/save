use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_metadata::raft::{RaftNode, run_server as run_raft_server};
use save_storage::ObjectStorage;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{error, info};

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "save_api=info,tower_http=info".into());

    match std::env::var("LOG_FORMAT").as_deref() {
        Ok("json") => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(env_filter)
                .with_target(true)
                .with_current_span(false)
                .init();
        }
        _ => {
            tracing_subscriber::fmt().with_env_filter(env_filter).init();
        }
    }
}

fn main() -> anyhow::Result<()> {
    init_tracing();

    let config_path = std::env::var("SAVE_CONFIG").unwrap_or_else(|_| "save.toml".to_string());
    info!("Loading configuration from: {}", config_path);

    let config = if Path::new(&config_path).exists() {
        SaveConfig::load(Path::new(&config_path))?
    } else {
        info!("Config file not found, using defaults");
        SaveConfig::default()
    };

    info!(
        "Configuring tokio runtime - worker_threads: {}, max_blocking_threads: {}",
        config.server.worker_threads, config.server.max_blocking_threads
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(config.server.worker_threads)
        .max_blocking_threads(config.server.max_blocking_threads)
        .enable_all()
        .build()?;

    runtime.block_on(async_main_with_config(config))
}

async fn async_main_with_config(config: SaveConfig) -> anyhow::Result<()> {
    info!("Starting save object storage server");

    info!("Initializing storage at: {}", config.storage.data_path);
    let storage =
        ObjectStorage::new_with_fsync_mode(&config.storage.data_path, &config.storage.fsync_mode)
            .await?;

    info!("Initializing metadata at: {}", config.storage.metadata_path);
    info!(
        "Metadata config - write_buffer: {}MB, cache: {}MB, bg_jobs: {}",
        config.metadata.write_buffer_size_mb,
        config.metadata.block_cache_size_mb,
        config.metadata.max_background_jobs
    );
    let metadata = MetadataStore::new_with_config(&config.storage.metadata_path, &config.metadata)?;

    // Shutdown channel - we'll subscribe workers to this
    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(1);

    // Initialize Raft cluster
    info!(
        "Initializing Raft cluster - node_id: {}, raft_bind_addr: {}",
        config.cluster.node_id, config.cluster.raft_bind_addr
    );

    let raft_node = RaftNode::from_cluster_config(metadata.db(), &config.cluster).await?;

    // Auto-bootstrap single-node clusters
    if config.cluster.peers.is_empty() && !raft_node.is_initialized() {
        info!("Single-node cluster detected, auto-bootstrapping");
        let raft_addr = format!("http://{}", config.cluster.raft_bind_addr);
        raft_node
            .initialize(vec![(config.cluster.node_id, raft_addr)])
            .await?;
    }

    // Start Raft gRPC server with shutdown support
    let raft_addr: SocketAddr = config.cluster.raft_bind_addr.parse()?;
    let raft = raft_node.raft().clone();
    let raft_shutdown_rx = shutdown_tx.subscribe();
    let raft_handle = tokio::spawn(async move {
        if let Err(e) = run_raft_server(raft, raft_addr, Some(raft_shutdown_rx)).await {
            error!("Raft server failed: {}", e);
            Err(e)
        } else {
            Ok(())
        }
    });

    // Give the Raft server a moment to bind and verify it started
    tokio::time::sleep(Duration::from_millis(50)).await;
    if raft_handle.is_finished() {
        // Server failed to start - get the error
        let result = raft_handle.await;
        match result {
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("Raft server failed to start: {}", e));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Raft server task panicked: {}", e));
            }
            Ok(Ok(())) => {
                return Err(anyhow::anyhow!("Raft server exited unexpectedly"));
            }
        }
    }

    info!("Raft gRPC server started successfully on {}", raft_addr);

    let bind_addr = config.server.bind_address.clone();
    let state = save_api::AppState::new(storage, metadata, config.clone(), raft_node);

    let gc_config = save_api::GcConfig {
        interval: Duration::from_secs(config.storage.gc_interval_secs),
        temp_file_max_age: Duration::from_secs(config.storage.gc_temp_file_max_age_secs),
    };

    let temp_dir = PathBuf::from(&config.storage.data_path).join("temp");
    let gc_metadata = Arc::clone(&state.metadata);

    info!(
        interval_secs = gc_config.interval.as_secs(),
        max_age_secs = gc_config.temp_file_max_age.as_secs(),
        "Starting GC worker"
    );

    let gc_shutdown_rx = shutdown_tx.subscribe();
    let gc_handle = tokio::spawn(async move {
        if let Err(e) =
            save_api::run_gc_worker(gc_metadata, temp_dir, gc_config, gc_shutdown_rx).await
        {
            error!("GC worker failed: {}", e);
        }
    });

    let metrics_state = state.clone();
    let mut metrics_shutdown = shutdown_tx.subscribe();
    info!("Starting metrics collection worker");

    let metrics_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    save_api::collect_metrics(&metrics_state).await;
                }
                _ = metrics_shutdown.recv() => {
                    info!("Metrics collection worker shutting down");
                    break;
                }
            }
        }
    });

    let cache_cleanup_state = state.clone();
    let mut cache_cleanup_shutdown = shutdown_tx.subscribe();
    let cache_cleanup_interval_secs = config.server.bucket_cache_ttl_secs.max(60);
    info!(
        interval_secs = cache_cleanup_interval_secs,
        "Starting bucket cache cleanup worker"
    );

    let cache_cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(cache_cleanup_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    cache_cleanup_state.bucket_cache.cleanup_expired();
                }
                _ = cache_cleanup_shutdown.recv() => {
                    info!("Bucket cache cleanup worker shutting down");
                    break;
                }
            }
        }
    });

    let request_tracker = Arc::clone(&state.request_tracker);
    let drain_timeout = Duration::from_secs(config.shutdown.drain_timeout_secs);

    let app = save_api::app(state);
    let addr: SocketAddr = bind_addr.parse()?;

    info!("Starting save-api server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("Server listening on http://{}", addr);

    let graceful = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = signal::ctrl_c().await;
        info!("Shutdown signal received, initiating graceful shutdown");

        let _ = shutdown_tx.send(());

        info!(
            "Draining in-flight requests (timeout: {}s)",
            drain_timeout.as_secs()
        );

        let start = tokio::time::Instant::now();
        loop {
            let in_flight = request_tracker.in_flight_count();
            if in_flight == 0 {
                info!("All requests drained");
                break;
            }

            if start.elapsed() >= drain_timeout {
                info!(
                    in_flight_requests = in_flight,
                    "Drain timeout reached, forcing shutdown"
                );
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    match graceful.await {
        Ok(_) => {
            info!("Server shutdown gracefully");
            let _ = gc_handle.await;
            let _ = metrics_handle.await;
            let _ = cache_cleanup_handle.await;
            info!("Waiting for Raft server to shutdown");
            let _ = raft_handle.await;
            Ok(())
        }
        Err(e) => {
            error!("Server error: {}", e);
            Err(e.into())
        }
    }
}
