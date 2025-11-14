pub mod config;
pub mod metrics;
pub mod objects;
pub mod reporting;
pub mod scenarios;
pub mod signing;
pub mod system_metrics;
pub mod transactions;

use std::sync::Arc;
use transactions::AppState;

pub static GLOBAL_STATE: std::sync::OnceLock<Arc<AppState>> = std::sync::OnceLock::new();
