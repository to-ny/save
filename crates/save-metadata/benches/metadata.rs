use chrono::Utc;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use save_metadata::{MetadataStore, ObjectMetadata};
use tempfile::TempDir;
use tokio::runtime::Runtime;

fn bench_bucket_create(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("metadata_bucket_create");

    group.bench_function("single", |b| {
        b.iter_batched(
            || {
                let temp = TempDir::new().unwrap();
                let store = MetadataStore::new(temp.path()).unwrap();
                (store, temp)
            },
            |(store, _temp)| {
                rt.block_on(async {
                    let bucket_name = format!("bench-{}", uuid::Uuid::new_v4());
                    black_box(store.create_bucket(&bucket_name).await.unwrap());
                })
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_bucket_list(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("metadata_bucket_list");

    for count in [1, 10, 100, 1000] {
        let temp = TempDir::new().unwrap();
        let store = MetadataStore::new(temp.path()).unwrap();

        for i in 0..count {
            let bucket_name = format!("bench-{:06}", i);
            rt.block_on(store.create_bucket(&bucket_name)).unwrap();
        }

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.to_async(&rt).iter(|| async {
                black_box(store.list_buckets().await.unwrap());
            });
        });
    }
    group.finish();
}

fn bench_object_put(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("metadata_object_put");

    group.bench_function("single", |b| {
        b.iter_batched(
            || {
                let temp = TempDir::new().unwrap();
                let store = MetadataStore::new(temp.path()).unwrap();
                rt.block_on(store.create_bucket("bench-bucket")).unwrap();
                (store, temp)
            },
            |(store, _temp)| {
                rt.block_on(async {
                    let now = Utc::now();
                    let object = ObjectMetadata {
                        bucket: "bench-bucket".to_string(),
                        key: format!("key-{}", uuid::Uuid::new_v4()),
                        etag: format!("{:064x}", rand::random::<u128>()),
                        size: 1024,
                        content_type: Some("application/octet-stream".to_string()),
                        created_at: now,
                        modified_at: now,
                    };
                    store.put_object_metadata(object).await.unwrap();
                    black_box(());
                })
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_object_get(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("metadata_object_get");

    for count in [1, 10, 100, 1000] {
        let temp = TempDir::new().unwrap();
        let store = MetadataStore::new(temp.path()).unwrap();
        rt.block_on(store.create_bucket("bench-bucket")).unwrap();

        let now = Utc::now();
        for i in 0..count {
            let object = ObjectMetadata {
                bucket: "bench-bucket".to_string(),
                key: format!("key-{:06}", i),
                etag: format!("{:064x}", rand::random::<u128>()),
                size: 1024,
                content_type: Some("application/octet-stream".to_string()),
                created_at: now,
                modified_at: now,
            };
            rt.block_on(store.put_object_metadata(object)).unwrap();
        }

        let key = format!("key-{:06}", count / 2);
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.to_async(&rt).iter(|| async {
                black_box(
                    store
                        .get_object_metadata("bench-bucket", &key)
                        .await
                        .unwrap(),
                );
            });
        });
    }
    group.finish();
}

fn bench_object_list(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("metadata_object_list");

    for count in [10, 100, 1000] {
        let temp = TempDir::new().unwrap();
        let store = MetadataStore::new(temp.path()).unwrap();
        rt.block_on(store.create_bucket("bench-bucket")).unwrap();

        let now = Utc::now();
        for i in 0..count {
            let object = ObjectMetadata {
                bucket: "bench-bucket".to_string(),
                key: format!("key-{:06}", i),
                etag: format!("{:064x}", rand::random::<u128>()),
                size: 1024,
                content_type: Some("application/octet-stream".to_string()),
                created_at: now,
                modified_at: now,
            };
            rt.block_on(store.put_object_metadata(object)).unwrap();
        }

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.to_async(&rt).iter(|| async {
                black_box(store.list_objects("bench-bucket", None).await.unwrap());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_bucket_create,
    bench_bucket_list,
    bench_object_put,
    bench_object_get,
    bench_object_list
);
criterion_main!(benches);
