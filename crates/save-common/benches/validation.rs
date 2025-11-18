use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use save_common::{ObjectLockManager, validate_bucket_name, validate_object_key};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

fn bench_validate_bucket_name(c: &mut Criterion) {
    let mut group = c.benchmark_group("common_validate_bucket");

    let cases = vec![
        ("valid", "my-bucket-123"),
        ("short", "abc"),
        (
            "long",
            "a-very-long-bucket-name-with-many-characters-in-it-test",
        ),
        ("dots", "my.bucket.with.dots"),
    ];

    for (name, bucket) in cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), &bucket, |b, &bucket| {
            b.iter(|| {
                black_box(validate_bucket_name(bucket).is_ok());
            });
        });
    }
    group.finish();
}

fn bench_validate_object_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("common_validate_key");

    let long_key = "x".repeat(500);
    let cases = vec![
        ("short", "file.txt"),
        ("nested", "path/to/my/file.txt"),
        ("deep", "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/file.txt"),
        ("long", long_key.as_str()),
    ];

    for (name, key) in cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), &key, |b, &key| {
            b.iter(|| {
                black_box(validate_object_key(key).is_ok());
            });
        });
    }
    group.finish();
}

fn bench_object_lock_acquire_release(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("common_object_lock");

    group.bench_function("uncontended", |b| {
        let lock_mgr = ObjectLockManager::new(Duration::from_secs(30));
        b.to_async(&rt).iter(|| async {
            let _guard = lock_mgr.acquire_write_lock("bucket", "key").await.unwrap();
            black_box(());
        });
    });

    group.bench_function("different_keys", |b| {
        let lock_mgr = ObjectLockManager::new(Duration::from_secs(30));
        b.iter_batched(
            || uuid::Uuid::new_v4().to_string(),
            |key| {
                rt.block_on(async {
                    let _guard = lock_mgr.acquire_write_lock("bucket", &key).await.unwrap();
                    black_box(());
                })
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_object_lock_contention(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("common_object_lock_contention");

    for concurrent in [2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrent),
            &concurrent,
            |b, &concurrent| {
                let lock_mgr = Arc::new(ObjectLockManager::new(Duration::from_secs(30)));
                b.to_async(&rt).iter(|| async {
                    let mut handles = vec![];
                    for i in 0..concurrent {
                        let mgr = Arc::clone(&lock_mgr);
                        let handle = tokio::spawn(async move {
                            let _guard =
                                mgr.acquire_write_lock("bucket", "same-key").await.unwrap();
                            tokio::time::sleep(tokio::time::Duration::from_micros(100)).await;
                            black_box(i);
                        });
                        handles.push(handle);
                    }
                    for handle in handles {
                        handle.await.unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_validate_bucket_name,
    bench_validate_object_key,
    bench_object_lock_acquire_release,
    bench_object_lock_contention
);
criterion_main!(benches);
