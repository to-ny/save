use super::environment::TestEnvironment;
use super::verify::{verify_no_orphans_or_gc_pending, verify_no_phantom_objects};
use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub struct SingleNodeEnv {
    child: Option<Child>,
    port: u16,
    _data_dir: TempDir, // Keep TempDir alive
    data_path: PathBuf,
    metadata_path: PathBuf,
    _config_path: PathBuf, // Keep config file alive
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

[auth]
access_key = "test-access-key"
secret_key = "test-secret-key"
"#,
            port,
            data_dir.path().display(),
            data_dir.path().display()
        );

        let config_path = data_dir.path().join("config.toml");
        std::fs::write(&config_path, config)?;

        // Build binary with failpoints enabled
        ensure_binary_built_with_failpoints()?;

        // Spawn server
        let child = Command::new("target/debug/save-api")
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

    fn configure_failpoint(&self, name: &str, action: &str) -> Result<()> {
        // Configure failpoint using the fail crate
        fail::cfg(name, action)
            .map_err(|e| anyhow::anyhow!("Failed to configure failpoint {}: {}", name, e))
    }

    async fn wait_for_failpoint(&self, _name: &str) -> Result<()> {
        // When using "pause" action, the failpoint blocks the thread.
        // We detect this by checking if the server stops responding to health checks
        // or by using a marker file approach.

        // For now, use a simple time-based approach: wait a bit for the failpoint to be hit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // TODO: Implement more robust detection using HTTP endpoint or marker files
        Ok(())
    }

    async fn crash_at_failpoint(&mut self, name: &str) -> Result<()> {
        if let Some(ref mut child) = self.child {
            let pid = Pid::from_raw(child.id() as i32);
            kill(pid, Signal::SIGKILL).context("Failed to send SIGKILL")?;
            child.wait().context("Failed to wait for child process")?;
        }
        self.child = None;

        // Remove failpoint config so restart works normally
        fail::remove(name);

        Ok(())
    }

    async fn restart(&mut self) -> Result<()> {
        let child = Command::new("target/debug/save-api")
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

fn ensure_binary_built_with_failpoints() -> Result<()> {
    let binary_path = "target/debug/save-api";

    // Check if binary exists and was built recently
    if let Ok(metadata) = std::fs::metadata(binary_path)
        && let Ok(modified) = metadata.modified()
        && let Ok(elapsed) = modified.elapsed()
    {
        // If binary was built within last 5 minutes, assume it's up to date
        if elapsed < Duration::from_secs(300) {
            return Ok(());
        }
    }

    println!("Building save-api with failpoints enabled...");
    // TODO Fails with message = "error: the package 'crash-recovery-tests' does not contain this feature: failpoints"
    let status = Command::new("cargo")
        .args(["build", "--bin", "save-api", "--features", "failpoints"])
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
