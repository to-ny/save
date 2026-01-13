//! Traffic simulator for Save distributed object store.

use clap::Parser;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod config;
mod operations;
mod patterns;
mod simulator;
mod user;

pub use config::Config;
pub use operations::{OperationResult, OperationType, Operations};
pub use patterns::TrafficPattern;
pub use simulator::Simulator;
pub use user::VirtualUser;

#[derive(Parser, Debug)]
#[command(name = "save-traffic-simulator")]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, env = "SAVE_SIM_CONFIG")]
    config: Option<String>,

    #[arg(long, env = "SAVE_SIM_ENDPOINT")]
    endpoint: Option<String>,

    #[arg(long, env = "SAVE_SIM_ACCESS_KEY")]
    access_key: Option<String>,

    #[arg(long, env = "SAVE_SIM_SECRET_KEY")]
    secret_key: Option<String>,

    #[arg(long, env = "SAVE_SIM_BUCKET")]
    bucket: Option<String>,

    #[arg(long, env = "SAVE_SIM_VIRTUAL_USERS")]
    virtual_users: Option<u32>,

    #[arg(long, env = "SAVE_SIM_RPS")]
    rps: Option<f64>,

    #[arg(long, default_value = "info", env = "SAVE_SIM_LOG_LEVEL")]
    log_level: String,

    #[arg(long, default_value = "json", env = "SAVE_SIM_LOG_FORMAT")]
    log_format: String,
}

fn setup_logging(level: &str, format: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    match format {
        "json" => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .init();
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Setup logging
    setup_logging(&args.log_level, &args.log_format);

    info!("Save Traffic Simulator starting");

    // Load configuration
    let mut config = if let Some(config_path) = &args.config {
        Config::from_file(config_path)?
    } else {
        // Create default config, require endpoint/access_key/secret_key from args
        let endpoint = args
            .endpoint
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Either --config or --endpoint is required"))?;
        let access_key = args
            .access_key
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Either --config or --access-key is required"))?;
        let secret_key = args
            .secret_key
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Either --config or --secret-key is required"))?;
        let bucket = args
            .bucket
            .clone()
            .unwrap_or_else(|| "demo-bucket".to_string());

        Config {
            target: config::TargetConfig {
                endpoint,
                access_key,
                secret_key,
                bucket,
                region: "us-east-1".to_string(),
            },
            simulation: config::SimulationConfig {
                pattern: config::PatternConfig::Mixed,
                virtual_users: args.virtual_users.unwrap_or(10),
                read_ratio: 0.7,
                write_ratio: 0.3,
                requests_per_second: args.rps.unwrap_or(5.0),
            },
            objects: config::ObjectConfig::default(),
            logging: config::LoggingConfig::default(),
        }
    };

    // Apply CLI overrides (only if config was loaded from file)
    if args.config.is_some() {
        if let Some(endpoint) = args.endpoint {
            config.target.endpoint = endpoint;
        }
        if let Some(access_key) = args.access_key {
            config.target.access_key = access_key;
        }
        if let Some(secret_key) = args.secret_key {
            config.target.secret_key = secret_key;
        }
        if let Some(bucket) = args.bucket {
            config.target.bucket = bucket;
        }
        if let Some(virtual_users) = args.virtual_users {
            config.simulation.virtual_users = virtual_users;
        }
        if let Some(rps) = args.rps {
            config.simulation.requests_per_second = rps;
        }
    }

    // Create and run simulator
    let simulator = Simulator::new(config).await?;
    simulator.run().await?;

    Ok(())
}
