#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::matcher::{Capacity, Order, OrderPool, PriceLevel, Side};

fn bench_place_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_place");

    for size in [0, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_with_setup(
                || {
                    let pool_capacity = size + 16;
                    let capacity = Arc::new(Capacity::new(pool_capacity as u64));
                    let mut pool = OrderPool::new(pool_capacity, capacity);
                    let mut level = PriceLevel::new(10000);

                    for i in 0..size {
                        let i_u64 = i as u64;
                        let order = Order::new_limit(i_u64, 1, Side::Bid, 10000, 100, i_u64);
                        level.add_order(order, &mut pool, i_u64).unwrap();
                    }
                    (pool, level, size as u64)
                },
                |(mut pool, mut level, size)| {
                    let id = size; // Use slot just after pre-populated range
                    let order = Order::new_limit(id, 1, Side::Bid, 10000, 100, id);
                    black_box(level.add_order(order, &mut pool, id).unwrap());
                },
            );
        });
    }
    group.finish();
}

fn bench_fill_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_fill");

    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_with_setup(
                || {
                    let pool_capacity = size + 16;
                    let capacity = Arc::new(Capacity::new(pool_capacity as u64));
                    let mut pool = OrderPool::new(pool_capacity, capacity);
                    let mut level = PriceLevel::new(10000);
                    for i in 0..size {
                        let i_u64 = i as u64;
                        let order = Order::new_limit(i_u64, 1, Side::Bid, 10000, 100, i_u64);
                        level.add_order(order, &mut pool, i_u64).unwrap();
                    }
                    (pool, level)
                },
                |(mut pool, mut level)| {
                    black_box(level.remove_head(&mut pool));
                },
            );
        });
    }
    group.finish();
}

fn bench_remove_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_remove");

    for position in ["head", "middle", "tail"].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(position), position, |b, &position| {
            b.iter_with_setup(
                || {
                    let pool_capacity = 128;
                    let capacity = Arc::new(Capacity::new(pool_capacity as u64));
                    let mut pool = OrderPool::new(pool_capacity, capacity);
                    let mut level = PriceLevel::new(10000);

                    let target_id = 100u64;
                    let target_px = match position {
                        "head" => 10001,
                        "middle" => 10000,
                        "tail" => 9999,
                        _ => unreachable!(),
                    };
                    let target_order = Order::new_limit(target_id, 1, Side::Bid, target_px, 100, target_id);
                    let (target_handle, _) = level.add_order(target_order, &mut pool, target_id).unwrap();

                    // Add remaining orders
                    for i in 0..99u64 {
                        let order = Order::new_limit(i, 1, Side::Bid, 10000, 100, i);
                        level.add_order(order, &mut pool, i).unwrap();
                    }

                    (pool, level, target_handle)
                },
                |(mut pool, mut level, handle)| {
                    black_box(level.remove_order(handle, &mut pool));
                },
            );
        });
    }
    group.finish();
}

fn bench_update_quantity(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_update_qty");

    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_with_setup(
                || {
                    let pool_capacity = size + 8;
                    let capacity = Arc::new(Capacity::new(pool_capacity as u64));
                    let mut pool = OrderPool::new(pool_capacity, capacity);
                    let mut level = PriceLevel::new(10000);
                    for i in 0..size {
                        let i_u64 = i as u64;
                        let order = Order::new_limit(i_u64, 1, Side::Bid, 10000, 100, i_u64);
                        level.add_order(order, &mut pool, i_u64).unwrap();
                    }
                    (pool, level)
                },
                |(mut pool, mut level)| {
                    black_box(level.update_head_qty(50, &mut pool));
                },
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_place_order, bench_fill_order, bench_remove_order, bench_update_quantity);
criterion_main!(benches);
