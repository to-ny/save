mod common;

use common::{format_size, random_bytes};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use save_storage::ObjectStorage;
use save_storage::cluster::{ClusterState, NodeState};
use save_storage::replication::{QuorumConfig, ReplicationCoordinator};
use std::hint::black_box;
use std::io::Cursor;
use tempfile::TempDir;
use tokio::runtime::Runtime;

const NODE_COUNTS: [usize; 5] = [3, 5, 7, 11, 21];

fn bench_quorum_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("cluster_quorum");

    for node_count in NODE_COUNTS {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_nodes", node_count)),
            &node_count,
            |b, &count| {
                let mut state = ClusterState::new(1);
                for i in 2..=count as u64 {
                    let mut node = NodeState::new(i, format!("host{}:9001", i));
                    if i % 2 == 0 {
                        node.mark_healthy(5);
                    } else {
                        node.mark_unreachable();
                    }
                    state.nodes.insert(i, node);
                }

                b.iter(|| black_box(state.check_quorum()));
            },
        );
    }
    group.finish();
}

fn bench_partition_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("cluster_partition");

    for node_count in NODE_COUNTS {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_nodes", node_count)),
            &node_count,
            |b, &count| {
                let mut state = ClusterState::new(1);
                for i in 2..=count as u64 {
                    let mut node = NodeState::new(i, format!("host{}:9001", i));
                    if i % 3 == 0 {
                        node.mark_unreachable();
                    } else if i % 3 == 1 {
                        node.mark_degraded(100);
                    } else {
                        node.mark_healthy(5);
                    }
                    state.nodes.insert(i, node);
                }

                b.iter(|| black_box(state.detect_partition()));
            },
        );
    }
    group.finish();
}

fn bench_available_nodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("cluster_available_nodes");

    for node_count in NODE_COUNTS {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_nodes", node_count)),
            &node_count,
            |b, &count| {
                let mut state = ClusterState::new(1);
                for i in 2..=count as u64 {
                    let mut node = NodeState::new(i, format!("host{}:9001", i));
                    match i % 4 {
                        0 => node.mark_healthy(5),
                        1 => node.mark_degraded(100),
                        2 => node.mark_unreachable(),
                        _ => {} // Unknown
                    }
                    state.nodes.insert(i, node);
                }

                b.iter(|| black_box(state.available_nodes()));
            },
        );
    }
    group.finish();
}

fn bench_local_replication_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("replication_overhead");

    for size in [1024, 10_240, 102_400] {
        // Direct storage write (baseline)
        group.bench_with_input(
            BenchmarkId::new("direct", format_size(size)),
            &size,
            |b, &size| {
                let temp = TempDir::new().unwrap();
                let storage = rt.block_on(ObjectStorage::new(temp.path())).unwrap();
                let data = random_bytes(size);

                b.to_async(&rt).iter(|| {
                    let data = data.clone();
                    let storage = &storage;
                    async move {
                        let reader = Cursor::new(data);
                        storage.put_object("test-key", reader).await.unwrap();
                        black_box(());
                    }
                });
            },
        );

        // Replication coordinator with factor=1 (local only, shows 2PC overhead)
        group.bench_with_input(
            BenchmarkId::new("coordinator_local", format_size(size)),
            &size,
            |b, &size| {
                let temp = TempDir::new().unwrap();
                let storage = rt.block_on(ObjectStorage::new(temp.path())).unwrap();
                let coordinator =
                    ReplicationCoordinator::new(1, QuorumConfig::with_replication_factor(1));
                let data = random_bytes(size);

                b.to_async(&rt).iter(|| {
                    let data = data.clone();
                    let storage = &storage;
                    let coordinator = &coordinator;
                    async move {
                        let key = "test-key";
                        let data_clone = data.clone();
                        coordinator
                            .replicate_write(key, data, async move {
                                let reader = Cursor::new(data_clone);
                                storage.put_object(key, reader).await
                            })
                            .await
                            .unwrap();
                        black_box(());
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_quorum_check,
    bench_partition_detection,
    bench_available_nodes,
    bench_local_replication_overhead,
);
criterion_main!(benches);
