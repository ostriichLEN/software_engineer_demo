use criterion::{
    criterion_group, criterion_main,
    Criterion, Throughput
};

use final_project::sum_slice;

fn benchmark_sum_slice(c: &mut Criterion) {

    let mut group = c.benchmark_group("sum_slice");

    // 1. 建立一個 1,000,000 筆整數資料的向量，資料型態為 i64 (8bytes)
    let size_1m = 1_000_000usize;
    let data_1m: Vec<i64> = (1..=size_1m as i64).collect();

    // 提供計算 throughput 的資料量
    // Throughput = 處理資料量 / 執行時間
    // 1,000,000 × 8 Bytes ≈ 7.63 MiB
    group.throughput(
        Throughput::Bytes((size_1m * 8) as u64)
    );

    // 2. 執行 sum_slice() 函式對陣列逐項加總
    group.bench_function("1M_elements", |b| {
        b.iter(|| sum_slice(&data_1m)) // 重複多次執行函式
    });

    // 10M
    let size_10m = 10_000_000usize;
    let data_10m: Vec<i64> = (1..=size_10m as i64).collect();

    group.throughput(
    Throughput::Bytes((size_10m * 8) as u64)
    );

    group.bench_function("10M_elements", |b| {
    b.iter(|| sum_slice(&data_10m))
    });

    group.finish();
}

criterion_group!(benches, benchmark_sum_slice);
criterion_main!(benches);