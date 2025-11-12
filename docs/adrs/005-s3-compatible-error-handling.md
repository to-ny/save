# ADR-005: S3-Compatible Error Handling

**Status**: Accepted

## Context

API must return errors in AWS S3 XML format for client compatibility. Internal errors need rich context for debugging while client responses must be sanitized.

## Decision

**External (Client-Facing)**:
- XML responses matching AWS S3 error format
- Standard error codes (NoSuchBucket, NoSuchKey, AccessDenied, etc.)
- Unique request ID (UUID v4) in every error response
- HTTP status code mapping (404, 403, 409, 400, 500)

**Internal**:
- `thiserror` for structured error types in libraries
- `anyhow` for error propagation at API level
- `tracing::error!` at error creation time with full context
- Error sanitization at HTTP boundary to prevent information leakage

## Consequences

**Positive**:
- Client compatibility with AWS SDKs and CLI
- Request correlation across logs, metrics, retries
- Rich internal debugging context
- Security via sanitized client responses

**Negative**:
- XML serialization overhead
- Dual error representation (internal vs external)
- Careful boundary management to prevent leaks
