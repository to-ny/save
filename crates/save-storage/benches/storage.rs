mod common;

use common::{format_size, random_bytes};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use save_storage::ObjectStorage;
use std::hint::black_box;
use std::io::Cursor;
use tempfile::TempDir;
use tokio::runtime::Runtime;

fn bench_put(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("storage_put");

    for size in [1024, 10_240, 102_400, 1_048_576, 10_485_760] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format_size(size)),
            &size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let temp = TempDir::new().unwrap();
                    let storage = ObjectStorage::new(temp.path()).await.unwrap();
                    let data = random_bytes(size);
                    let reader = Cursor::new(data);
                    storage.put_object("test-key", reader).await.unwrap();
                    black_box(());
                });
            },
        );
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("storage_get");

    for size in [1024, 10_240, 102_400, 1_048_576, 10_485_760] {
        group.throughput(Throughput::Bytes(size as u64));
        let temp = TempDir::new().unwrap();
        let storage = rt.block_on(ObjectStorage::new(temp.path())).unwrap();
        let data = random_bytes(size);
        let reader = Cursor::new(data);
        rt.block_on(storage.put_object("test-key", reader)).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(format_size(size)),
            &size,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    black_box(storage.get_object("test-key").await.unwrap());
                });
            },
        );
    }
    group.finish();
}

fn bench_delete(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("storage_delete");

    for size in [1024, 10_240, 102_400, 1_048_576] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format_size(size)),
            &size,
            |b, &size| {
                b.iter_batched(
                    || {
                        let temp = TempDir::new().unwrap();
                        let storage = rt.block_on(ObjectStorage::new(temp.path())).unwrap();
                        let data = random_bytes(size);
                        let reader = Cursor::new(data);
                        rt.block_on(storage.put_object("test-key", reader)).unwrap();
                        (storage, temp)
                    },
                    |(storage, _temp)| {
                        rt.block_on(async {
                            storage.delete_object("test-key").await.unwrap();
                            black_box(());
                        })
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_hash_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_hash");

    for size in [1024, 10_240, 102_400, 1_048_576, 10_485_760] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format_size(size)),
            &size,
            |b, &size| {
                let data = random_bytes(size);
                b.iter(|| {
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(&data);
                    black_box(hasher.finalize());
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_put, bench_get, bench_delete, bench_hash_only);
criterion_main!(benches);
