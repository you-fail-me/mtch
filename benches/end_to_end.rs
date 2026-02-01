#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use mtch::core::Side;
use mtch::engine::{Engine, EngineConfig};
use std::hint::black_box;

fn bench_e2e_add_limit_order(c: &mut Criterion) {
    c.bench_function("e2e_add_limit_order", |b| {
        // 128k pool for 100k max orders, 64 ticks (all at similar price)
        let mut engine = Engine::new(EngineConfig::new(100, 1, 10000, 32768, 64, 131072)).unwrap();
        let mut id = 0;

        b.iter(|| {
            id = (id + 1) % 100000; // Reuse IDs within capacity
            black_box(engine.add_limit_order(id, 1, Side::Bid, 9900, 100).unwrap())
        });

        let _ = engine.stop();
    });
}

fn bench_e2e_cancel_order(c: &mut Criterion) {
    c.bench_function("e2e_cancel_order", |b| {
        // 16k pool for 10k pre-populated orders, 128 ticks (100 price levels)
        let mut engine = Engine::new(EngineConfig::new(100, 1, 10000, 32768, 128, 16384)).unwrap();

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
        // 16k pool for 10k maker orders + market orders, 128 ticks (100 price levels)
        let mut engine = Engine::new(EngineConfig::new(100, 1, 10000, 32768, 128, 16384)).unwrap();

        for i in 0..10000 {
            let _ = engine.add_limit_order(i, 1, Side::Ask, 10000 + (i % 100) * 10, 100);
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
