use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use save_common::{validate_bucket_name, validate_object_key};
use std::hint::black_box;

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

criterion_group!(
    benches,
    bench_validate_bucket_name,
    bench_validate_object_key,
);
criterion_main!(benches);
