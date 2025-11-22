# TODO Update with accurate data
# save
A Rust-based, S3-compatible object storage system.  

## Overview
`save` evolves in stages:

| Phase | Goal |
|--------|------|
| 1 | Local S3-compatible store (RocksDB + FS) |
| 2 | Cluster coordination (Raft/etcd) |
| 3 | Erasure coding, healing |
| 4 | IAM, metrics, operator tools |

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for details.

## Usage

### Lint and Format

```bash
cargo fmt --check
cargo clippy -- -D warnings
```

### Test

```bash
cargo test
```

### Build and Run
```bash
cargo build
cargo run
```
