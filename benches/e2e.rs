#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::core::Side;
use mtch::engine::{Engine, EngineConfig};
use std::hint::black_box;
use std::time::{Duration, Instant};

fn bench_e2e_add_limit_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_add_limit_order");
    let mut id = 0u64;

    for (side, name) in [(Side::Bid, "Buy"), (Side::Ask, "Sell")] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &side, |b, side| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;

                for _ in 0..iters {
                    let config = EngineConfig {
                        symbol: 100,
                        tick_size: 1,
                        base_px: 10000,
                        rb_cap: 256,
                        tick_capacity: 256,
                        pool_capacity: 256,
                        min_order_qty: 50,
                        max_order_qty: 5000,
                    };
                    let mut engine = Engine::new(config).unwrap();

                    // add 100 orders on each side to warm up threads
                    for i in 0..100 {
                        engine.add_limit_order(i, 1, Side::Bid, 9900 - i, 100).unwrap();
                        engine.add_limit_order(100 + i, 1, Side::Ask, 10000 + i, 100).unwrap();
                    }
                    id = 200;

                    let start = Instant::now();
                    id += 1;
                    black_box(engine.add_limit_order(id, 1, *side, 10000, 100).unwrap());
                    total += start.elapsed();
                }

                total
            });
        });
    }

    group.finish();
}

fn bench_e2e_cancel_order(c: &mut Criterion) {
    c.bench_function("e2e_cancel_order", |b| {
        let mut id = 0u64;

        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let config = EngineConfig {
                    symbol: 100,
                    tick_size: 1,
                    base_px: 10000,
                    rb_cap: 256,
                    tick_capacity: 256,
                    pool_capacity: 256,
                    min_order_qty: 50,
                    max_order_qty: 5000,
                };
                let mut engine = Engine::new(config).unwrap();

                // add 100 orders on each side to warm up threads
                for i in 0..100 {
                    engine.add_limit_order(i, 1, Side::Bid, 9900 - i, 100).unwrap();
                    engine.add_limit_order(100 + i, 1, Side::Ask, 10000 + i, 100).unwrap();
                }

                for i in 200..210 {
                    engine.add_limit_order(i, 1, Side::Bid, 9000 + i, 100).unwrap();
                }

                let start = Instant::now();
                black_box(engine.cancel_order(200 + id).unwrap());
                total += start.elapsed();

                id = (id + 1) % 10;
            }

            total
        });
    });
}

fn bench_e2e_market_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_market_order");
    let mut id = 200u64;

    for (side, name) in [(Side::Bid, "Buy"), (Side::Ask, "Sell")] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &side, |b, side| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;

                for _ in 0..iters {
                    let config = EngineConfig {
                        symbol: 100,
                        tick_size: 1,
                        base_px: 10000,
                        rb_cap: 256,
                        tick_capacity: 256,
                        pool_capacity: 256,
                        min_order_qty: 50,
                        max_order_qty: 5000,
                    };
                    let mut engine = Engine::new(config).unwrap();

                    // add liquidity to match against
                    if *side == Side::Bid {
                        for i in 0..100 {
                            engine.add_limit_order(i, 1, Side::Ask, 10000 + i, 100).unwrap();
                        }
                    } else {
                        for i in 0..100 {
                            engine.add_limit_order(i, 1, Side::Bid, 9900 - i, 100).unwrap();
                        }
                    }

                    let start = Instant::now();
                    id += 1;
                    black_box(engine.add_market_order(id, 2, *side, 600).unwrap());
                    total += start.elapsed();
                }

                total
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_e2e_add_limit_order, bench_e2e_cancel_order, bench_e2e_market_order);
criterion_main!(benches);
