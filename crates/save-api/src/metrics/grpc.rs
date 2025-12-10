//! gRPC metrics.

use prometheus::{
    HistogramVec, IntCounterVec, IntGaugeVec, register_histogram_vec, register_int_counter_vec,
    register_int_gauge_vec,
};
use std::sync::OnceLock;

static GRPC_REQUESTS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static GRPC_REQUEST_DURATION_SECONDS: OnceLock<HistogramVec> = OnceLock::new();
static GRPC_STREAMS_ACTIVE: OnceLock<IntGaugeVec> = OnceLock::new();
static GRPC_STREAM_DURATION_SECONDS: OnceLock<HistogramVec> = OnceLock::new();

/// Total gRPC requests by service, method, and status.
pub fn grpc_requests_total() -> &'static IntCounterVec {
    GRPC_REQUESTS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_grpc_requests_total",
            "Total gRPC requests by service, method, and status",
            &["service", "method", "status"]
        )
        .expect("Failed to register save_grpc_requests_total metric")
    })
}

/// gRPC request duration in seconds.
pub fn grpc_request_duration_seconds() -> &'static HistogramVec {
    GRPC_REQUEST_DURATION_SECONDS.get_or_init(|| {
        register_histogram_vec!(
            "save_grpc_request_duration_seconds",
            "gRPC request duration in seconds",
            &["service", "method"],
            vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
            ]
        )
        .expect("Failed to register save_grpc_request_duration_seconds metric")
    })
}

/// Number of active gRPC streams by service and method.
pub fn grpc_streams_active() -> &'static IntGaugeVec {
    GRPC_STREAMS_ACTIVE.get_or_init(|| {
        register_int_gauge_vec!(
            "save_grpc_streams_active",
            "Number of active gRPC streams",
            &["service", "method"]
        )
        .expect("Failed to register save_grpc_streams_active metric")
    })
}

/// gRPC stream duration in seconds.
pub fn grpc_stream_duration_seconds() -> &'static HistogramVec {
    GRPC_STREAM_DURATION_SECONDS.get_or_init(|| {
        register_histogram_vec!(
            "save_grpc_stream_duration_seconds",
            "gRPC stream duration in seconds",
            &["service", "method"],
            vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]
        )
        .expect("Failed to register save_grpc_stream_duration_seconds metric")
    })
}

/// Record a gRPC request.
pub fn record_grpc_request(service: &str, method: &str, status: &str) {
    grpc_requests_total()
        .with_label_values(&[service, method, status])
        .inc();
}

/// Record gRPC request duration.
pub fn record_grpc_request_duration(service: &str, method: &str, duration_secs: f64) {
    grpc_request_duration_seconds()
        .with_label_values(&[service, method])
        .observe(duration_secs);
}

/// Increment active gRPC streams counter.
pub fn inc_grpc_stream(service: &str, method: &str) {
    grpc_streams_active()
        .with_label_values(&[service, method])
        .inc();
}

/// Decrement active gRPC streams counter.
pub fn dec_grpc_stream(service: &str, method: &str) {
    grpc_streams_active()
        .with_label_values(&[service, method])
        .dec();
}

/// Record gRPC stream duration.
pub fn record_grpc_stream_duration(service: &str, method: &str, duration_secs: f64) {
    grpc_stream_duration_seconds()
        .with_label_values(&[service, method])
        .observe(duration_secs);
}

/// RAII guard for tracking gRPC stream lifecycle.
pub struct GrpcStreamGuard {
    service: &'static str,
    method: &'static str,
    start: std::time::Instant,
}

impl GrpcStreamGuard {
    pub fn new(service: &'static str, method: &'static str) -> Self {
        inc_grpc_stream(service, method);
        Self {
            service,
            method,
            start: std::time::Instant::now(),
        }
    }
}

impl Drop for GrpcStreamGuard {
    fn drop(&mut self) {
        dec_grpc_stream(self.service, self.method);
        record_grpc_stream_duration(
            self.service,
            self.method,
            self.start.elapsed().as_secs_f64(),
        );
    }
}

pub(crate) fn init() {
    let _ = grpc_requests_total();
    let _ = grpc_request_duration_seconds();
    let _ = grpc_streams_active();
    let _ = grpc_stream_duration_seconds();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_metrics() {
        record_grpc_request("ReplicationService", "WriteObject", "ok");
        record_grpc_request_duration("ReplicationService", "WriteObject", 0.1);
    }

    #[test]
    fn test_grpc_stream_guard() {
        {
            let _guard = GrpcStreamGuard::new("TestService", "TestMethod");
            assert_eq!(
                grpc_streams_active()
                    .with_label_values(&["TestService", "TestMethod"])
                    .get(),
                1
            );
        }
        // Guard dropped
        assert_eq!(
            grpc_streams_active()
                .with_label_values(&["TestService", "TestMethod"])
                .get(),
            0
        );
    }
}
