#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use mtch::core::Side;
use mtch::engine::{Engine, EngineConfig};
use std::hint::black_box;

fn bench_e2e_add_limit_order(c: &mut Criterion) {
    c.bench_function("e2e_add_limit_order", |b| {
        let mut engine = Engine::new(EngineConfig::new(100, 1, 10000, 1024, 128, 16384, 50, 5000)).unwrap();

        // Create spread: bids @ 9900-9949, asks @ 10000-10049
        for i in 0..5000 {
            let _ = engine.add_limit_order(i, 1, Side::Bid, 9900 + (i % 50), 100);
            let _ = engine.add_limit_order(i + 5000, 1, Side::Ask, 10000 + (i % 50), 100);
        }

        let mut id = 10000;
        let mut px = 9990;
        b.iter(|| {
            id += 1;
            // Add resting bids inside spread (9990-9999 < best ask 10000)
            px = 9990 + (px % 10);
            black_box(engine.add_limit_order(id, 2, Side::Bid, px, 100).unwrap())
        });

        let _ = engine.stop();
    });
}

fn bench_e2e_cancel_order(c: &mut Criterion) {
    c.bench_function("e2e_cancel_order", |b| {
        let mut engine = Engine::new(EngineConfig::new(100, 1, 10000, 1024, 128, 16384, 50, 5000)).unwrap();

        for i in 0..10000 {
            let _ = engine.add_limit_order(i, 1, Side::Bid, 9000 + (i % 100), 100);
        }

        let mut cancel_id = 0;
        b.iter(|| {
            let result = black_box(engine.cancel_order(cancel_id).unwrap());
            cancel_id += 1;
            result
        });

        let _ = engine.stop();
    });
}

fn bench_e2e_market_order(c: &mut Criterion) {
    c.bench_function("e2e_market_order", |b| {
        let mut engine = Engine::new(EngineConfig::new(100, 1, 10000, 1024, 128, 16384, 50, 5000)).unwrap();

        // 10k asks @ 10000-10099, qty 1000 each (10M total liquidity)
        for i in 0..10000 {
            let _ = engine.add_limit_order(i, 1, Side::Ask, 10000 + (i % 100), 1000);
        }

        let mut id = 10000;
        b.iter(|| {
            id += 1;
            black_box(engine.add_market_order(id, 2, Side::Bid, 50).unwrap())
        });

        let _ = engine.stop();
    });
}

criterion_group!(benches, bench_e2e_add_limit_order, bench_e2e_cancel_order, bench_e2e_market_order);
criterion_main!(benches);
