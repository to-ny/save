//! Distributed tracing support.
//!
//! Provides `TraceContext` for propagating trace information across node boundaries.

use uuid::Uuid;

/// Header for distributed trace ID propagation across nodes.
pub const TRACE_ID_HEADER: &str = "x-trace-id";
/// Header for parent span ID propagation.
pub const PARENT_SPAN_HEADER: &str = "x-parent-span-id";
/// Header for request ID propagation.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Trace context for distributed tracing.
///
/// Used to propagate trace information across HTTP and gRPC boundaries.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Unique identifier for the entire trace (same across all nodes).
    pub trace_id: String,
    /// Unique identifier for this span within the trace.
    pub span_id: String,
    /// Span ID of the parent span (if this is a child span).
    pub parent_span_id: Option<String>,
    /// Request ID for correlation (may differ from trace_id).
    pub request_id: String,
}

impl TraceContext {
    /// Create a new trace context with fresh IDs.
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string(),
            span_id: generate_span_id(),
            parent_span_id: None,
            request_id: Uuid::new_v4().to_string(),
        }
    }

    /// Create a trace context with specific values.
    pub fn with_values(
        trace_id: String,
        span_id: String,
        parent_span_id: Option<String>,
        request_id: String,
    ) -> Self {
        Self {
            trace_id,
            span_id,
            parent_span_id,
            request_id,
        }
    }

    /// Create a child context for downstream calls.
    ///
    /// The child inherits the trace_id and request_id, but gets a new span_id
    /// and uses the current span_id as its parent.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: generate_span_id(),
            parent_span_id: Some(self.span_id.clone()),
            request_id: self.request_id.clone(),
        }
    }

    /// Create headers for propagating context to downstream services.
    pub fn to_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            (TRACE_ID_HEADER, self.trace_id.clone()),
            (PARENT_SPAN_HEADER, self.span_id.clone()),
            (REQUEST_ID_HEADER, self.request_id.clone()),
        ]
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a random span ID (16 hex characters).
pub fn generate_span_id() -> String {
    format!("{:016x}", rand::random::<u64>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_new() {
        let ctx = TraceContext::new();
        assert!(!ctx.trace_id.is_empty());
        assert!(!ctx.span_id.is_empty());
        assert!(!ctx.request_id.is_empty());
        assert!(ctx.parent_span_id.is_none());
    }

    #[test]
    fn test_trace_context_child() {
        let parent = TraceContext::new();
        let child = parent.child();

        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.request_id, parent.request_id);
        assert_eq!(child.parent_span_id, Some(parent.span_id.clone()));
        assert_ne!(child.span_id, parent.span_id);
    }

    #[test]
    fn test_span_id_format() {
        let span_id = generate_span_id();
        assert_eq!(span_id.len(), 16);
        assert!(span_id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_to_headers() {
        let ctx = TraceContext::new();
        let headers = ctx.to_headers();

        assert_eq!(headers.len(), 3);
        assert!(headers.iter().any(|(k, _)| *k == TRACE_ID_HEADER));
        assert!(headers.iter().any(|(k, _)| *k == PARENT_SPAN_HEADER));
        assert!(headers.iter().any(|(k, _)| *k == REQUEST_ID_HEADER));
    }

    #[test]
    fn test_with_values() {
        let ctx = TraceContext::with_values(
            "trace-123".to_string(),
            "span-456".to_string(),
            Some("parent-789".to_string()),
            "req-000".to_string(),
        );

        assert_eq!(ctx.trace_id, "trace-123");
        assert_eq!(ctx.span_id, "span-456");
        assert_eq!(ctx.parent_span_id, Some("parent-789".to_string()));
        assert_eq!(ctx.request_id, "req-000");
    }
}
