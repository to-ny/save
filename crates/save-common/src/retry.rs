//! Retry utilities with exponential backoff and jitter.

use std::future::Future;
use std::time::Duration;

/// Configuration for retry with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 = no retries).
    pub max_retries: u32,
    /// Initial delay before first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles delay each retry).
    pub multiplier: f64,
    /// Jitter factor (0.0 to 1.0). Adds randomness to prevent thundering herd.
    /// E.g., 0.2 means ±20% variation in delay.
    pub jitter: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            multiplier: 2.0,
            jitter: 0.2, // ±20% by default
        }
    }
}

impl RetryConfig {
    /// Create a config with no retries.
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Create a config with custom max retries.
    pub fn with_max_retries(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Default::default()
        }
    }

    /// Calculate delay for a given attempt (0-indexed), with jitter.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_delay = if attempt == 0 {
            self.initial_delay
        } else {
            let delay_ms =
                self.initial_delay.as_millis() as f64 * self.multiplier.powi(attempt as i32);
            let delay = Duration::from_millis(delay_ms as u64);
            std::cmp::min(delay, self.max_delay)
        };

        self.apply_jitter(base_delay)
    }

    /// Apply jitter to a duration.
    fn apply_jitter(&self, delay: Duration) -> Duration {
        if self.jitter <= 0.0 {
            return delay;
        }

        use rand::Rng;
        let jitter_range = delay.as_millis() as f64 * self.jitter;
        let jitter_offset = rand::rng().random_range(-jitter_range..jitter_range);
        let jittered_ms = (delay.as_millis() as f64 + jitter_offset).max(1.0);
        Duration::from_millis(jittered_ms as u64)
    }
}

/// Execute an async operation with retry on transient errors.
///
/// The `is_retryable` function determines if an error should trigger a retry.
pub async fn retry_with_backoff<F, Fut, T, E, R>(
    config: &RetryConfig,
    mut operation: F,
    is_retryable: R,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    R: Fn(&E) -> bool,
{
    let mut attempt = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if attempt < config.max_retries && is_retryable(&e) => {
                let delay = config.delay_for_attempt(attempt);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, Duration::from_millis(100));
        assert_eq!(config.max_delay, Duration::from_secs(5));
        assert!((config.multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_no_retry_config() {
        let config = RetryConfig::no_retry();
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn test_delay_calculation() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
            jitter: 0.0, // Disable jitter for deterministic test
            ..Default::default()
        };

        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(config.delay_for_attempt(3), Duration::from_millis(800));
    }

    #[test]
    fn test_delay_capped_at_max() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
            multiplier: 2.0,
            jitter: 0.0, // Disable jitter for deterministic test
            ..Default::default()
        };

        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(config.delay_for_attempt(3), Duration::from_millis(500)); // Capped
        assert_eq!(config.delay_for_attempt(10), Duration::from_millis(500)); // Still capped
    }

    #[test]
    fn test_jitter_applied() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(1000),
            jitter: 0.5, // ±50%
            ..Default::default()
        };

        // With 50% jitter, delay should be between 500ms and 1500ms
        let delay = config.delay_for_attempt(0);
        assert!(delay >= Duration::from_millis(500));
        assert!(delay <= Duration::from_millis(1500));
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let config = RetryConfig::default();
        let attempts = AtomicU32::new(0);

        let result: Result<u32, &str> = retry_with_backoff(
            &config,
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Ok(42) }
            },
            |_| true,
        )
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };
        let attempts = AtomicU32::new(0);

        let result: Result<u32, &str> = retry_with_backoff(
            &config,
            || {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                async move { if n < 2 { Err("transient") } else { Ok(42) } }
            },
            |_| true,
        )
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };
        let attempts = AtomicU32::new(0);

        let result: Result<u32, &str> = retry_with_backoff(
            &config,
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err("always fails") }
            },
            |_| true,
        )
        .await;

        assert_eq!(result, Err("always fails"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3); // Initial + 2 retries
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };
        let attempts = AtomicU32::new(0);

        let result: Result<u32, &str> = retry_with_backoff(
            &config,
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err("permanent") }
            },
            |e| *e != "permanent", // Not retryable
        )
        .await;

        assert_eq!(result, Err("permanent"));
        assert_eq!(attempts.load(Ordering::SeqCst), 1); // No retries
    }

    #[tokio::test]
    async fn test_no_retry_config_fails_immediately() {
        let config = RetryConfig::no_retry();
        let attempts = AtomicU32::new(0);

        let result: Result<u32, &str> = retry_with_backoff(
            &config,
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err("fail") }
            },
            |_| true,
        )
        .await;

        assert_eq!(result, Err("fail"));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
