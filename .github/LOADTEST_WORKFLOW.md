# Load Testing GitHub Actions Workflow

This document describes how to use the on-demand load testing workflow for the Save object store.

## Overview

The load testing workflow allows you to run performance tests on-demand through GitHub Actions with configurable:
- **Runner profiles** (CPU/memory resources)
- **Workload profiles** (test duration and intensity)
- **Server modes** (local or external endpoints)
- **Test scenarios** (read-heavy, write-heavy, mixed, multipart)

All test reports are automatically committed to the repository for historical tracking and are also available as downloadable artifacts.

## Running a Load Test

### Via GitHub UI

1. Navigate to the **Actions** tab in your GitHub repository
2. Select the **Load Testing** workflow from the left sidebar
3. Click the **Run workflow** button (top right)
4. Configure the test parameters:

   | Parameter | Description | Options | Default |
   |-----------|-------------|---------|---------|
   | **Runner Profile** | GitHub runner size | `small`, `medium`, `large` | `small` |
   | **Workload Profile** | Test intensity/duration | `quick-smoke`, `medium-load`, `stress-test`, `soak-test` | `quick-smoke` |
   | **Server Mode** | Where to run tests | `local` (start server in CI), `external` (connect to remote) | `local` |
   | **Scenario** | Test workload pattern | `mixed`, `read-heavy`, `write-heavy`, `multipart`, `all` | `mixed` |
   | **External Endpoint** | Remote server URL | e.g., `http://staging.example.com:9000` | (empty) |

5. Click **Run workflow** to start the test

### Via GitHub CLI

```bash
# Quick smoke test on small runner (local server)
gh workflow run loadtest.yml \
  -f runner_profile=small \
  -f workload_profile=quick-smoke \
  -f server_mode=local \
  -f scenario=mixed

# Stress test on large runner (external server)
gh workflow run loadtest.yml \
  -f runner_profile=large \
  -f workload_profile=stress-test \
  -f server_mode=external \
  -f scenario=all \
  -f external_endpoint=http://prod.example.com:9000

# Soak test on medium runner
gh workflow run loadtest.yml \
  -f runner_profile=medium \
  -f workload_profile=soak-test \
  -f server_mode=local \
  -f scenario=read-heavy
```

## Runner Profiles

Runner profiles control the GitHub Actions runner specifications:

| Profile | Runner Type | vCPU | RAM | Use Case |
|---------|-------------|------|-----|----------|
| **small** | `ubuntu-latest` | 2 | 7 GB | Quick validation, smoke tests |
| **medium** | `ubuntu-latest-4-cores` | 4 | 16 GB | Standard load tests |
| **large** | `ubuntu-latest-8-cores` | 8 | 32 GB | Stress tests, high concurrency |

## Workload Profiles

Workload profiles are defined in `.github/loadtest-profiles/` and control test parameters:

| Profile | Duration | Max Users | Hatch Rate | Use Case |
|---------|----------|-----------|------------|----------|
| **quick-smoke** | 30s | 10 | 5/s | Fast validation, PR checks |
| **medium-load** | 5 min | 50 | 5/s | Standard performance testing |
| **stress-test** | 15 min | 200 | 10/s | Find performance limits |
| **soak-test** | 1 hour | 100 | 5/s | Stability, memory leak detection |

You can create custom profiles by adding new `.toml` files to `.github/loadtest-profiles/`.

## Test Scenarios

| Scenario | Description | Operations Mix |
|----------|-------------|----------------|
| **mixed** | Balanced workload | PUT(3):GET(5):DELETE(1):LIST(1) |
| **read-heavy** | Primarily reads | PUT(1):GET(8):DELETE(1):LIST(1) |
| **write-heavy** | Primarily writes | PUT(7):GET(2):DELETE(1):LIST(1) |
| **multipart** | Multipart uploads | Initiate → Upload Parts → Complete |
| **all** | Run all scenarios | Executes all test scenarios sequentially |

## Server Modes

### Local Mode (default)

The workflow builds and starts the Save server in the GitHub Actions runner:

- Server runs on `localhost:9000`
- Isolated environment per test run
- Consistent baseline for CI/CD validation
- Best for: smoke tests, CI checks, regression testing

### External Mode

Connect to an already-running server at a specified endpoint:

- Requires `external_endpoint` parameter (e.g., `http://staging.example.com:9000`)
- Tests against real deployments
- Best for: production-like load testing, capacity planning

## Report Output

After each test run, reports are generated in multiple formats and stored in two locations:

### 1. Git Repository (Permanent)

Reports are committed to `reports/loadtest/` with this structure:

```
reports/loadtest/
├── INDEX.md                              # Summary of all test runs
├── 2025-11-16/
│   ├── mixed-quick-smoke-143022/
│   │   ├── environment.json              # Environment/system specs
│   │   ├── mixed-workload-20251116-143022.json  # Detailed JSON report
│   │   ├── mixed-workload-20251116-143022.md    # Human-readable report
│   │   └── mixed-workload-20251116-143022.csv   # CSV export
│   └── read-heavy-medium-load-145533/
│       └── ...
└── 2025-11-17/
    └── ...
```

### 2. GitHub Artifacts (90 days)

Reports are also uploaded as workflow artifacts for easy download:

- Navigate to the workflow run
- Scroll to **Artifacts** section at the bottom
- Download `loadtest-reports-{scenario}-{profile}-{run-number}.zip`

## Report Contents

Each report includes:

### Environment Specifications
- Runner type and resources (CPU, RAM)
- OS and Rust version
- Save server version and git commit
- Workload profile name
- Server mode (local/external)
- Endpoint URL

### Performance Metrics
- Total/successful/failed request counts
- Requests per second (RPS)
- Latency percentiles (p50, p95, p99, max)
- Prometheus metrics (if available)
- System metrics (CPU, memory, disk I/O)

### Example Markdown Report

```markdown
# Load Test Report: mixed-workload

**Start**: 2025-11-16 14:30:22 UTC
**End**: 2025-11-16 14:30:52 UTC
**Duration**: 30s

## Environment

| Property | Value |
|----------|-------|
| Runner Type | small |
| CPU Cores | 2 |
| Memory (GB) | 7 |
| Workload Profile | quick-smoke |
| Server Mode | local |
| Endpoint | http://localhost:9000 |

## Summary

| Metric | Value |
|--------|-------|
| Total Requests | 1234 |
| Successful | 1230 |
| Failed | 4 |
| Requests/sec | 41.13 |

## Latency

| Percentile | Latency (ms) |
|------------|-------------|
| p50 | 12.5 |
| p95 | 45.2 |
| p99 | 89.7 |
| max | 156.3 |
```

## Customizing Profiles

### Creating a Custom Workload Profile

1. Copy an existing profile from `.github/loadtest-profiles/`
2. Modify parameters:
   - `workload.duration_secs` - Test duration
   - `workload.users.max` - Maximum concurrent users
   - `workload.users.hatch_rate` - Users spawned per second
   - `workload.object_sizes.*` - Distribution of object sizes
   - `scenarios.*` - Operation weight ratios per scenario
3. Save as `.github/loadtest-profiles/my-profile.toml`
4. Update the workflow file to add it as an option:

```yaml
workload_profile:
  type: choice
  options:
    - quick-smoke
    - medium-load
    - stress-test
    - soak-test
    - my-profile  # Add your custom profile here
```

## Troubleshooting

### Test Failures

If tests fail, check the workflow logs:

1. Go to the **Actions** tab
2. Click on the failed workflow run
3. Expand the **Run load tests** step
4. Review error messages and stack traces

Common issues:
- **Connection refused**: Server failed to start (check **Start save server** step logs)
- **High failure rate**: Server overloaded (try lower user count or slower hatch rate)
- **Timeout errors**: Increase `timeout` in workflow or reduce test duration

### Report Not Generated

If reports aren't committed:

1. Check **Commit reports to repository** step for errors
2. Verify GitHub Actions has write permissions to the repository
3. Ensure `reports/loadtest/` directory exists

### Server Won't Start (Local Mode)

1. Check **Build save server** step completed successfully
2. Review **Start save server** logs for startup errors
3. Verify port 9000 is not already in use

## Best Practices

1. **Start small**: Begin with `quick-smoke` profile to validate setup
2. **Gradual scaling**: Increase load progressively (small → medium → large runners)
3. **External testing**: Use `external` mode for realistic load against deployed environments
4. **Compare reports**: Use the `INDEX.md` file to track performance trends over time
5. **Resource matching**: Match runner size to test intensity (small for smoke, large for stress)
6. **Soak tests**: Run during off-hours to avoid impacting other CI jobs

## CI/CD Integration

### Automated Testing on PR

Add this to your main CI workflow to run smoke tests automatically:

```yaml
- name: Run load test smoke check
  uses: ./.github/workflows/loadtest.yml
  with:
    runner_profile: small
    workload_profile: quick-smoke
    server_mode: local
    scenario: mixed
```

### Scheduled Load Tests

Add a schedule trigger to run tests periodically:

```yaml
on:
  workflow_dispatch:
    # ... existing inputs ...
  schedule:
    - cron: '0 2 * * *'  # Daily at 2 AM UTC
```

## Further Reading

- [Goose Documentation](https://book.goose.rs/) - Load testing framework
- [GitHub Actions Runners](https://docs.github.com/en/actions/using-github-hosted-runners/about-github-hosted-runners) - Runner specifications
- [Save Load Test Implementation](../../tests/loadtest/README.md) - Technical details
