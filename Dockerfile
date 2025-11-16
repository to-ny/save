# syntax=docker/dockerfile:1.4

# ============================================================================
# Stage 1: cargo-chef planner - analyzes dependencies
# ============================================================================
FROM rustlang/rust:nightly-slim AS chef

# Install cargo-chef
RUN cargo install cargo-chef --locked

WORKDIR /build

# ============================================================================
# Stage 2: Prepare recipe for dependency caching
# ============================================================================
FROM chef AS planner

# Copy all source files for analysis
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Remove test workspace members (not needed in production)
RUN sed -i '/tests\//d' Cargo.toml

# Generate dependency recipe
RUN cargo chef prepare --recipe-path recipe.json

# ============================================================================
# Stage 3: Build dependencies (cached layer)
# ============================================================================
FROM chef AS dependencies

# Install build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        clang \
        cmake \
        curl && \
    rm -rf /var/lib/apt/lists/*

# Install sccache for compilation caching
ARG SCCACHE_VERSION=0.8.2
RUN curl -L "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz" | \
    tar xz && \
    mv sccache-*/sccache /usr/local/bin/ && \
    chmod +x /usr/local/bin/sccache && \
    rm -rf sccache-*

# Configure sccache
ENV RUSTC_WRAPPER=/usr/local/bin/sccache
ENV SCCACHE_DIR=/sccache
ENV CARGO_INCREMENTAL=0

# Copy recipe from planner
COPY --from=planner /build/recipe.json recipe.json

# Build dependencies only (cached layer)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json && \
    sccache --show-stats

# ============================================================================
# Stage 4: Build application
# ============================================================================
FROM dependencies AS builder

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Remove test workspace members
RUN sed -i '/tests\//d' Cargo.toml

# Build release binary with caching
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/sccache,sharing=locked \
    cargo build --release --bin save-api && \
    sccache --show-stats

# Strip binary to reduce size
RUN strip /build/target/release/save-api

# ============================================================================
# Stage 5: Runtime image (minimal, production-ready)
# ============================================================================
FROM debian:trixie-slim

# Install runtime dependencies only
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        curl && \
    rm -rf /var/lib/apt/lists/* && \
    apt-get clean

# Create non-root user with fixed UID/GID
RUN groupadd -g 1000 save && \
    useradd -m -u 1000 -g save -s /bin/bash save && \
    mkdir -p /var/lib/save/data /var/lib/save/metadata && \
    chown -R save:save /var/lib/save

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/save-api /usr/local/bin/save-api

# Switch to non-root user
USER save

# Default config path
ENV SAVE_CONFIG=/app/save.toml

# Expose S3 API port
EXPOSE 9000

# Volume for persistent data
VOLUME ["/var/lib/save/data", "/var/lib/save/metadata"]

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9000/health || exit 1

# Labels for metadata
# TODO Update with accurate data
LABEL org.opencontainers.image.title="save" \
      org.opencontainers.image.description="High-performance S3-compatible object storage" \
      org.opencontainers.image.vendor="Save Contributors" \
      org.opencontainers.image.licenses="Apache-2.0"

CMD ["save-api"]
