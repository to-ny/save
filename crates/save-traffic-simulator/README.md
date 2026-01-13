# save-traffic-simulator

Generates realistic S3 traffic for testing and dashboard visualization.

## Usage

```bash
# Minimal
cargo run -p save-traffic-simulator --release -- \
  --endpoint http://localhost:9000 \
  --access-key KEY \
  --secret-key SECRET

# With options
cargo run -p save-traffic-simulator --release -- \
  --endpoint http://localhost:9000 \
  --access-key KEY \
  --secret-key SECRET \
  --bucket my-bucket \
  --virtual-users 10 \
  --rps 5

# From config file
cargo run -p save-traffic-simulator --release -- --config traffic-simulator.toml
```

## Options

| Flag | Env | Description |
|------|-----|-------------|
| `--endpoint` | `SAVE_SIM_ENDPOINT` | S3 endpoint URL |
| `--access-key` | `SAVE_SIM_ACCESS_KEY` | S3 access key |
| `--secret-key` | `SAVE_SIM_SECRET_KEY` | S3 secret key |
| `--bucket` | `SAVE_SIM_BUCKET` | Bucket name (default: demo-bucket) |
| `--virtual-users` | `SAVE_SIM_VIRTUAL_USERS` | Concurrent users (default: 10) |
| `--rps` | `SAVE_SIM_RPS` | Requests per second (default: 5) |
| `--config` | `SAVE_SIM_CONFIG` | Path to TOML config file |

## Traffic Pattern

Default mix: 70% reads (GET/HEAD/LIST), 30% writes (PUT/DELETE).
Object sizes range from 1KB to 10MB based on weighted distribution.
