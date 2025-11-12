# Crash Recovery Tests

Crash recovery and fault injection tests using subprocess management and failpoints.

## Running

```bash
cargo test -p crash-recovery-tests --features crash_tests
```

Tests automatically build the server with failpoints and spawn subprocesses. No need to start the server manually.

## Notes

Tests are gated behind the `crash_tests` feature flag to avoid interference with normal test runs.
Enable with `--features crash_tests`.