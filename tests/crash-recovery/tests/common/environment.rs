use anyhow::Result;
use aws_sdk_s3::Client;
use std::path::Path;

/// Abstraction over test deployment - works for single-node AND distributed!
///
/// This trait allows the same test scenarios to run on both:
/// - Phase 1: Single-node subprocess (current)
/// - Phase 2: Multi-node Docker cluster (future)
#[async_trait::async_trait]
#[allow(dead_code)] // Some methods only used with crash_tests feature
pub trait TestEnvironment: Sized {
    /// Setup test environment
    async fn setup() -> Result<Self>;

    /// Configure failpoint (pause, return error, panic, etc.)
    fn configure_failpoint(&self, name: &str, action: &str) -> Result<()>;

    /// Wait for failpoint to be hit (process paused or marker file created)
    async fn wait_for_failpoint(&self, name: &str) -> Result<()>;

    /// Crash at the given failpoint (kills process, removes failpoint config)
    async fn crash_at_failpoint(&mut self, name: &str) -> Result<()>;

    /// Restart after crash
    async fn restart(&mut self) -> Result<()>;

    /// Get S3 client for operations
    fn client(&self) -> &Client;

    /// Verify consistency invariants
    async fn verify_consistency(&self) -> Result<()>;

    /// Get data directory for inspection
    fn data_dir(&self) -> &Path;

    /// Get metadata directory for inspection
    fn metadata_dir(&self) -> &Path;
}
