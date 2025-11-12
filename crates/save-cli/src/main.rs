use anyhow::Result;
use aws_config::BehaviorVersion;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use clap::{Parser, Subcommand};
use tokio::io::{self, AsyncWriteExt};

#[derive(Parser)]
#[command(name = "save")]
#[command(about = "CLI tool for save object storage", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, env = "S3_ENDPOINT", default_value = "http://localhost:9000")]
    endpoint: String,

    #[arg(long, env = "AWS_ACCESS_KEY_ID", default_value = "saveadmin")]
    access_key: String,

    #[arg(long, env = "AWS_SECRET_ACCESS_KEY", default_value = "savepass")]
    secret_key: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Check server health
    Health,

    /// Bucket operations
    #[command(subcommand)]
    Bucket(BucketCommands),

    /// Object operations
    #[command(subcommand)]
    Object(ObjectCommands),
}

#[derive(Subcommand)]
enum BucketCommands {
    /// List all buckets
    List,
    /// Create a new bucket
    Create { name: String },
    /// Delete a bucket
    Delete { name: String },
}

#[derive(Subcommand)]
enum ObjectCommands {
    /// List objects in a bucket
    List {
        bucket: String,
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Get an object
    Get { bucket: String, key: String },
    /// Put an object
    Put {
        bucket: String,
        key: String,
        file: String,
    },
    /// Delete an object
    Delete { bucket: String, key: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Health => check_health(&cli.endpoint).await,
        Commands::Bucket(cmd) => handle_bucket(cmd, &cli).await,
        Commands::Object(cmd) => handle_object(cmd, &cli).await,
    }
}

async fn check_health(endpoint: &str) -> Result<()> {
    let url = format!("{}/health", endpoint);
    let response = reqwest::get(&url).await?;

    if response.status().is_success() {
        let body = response.text().await?;
        println!("{}", body);
        Ok(())
    } else {
        anyhow::bail!("Health check failed: {}", response.status());
    }
}

async fn create_client(cli: &Cli) -> Client {
    let credentials = Credentials::new(&cli.access_key, &cli.secret_key, None, None, "static");

    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(&cli.endpoint)
        .force_path_style(true)
        .build();

    Client::from_conf(s3_config)
}

async fn handle_bucket(cmd: &BucketCommands, cli: &Cli) -> Result<()> {
    let client = create_client(cli).await;

    match cmd {
        BucketCommands::List => {
            let response = client.list_buckets().send().await?;
            for bucket in response.buckets() {
                if let Some(name) = bucket.name() {
                    println!("{}", name);
                }
            }
        }
        BucketCommands::Create { name } => {
            client.create_bucket().bucket(name.as_str()).send().await?;
            println!("Bucket '{}' created", name);
        }
        BucketCommands::Delete { name } => {
            client.delete_bucket().bucket(name.as_str()).send().await?;
            println!("Bucket '{}' deleted", name);
        }
    }

    Ok(())
}

async fn handle_object(cmd: &ObjectCommands, cli: &Cli) -> Result<()> {
    let client = create_client(cli).await;

    match cmd {
        ObjectCommands::List { bucket, prefix } => {
            let mut req = client.list_objects_v2().bucket(bucket.as_str());
            if let Some(p) = prefix {
                req = req.prefix(p.as_str());
            }

            let response = req.send().await?;
            for object in response.contents() {
                if let Some(key) = object.key() {
                    println!("{}", key);
                }
            }
        }
        ObjectCommands::Get { bucket, key } => {
            let response = client
                .get_object()
                .bucket(bucket.as_str())
                .key(key.as_str())
                .send()
                .await?;

            let data = response.body.collect().await?;
            let bytes = data.into_bytes();

            let mut stdout = io::stdout();
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
        ObjectCommands::Put { bucket, key, file } => {
            let body = tokio::fs::read(file).await?;
            client
                .put_object()
                .bucket(bucket.as_str())
                .key(key.as_str())
                .body(body.into())
                .send()
                .await?;
            println!("Object '{}/{}' uploaded", bucket, key);
        }
        ObjectCommands::Delete { bucket, key } => {
            client
                .delete_object()
                .bucket(bucket.as_str())
                .key(key.as_str())
                .send()
                .await?;
            println!("Object '{}/{}' deleted", bucket, key);
        }
    }

    Ok(())
}
