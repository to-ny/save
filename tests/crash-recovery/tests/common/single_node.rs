use super::environment::TestEnvironment;
use super::verify::{verify_no_orphans_or_gc_pending, verify_no_phantom_objects};
use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct ConfigureFailpointRequest {
    name: String,
    action: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ConfigureFailpointResponse {
    success: bool,
    message: String,
}

pub struct SingleNodeEnv {
    child: Option<Child>,
    port: u16,
    _data_dir: TempDir,
    data_path: PathBuf,
    metadata_path: PathBuf,
    _config_path: PathBuf,
    client: Client,
}

impl SingleNodeEnv {
    async fn wait_ready(&self) -> Result<()> {
        let url = format!("http://0.0.0.0:{}/health", self.port);
        let start = Instant::now();

        loop {
            if start.elapsed() > Duration::from_secs(30) {
                anyhow::bail!("Server failed to start within 30 seconds");
            }

            match reqwest::get(&url).await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

#[async_trait::async_trait]
impl TestEnvironment for SingleNodeEnv {
    async fn setup() -> Result<Self> {
        let data_dir = tempfile::tempdir()?;
        let port = find_free_port()?;
        let raft_port = find_free_port()?;

        // Create config
        let config = format!(
            r#"
[server]
bind_address = "0.0.0.0:{}"

[storage]
data_path = "{}/data"
metadata_path = "{}/metadata"
gc_interval_secs = 10
gc_temp_file_max_age_secs = 60

[credentials]
access_key = "test-access-key"
secret_key = "test-secret-key"

[cluster]
node_id = 1
raft_bind_addr = "127.0.0.1:{}"
"#,
            port,
            data_dir.path().display(),
            data_dir.path().display(),
            raft_port
        );

        let config_path = data_dir.path().join("config.toml");
        std::fs::write(&config_path, config)?;

        // Build binary with failpoints enabled
        ensure_binary_built_with_failpoints()?;

        // Spawn server
        let child = Command::new(get_binary_path())
            .env("SAVE_CONFIG", &config_path)
            .env("RUST_LOG", "info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to spawn save-api")?;

        let client = create_client(&format!("http://0.0.0.0:{}", port)).await;

        let data_path = data_dir.path().join("data");
        let metadata_path = data_dir.path().join("metadata");

        let env = Self {
            child: Some(child),
            port,
            _data_dir: data_dir,
            data_path,
            metadata_path,
            _config_path: config_path,
            client,
        };

        env.wait_ready().await?;
        Ok(env)
    }

    async fn configure_failpoint(&self, name: &str, action: &str) -> Result<()> {
        let url = format!("http://0.0.0.0:{}/_failpoint/configure", self.port);
        let req = ConfigureFailpointRequest {
            name: name.to_string(),
            action: action.to_string(),
        };

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&req)
            .send()
            .await
            .context("Failed to send failpoint configuration")?;

        let status = response.status();
        let body: ConfigureFailpointResponse = response
            .json()
            .await
            .context("Failed to parse failpoint response")?;

        if !status.is_success() || !body.success {
            anyhow::bail!("Failed to configure failpoint: {}", body.message);
        }

        Ok(())
    }

    async fn wait_for_failpoint(&self, _name: &str) -> Result<()> {
        // When using "pause" action, the failpoint blocks the request thread indefinitely.
        // Wait for the request to reach the failpoint and block.
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    async fn crash_at_failpoint(&mut self, name: &str) -> Result<()> {
        if let Some(ref mut child) = self.child {
            let pid = Pid::from_raw(child.id() as i32);
            kill(pid, Signal::SIGKILL).context("Failed to send SIGKILL")?;
            child.wait().context("Failed to wait for child process")?;
        }
        self.child = None;

        fail::remove(name);

        Ok(())
    }

    async fn restart(&mut self) -> Result<()> {
        let child = Command::new(get_binary_path())
            .env("SAVE_CONFIG", &self._config_path)
            .env("RUST_LOG", "info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to restart save-api")?;

        self.child = Some(child);
        self.wait_ready().await?;
        Ok(())
    }

    fn client(&self) -> &Client {
        &self.client
    }

    async fn verify_consistency(&self) -> Result<()> {
        // Need to verify with server stopped to avoid RocksDB lock conflicts
        // This is safe because verification is typically the last step in tests
        if let Some(ref child) = self.child {
            let pid = Pid::from_raw(child.id() as i32);
            kill(pid, Signal::SIGTERM).ok();

            // Wait briefly for graceful shutdown
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        verify_no_phantom_objects(self.data_dir(), self.metadata_dir()).await?;
        verify_no_orphans_or_gc_pending(self.data_dir(), self.metadata_dir()).await?;
        Ok(())
    }

    fn data_dir(&self) -> &Path {
        &self.data_path
    }

    fn metadata_dir(&self) -> &Path {
        &self.metadata_path
    }
}

impl Drop for SingleNodeEnv {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn find_free_port() -> Result<u16> {
    use std::net::TcpListener;
    let listener = TcpListener::bind("0.0.0.0:0")?;
    Ok(listener.local_addr()?.port())
}

fn get_binary_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/save-api")
}

fn ensure_binary_built_with_failpoints() -> Result<()> {
    let status = Command::new("cargo")
        .args(["build", "-p", "save-api", "--features", "failpoints"])
        .stdout(std::process::Stdio::null())
        .status()
        .context("Failed to run cargo build")?;

    if !status.success() {
        anyhow::bail!("Failed to build save-api with failpoints");
    }

    Ok(())
}

async fn create_client(endpoint: &str) -> Client {
    let credentials = Credentials::new("test-access-key", "test-secret-key", None, None, "static");

    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(endpoint)
        .force_path_style(true)
        .build();

    Client::from_conf(s3_config)
}
