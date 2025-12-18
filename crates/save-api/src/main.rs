use save_common::config::SaveConfig;
use save_metadata::MetadataError;
use save_metadata::raft::{RaftNode, run_server as run_raft_server};
use save_metadata::MetadataStore;
use save_storage::LocalBackend;
use socket2::{Domain, Socket, Type};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "save_api=info,tower_http=info".into());

    match std::env::var("LOG_FORMAT").as_deref() {
        Ok("json") => {
            tracing_subscriber::fmt()
                .json()
                .with_writer(std::io::stderr)
                .with_env_filter(env_filter)
                .with_target(true)
                .with_current_span(false)
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(env_filter)
                .init();
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

/// Initialize storage and metadata backends.
async fn init_storage(config: &SaveConfig) -> anyhow::Result<(LocalBackend, MetadataStore)> {
    info!("Initializing storage at: {}", config.storage.data_path);
    let storage =
        LocalBackend::new_with_fsync_mode(&config.storage.data_path, &config.storage.fsync_mode)
            .await?;

    info!("Initializing metadata at: {}", config.storage.metadata_path);
    info!(
        "Metadata config - write_buffer: {}MB, cache: {}MB, bg_jobs: {}",
        config.metadata.write_buffer_size_mb,
        config.metadata.block_cache_size_mb,
        config.metadata.max_background_jobs
    );
    let metadata = MetadataStore::new_with_config(&config.storage.metadata_path, &config.metadata)?;

    Ok((storage, metadata))
}

/// Initialize Raft node with error recovery for corrupted state.
async fn init_raft_node(metadata: &MetadataStore, config: &SaveConfig) -> anyhow::Result<RaftNode> {
    info!(
        "Initializing Raft cluster - node_id: {}, raft_bind_addr: {}",
        config.cluster.node_id, config.cluster.raft_bind_addr
    );

    let raft_node = match RaftNode::from_cluster_config(metadata.db(), &config.cluster).await {
        Ok(node) => node,
        Err(e) => {
            if e.is_recoverable_raft_corruption() {
                warn!("Raft state corrupted: {}. Clearing and recovering...", e);

                if let Err(clear_err) = metadata.clear_raft_state() {
                    error!("Failed to clear Raft state: {}", clear_err);
                    return Err(e.into());
                }

                info!("Cleared corrupted Raft state, retrying initialization...");
                RaftNode::from_cluster_config(metadata.db(), &config.cluster).await?
            } else {
                error!("Raft initialization failed: {}", e);
                return Err(e.into());
            }
        }
    };

    // Auto-bootstrap standalone clusters (no seed_nodes configured)
    if config.cluster.seed_nodes.is_empty() && !raft_node.is_initialized() {
        info!("Standalone cluster, auto-bootstrapping Raft with single member");
        let raft_addr = format!("http://{}", config.cluster.raft_bind_addr);
        let http_addr = format!("http://{}", config.server.bind_address);
        match raft_node
            .initialize(vec![(config.cluster.node_id, raft_addr, http_addr)])
            .await
        {
            Ok(()) => info!("Raft cluster bootstrapped successfully"),
            Err(MetadataError::AlreadyInitialized) => {
                info!("Raft cluster already initialized, continuing");
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    Ok(raft_node)
}

/// Start the Raft gRPC server with readiness signaling.
///
/// Returns the server handle. Waits for the server to signal readiness or fail.
async fn start_raft_server(
    raft_node: &RaftNode,
    addr: SocketAddr,
    shutdown_tx: &broadcast::Sender<()>,
) -> anyhow::Result<JoinHandle<Result<(), anyhow::Error>>> {
    let raft = raft_node.raft().clone();
    let shutdown_rx = shutdown_tx.subscribe();
    let (ready_tx, ready_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        if let Err(e) = run_raft_server(raft, addr, Some(shutdown_rx), Some(ready_tx)).await {
            error!("Raft server failed: {}", e);
            Err(e)
        } else {
            Ok(())
        }
    });

    // Wait for either readiness signal or server failure
    tokio::select! {
        _ = ready_rx => {
            info!("Raft gRPC server started successfully on {}", addr);
            Ok(handle)
        }
        result = &mut Box::pin(async { handle.is_finished().then_some(()) }) => {
            // If handle finished before ready signal, server failed
            if result.is_some() {
                let join_result = handle.await;
                match join_result {
                    Ok(Err(e)) => Err(anyhow::anyhow!("Raft server failed to start: {}", e)),
                    Err(e) => Err(anyhow::anyhow!("Raft server task panicked: {}", e)),
                    Ok(Ok(())) => Err(anyhow::anyhow!("Raft server exited unexpectedly")),
                }
            } else {
                // This shouldn't happen but handle it
                Ok(handle)
            }
        }
    }
}

/// Start the internal gRPC API server with readiness signaling.
///
/// Returns the server handle if configured, or None if disabled.
async fn start_internal_api_server(
    state: &save_api::AppState,
    config: &SaveConfig,
    shutdown_tx: &broadcast::Sender<()>,
) -> anyhow::Result<Option<JoinHandle<()>>> {
    if config.cluster.internal_api.bind_addr.is_empty() {
        info!("Internal API server disabled (no bind address configured)");
        return Ok(None);
    }

    let addr: SocketAddr = config.cluster.internal_api.bind_addr.parse()?;
    let internal_api_state = state.clone();
    let shutdown_rx = shutdown_tx.subscribe();
    let tls_config = config
        .cluster
        .internal_api
        .tls
        .as_ref()
        .or(config.cluster.replication.tls.as_ref())
        .cloned();
    let require_auth = config.cluster.internal_api.require_auth;
    let (ready_tx, ready_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        if let Err(e) = save_api::run_internal_api_server(
            internal_api_state,
            addr,
            Some(shutdown_rx),
            tls_config.as_ref(),
            require_auth,
            Some(ready_tx),
        )
        .await
        {
            error!("Internal API server failed: {}", e);
        }
    });

    // Wait for either readiness signal or server failure
    tokio::select! {
        _ = ready_rx => {
            info!("Internal gRPC API server started on {}", addr);
            Ok(Some(handle))
        }
        _ = async {
            while !handle.is_finished() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        } => {
            let result = handle.await;
            match result {
                Ok(()) => Err(anyhow::anyhow!("Internal API server exited unexpectedly")),
                Err(e) => Err(anyhow::anyhow!("Internal API server task panicked: {}", e)),
            }
        }
    }
}

/// Background workers that run for the lifetime of the server.
struct BackgroundWorkers {
    gc_handle: JoinHandle<()>,
    metrics_handle: JoinHandle<()>,
    cache_cleanup_handle: JoinHandle<()>,
}

/// Spawn all background workers (GC, metrics, cache cleanup).
fn spawn_background_workers(
    state: &save_api::AppState,
    config: &SaveConfig,
    shutdown_tx: &broadcast::Sender<()>,
    temp_dir: std::path::PathBuf,
) -> BackgroundWorkers {
    // GC worker
    let gc_config = save_api::GcConfig {
        interval: Duration::from_secs(config.storage.gc_interval_secs),
        temp_file_max_age: Duration::from_secs(config.storage.gc_temp_file_max_age_secs),
    };
    let gc_metadata = Arc::clone(&state.metadata);
    let gc_shutdown_rx = shutdown_tx.subscribe();

    info!(
        interval_secs = gc_config.interval.as_secs(),
        max_age_secs = gc_config.temp_file_max_age.as_secs(),
        "Starting GC worker"
    );

    let gc_handle = tokio::spawn(async move {
        if let Err(e) =
            save_api::run_gc_worker(gc_metadata, temp_dir, gc_config, gc_shutdown_rx).await
        {
            error!("GC worker failed: {}", e);
        }
    });

    // Metrics worker
    let metrics_state = state.clone();
    let mut metrics_shutdown = shutdown_tx.subscribe();
    let metrics_interval_secs = config.server.metrics_interval_secs;
    info!(
        interval_secs = metrics_interval_secs,
        "Starting metrics collection worker"
    );

    let metrics_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(metrics_interval_secs));
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

    // Cache cleanup worker
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

    BackgroundWorkers {
        gc_handle,
        metrics_handle,
        cache_cleanup_handle,
    }
}

/// Create a TCP listener with SO_REUSEADDR for faster restart after crash.
fn create_tcp_listener(addr: SocketAddr) -> anyhow::Result<tokio::net::TcpListener> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(socket.into())?;
    Ok(listener)
}

/// Spawn the auto-join worker for multi-node clusters.
fn spawn_auto_join_worker(
    raft_node: Arc<RaftNode>,
    config: &SaveConfig,
    shutdown_tx: &broadcast::Sender<()>,
) -> Option<JoinHandle<()>> {
    if config.cluster.seed_nodes.is_empty() {
        return None;
    }

    let join_config = config.cluster.clone();
    let join_http_addr = format!("http://{}", config.server.bind_address);
    let join_shutdown_rx = shutdown_tx.subscribe();

    Some(tokio::spawn(async move {
        save_api::scaling::run_auto_join_worker(
            raft_node,
            join_config,
            join_http_addr,
            join_shutdown_rx,
        )
        .await;
    }))
}

/// Handle graceful shutdown: drain requests and leave cluster if multi-node.
async fn handle_graceful_shutdown(
    shutdown_tx: broadcast::Sender<()>,
    request_tracker: Arc<save_api::RequestTracker>,
    drain_timeout: Duration,
    auto_join_handle: Option<JoinHandle<()>>,
    raft_node: Arc<RaftNode>,
    config: &SaveConfig,
) {
    // Register signal handlers
    #[cfg(unix)]
    let mut sigterm = {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler")
    };

    // Wait for shutdown signal
    #[cfg(unix)]
    {
        tokio::select! {
            _ = signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = signal::ctrl_c().await;
    }

    debug!("Shutdown signal received");
    info!("Shutdown signal received, initiating graceful shutdown");

    let _ = shutdown_tx.send(());

    let is_multi_node = !config.cluster.seed_nodes.is_empty();

    // Wait for auto-join worker to complete before graceful leave.
    // This prevents race conditions where auto-join re-adds us after we leave.
    if let Some(handle) = auto_join_handle {
        debug!("Waiting for auto-join worker to complete before graceful leave");
        let join_timeout = Duration::from_secs(config.cluster.join_timeout_secs);
        match tokio::time::timeout(join_timeout, handle).await {
            Ok(Ok(())) => debug!("Auto-join worker completed"),
            Ok(Err(e)) => warn!("Auto-join worker panicked: {}", e),
            Err(_) => warn!("Auto-join worker didn't complete within timeout"),
        }
    }

    // Drain in-flight requests
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

    // Graceful leave: remove node from cluster before shutdown (multi-node only)
    if is_multi_node {
        debug!("Multi-node cluster, initiating graceful leave");
        info!("Removing node from cluster before shutdown");
        match save_api::scaling::graceful_leave(raft_node, &config.cluster).await {
            Ok(true) => {
                info!("Successfully left cluster");
            }
            Ok(false) => {
                debug!("Node was not a cluster member");
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Graceful leave failed. Node may remain in cluster membership \
                     until manually removed or cluster detects it as unhealthy."
                );
            }
        }
    } else {
        debug!("Standalone cluster, no other nodes to notify");
    }
}

async fn async_main_with_config(config: SaveConfig) -> anyhow::Result<()> {
    info!("Starting save object storage server");

    // Initialize storage and metadata
    let (storage, metadata) = init_storage(&config).await?;

    // Shutdown channel for coordinating graceful shutdown
    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    // Initialize Raft
    let raft_node = init_raft_node(&metadata, &config).await?;

    // Start Raft gRPC server
    let raft_addr: SocketAddr = config.cluster.raft_bind_addr.parse()?;
    let raft_handle = start_raft_server(&raft_node, raft_addr, &shutdown_tx).await?;

    // Mark standalone clusters as joined immediately
    if config.cluster.seed_nodes.is_empty() {
        save_api::scaling::CLUSTER_JOINED.store(true, std::sync::atomic::Ordering::Release);
    }

    // Get temp_dir before moving storage into AppState
    let temp_dir = storage.temp_dir();

    // Create application state
    let state = save_api::AppState::new(storage, metadata, config.clone(), raft_node);

    // Start internal gRPC API server
    let internal_api_handle = start_internal_api_server(&state, &config, &shutdown_tx).await?;

    // Start background workers
    let workers = spawn_background_workers(&state, &config, &shutdown_tx, temp_dir);

    // Create TCP listener for HTTP server
    let addr: SocketAddr = config.server.bind_address.parse()?;
    let listener = create_tcp_listener(addr)?;
    info!("Server listening on http://{}", addr);

    // Spawn auto-join worker (after HTTP server is ready so health checks pass)
    let auto_join_handle =
        spawn_auto_join_worker(Arc::clone(&state.raft_node), &config, &shutdown_tx);

    // Prepare shutdown handler context
    let request_tracker = Arc::clone(&state.request_tracker);
    let drain_timeout = Duration::from_secs(config.shutdown.drain_timeout_secs);
    let raft_node_for_leave = Arc::clone(&state.raft_node);
    let config_for_shutdown = config.clone();

    // Build the application
    let app = save_api::app(state);

    info!("Starting save-api server on {}", addr);

    // Run server with graceful shutdown
    let graceful = axum::serve(listener, app).with_graceful_shutdown(async move {
        handle_graceful_shutdown(
            shutdown_tx,
            request_tracker,
            drain_timeout,
            auto_join_handle,
            raft_node_for_leave,
            &config_for_shutdown,
        )
        .await;
    });

    match graceful.await {
        Ok(_) => {
            info!("Server shutdown gracefully");
            let _ = workers.gc_handle.await;
            let _ = workers.metrics_handle.await;
            let _ = workers.cache_cleanup_handle.await;
            if let Some(handle) = internal_api_handle {
                info!("Waiting for internal API server to shutdown");
                let _ = handle.await;
            }
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
