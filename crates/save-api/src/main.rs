use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use std::net::SocketAddr;
use std::path::Path;
use tracing::{error, info};

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "save_api=info,tower_http=info".into()),
        )
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config_path = std::env::var("SAVE_CONFIG").unwrap_or_else(|_| "save.toml".to_string());
    info!("Loading configuration from: {}", config_path);

    let config = if Path::new(&config_path).exists() {
        SaveConfig::load(Path::new(&config_path))?
    } else {
        info!("Config file not found, using defaults");
        SaveConfig::default()
    };

    info!("Initializing storage at: {}", config.storage.data_path);
    let storage = ObjectStorage::new(&config.storage.data_path).await?;

    info!(
        "Initializing metadata at: {}",
        config.storage.metadata_path
    );
    let metadata = MetadataStore::new(&config.storage.metadata_path)?;

    let bind_addr = config.server.bind_address.clone();
    let state = save_api::AppState::new(storage, metadata, config);

    let app = save_api::app(state);
    let addr: SocketAddr = bind_addr.parse()?;

    info!("Starting save-api server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("Server listening on http://{}", addr);

    match axum::serve(listener, app).await {
        Ok(_) => {
            info!("Server shutdown gracefully");
            Ok(())
        }
        Err(e) => {
            error!("Server error: {}", e);
            Err(e.into())
        }
    }
}
