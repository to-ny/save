use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadTestError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid configuration: {0}")]
    ConfigValidation(String),

    #[error("S3 operation failed: {operation}")]
    S3Operation {
        operation: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("Metrics collection failed: {0}")]
    MetricsCollection(#[source] anyhow::Error),

    #[error("Report generation failed: {0}")]
    ReportGeneration(#[source] anyhow::Error),

    #[error("HTTP request failed: {0}")]
    HttpRequest(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    JsonSerialization(#[from] serde_json::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("Invalid endpoint: {0}")]
    InvalidEndpoint(String),

    #[error("Environment detection failed: {0}")]
    EnvironmentDetection(String),

    #[error("Prometheus parsing failed: {0}")]
    PrometheusParseError(String),

    #[error("Test execution failed: {0}")]
    TestExecution(String),
}

pub type Result<T> = std::result::Result<T, LoadTestError>;
