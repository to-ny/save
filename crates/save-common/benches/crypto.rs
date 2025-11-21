use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::hint::black_box;

type HmacSha256 = Hmac<Sha256>;

fn random_bytes(size: usize) -> Vec<u8> {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..size).map(|_| rng.random::<u8>()).collect()
}

fn bench_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_sha256");

    for size in [1024, 10_240, 102_400, 1_048_576, 10_485_760, 104_857_600] {
        group.throughput(Throughput::Bytes(size as u64));
        let data = random_bytes(size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format_size(size)),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut hasher = Sha256::new();
                    hasher.update(&data);
                    black_box(hasher.finalize());
                });
            },
        );
    }
    group.finish();
}

fn bench_hmac_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_hmac_sha256");
    let key = b"test-secret-key-for-benchmarking";

    for size in [256, 1024, 4096, 16_384] {
        group.throughput(Throughput::Bytes(size as u64));
        let data = random_bytes(size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format_size(size)),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut mac = HmacSha256::new_from_slice(key).unwrap();
                    mac.update(&data);
                    black_box(mac.finalize());
                });
            },
        );
    }
    group.finish();
}

fn bench_sigv4_canonical_request(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_sigv4_canonical");

    group.bench_function("simple_get", |b| {
        let method = "GET";
        let uri = "/test-bucket/test-key";
        let query = "";
        let headers = [
            ("host", "localhost:8080"),
            (
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            ("x-amz-date", "20250114T120000Z"),
        ];
        let payload_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        b.iter(|| {
            let canonical = format!(
                "{}\n{}\n{}\n{}\n\n{}\n{}",
                method,
                uri,
                query,
                headers
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k, v))
                    .collect::<Vec<_>>()
                    .join("\n"),
                headers
                    .iter()
                    .map(|(k, _)| *k)
                    .collect::<Vec<_>>()
                    .join(";"),
                payload_hash
            );
            black_box(canonical);
        });
    });

    group.bench_function("complex_put", |b| {
        let method = "PUT";
        let uri = "/test-bucket/path/to/object";
        let query = "uploadId=12345&partNumber=1";
        let headers = [
            ("content-length", "1048576"),
            ("content-type", "application/octet-stream"),
            ("host", "localhost:8080"),
            ("x-amz-content-sha256", "abc123def456"),
            ("x-amz-date", "20250114T120000Z"),
        ];
        let payload_hash = "abc123def456";

        b.iter(|| {
            let canonical = format!(
                "{}\n{}\n{}\n{}\n\n{}\n{}",
                method,
                uri,
                query,
                headers
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k, v))
                    .collect::<Vec<_>>()
                    .join("\n"),
                headers
                    .iter()
                    .map(|(k, _)| *k)
                    .collect::<Vec<_>>()
                    .join(";"),
                payload_hash
            );
            black_box(canonical);
        });
    });

    group.finish();
}

fn bench_hex_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_hex_encode");

    for size in [16, 32, 64, 128] {
        let data = random_bytes(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                black_box(hex::encode(&data));
            });
        });
    }
    group.finish();
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{}MB", bytes / 1_048_576)
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}

criterion_group!(
    benches,
    bench_sha256,
    bench_hmac_sha256,
    bench_sigv4_canonical_request,
    bench_hex_encoding
);
criterion_main!(benches);
