# Build stage
FROM rust:1.85-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Build release binary
RUN cargo build --release --bin save-api

# Runtime stage
FROM debian:12-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash save && \
    mkdir -p /var/lib/save/data /var/lib/save/metadata && \
    chown -R save:save /var/lib/save

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/save-api /usr/local/bin/save-api

# Copy example config
COPY save.toml.example /app/save.toml.example

USER save

# Default config path
ENV SAVE_CONFIG=/app/save.toml

EXPOSE 9000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9000/health || exit 1

CMD ["save-api"]
