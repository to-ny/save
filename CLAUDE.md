# CLAUDE.md

## Mission
Design and build an open, high-performance, MinIO-like object store, stepwise:
1. Phase 1 — Local single-node S3-compatible storage (RocksDB + FS)
2. Phase 2 — Replication & cluster coordination (etcd or Raft)
3. Phase 3 — Erasure coding, healing, rebalancing
4. Phase 4 — Multi-tenant security, IAM, metrics, operator tooling

## Coding standards
- Language: Rust (edition 2024)
- Linting: `cargo fmt`, `cargo clippy`
- Testing: `cargo test`, integration tests under `/tests`
- Doc style: concise `///` comments for public APIs; `README` in each crate
- Error handling: `anyhow` for top-level, `thiserror` for library errors
- Async runtime: `tokio`
- Logging: `tracing` with JSON output in prod mode
- Config: `serde` + `toml` config file per node

## Development workflow
- One crate per major subsystem (api, storage, metadata, common)
- Tasks tracked in `/tasks/*.md`
- Always document module boundaries in `ARCHITECTURE.md`
- Use `prompt_context/` folder for ongoing design notes or partial drafts

## AI assistant guidelines
- When generating code, prefer small, incremental commits
- Do not rewrite existing files unless explicitly asked
- Maintain compatibility with the latest stable Rust
- Summaries, not verbosity; follow minimalism and composability

## Crate structure
save/
├─ crates/
│ ├─ save-api/ # S3-compatible REST API (Axum)
│ ├─ save-storage/ # Object storage (FS + replication)
│ ├─ save-metadata/ # RocksDB metadata + Raft consensus
│ ├─ save-common/ # Shared types, errors, config
│ ├─ save-proto/ # gRPC protobuf definitions
│ └─ save-cli/ # CLI administration tool
├─ charts/ # Helm charts for Kubernetes
├─ docs/ # Architecture, ADRs, design notes
├─ tasks/ # Phase task tracking
├─ tests/loadtest/ # Load testing with Goose
├─ Cargo.toml
├─ CLAUDE.md (this file)
└─ README.md

