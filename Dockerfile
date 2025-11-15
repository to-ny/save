# Build stage
FROM rustlang/rust:nightly-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev clang && \
    rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Remove test workspace members from Cargo.toml
RUN sed -i '/tests\//d' Cargo.toml

# Build release binary
RUN cargo build --release --bin save-api

# Runtime stage
FROM debian:trixie-slim

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

USER save

# Default config path
ENV SAVE_CONFIG=/app/save.toml

EXPOSE 9000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9000/health || exit 1

CMD ["save-api"]
