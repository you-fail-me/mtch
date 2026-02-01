#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::core::{Order, OrderPool, PriceLevel, Side};
use std::hint::black_box;

fn bench_place_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_place");

    for size in [0, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Config sized for pre-populated orders + a few extra
            let pool_capacity = (size + 16) as usize;
            let mut pool = OrderPool::new(pool_capacity);
            let mut level = PriceLevel::new(10000);

            // Pre-populate with existing orders
            for i in 0..size {
                let order = Order::new_limit(i, 1, Side::Bid, 10000, 100, i);
                level.add_order(order, &mut pool);
            }

            let id = size + 1;
            b.iter(|| {
                let order = Order::new_limit(id, 1, Side::Bid, 10000, 100, id);
                let (handle, _) = black_box(level.add_order(order, &mut pool).unwrap());
                // Clean up to maintain steady state
                level.remove_order(handle, &mut pool);
                pool.dealloc(handle);
            });
        });
    }
    group.finish();
}

fn bench_fill_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_fill");

    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Config sized for pre-populated orders + 1 for iteration
            let pool_capacity = (size + 16) as usize;
            let mut pool = OrderPool::new(pool_capacity);
            let mut level = PriceLevel::new(10000);

            // Pre-populate with orders to be filled
            let mut handles: Vec<_> = (0..size)
                .map(|i| {
                    let order = Order::new_limit(i, 1, Side::Bid, 10000, 100, i);
                    level.add_order(order, &mut pool).unwrap().0
                })
                .collect();

            let id = size as u64;
            b.iter(|| {
                // Add a new order that will be filled
                let order = Order::new_limit(id, 1, Side::Bid, 10000, 100, id);
                let (handle, _) = level.add_order(order, &mut pool).unwrap();
                handles.push(handle);

                // Fill the head order (FIFO matching)
                black_box(level.remove_head(&mut pool));

                // Clean up handle
                pool.dealloc(handles.remove(0));
            });
        });
    }
    group.finish();
}

fn bench_remove_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_remove");

    for position in ["head", "middle", "tail"].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(position), position, |b, &position| {
            // Fixed config for 100 pre-populated orders + 1 for iteration
            let pool_capacity = 128;
            let mut pool = OrderPool::new(pool_capacity);
            let mut level = PriceLevel::new(10000);

            // Pre-populate with orders
            for i in 0..100u64 {
                let order = Order::new_limit(i, 1, Side::Bid, 10000, 100, i);
                level.add_order(order, &mut pool);
            }

            let insert_id = 100u64;
            let insert_px = match position {
                "head" => 10001,
                "middle" => 10000,
                "tail" => 9999,
                _ => unreachable!(),
            };

            b.iter(|| {
                let order = Order::new_limit(insert_id, 1, Side::Bid, insert_px, 100, insert_id);
                let (handle, _) = level.add_order(order, &mut pool).unwrap();
                // Remove the order (cancellation)
                black_box(level.remove_order(handle, &mut pool));
                pool.dealloc(handle);
            });
        });
    }
    group.finish();
}

fn bench_update_quantity(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_update_qty");

    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Config sized exactly for pre-populated orders
            let pool_capacity = (size + 8) as usize;
            let mut pool = OrderPool::new(pool_capacity);
            let mut level = PriceLevel::new(10000);

            // Pre-populate with orders
            for i in 0..size {
                let order = Order::new_limit(i, 1, Side::Bid, 10000, 100, i);
                level.add_order(order, &mut pool);
            }

            let mut toggle = false;
            b.iter(|| {
                let qty = if toggle { 100 } else { 50 };
                toggle = !toggle;
                // Update head order quantity (partial fill scenario)
                black_box(level.update_head_qty(qty, &mut pool));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_place_order, bench_fill_order, bench_remove_order, bench_update_quantity);
criterion_main!(benches);
