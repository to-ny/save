[server]
bind_address = "0.0.0.0:9000"
worker_threads = 4
max_blocking_threads = 512

[storage]
data_path = "/var/lib/save/data"
metadata_path = "/var/lib/save/metadata"
fsync_mode = "data"

[metadata]
write_buffer_size_mb = 128
max_write_buffer_number = 4
block_cache_size_mb = 512
max_background_jobs = 4

[credentials]
access_key = "${save_access_key}"
secret_key = "${save_secret_key}"

[limits]
max_concurrent_requests = 1000
requests_per_second = 500

[shutdown]
drain_timeout_secs = 30
